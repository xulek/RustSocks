use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

// HMAC for Altcha signature
use hmac::{Hmac, Mac};
use rand::RngCore;
type HmacSha256 = Hmac<Sha256>;

use crate::auth::verify_password;
use crate::config::DashboardAuthSettings;
#[cfg(feature = "database")]
use crate::smtp::notifications::{notify_with_pool, NotificationDecision, NotificationKind};
#[cfg(feature = "database")]
use sqlx::{Any, Pool};

const SESSION_COOKIE_NAME: &str = "rustsocks_session";

#[derive(Clone)]
pub struct AuthState {
    pub settings: DashboardAuthSettings,
    pub sessions: Arc<DashMap<String, Session>>,
    pub base_path: String,
    pub api_token: Option<String>,
    #[cfg(feature = "database")]
    pub smtp_pool: Option<Pool<Any>>,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub username: String,
    pub created_at: u64,
    pub expires_at: u64,
}

impl AuthState {
    #[cfg(not(feature = "database"))]
    pub fn new(
        settings: DashboardAuthSettings,
        base_path: String,
        api_token: Option<String>,
    ) -> Self {
        Self {
            settings,
            sessions: Arc::new(DashMap::new()),
            base_path,
            api_token,
        }
    }

    #[cfg(feature = "database")]
    pub fn new(
        settings: DashboardAuthSettings,
        base_path: String,
        api_token: Option<String>,
        smtp_pool: Option<Pool<Any>>,
    ) -> Self {
        Self {
            settings,
            sessions: Arc::new(DashMap::new()),
            base_path,
            api_token,
            smtp_pool,
        }
    }

    pub fn create_session(&self, username: String) -> String {
        let now = current_timestamp();
        let expires_at = now + (self.settings.session_duration_hours * 3600);

        // Generate session token
        let token = generate_session_token(&username, now, &self.settings.session_secret);

        let session = Session {
            username,
            created_at: now,
            expires_at,
        };

        self.sessions.insert(token.clone(), session);

        // Cleanup expired sessions
        self.cleanup_expired_sessions();

        token
    }

    pub fn validate_session(&self, token: &str) -> Option<String> {
        if let Some(session) = self.sessions.get(token) {
            let now = current_timestamp();
            if session.expires_at > now {
                return Some(session.username.clone());
            } else {
                // Session expired
                drop(session);
                self.sessions.remove(token);
            }
        }
        None
    }

    pub fn delete_session(&self, token: &str) {
        self.sessions.remove(token);
    }

    pub fn cleanup_expired_sessions(&self) {
        let now = current_timestamp();
        self.sessions.retain(|_, session| session.expires_at > now);
    }

    pub fn verify_credentials(&self, username: &str, password: &str) -> bool {
        self.settings
            .users
            .iter()
            .any(|user| user.username == username && verify_password(&user.password, password))
    }

    pub fn cookie_path(&self) -> &str {
        if self.base_path.is_empty() {
            "/"
        } else {
            self.base_path.as_str()
        }
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn generate_session_token(username: &str, timestamp: u64, secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(username.as_bytes());
    hasher.update(timestamp.to_string().as_bytes());
    hasher.update(secret.as_bytes());
    hasher.update(rand_bytes(16).as_slice());
    format!("{:x}", hasher.finalize())
}

fn rand_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes
}

// Request/Response types
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub altcha: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AltchaPayload {
    algorithm: String,
    challenge: String,
    number: u32,
    salt: String,
    signature: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthCheckResponse {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AltchaConfigResponse {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_url: Option<String>,
}

// Handlers
pub async fn login_handler(
    State(auth_state): State<Arc<AuthState>>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    use axum::body::Body;
    use axum::response::Response;

    debug!("Login attempt for user: {}", req.username);

    // Verify credentials
    if !auth_state.verify_credentials(&req.username, &req.password) {
        warn!("Failed login attempt for user: {}", req.username);
        #[cfg(feature = "database")]
        spawn_security_notification(
            &auth_state,
            &req.username,
            "Invalid username or password",
        );
        return (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                message: "Invalid username or password".to_string(),
                username: None,
            }),
        )
            .into_response();
    }

    // Verify Altcha if enabled
    if auth_state.settings.altcha_enabled {
        match &req.altcha {
            Some(altcha_payload) => {
                if let Err(err_msg) =
                    verify_altcha(altcha_payload, &auth_state.settings.session_secret)
                {
                    warn!(
                        "Altcha verification failed for user {}: {}",
                        req.username, err_msg
                    );
                    #[cfg(feature = "database")]
                    spawn_security_notification(
                        &auth_state,
                        &req.username,
                        &format!("Altcha verification failed: {}", err_msg),
                    );
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(LoginResponse {
                            success: false,
                            message: format!("CAPTCHA verification failed: {}", err_msg),
                            username: None,
                        }),
                    )
                        .into_response();
                }
            }
            None => {
                warn!(
                    "Altcha required but not provided for user: {}",
                    req.username
                );
                #[cfg(feature = "database")]
                spawn_security_notification(
                    &auth_state,
                    &req.username,
                    "Altcha required but not provided",
                );
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(LoginResponse {
                        success: false,
                        message: "CAPTCHA verification required".to_string(),
                        username: None,
                    }),
                )
                    .into_response();
            }
        }
    }

    // Create session
    let token = auth_state.create_session(req.username.clone());

    info!("User logged in: {}", req.username);

    let mut cookie = format!(
        "{}={}; Path={}; HttpOnly; SameSite=Strict; Max-Age={}",
        SESSION_COOKIE_NAME,
        token,
        auth_state.cookie_path(),
        auth_state.settings.session_duration_hours * 3600
    );

    if auth_state.settings.cookie_secure {
        cookie.push_str("; Secure");
    }

    let json_body = serde_json::to_string(&LoginResponse {
        success: true,
        message: "Login successful".to_string(),
        username: Some(req.username),
    })
    .unwrap();

    let mut response = Response::new(Body::from(json_body));

    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *response.status_mut() = StatusCode::OK;

    response
}

