use thiserror::Error;

#[cfg(not(unix))]
mod stub;

#[derive(Debug, Clone, Copy)]
pub enum PamMethod {
    Address,
    Username,
}

#[derive(Debug, Error)]
pub enum PamAuthError {
    #[cfg_attr(not(unix), allow(dead_code))]
    #[error("PAM authentication failed: {0}")]
    AuthFailed(String),
    #[cfg_attr(not(unix), allow(dead_code))]
    #[error("PAM configuration error: {0}")]
    Config(String),
    #[cfg_attr(not(unix), allow(dead_code))]
    #[error("PAM system error: {0}")]
    System(String),
    #[cfg_attr(unix, allow(dead_code))]
    #[error("PAM not supported on this platform: {0}")]
    NotSupported(String),
}

#[cfg(unix)]
mod unix {
    use super::{PamAuthError, PamMethod};
    use crate::config::PamSettings;
    use libc::{c_int, c_void};
    use pam_sys::raw;
    use pam_sys::types::{
        PamConversation, PamHandle, PamItemType, PamMessage, PamMessageStyle, PamResponse,
        PamReturnCode,
    };
    use pam_sys::wrapped;
    use std::ffi::{CStr, CString};
    use std::mem::size_of;
    use std::net::IpAddr;
    use std::path::Path;
    use std::ptr;
    use tokio::task::spawn_blocking;
    use tracing::{debug, info, warn};

    pub struct PamAuthenticator {
        method: PamMethod,
        service_name: String,
        default_user: String,
        default_ruser: String,
        verbose: bool,
    }

    struct ConversationData {
        username: CString,
        password: CString,
    }

    struct PamTransaction {
        handle: *mut PamHandle,
        #[allow(dead_code)]
        // Kept alive for the lifetime of the PAM handle; the callback pointer is registered from it.
        conversation: PamConversation,
        #[allow(dead_code)]
        // Owns the strings referenced by the PAM conversation callback.
        conversation_data: Box<ConversationData>,
        user: CString,
        rhost: CString,
        ruser: CString,
        last_status: PamReturnCode,
    }

    impl PamAuthenticator {
        pub fn new(method: PamMethod, settings: &PamSettings) -> Result<Self, PamAuthError> {
            let service_name = match method {
                PamMethod::Address => settings.address_service.clone(),
                PamMethod::Username => settings.username_service.clone(),
            };

            if service_name.trim().is_empty() {
                return Err(PamAuthError::Config(
                    "PAM service name cannot be empty".to_string(),
                ));
            }

            if settings.verify_service {
                let service_path = Path::new("/etc/pam.d").join(&service_name);
                if !service_path.exists() {
                    warn!(
                        "PAM service file not found at {}. Authentication may fail.",
                        service_path.display()
                    );
                }
            }

            Ok(Self {
                method,
                service_name,
                default_user: settings.default_user.clone(),
                default_ruser: settings.default_ruser.clone(),
                verbose: settings.verbose,
            })
        }

        pub async fn authenticate_address(&self, client_ip: IpAddr) -> Result<(), PamAuthError> {
            if !matches!(self.method, PamMethod::Address) {
                return Err(PamAuthError::Config(
                    "authenticate_address called for non-address PAM method".to_string(),
                ));
            }

            let service = self.service_name.clone();
            let default_user = self.default_user.clone();
            let default_ruser = self.default_ruser.clone();
            let client_ip = client_ip.to_string();
            let verbose = self.verbose;

            spawn_blocking(move || {
                debug!(
                    service = %service,
                    client_ip = %client_ip,
                    "Starting PAM address authentication"
                );

                let mut tx =
                    PamTransaction::new(&service, &default_user, "", &client_ip, &default_ruser)?;
                tx.authenticate(verbose, "PAM address authentication failed")?;

                info!(
                    service = %service,
                    client_ip = %client_ip,
                    "PAM address authentication successful"
                );
                Ok(())
            })
            .await
            .map_err(|e| PamAuthError::System(format!("PAM task join error: {}", e)))?
        }

