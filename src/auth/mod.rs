mod groups;
mod pam;

use self::pam::{PamAuthError, PamAuthenticator, PamMethod};
use crate::config::AuthConfig;
use crate::protocol::{parse_userpass_auth, send_auth_response, AuthMethod};
use crate::utils::error::{Result, RustSocksError};
pub use groups::get_user_groups;
use std::collections::HashMap;
use std::net::IpAddr;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, info, warn};

pub struct AuthManager {
    client_backend: AuthBackend,
    socks_backend: AuthBackend,
}

enum AuthBackend {
    None,
    UserPass(UserPassAuthenticator),
    PamAddress(PamAuthenticator),
    PamUsername(PamAuthenticator),
}

struct UserPassAuthenticator {
    users: HashMap<String, String>,
}

impl AuthManager {
    pub fn new(config: &AuthConfig) -> Result<Self> {
        let client_backend = Self::build_backend(&config.client_method, config)?;
        let socks_backend = Self::build_backend(&config.socks_method, config)?;

        Ok(Self {
            client_backend,
            socks_backend,
        })
    }

    fn build_backend(method: &str, config: &AuthConfig) -> Result<AuthBackend> {
        match method {
            "none" => Ok(AuthBackend::None),
            "userpass" => {
                let mut users = HashMap::new();
                for user in &config.users {
                    users.insert(user.username.clone(), user.password.clone());
                }
                Ok(AuthBackend::UserPass(UserPassAuthenticator { users }))
            }
            "pam.address" => {
                let authenticator = PamAuthenticator::new(PamMethod::Address, &config.pam)
                    .map_err(map_pam_config_error)?;
                Ok(AuthBackend::PamAddress(authenticator))
            }
            "pam.username" => {
                let authenticator = PamAuthenticator::new(PamMethod::Username, &config.pam)
                    .map_err(map_pam_config_error)?;
                Ok(AuthBackend::PamUsername(authenticator))
            }
            other => Err(RustSocksError::Config(format!(
                "Unsupported authentication method: {}",
                other
            ))),
        }
    }

    /// Method advertised during SOCKS5 negotiation
    pub fn get_method(&self) -> AuthMethod {
        match self.socks_backend {
            AuthBackend::None | AuthBackend::PamAddress(_) => AuthMethod::NoAuth,
            AuthBackend::UserPass(_) | AuthBackend::PamUsername(_) => AuthMethod::UserPass,
        }
    }

    /// Check if a specific auth method is supported
    pub fn supports(&self, method: AuthMethod) -> bool {
        let server_method = self.get_method();
        if server_method == method {
            return true;
        }

        matches!(method, AuthMethod::NoAuth)
            && matches!(
                self.socks_backend,
                AuthBackend::None | AuthBackend::PamAddress(_)
            )
    }

    /// Perform client-level authentication (before SOCKS negotiation)
    pub async fn authenticate_client(&self, client_ip: IpAddr) -> Result<()> {
        match &self.client_backend {
            AuthBackend::None => Ok(()),
            AuthBackend::PamAddress(pam) => pam
                .authenticate_address(client_ip)
                .await
                .map_err(map_pam_runtime_error),
            AuthBackend::UserPass(_) | AuthBackend::PamUsername(_) => Err(RustSocksError::Config(
                "Invalid client auth configuration: only none or pam.address are supported"
                    .to_string(),
            )),
        }
    }