#[cfg(feature = "database")]
fn spawn_security_notification(auth_state: &AuthState, username: &str, reason: &str) {
    let Some(pool) = auth_state.smtp_pool.as_ref() else {
        return;
    };
    let subject = "RustSocks security alert: failed login attempt".to_string();
    let body = format!(
        "A failed dashboard login attempt was detected.\n\nUsername: {}\nReason: {}\n",
        username, reason
    );
    let pool = pool.clone();
    let api_token = auth_state.api_token.clone();

    tokio::spawn(async move {
        match notify_with_pool(
            &pool,
            api_token,
            NotificationKind::Security,
            subject,
            body,
        )
        .await
        {
            Ok(NotificationDecision::Sent { recipients }) => {
                info!("Security notification sent to {} recipient(s)", recipients);
            }
            Ok(NotificationDecision::Skipped(_)) => {}
            Err(err) => {
                warn!("Failed to send security notification: {}", err);
            }
        }
    });
}

fn verify_altcha(payload_str: &str, secret_key: &str) -> Result<(), String> {
    use base64::engine::general_purpose;
    use base64::Engine;

    let payload_bytes = general_purpose::STANDARD
        .decode(payload_str)
        .map_err(|_| "Invalid base64 payload".to_string())?;

    let payload_json =
        std::str::from_utf8(&payload_bytes).map_err(|_| "Invalid UTF-8 in payload".to_string())?;

    let payload: AltchaPayload =
        serde_json::from_str(payload_json).map_err(|e| format!("Invalid JSON payload: {}", e))?;

    // 1. Verify algorithm
    if payload.algorithm != "SHA-256" {
        return Err(format!("Unsupported algorithm: {}", payload.algorithm));
    }

    // 2. Verify signature
    let signature_payload = format!("{}?{}", payload.salt, payload.challenge);
    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(signature_payload.as_bytes());
    let expected_signature = format!("{:x}", mac.finalize().into_bytes());

    if payload.signature != expected_signature {
        return Err("Invalid signature".to_string());
    }

    // 3. Verify proof-of-work
    let challenge_input = format!("{}{}", payload.salt, payload.number);
    let mut hasher = Sha256::new();
    hasher.update(challenge_input.as_bytes());
    let computed_challenge = format!("{:x}", hasher.finalize());

    if payload.challenge != computed_challenge {
        return Err("Invalid proof-of-work solution".to_string());
    }

    // 4. Check expiration
    if let Some(expires_pos) = payload.salt.find("?expires=") {
        let expires_str = &payload.salt[expires_pos + 9..];
        if let Ok(expires_timestamp) = expires_str.parse::<u64>() {
            let now = current_timestamp();
            if now > expires_timestamp {
                return Err("Challenge expired".to_string());
            }
        }
    }

    debug!("Altcha verification successful");
    Ok(())
}

pub async fn logout_handler(
    State(auth_state): State<Arc<AuthState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Extract session token from cookie
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some(token) = cookie.strip_prefix(&format!("{}=", SESSION_COOKIE_NAME)) {
                    auth_state.delete_session(token);
                    info!("User logged out");
                    break;
                }
            }
        }
    }

    let mut response_headers = HeaderMap::new();
    let mut cookie = format!(
        "{}=; Path={}; HttpOnly; SameSite=Strict; Max-Age=0",
        SESSION_COOKIE_NAME,
        auth_state.cookie_path()
    );
    if auth_state.settings.cookie_secure {
        cookie.push_str("; Secure");
    }
    response_headers.insert(header::SET_COOKIE, cookie.parse().unwrap());

    (
        StatusCode::OK,
        response_headers,
        Json(serde_json::json!({
            "success": true,
            "message": "Logged out successfully"
        })),
    )
}

