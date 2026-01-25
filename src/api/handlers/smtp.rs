use axum::{extract::State, http::StatusCode, Json};
use tracing::{error, info};

use crate::api::handlers::sessions::ApiState;
use crate::api::types::{
    SmtpConfigResponse, SmtpConfigUpdateRequest, SmtpModeOption, SmtpModesResponse,
    SmtpTestRequest, SmtpTestResponse,
};
use crate::smtp::{SmtpClient, SmtpConfig, SmtpMode};

#[cfg(feature = "database")]
use crate::smtp::SmtpRepository;

/// GET /api/smtp/modes - Get available SMTP modes
pub async fn get_smtp_modes() -> (StatusCode, Json<SmtpModesResponse>) {
    let modes = vec![
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

    (StatusCode::OK, Json(SmtpModesResponse { modes: mode_options }))
}

/// GET /api/smtp/config - Get current SMTP configuration
pub async fn get_smtp_config(
    State(state): State<ApiState>,
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
        let Some(ref session_store) = state.session_store else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Database not configured"
                })),
            );
        };

        let pool = session_store.pool();
        let api_token = state.config_snapshot.sessions.api_token.clone();
        let repo = SmtpRepository::new(pool.clone(), api_token);

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
                (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
    State(state): State<ApiState>,
    Json(request): Json<SmtpConfigUpdateRequest>,
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
        let Some(ref session_store) = state.session_store else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Database not configured"
                })),
            );
        };

        let pool = session_store.pool();
        let api_token = state.config_snapshot.sessions.api_token.clone();
        let repo = SmtpRepository::new(pool.clone(), api_token);

        let mode: SmtpMode = match request.mode.parse() {
            Ok(m) => m,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                );
            }
        };

        let config = SmtpConfig {
            enabled: request.enabled,
            mode,
            host: request.host,
            port: request.port,
            from_address: request.from_address,
            from_name: request.from_name,
            username: request.username,
            password: None,
            has_password: false,
            notify_recipients: request.notify_recipients,
            notify_critical: request.notify_critical,
            notify_security: request.notify_security,
            notify_config_changes: request.notify_config_changes,
            notify_service_status: request.notify_service_status,
            notify_resource_pressure: request.notify_resource_pressure,
            notify_connection_pressure: request.notify_connection_pressure,
            notify_cooldown_seconds: request.notify_cooldown_seconds,
            notify_cpu_threshold: request.notify_cpu_threshold,
            notify_ram_threshold: request.notify_ram_threshold,
            notify_disk_threshold: request.notify_disk_threshold,
            notify_connection_percent_threshold: request.notify_connection_percent_threshold,
        };

        match repo
            .save_config(&config, request.password.as_deref())
            .await
        {
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
    State(state): State<ApiState>,
    Json(request): Json<SmtpTestRequest>,
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
        let Some(ref session_store) = state.session_store else {
            return (
                StatusCode::BAD_REQUEST,
                Json(SmtpTestResponse {
                    success: false,
                    message: "Database not configured".to_string(),
                    error: Some("Database not configured".to_string()),
                }),
            );
        };

        let pool = session_store.pool();
        let api_token = state.config_snapshot.sessions.api_token.clone();
        let repo = SmtpRepository::new(pool.clone(), api_token);

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

        match client.send_test(&request.recipient).await {
            Ok(()) => (
                StatusCode::OK,
                Json(SmtpTestResponse {
                    success: true,
                    message: format!(
                        "Test email sent successfully to {}",
                        request.recipient
                    ),
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
