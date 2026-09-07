use axum::{extract::State, http::StatusCode, Json};
#[cfg(feature = "database")]
use tracing::{error, info};

use crate::api::handlers::sessions::ApiState;
use crate::api::types::{
    SmtpConfigUpdateRequest, SmtpModeOption, SmtpModesResponse, SmtpTestRequest, SmtpTestResponse,
};
#[cfg(feature = "database")]
use crate::api::types::SmtpConfigResponse;
use crate::smtp::SmtpMode;
#[cfg(feature = "database")]
use crate::smtp::{SmtpClient, SmtpConfig, SmtpRepository};

/// GET /api/smtp/modes - Get available SMTP modes
pub async fn get_smtp_modes() -> (StatusCode, Json<SmtpModesResponse>) {
    let modes = [
        SmtpMode::PlainNoauth,
        SmtpMode::PlainAuth,
        SmtpMode::StarttlsNoauth,
        SmtpMode::StarttlsAuth,
        SmtpMode::StarttlsRequired,
        SmtpMode::SmtpsNoauth,
        SmtpMode::SmtpsAuth,
    ];

    let mode_options: Vec<SmtpModeOption> = modes
        .iter()
        .map(|m| SmtpModeOption {
            value: m.to_string(),
            label: m.display_name().to_string(),
            default_port: m.default_port(),
            requires_auth: m.requires_auth(),
        })
        .collect();

    (
        StatusCode::OK,
        Json(SmtpModesResponse {
            modes: mode_options,
        }),
    )
}

/// GET /api/smtp/config - Get current SMTP configuration
pub async fn get_smtp_config(
    State(_state): State<ApiState>,
) -> (StatusCode, Json<serde_json::Value>) {
    #[cfg(not(feature = "database"))]
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Database feature not enabled"
            })),
        );
    }

    #[cfg(feature = "database")]
    {
        let Some(ref session_store) = _state.session_store else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Database not configured"
                })),
            );
        };

        let smtp_encryption_key = _state.config_snapshot.sessions.smtp_encryption_key.clone();
        let repo = SmtpRepository::new(session_store.clone(), smtp_encryption_key);

        match repo.get_config().await {
            Ok(config) => {
                let response = SmtpConfigResponse {
                    enabled: config.enabled,
                    mode: config.mode.to_string(),
                    host: config.host,
                    port: config.port,
                    from_address: config.from_address,
                    from_name: config.from_name,
                    username: config.username,
                    has_password: config.has_password,
                    notify_recipients: config.notify_recipients,
                    notify_critical: config.notify_critical,
                    notify_security: config.notify_security,
                    notify_config_changes: config.notify_config_changes,
                    notify_service_status: config.notify_service_status,
                    notify_resource_pressure: config.notify_resource_pressure,
                    notify_connection_pressure: config.notify_connection_pressure,
                    notify_cooldown_seconds: config.notify_cooldown_seconds,
                    notify_cpu_threshold: config.notify_cpu_threshold,
                    notify_ram_threshold: config.notify_ram_threshold,
                    notify_disk_threshold: config.notify_disk_threshold,
                    notify_connection_percent_threshold: config.notify_connection_percent_threshold,
                };
                (
                    StatusCode::OK,
                    Json(serde_json::to_value(response).unwrap()),
                )
            }
            Err(e) => {
                error!("Failed to get SMTP config: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
            }
        }
    }
}

/// PUT /api/smtp/config - Update SMTP configuration
pub async fn update_smtp_config(
    State(_state): State<ApiState>,
    Json(_request): Json<SmtpConfigUpdateRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    #[cfg(not(feature = "database"))]
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Database feature not enabled"
            })),
        );
    }

    #[cfg(feature = "database")]
    {
        let Some(ref session_store) = _state.session_store else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Database not configured"
                })),
            );
        };

        let smtp_encryption_key = _state.config_snapshot.sessions.smtp_encryption_key.clone();
        let repo = SmtpRepository::new(session_store.clone(), smtp_encryption_key);

        let mode: SmtpMode = match _request.mode.parse() {
            Ok(m) => m,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                );
            }
        };

        let config = SmtpConfig {
            enabled: _request.enabled,
            mode,
            host: _request.host,
            port: _request.port,
            from_address: _request.from_address,
            from_name: _request.from_name,
            username: _request.username,
            password: None,
            has_password: false,
            notify_recipients: _request.notify_recipients,
            notify_critical: _request.notify_critical,
            notify_security: _request.notify_security,
            notify_config_changes: _request.notify_config_changes,
            notify_service_status: _request.notify_service_status,
            notify_resource_pressure: _request.notify_resource_pressure,
            notify_connection_pressure: _request.notify_connection_pressure,
            notify_cooldown_seconds: _request.notify_cooldown_seconds,
            notify_cpu_threshold: _request.notify_cpu_threshold,
            notify_ram_threshold: _request.notify_ram_threshold,
            notify_disk_threshold: _request.notify_disk_threshold,
            notify_connection_percent_threshold: _request.notify_connection_percent_threshold,
        };

        match repo.save_config(&config, _request.password.as_deref()).await {
            Ok(()) => {
                info!("SMTP configuration updated");
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "success": true,
                        "message": "SMTP configuration saved"
                    })),
                )
            }
            Err(e) => {
                error!("Failed to save SMTP config: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
            }
        }
    }
}