pub async fn check_auth_handler(
    State(auth_state): State<Arc<AuthState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Extract session token from cookie
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some(token) = cookie.strip_prefix(&format!("{}=", SESSION_COOKIE_NAME)) {
                    if let Some(username) = auth_state.validate_session(token) {
                        return Json(AuthCheckResponse {
                            authenticated: true,
                            username: Some(username),
                        });
                    }
                }
            }
        }
    }

    Json(AuthCheckResponse {
        authenticated: false,
        username: None,
    })
}

pub async fn altcha_config_handler(State(auth_state): State<Arc<AuthState>>) -> impl IntoResponse {
    Json(AltchaConfigResponse {
        enabled: auth_state.settings.altcha_enabled,
        challenge_url: auth_state.settings.altcha_challenge_url.clone(),
    })
}

#[derive(Debug, Serialize)]
pub struct AltchaChallengeResponse {
    algorithm: String,
    challenge: String,
    salt: String,
    signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    maxnumber: Option<u32>,
}

pub async fn altcha_challenge_handler(
    State(auth_state): State<Arc<AuthState>>,
) -> impl IntoResponse {
    if !auth_state.settings.altcha_enabled {
        return (StatusCode::NOT_FOUND, Body::from("Altcha is not enabled")).into_response();
    }

    // Generate random salt with expiration
    let timestamp = current_timestamp();
    let random_part = generate_random_string(12);
    let salt = format!("{}?expires={}", random_part, timestamp + 1200); // 20 min expiry

    // Generate secret number (this is what client needs to find via PoW)
    let secret_number = (timestamp % 50000) as u32;

    // Challenge = SHA256(salt + secret_number)
    // Client will try numbers 0..maxnumber until they find one where SHA256(salt + number) == challenge
    let challenge_input = format!("{}{}", salt, secret_number);
    let mut hasher = Sha256::new();
    hasher.update(challenge_input.as_bytes());
    let challenge_hash = hasher.finalize();
    let challenge = format!("{:x}", challenge_hash);

    // Generate HMAC-SHA256 signature
    // Signature = HMAC-SHA256(salt + "?" + challenge, secret_key)
    // This allows server to verify the challenge was generated by us
    let signature_payload = format!("{}?{}", salt, challenge);
    let mut mac = HmacSha256::new_from_slice(auth_state.settings.session_secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(signature_payload.as_bytes());
    let signature_bytes = mac.finalize().into_bytes();
    let signature = format!("{:x}", signature_bytes);

    let response = AltchaChallengeResponse {
        algorithm: "SHA-256".to_string(),
        challenge,
        salt,
        signature,
        maxnumber: Some(50000), // PoW difficulty (max number to try)
    };

    Json(response).into_response()
}

fn generate_random_string(len: usize) -> String {
    const CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

    let mut bytes = vec![0u8; len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);

    let mut result = String::with_capacity(len);
    for byte in bytes {
        let ch = CHARSET[(byte as usize) % CHARSET.len()] as char;
        result.push(ch);
    }
    result
}

// Extract session from request headers
pub fn extract_session_from_headers(headers: &HeaderMap) -> Option<String> {
    for cookie_header in headers.get_all(header::COOKIE).iter() {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some(token) = cookie.strip_prefix(&format!("{}=", SESSION_COOKIE_NAME)) {
                    return Some(token.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, HeaderMap, HeaderValue};

    #[test]
    fn random_string_has_expected_shape() {
        let value = generate_random_string(32);
        assert_eq!(value.len(), 32);
        assert!(value.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn rand_bytes_returns_requested_length() {
        let bytes = rand_bytes(24);
        assert_eq!(bytes.len(), 24);
    }

    #[test]
    fn cookie_path_uses_base_path_or_root() {
        let settings = DashboardAuthSettings::default();
        let state = {
            #[cfg(feature = "database")]
            {
                AuthState::new(settings.clone(), "/rustsocks".to_string(), None, None)
            }
            #[cfg(not(feature = "database"))]
            {
                AuthState::new(settings.clone(), "/rustsocks".to_string(), None)
            }
        };
        assert_eq!(state.cookie_path(), "/rustsocks");

        let root_state = {
            #[cfg(feature = "database")]
            {
                AuthState::new(settings, "".to_string(), None, None)
            }
            #[cfg(not(feature = "database"))]
            {
                AuthState::new(settings, "".to_string(), None)
            }
        };
        assert_eq!(root_state.cookie_path(), "/");
    }

    #[test]
    fn extract_session_from_multiple_cookie_headers() {
        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, HeaderValue::from_static("other=1"));
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("rustsocks_session=abc123; theme=dark"),
        );

        assert_eq!(
            extract_session_from_headers(&headers),
            Some("abc123".to_string())
        );
    }
}
