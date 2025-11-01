use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub enum PamMethod {
    Address,
    Username,
}

#[derive(Debug, Error)]
pub enum PamAuthError {
    #[error("PAM authentication failed: {0}")]
    AuthFailed(String),
    #[error("PAM configuration error: {0}")]
    Config(String),
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
    use pam::Authenticator;
    use pam::PamError;
    use std::net::IpAddr;
    use std::path::Path;
    use tokio::task::spawn_blocking;
    use tracing::{debug, info, warn};

    pub struct PamAuthenticator {
        method: PamMethod,
        service_name: String,
        default_user: String,
        verbose: bool,
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
            let client_ip = client_ip.to_string();
            let verbose = self.verbose;

            spawn_blocking(move || {
                debug!(
                    service = service,
                    client_ip = client_ip,
                    "Starting PAM address authentication"
                );

                let mut auth = Authenticator::with_password(&service)
                    .map_err(|e| PamAuthError::System(format!("PAM init failed: {:?}", e)))?;

                auth.get_handler().set_credentials(&default_user, "");

                match auth.authenticate() {
                    Ok(_) => {
                        info!(
                            service = service,
                            client_ip = client_ip,
                            "PAM address authentication successful"
                        );
                        Ok(())
                    }
                    Err(e) => Err(map_pam_error(
                        e,
                        "PAM address authentication failed",
                        verbose,
                    )),
                }
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
                    service = service,
                    user = username,
                    client_ip = client_ip,
                    "Starting PAM username authentication"
                );

                let mut auth = Authenticator::with_password(&service)
                    .map_err(|e| PamAuthError::System(format!("PAM init failed: {:?}", e)))?;

                auth.get_handler().set_credentials(&username, &password);

                match auth.authenticate() {
                    Ok(_) => {
                        info!(
                            service = service,
                            user = username,
                            client_ip = client_ip,
                            "PAM username authentication successful"
                        );
                        Ok(())
                    }
                    Err(e) => Err(map_pam_error(
                        e,
                        "PAM username authentication failed",
                        verbose,
                    )),
                }
            })
            .await
            .map_err(|e| PamAuthError::System(format!("PAM task join error: {}", e)))?
        }
    }

    fn map_pam_error(error: PamError, context: &str, verbose: bool) -> PamAuthError {
        if verbose {
            warn!(error = %error, "{context}");
        }

        let err_str = error.to_string();
        const AUTH_FAILURE_CODES: &[&str] = &[
            "AUTH_ERR",
            "USER_UNKNOWN",
            "MAXTRIES",
            "NEW_AUTHTOK_REQD",
            "PERM_DENIED",
        ];

        if AUTH_FAILURE_CODES.iter().any(|code| err_str.contains(code)) {
            PamAuthError::AuthFailed(format!("{context}: {err_str}"))
        } else {
            PamAuthError::System(format!("{context}: {err_str}"))
        }
    }

    pub use PamAuthenticator as InnerPamAuthenticator;

    #[cfg(test)]
    mod tests {
        use super::*;
        use pam_sys::PamReturnCode;

        fn base_settings(username_service: &str, address_service: &str) -> PamSettings {
            PamSettings {
                username_service: username_service.to_string(),
                address_service: address_service.to_string(),
                default_user: "pamtest".to_string(),
                default_ruser: "pamtest".to_string(),
                verbose: false,
                verify_service: false,
            }
        }

        #[test]
        fn map_pam_error_marks_auth_failures() {
            let error = PamError::from(PamReturnCode::AUTH_ERR);
            match map_pam_error(error, "auth ctx", false) {
                PamAuthError::AuthFailed(msg) => assert!(
                    msg.contains("auth ctx"),
                    "expected context in message, got {msg}"
                ),
                other => panic!("expected AuthFailed, got {:?}", other),
            }
        }

        #[test]
        fn map_pam_error_marks_system_failures_when_not_auth_related() {
            let error = PamError::from(PamReturnCode::SYSTEM_ERR);
            match map_pam_error(error, "system ctx", false) {
                PamAuthError::System(msg) => assert!(
                    msg.contains("system ctx"),
                    "expected context in message, got {msg}"
                ),
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
        fn map_pam_error_handles_user_unknown() {
            let error = PamError::from(PamReturnCode::USER_UNKNOWN);
            match map_pam_error(error, "user ctx", false) {
                PamAuthError::AuthFailed(msg) => assert!(
                    msg.contains("user ctx"),
                    "expected context in message, got {msg}"
                ),
                other => panic!("expected AuthFailed for USER_UNKNOWN, got {:?}", other),
            }
        }

        #[test]
        fn map_pam_error_handles_maxtries() {
            let error = PamError::from(PamReturnCode::MAXTRIES);
            match map_pam_error(error, "maxtries ctx", false) {
                PamAuthError::AuthFailed(msg) => assert!(
                    msg.contains("maxtries ctx"),
                    "expected context in message, got {msg}"
                ),
                other => panic!("expected AuthFailed for MAXTRIES, got {:?}", other),
            }
        }

        #[test]
        fn map_pam_error_handles_perm_denied() {
            let error = PamError::from(PamReturnCode::PERM_DENIED);
            match map_pam_error(error, "perm ctx", false) {
                PamAuthError::AuthFailed(msg) => assert!(
                    msg.contains("perm ctx"),
                    "expected context in message, got {msg}"
                ),
                other => panic!("expected AuthFailed for PERM_DENIED, got {:?}", other),
            }
        }

        #[test]
        fn map_pam_error_preserves_context_in_messages() {
            let error = PamError::from(PamReturnCode::AUTH_ERR);
            let custom_context = "custom authentication context";
            match map_pam_error(error, custom_context, false) {
                PamAuthError::AuthFailed(msg) => assert!(
                    msg.contains(custom_context),
                    "expected custom context '{custom_context}' in message, got {msg}"
                ),
                other => panic!("expected AuthFailed, got {:?}", other),
            }
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
        fn new_respects_default_user_setting() {
            let mut settings = base_settings("svc", "addr_svc");
            settings.default_user = "customuser".to_string();
            let auth = PamAuthenticator::new(PamMethod::Address, &settings)
                .expect("expected valid authenticator");
            assert_eq!(auth.default_user, "customuser");
        }

        #[tokio::test]
        async fn authenticate_address_rejects_wrong_method() {
            let settings = base_settings("username_svc", "address_svc");
            let auth = PamAuthenticator::new(PamMethod::Username, &settings)
                .expect("expected valid authenticator");

            let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
            match auth.authenticate_address(client_ip).await {
                Err(PamAuthError::Config(msg)) => {
                    assert!(msg.contains("non-address"), "expected method mismatch error, got {msg}");
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
                    assert!(msg.contains("non-username"), "expected method mismatch error, got {msg}");
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
pub struct PamAuthenticator;

#[cfg(not(unix))]
impl PamAuthenticator {
    pub fn new(
        _method: PamMethod,
        _settings: &crate::config::PamSettings,
    ) -> Result<Self, PamAuthError> {
        Err(PamAuthError::NotSupported(
            "PAM is not available on this platform".to_string(),
        ))
    }

    pub async fn authenticate_address(
        &self,
        _client_ip: std::net::IpAddr,
    ) -> Result<(), PamAuthError> {
        Err(PamAuthError::NotSupported(
            "PAM is not available on this platform".to_string(),
        ))
    }

    pub async fn authenticate_username(
        &self,
        _client_ip: std::net::IpAddr,
        _username: &str,
        _password: &str,
    ) -> Result<(), PamAuthError> {
        Err(PamAuthError::NotSupported(
            "PAM is not available on this platform".to_string(),
        ))
    }
}
