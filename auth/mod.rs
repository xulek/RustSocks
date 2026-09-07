mod groups;
#[cfg(feature = "gssapi")]
mod gssapi;
mod pam;

#[cfg(feature = "gssapi")]
use self::gssapi::{GssApiAuthError, GssApiAuthenticator};
use self::pam::{PamAuthError, PamAuthenticator, PamMethod};
use crate::config::AuthConfig;
use crate::protocol::{parse_userpass_auth, send_auth_response, AuthMethod};
use crate::utils::error::{Result, RustSocksError};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use dashmap::DashMap;
use rand::rngs::OsRng as RandOsRng;
pub use groups::get_user_groups;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Semaphore;
use tokio::time::timeout;
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
    #[cfg(feature = "gssapi")]
    Gssapi(GssApiAuthenticator),
}

#[derive(Clone, Copy)]
struct LoginAttemptState {
    failures: u32,
    first_failure: Instant,
}

struct UserPassAuthenticator {
    users: HashMap<String, String>,
    attempts: DashMap<IpAddr, LoginAttemptState>,
}

const USERPASS_MAX_FAILURES: u32 = 10;
const USERPASS_LOCKOUT: Duration = Duration::from_secs(60);
const GROUP_CACHE_TTL: Duration = Duration::from_secs(60);
const GROUP_CACHE_MAX_ENTRIES: usize = 4096;
const GROUP_LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);
const GROUP_LOOKUP_MAX_CONCURRENCY: usize = 32;

#[derive(Clone)]
struct GroupCacheEntry {
    groups: Vec<String>,
    loaded_at: Instant,
}

fn group_cache() -> &'static DashMap<String, GroupCacheEntry> {
    static CACHE: OnceLock<DashMap<String, GroupCacheEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn group_lookup_semaphore() -> Arc<Semaphore> {
    static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(GROUP_LOOKUP_MAX_CONCURRENCY)))
        .clone()
}