        pub async fn authenticate_username(
            &self,
            client_ip: IpAddr,
            username: &str,
            password: &str,
        ) -> Result<(), PamAuthError> {
            if !matches!(self.method, PamMethod::Username) {
                return Err(PamAuthError::Config(
                    "authenticate_username called for non-username PAM method".to_string(),
                ));
            }

            let service = self.service_name.clone();
            let username = username.to_string();
            let password = password.to_string();
            let client_ip = client_ip.to_string();
            let verbose = self.verbose;

            spawn_blocking(move || {
                debug!(
                    service = %service,
                    user = %username,
                    client_ip = %client_ip,
                    "Starting PAM username authentication"
                );

                let mut tx =
                    PamTransaction::new(&service, &username, &password, &client_ip, &username)?;
                tx.authenticate(verbose, "PAM username authentication failed")?;

                info!(
                    service = %service,
                    user = %username,
                    client_ip = %client_ip,
                    "PAM username authentication successful"
                );
                Ok(())
            })
            .await
            .map_err(|e| PamAuthError::System(format!("PAM task join error: {}", e)))?
        }
    }

    impl PamTransaction {
        fn new(
            service_name: &str,
            user: &str,
            password: &str,
            rhost: &str,
            ruser: &str,
        ) -> Result<Self, PamAuthError> {
            let user_cstr = cstring("user", user)?;
            let password_cstr = cstring("password", password)?;
            let rhost_cstr = cstring("rhost", rhost)?;
            let ruser_cstr = cstring("ruser", ruser)?;
            let mut conversation_data = Box::new(ConversationData {
                username: user_cstr.clone(),
                password: password_cstr,
            });
            let conversation = PamConversation {
                conv: Some(conversation_fn),
                data_ptr: (&mut *conversation_data as *mut ConversationData).cast::<c_void>(),
            };

            let mut handle = ptr::null_mut();
            let start_status = wrapped::start(service_name, Some(user), &conversation, &mut handle);
            if start_status != PamReturnCode::SUCCESS {
                return Err(map_pam_status(
                    start_status,
                    handle,
                    "PAM init failed",
                    false,
                ));
            }

            let mut transaction = Self {
                handle,
                conversation,
                conversation_data,
                user: user_cstr,
                rhost: rhost_cstr,
                ruser: ruser_cstr,
                last_status: PamReturnCode::SUCCESS,
            };

            let rhost = transaction.rhost.clone();
            let ruser = transaction.ruser.clone();
            let user = transaction.user.clone();

            transaction.set_item(PamItemType::RHOST, &rhost)?;
            transaction.set_item(PamItemType::RUSER, &ruser)?;
            transaction.set_item(PamItemType::USER, &user)?;

            Ok(transaction)
        }

        fn authenticate(&mut self, verbose: bool, context: &str) -> Result<(), PamAuthError> {
            let auth_status = wrapped::authenticate(self.handle_mut(), pam_sys::PamFlag::NONE);
            self.last_status = auth_status;
            if auth_status != PamReturnCode::SUCCESS {
                return Err(map_pam_status(auth_status, self.handle, context, verbose));
            }

            let acct_status = wrapped::acct_mgmt(self.handle_mut(), pam_sys::PamFlag::NONE);
            self.last_status = acct_status;
            if acct_status != PamReturnCode::SUCCESS {
                return Err(map_pam_status(acct_status, self.handle, context, verbose));
            }

            Ok(())
        }

        fn set_item(&mut self, item: PamItemType, value: &CString) -> Result<(), PamAuthError> {
            let status = unsafe {
                PamReturnCode::from(raw::pam_set_item(
                    self.handle,
                    item as c_int,
                    value.as_ptr().cast::<c_void>(),
                ))
            };
            self.last_status = status;
            if status == PamReturnCode::SUCCESS {
                Ok(())
            } else {
                Err(map_pam_status(
                    status,
                    self.handle,
                    &format!("Failed to set PAM item {}", item),
                    false,
                ))
            }
        }

        fn handle_mut(&mut self) -> &mut PamHandle {
            unsafe { &mut *self.handle }
        }
    }

