use crate::config::AuthConfig;
use crate::protocol::{parse_userpass_auth, send_auth_response, AuthMethod};
use crate::utils::error::{Result, RustSocksError};
use std::collections::HashMap;
use tokio::net::TcpStream;
use tracing::{debug, warn};

pub struct AuthManager {
    method: AuthenticationMethod,
}

enum AuthenticationMethod {
    NoAuth,
    UserPass(UserPassAuthenticator),
}

struct UserPassAuthenticator {
    users: HashMap<String, String>, // username -> password
}

impl AuthManager {
    pub fn new(config: &AuthConfig) -> Result<Self> {
        let method = match config.method.as_str() {
            "none" => AuthenticationMethod::NoAuth,
            "userpass" => {
                let mut users = HashMap::new();
                for user in &config.users {
                    users.insert(user.username.clone(), user.password.clone());
                }
                AuthenticationMethod::UserPass(UserPassAuthenticator { users })
            }
            _ => {
                return Err(RustSocksError::Config(format!(
                    "Unsupported auth method: {}",
                    config.method
                )));
            }
        };

        Ok(Self { method })
    }

    /// Get the supported authentication method
    pub fn get_method(&self) -> AuthMethod {
        match self.method {
            AuthenticationMethod::NoAuth => AuthMethod::NoAuth,
            AuthenticationMethod::UserPass(_) => AuthMethod::UserPass,
        }
    }

    /// Perform authentication
    pub async fn authenticate(
        &self,
        stream: &mut TcpStream,
        method: AuthMethod,
    ) -> Result<Option<String>> {
        match (&self.method, method) {
            (AuthenticationMethod::NoAuth, AuthMethod::NoAuth) => {
                debug!("No authentication required");
                Ok(None)
            }
            (AuthenticationMethod::UserPass(auth), AuthMethod::UserPass) => {
                debug!("Performing username/password authentication");

                let (username, password) = parse_userpass_auth(stream).await?;

                let is_valid = auth.authenticate(&username, &password);

                send_auth_response(stream, is_valid).await?;

                if is_valid {
                    debug!("Authentication successful for user: {}", username);
                    Ok(Some(username))
                } else {
                    warn!("Authentication failed for user: {}", username);
                    Err(RustSocksError::AuthFailed(format!(
                        "Invalid credentials for user: {}",
                        username
                    )))
                }
            }
            _ => {
                warn!(
                    "Method mismatch: expected {:?}, got {:?}",
                    self.get_method(),
                    method
                );
                Err(RustSocksError::AuthFailed(
                    "Authentication method mismatch".to_string(),
                ))
            }
        }
    }

    /// Check if a specific auth method is supported
    pub fn supports(&self, method: AuthMethod) -> bool {
        self.get_method() == method
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthConfig, User};

    #[test]
    fn test_no_auth() {
        let config = AuthConfig {
            method: "none".to_string(),
            users: vec![],
        };

        let auth_manager = AuthManager::new(&config).unwrap();
        assert_eq!(auth_manager.get_method(), AuthMethod::NoAuth);
        assert!(auth_manager.supports(AuthMethod::NoAuth));
        assert!(!auth_manager.supports(AuthMethod::UserPass));
    }

    #[test]
    fn test_userpass_auth() {
        let config = AuthConfig {
            method: "userpass".to_string(),
            users: vec![
                User {
                    username: "alice".to_string(),
                    password: "secret123".to_string(),
                },
                User {
                    username: "bob".to_string(),
                    password: "password".to_string(),
                },
            ],
        };

        let auth_manager = AuthManager::new(&config).unwrap();
        assert_eq!(auth_manager.get_method(), AuthMethod::UserPass);

        if let AuthenticationMethod::UserPass(authenticator) = &auth_manager.method {
            assert!(authenticator.authenticate("alice", "secret123"));
            assert!(!authenticator.authenticate("alice", "wrong"));
            assert!(!authenticator.authenticate("charlie", "password"));
        }
    }
}