pub(crate) async fn resolve_user_groups(username: &str) -> Vec<String> {
    let cache_key = username.to_ascii_lowercase();
    if let Some(entry) = group_cache().get(&cache_key) {
        if entry.loaded_at.elapsed() <= GROUP_CACHE_TTL {
            return entry.groups.clone();
        }
        drop(entry);
        group_cache().remove(&cache_key);
    }

    let permit = match timeout(GROUP_LOOKUP_TIMEOUT, group_lookup_semaphore().acquire_owned()).await {
        Ok(Ok(permit)) => permit,
        Ok(Err(_)) => {
            warn!(user = %username, "Group lookup semaphore closed");
            return Vec::new();
        }
        Err(_) => {
            warn!(user = %username, "Timed out waiting for group lookup capacity");
            return Vec::new();
        }
    };

    let owned_username = username.to_string();
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        get_user_groups(&owned_username)
    });

    let groups = match timeout(GROUP_LOOKUP_TIMEOUT, task).await {
        Ok(Ok(Ok(groups))) => groups,
        Ok(Ok(Err(err))) => {
            warn!(user = %username, error = %err, "Failed to retrieve user groups from system");
            Vec::new()
        }
        Ok(Err(err)) => {
            warn!(user = %username, error = %err, "Group lookup worker failed");
            Vec::new()
        }
        Err(_) => {
            warn!(user = %username, "Timed out retrieving user groups from system");
            Vec::new()
        }
    };

    let cache = group_cache();
    if cache.len() >= GROUP_CACHE_MAX_ENTRIES {
        let expired: Vec<String> = cache
            .iter()
            .filter(|entry| entry.loaded_at.elapsed() > GROUP_CACHE_TTL)
            .map(|entry| entry.key().clone())
            .collect();
        for key in expired {
            cache.remove(&key);
        }
    }
    if cache.len() < GROUP_CACHE_MAX_ENTRIES {
        cache.insert(
            cache_key,
            GroupCacheEntry {
                groups: groups.clone(),
                loaded_at: Instant::now(),
            },
        );
    }

    groups
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
                Ok(AuthBackend::UserPass(UserPassAuthenticator {
                    users,
                    attempts: DashMap::new(),
                }))
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
            #[cfg(feature = "gssapi")]
            "gssapi" => {
                let authenticator =
                    GssApiAuthenticator::new(&config.gssapi).map_err(map_gssapi_config_error)?;
                Ok(AuthBackend::Gssapi(authenticator))
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
            #[cfg(feature = "gssapi")]
            AuthBackend::Gssapi(_) => AuthMethod::Gssapi,
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
            #[cfg(feature = "gssapi")]
            AuthBackend::UserPass(_) | AuthBackend::PamUsername(_) | AuthBackend::Gssapi(_) => {
                Err(RustSocksError::Config(
                    "Invalid client auth configuration: only none or pam.address are supported"
                        .to_string(),
                ))
            }
            #[cfg(not(feature = "gssapi"))]
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
                if auth.is_rate_limited(client_ip) {
                    // Perform a dummy verification even while locked to reduce timing leakage.
                    let _ = verify_password(dummy_password_hash(), &password);
                    send_auth_response(stream, false).await?;
                    warn!(client_ip = %client_ip, "User/pass authentication rate limited");
                    return Err(RustSocksError::AuthFailed(
                        "Too many authentication failures".to_string(),
                    ));
                }

                let is_valid = auth.authenticate(&username, &password);
                if is_valid {
                    auth.clear_failures(client_ip);
                } else {
                    auth.record_failure(client_ip);
                }
                send_auth_response(stream, is_valid).await?;

                if is_valid {
                    info!(user = %username, "User/pass authentication successful");

                    // NSS/SSSD lookups are blocking and can involve LDAP. Resolve them
                    // on the blocking pool with timeout, concurrency limit and short cache.
                    let groups = resolve_user_groups(&username).await;

                    debug!(
                        user = %username,
                        group_count = groups.len(),
                        groups = ?groups,
                        "Retrieved user groups from system"
                    );

                    Ok(Some((username, groups)))
                } else {
                    warn!(user = %username, "User/pass authentication failed");
                    Err(RustSocksError::AuthFailed(
                        "Invalid credentials".to_string(),
                    ))
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

                        // NSS/SSSD lookups are blocking and can involve LDAP. Resolve them
                        // on the blocking pool with timeout, concurrency limit and short cache.
                        let groups = resolve_user_groups(&username).await;

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
            #[cfg(feature = "gssapi")]
            (AuthBackend::Gssapi(gssapi), AuthMethod::Gssapi) => {
                debug!("Performing GSS-API authentication");

                match gssapi.authenticate(stream).await {
                    Ok((username, groups)) => {
                        info!(
                            user = %username,
                            group_count = groups.len(),
                            groups = ?groups,
                            "GSS-API authentication successful with groups"
                        );
                        Ok(Some((username, groups)))
                    }
                    Err(e) => {
                        warn!(error = ?e, "GSS-API authentication failed");
                        Err(map_gssapi_runtime_error(e))
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
        match self.users.get(username) {
            Some(stored_password) => verify_password(stored_password, password),
            None => {
                let _ = verify_password(dummy_password_hash(), password);
                false
            }
        }
    }

    fn is_rate_limited(&self, client_ip: IpAddr) -> bool {
        let now = Instant::now();
        if let Some(entry) = self.attempts.get(&client_ip) {
            if now.duration_since(entry.first_failure) < USERPASS_LOCKOUT {
                return entry.failures >= USERPASS_MAX_FAILURES;
            }
        }
        self.attempts.remove(&client_ip);
        false
    }

    fn record_failure(&self, client_ip: IpAddr) {
        let now = Instant::now();
        self.attempts
            .entry(client_ip)
            .and_modify(|state| {
                if now.duration_since(state.first_failure) >= USERPASS_LOCKOUT {
                    *state = LoginAttemptState { failures: 1, first_failure: now };
                } else {
                    state.failures = state.failures.saturating_add(1);
                }
            })
            .or_insert(LoginAttemptState { failures: 1, first_failure: now });
    }

    fn clear_failures(&self, client_ip: IpAddr) {
        self.attempts.remove(&client_ip);
    }
}

fn dummy_password_hash() -> &'static str {
    static DUMMY_HASH: OnceLock<String> = OnceLock::new();
    DUMMY_HASH.get_or_init(|| {
        let salt = SaltString::generate(&mut RandOsRng);
        Argon2::default()
            .hash_password(b"rustsocks-userpass-dummy", &salt)
            .map(|hash| hash.to_string())
            .unwrap_or_else(|_| "rustsocks-userpass-dummy".to_string())
    })
}

pub(crate) fn verify_password(stored: &str, provided: &str) -> bool {
    if stored.starts_with("$argon2") {
        match PasswordHash::new(stored) {
            Ok(hash) => Argon2::default()
                .verify_password(provided.as_bytes(), &hash)
                .is_ok(),
            Err(_) => false,
        }
    } else {
        stored.as_bytes().ct_eq(provided.as_bytes()).into()
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

#[cfg(feature = "gssapi")]
fn map_gssapi_config_error(err: GssApiAuthError) -> RustSocksError {
    match err {
        GssApiAuthError::Config(msg) | GssApiAuthError::System(msg) => RustSocksError::Config(msg),
        GssApiAuthError::NotSupported(msg) => RustSocksError::Config(msg),
        GssApiAuthError::AuthFailed(msg) => RustSocksError::AuthFailed(msg),
    }
}

#[cfg(feature = "gssapi")]
fn map_gssapi_runtime_error(err: GssApiAuthError) -> RustSocksError {
    match err {
        GssApiAuthError::AuthFailed(msg) => RustSocksError::AuthFailed(msg),
        GssApiAuthError::Config(msg) | GssApiAuthError::NotSupported(msg) => {
            RustSocksError::Config(msg)
        }
        GssApiAuthError::System(msg) => RustSocksError::AuthFailed(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthConfig, PamSettings, User};
    use argon2::password_hash::{PasswordHasher, SaltString};
    use rand::rngs::OsRng;

    fn userpass_config() -> AuthConfig {
        AuthConfig {
            client_method: "none".to_string(),
            socks_method: "userpass".to_string(),
            users: vec![User {
                username: "alice".to_string(),
                password: "secret123".to_string(),
            }],
            pam: PamSettings::default(),
            gssapi: crate::config::GssApiSettings::default(),
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

    #[test]
    fn verify_password_plaintext() {
        assert!(verify_password("secret123", "secret123"));
        assert!(!verify_password("secret123", "wrong"));
    }

    #[test]
    fn verify_password_argon2() {
        let password = "s3cur3!";
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string();

        assert!(verify_password(&hash, password));
        assert!(!verify_password(&hash, "nope"));
    }
}