    impl Drop for PamTransaction {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                let status = self.last_status;
                let handle = self.handle;
                let _ = unsafe { wrapped::end(&mut *handle, status) };
            }
        }
    }

    extern "C" fn conversation_fn(
        num_msg: c_int,
        msg: *mut *mut PamMessage,
        resp: *mut *mut PamResponse,
        appdata_ptr: *mut c_void,
    ) -> c_int {
        if num_msg <= 0 || msg.is_null() || resp.is_null() || appdata_ptr.is_null() {
            return PamReturnCode::CONV_ERR as c_int;
        }

        let data = unsafe { &*(appdata_ptr as *const ConversationData) };
        let response_count = num_msg as usize;
        let responses =
            unsafe { libc::calloc(response_count, size_of::<PamResponse>()).cast::<PamResponse>() };

        if responses.is_null() {
            return PamReturnCode::BUF_ERR as c_int;
        }

        for index in 0..response_count {
            let message_ptr = unsafe { *msg.add(index) };
            if message_ptr.is_null() {
                unsafe { free_responses(responses, index) };
                return PamReturnCode::CONV_ERR as c_int;
            }

            let message = unsafe { &*message_ptr };
            let response = unsafe { responses.add(index) };
            let style = PamMessageStyle::from(message.msg_style);

            let duplicated = match style {
                PamMessageStyle::PROMPT_ECHO_OFF => unsafe { libc::strdup(data.password.as_ptr()) },
                PamMessageStyle::PROMPT_ECHO_ON => unsafe { libc::strdup(data.username.as_ptr()) },
                PamMessageStyle::TEXT_INFO | PamMessageStyle::ERROR_MSG => ptr::null_mut(),
            };

            if matches!(
                style,
                PamMessageStyle::PROMPT_ECHO_OFF | PamMessageStyle::PROMPT_ECHO_ON
            ) && duplicated.is_null()
            {
                unsafe { free_responses(responses, index) };
                return PamReturnCode::BUF_ERR as c_int;
            }

            unsafe {
                (*response).resp = duplicated;
                (*response).resp_retcode = 0;
            }
        }

        unsafe {
            *resp = responses;
        }
        PamReturnCode::SUCCESS as c_int
    }

    unsafe fn free_responses(responses: *mut PamResponse, initialized: usize) {
        for index in 0..initialized {
            let response = responses.add(index);
            if !(*response).resp.is_null() {
                libc::free((*response).resp.cast::<c_void>());
            }
        }
        libc::free(responses.cast::<c_void>());
    }

    fn cstring(field: &str, value: &str) -> Result<CString, PamAuthError> {
        CString::new(value).map_err(|_| {
            PamAuthError::Config(format!("PAM {} cannot contain embedded NUL bytes", field))
        })
    }

    fn pam_strerror(handle: *mut PamHandle, status: PamReturnCode) -> String {
        if handle.is_null() {
            return status.to_string();
        }

        let ptr = unsafe { raw::pam_strerror(handle, status as c_int) };
        if ptr.is_null() {
            status.to_string()
        } else {
            unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        }
    }

    fn map_pam_status(
        status: PamReturnCode,
        handle: *mut PamHandle,
        context: &str,
        verbose: bool,
    ) -> PamAuthError {
        let err_str = pam_strerror(handle, status);
        if verbose {
            warn!(status = ?status, error = %err_str, "{context}");
        }

        if is_auth_failure(status) {
            PamAuthError::AuthFailed(format!("{context}: {err_str}"))
        } else {
            PamAuthError::System(format!("{context}: {err_str}"))
        }
    }

    fn is_auth_failure(status: PamReturnCode) -> bool {
        matches!(
            status,
            PamReturnCode::AUTH_ERR
                | PamReturnCode::USER_UNKNOWN
                | PamReturnCode::MAXTRIES
                | PamReturnCode::NEW_AUTHTOK_REQD
                | PamReturnCode::PERM_DENIED
                | PamReturnCode::ACCT_EXPIRED
                | PamReturnCode::AUTHINFO_UNAVAIL
                | PamReturnCode::CRED_INSUFFICIENT
                | PamReturnCode::CRED_EXPIRED
        )
    }

    pub use PamAuthenticator as InnerPamAuthenticator;

    #[cfg(test)]
    mod tests {
        use super::*;

        fn base_settings(username_service: &str, address_service: &str) -> PamSettings {
            PamSettings {
                username_service: username_service.to_string(),
                address_service: address_service.to_string(),
                default_user: "pamtest".to_string(),
                default_ruser: "pamruser".to_string(),
                verbose: false,
                verify_service: false,
            }
        }

        #[test]
        fn auth_failures_are_classified_correctly() {
            let error = map_pam_status(PamReturnCode::AUTH_ERR, ptr::null_mut(), "auth ctx", false);
            match error {
                PamAuthError::AuthFailed(msg) => {
                    assert!(
                        msg.contains("auth ctx"),
                        "expected context in message, got {msg}"
                    );
                }
                other => panic!("expected AuthFailed, got {:?}", other),
            }
        }

        #[test]
        fn system_failures_are_classified_correctly() {
            let error = map_pam_status(
                PamReturnCode::SYSTEM_ERR,
                ptr::null_mut(),
                "system ctx",
                false,
            );
            match error {
                PamAuthError::System(msg) => {
                    assert!(
                        msg.contains("system ctx"),
                        "expected context in message, got {msg}"
                    );
                }
                other => panic!("expected System, got {:?}", other),
            }
        }

        #[test]
        fn new_rejects_empty_service_name() {
            let settings = base_settings("", "pam_address_service");
            match PamAuthenticator::new(PamMethod::Username, &settings) {
                Ok(_) => panic!("expected config error for empty service"),
                Err(PamAuthError::Config(msg)) => {
                    assert!(msg.contains("cannot be empty"), "unexpected message: {msg}")
                }
                Err(other) => panic!("expected Config error, got {:?}", other),
            }
        }

        #[test]
        fn new_accepts_valid_service_without_verification() {
            let settings = base_settings("pam_login", "pam_address");
            PamAuthenticator::new(PamMethod::Address, &settings)
                .expect("expected valid PAM authenticator");
        }

        #[test]
        fn new_rejects_whitespace_only_service_name() {
            let settings = base_settings("   ", "pam_address");
            match PamAuthenticator::new(PamMethod::Username, &settings) {
                Ok(_) => panic!("expected config error for whitespace-only service"),
                Err(PamAuthError::Config(msg)) => {
                    assert!(msg.contains("cannot be empty"), "unexpected message: {msg}")
                }
                Err(other) => panic!("expected Config error, got {:?}", other),
            }
        }

        #[test]
        fn new_selects_correct_service_for_address_method() {
            let settings = base_settings("username_svc", "address_svc");
            let auth = PamAuthenticator::new(PamMethod::Address, &settings)
                .expect("expected valid authenticator");
            assert_eq!(auth.service_name, "address_svc");
        }

        #[test]
        fn new_selects_correct_service_for_username_method() {
            let settings = base_settings("username_svc", "address_svc");
            let auth = PamAuthenticator::new(PamMethod::Username, &settings)
                .expect("expected valid authenticator");
            assert_eq!(auth.service_name, "username_svc");
        }

        #[test]
        fn new_respects_verbose_setting() {
            let mut settings = base_settings("svc", "addr_svc");
            settings.verbose = true;
            let auth = PamAuthenticator::new(PamMethod::Username, &settings)
                .expect("expected valid authenticator");
            assert!(auth.verbose, "verbose should be enabled");
        }

        #[test]
        fn new_respects_default_users() {
            let mut settings = base_settings("svc", "addr_svc");
            settings.default_user = "customuser".to_string();
            settings.default_ruser = "customruser".to_string();
            let auth = PamAuthenticator::new(PamMethod::Address, &settings)
                .expect("expected valid authenticator");
            assert_eq!(auth.default_user, "customuser");
            assert_eq!(auth.default_ruser, "customruser");
        }

        #[tokio::test]
        async fn authenticate_address_rejects_wrong_method() {
            let settings = base_settings("username_svc", "address_svc");
            let auth = PamAuthenticator::new(PamMethod::Username, &settings)
                .expect("expected valid authenticator");

            let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
            match auth.authenticate_address(client_ip).await {
                Err(PamAuthError::Config(msg)) => {
                    assert!(
                        msg.contains("non-address"),
                        "expected method mismatch error, got {msg}"
                    );
                }
                Ok(_) => panic!("expected error for method mismatch"),
                Err(other) => panic!("expected Config error, got {:?}", other),
            }
        }

        #[tokio::test]
        async fn authenticate_username_rejects_wrong_method() {
            let settings = base_settings("username_svc", "address_svc");
            let auth = PamAuthenticator::new(PamMethod::Address, &settings)
                .expect("expected valid authenticator");

            let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
            match auth.authenticate_username(client_ip, "user", "pass").await {
                Err(PamAuthError::Config(msg)) => {
                    assert!(
                        msg.contains("non-username"),
                        "expected method mismatch error, got {msg}"
                    );
                }
                Ok(_) => panic!("expected error for method mismatch"),
                Err(other) => panic!("expected Config error, got {:?}", other),
            }
        }
    }
}

#[cfg(unix)]
pub use unix::InnerPamAuthenticator as PamAuthenticator;

#[cfg(not(unix))]
pub use stub::PamAuthenticator;