/// POST /api/smtp/test - Send test email
pub async fn test_smtp(
    State(_state): State<ApiState>,
    Json(_request): Json<SmtpTestRequest>,
) -> (StatusCode, Json<SmtpTestResponse>) {
    #[cfg(not(feature = "database"))]
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(SmtpTestResponse {
                success: false,
                message: "Database feature not enabled".to_string(),
                error: Some("Database feature not enabled".to_string()),
            }),
        );
    }

    #[cfg(feature = "database")]
    {
        let Some(ref session_store) = _state.session_store else {
            return (
                StatusCode::BAD_REQUEST,
                Json(SmtpTestResponse {
                    success: false,
                    message: "Database not configured".to_string(),
                    error: Some("Database not configured".to_string()),
                }),
            );
        };

        let smtp_encryption_key = _state.config_snapshot.sessions.smtp_encryption_key.clone();
        let repo = SmtpRepository::new(session_store.clone(), smtp_encryption_key);

        let config = match repo.get_config().await {
            Ok(c) => c,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(SmtpTestResponse {
                        success: false,
                        message: "Failed to load SMTP configuration".to_string(),
                        error: Some(e),
                    }),
                );
            }
        };

        let client = SmtpClient::new(config);

        match client.send_test(&_request.recipient).await {
            Ok(()) => (
                StatusCode::OK,
                Json(SmtpTestResponse {
                    success: true,
                    message: format!("Test email sent successfully to {}", _request.recipient),
                    error: None,
                }),
            ),
            Err(e) => (
                StatusCode::OK,
                Json(SmtpTestResponse {
                    success: false,
                    message: "Failed to send test email".to_string(),
                    error: Some(e),
                }),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::server::pool::{ConnectionPool, PoolConfig};
    use crate::session::SessionManager;
    use std::sync::Arc;

    fn test_state() -> ApiState {
        ApiState {
            session_manager: Arc::new(SessionManager::new()),
            acl_engine: None,
            acl_config_path: None,
            connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
            start_time: std::time::Instant::now(),
            #[cfg(feature = "database")]
            session_store: None,
            metrics_history: None,
            telemetry_history: None,
            config_path: None,
            config_snapshot: Arc::new(Config::default()),
            original_args: Arc::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn smtp_modes_exposes_supported_values() {
        let (status, Json(response)) = get_smtp_modes().await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(response.modes.len(), 7);
        assert!(response
            .modes
            .iter()
            .any(|mode| mode.value == "starttls_auth" && mode.default_port == 587));
        assert!(response
            .modes
            .iter()
            .any(|mode| mode.value == "smtps_auth" && mode.requires_auth));
    }

    #[tokio::test]
    async fn smtp_handlers_reject_missing_database_configuration() {
        #[cfg(feature = "database")]
        let expected_error = "Database not configured";
        #[cfg(not(feature = "database"))]
        let expected_error = "Database feature not enabled";

        let (status, body) = get_smtp_config(State(test_state())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.0["error"], expected_error);

        let (status, body) = update_smtp_config(
            State(test_state()),
            Json(SmtpConfigUpdateRequest {
                enabled: true,
                mode: "starttls_auth".to_string(),
                host: "smtp.example.com".to_string(),
                port: 587,
                from_address: "noreply@example.com".to_string(),
                from_name: None,
                username: None,
                password: None,
                notify_recipients: vec![],
                notify_critical: false,
                notify_security: false,
                notify_config_changes: false,
                notify_service_status: false,
                notify_resource_pressure: false,
                notify_connection_pressure: false,
                notify_cooldown_seconds: 60,
                notify_cpu_threshold: 80,
                notify_ram_threshold: 80,
                notify_disk_threshold: 90,
                notify_connection_percent_threshold: 85,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.0["error"], expected_error);

        let (status, Json(response)) = test_smtp(
            State(test_state()),
            Json(SmtpTestRequest {
                recipient: "ops@example.com".to_string(),
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!response.success);
        assert_eq!(response.error.as_deref(), Some(expected_error));
    }

    #[cfg(feature = "database")]
    #[tokio::test]
    async fn update_smtp_config_validates_mode_before_save() {
        let store = Arc::new(
            crate::session::SessionStore::connect("sqlite::memory:")
                .await
                .expect("store"),
        );
        let mut config = Config::default();
        config.sessions.api_token = Some("token".to_string());

        let state = ApiState {
            session_manager: Arc::new(SessionManager::new()),
            acl_engine: None,
            acl_config_path: None,
            connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
            start_time: std::time::Instant::now(),
            session_store: Some(store),
            metrics_history: None,
            telemetry_history: None,
            config_path: None,
            config_snapshot: Arc::new(config),
            original_args: Arc::new(Vec::new()),
        };

        let (status, body) = update_smtp_config(
            State(state),
            Json(SmtpConfigUpdateRequest {
                enabled: true,
                mode: "invalid_mode".to_string(),
                host: "smtp.example.com".to_string(),
                port: 587,
                from_address: "noreply@example.com".to_string(),
                from_name: None,
                username: None,
                password: None,
                notify_recipients: vec!["ops@example.com".to_string()],
                notify_critical: false,
                notify_security: false,
                notify_config_changes: false,
                notify_service_status: false,
                notify_resource_pressure: false,
                notify_connection_pressure: false,
                notify_cooldown_seconds: 60,
                notify_cpu_threshold: 80,
                notify_ram_threshold: 80,
                notify_disk_threshold: 90,
                notify_connection_percent_threshold: 85,
            }),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.0["error"]
            .as_str()
            .expect("error string")
            .contains("Unknown SMTP mode"));
    }
}