    /// Perform SOCKS-level authentication
    ///
    /// Returns:
    /// - `Ok(None)` for no-auth methods
    /// - `Ok(Some((username, groups)))` for authenticated users with their LDAP groups
    pub async fn authenticate<S>(
        &self,
        stream: &mut S,
        method: AuthMethod,
        client_ip: IpAddr,
    ) -> Result<Option<(String, Vec<String>)>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        match (&self.socks_backend, method) {
            (AuthBackend::None, AuthMethod::NoAuth) => {
                debug!("No authentication required");
                Ok(None)
            }
            (AuthBackend::PamAddress(pam), AuthMethod::NoAuth) => {
                pam.authenticate_address(client_ip)
                    .await
                    .map_err(map_pam_runtime_error)?;
                debug!("PAM address authentication successful");
                Ok(None)
            }
            (AuthBackend::UserPass(auth), AuthMethod::UserPass) => {
                debug!("Performing username/password authentication");

                let (username, password) = parse_userpass_auth(stream).await?;
                let is_valid = auth.authenticate(&username, &password);
                send_auth_response(stream, is_valid).await?;

                if is_valid {
                    info!(user = %username, "User/pass authentication successful");

                    // Retrieve user groups from system (LDAP via NSS/SSSD)
                    let groups = get_user_groups(&username).unwrap_or_else(|e| {
                        warn!(
                            user = %username,
                            error = %e,
                            "Failed to retrieve user groups from system, using empty list"
                        );
                        Vec::new()
                    });

                    debug!(
                        user = %username,
                        group_count = groups.len(),
                        groups = ?groups,
                        "Retrieved user groups from system"
                    );

                    Ok(Some((username, groups)))
                } else {
                    warn!(user = %username, "User/pass authentication failed");
                    Err(RustSocksError::AuthFailed(format!(
                        "Invalid credentials for user: {}",
                        username
                    )))
                }
            }
            (AuthBackend::PamUsername(pam), AuthMethod::UserPass) => {
                debug!("Performing PAM username authentication");
                let (username, password) = parse_userpass_auth(stream).await?;

                match pam
                    .authenticate_username(client_ip, &username, &password)
                    .await
                {
                    Ok(()) => {
                        send_auth_response(stream, true).await?;
                        info!(user = %username, "PAM authentication successful");

                        // Retrieve user groups from system (LDAP via NSS/SSSD)
                        let groups = get_user_groups(&username).unwrap_or_else(|e| {
                            warn!(
                                user = %username,
                                error = %e,
                                "Failed to retrieve user groups from system, using empty list"
                            );
                            Vec::new()
                        });

                        info!(
                            user = %username,
                            group_count = groups.len(),
                            groups = ?groups,
                            "PAM authentication successful with LDAP groups"
                        );

                        Ok(Some((username, groups)))
                    }
                    Err(e) => {
                        send_auth_response(stream, false).await?;
                        warn!(user = %username, error = ?e, "PAM authentication failed");
                        Err(map_pam_runtime_error(e))
                    }
                }
            }
            _ => {
                warn!(
                    "Authentication method mismatch: expected {:?}, got {:?}",
                    self.get_method(),
                    method
                );
                Err(RustSocksError::AuthFailed(
                    "Authentication method mismatch".to_string(),
                ))
            }
        }
    }
}

impl UserPassAuthenticator {
    fn authenticate(&self, username: &str, password: &str) -> bool {
        self.users
            .get(username)
            .map(|stored_password| stored_password == password)
            .unwrap_or(false)
    }
}

fn map_pam_config_error(err: PamAuthError) -> RustSocksError {
    match err {
        PamAuthError::Config(msg) | PamAuthError::System(msg) => RustSocksError::Config(msg),
        PamAuthError::NotSupported(msg) => RustSocksError::Config(msg),
        PamAuthError::AuthFailed(msg) => RustSocksError::AuthFailed(msg),
    }
}

fn map_pam_runtime_error(err: PamAuthError) -> RustSocksError {
    match err {
        PamAuthError::AuthFailed(msg) => RustSocksError::AuthFailed(msg),
        PamAuthError::Config(msg) | PamAuthError::NotSupported(msg) => RustSocksError::Config(msg),
        PamAuthError::System(msg) => RustSocksError::AuthFailed(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthConfig, PamSettings, User};

    fn userpass_config() -> AuthConfig {
        AuthConfig {
            client_method: "none".to_string(),
            socks_method: "userpass".to_string(),
            users: vec![User {
                username: "alice".to_string(),
                password: "secret123".to_string(),
            }],
            pam: PamSettings::default(),
        }
    }

    #[test]
    fn test_no_auth() {
        let config = AuthConfig::default();
        let auth_manager = AuthManager::new(&config).unwrap();
        assert_eq!(auth_manager.get_method(), AuthMethod::NoAuth);
        assert!(auth_manager.supports(AuthMethod::NoAuth));
        assert!(!auth_manager.supports(AuthMethod::UserPass));
    }

    #[test]
    fn test_userpass_backend() {
        let config = userpass_config();
        let auth_manager = AuthManager::new(&config).unwrap();
        assert_eq!(auth_manager.get_method(), AuthMethod::UserPass);
    }
}
