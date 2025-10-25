use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::api::types::{AclTestRequest, AclTestResponse, HealthResponse};
use crate::api::handlers::sessions::ApiState;

/// GET /health - Health check endpoint
pub async fn health_check(
    State(_state): State<ApiState>,
) -> (StatusCode, Json<HealthResponse>) {
    // TODO: Get actual uptime from application start time
    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: 0,
    };

    (StatusCode::OK, Json(response))
}

#[derive(Serialize, Deserialize)]
pub struct ReloadResponse {
    pub success: bool,
    pub message: String,
}

/// POST /api/admin/reload-acl - Reload ACL configuration
pub async fn reload_acl(
    State(_state): State<ApiState>,
) -> (StatusCode, Json<ReloadResponse>) {
    // TODO: Implement ACL reload through AclWatcher
    let response = ReloadResponse {
        success: true,
        message: "ACL reloaded successfully".to_string(),
    };

    (StatusCode::OK, Json(response))
}

#[derive(Serialize, Deserialize)]
pub struct AclRulesResponse {
    pub rules: Vec<String>,
    pub default_policy: String,
}

/// GET /api/acl/rules - Get current ACL rules
pub async fn get_acl_rules(
    State(_state): State<ApiState>,
) -> (StatusCode, Json<AclRulesResponse>) {
    // TODO: Return current ACL configuration
    let response = AclRulesResponse {
        rules: vec![],
        default_policy: "allow".to_string(),
    };

    (StatusCode::OK, Json(response))
}

/// POST /api/acl/test - Test ACL decision for a connection
pub async fn test_acl_decision(
    State(_state): State<ApiState>,
    Json(request): Json<AclTestRequest>,
) -> (StatusCode, Json<AclTestResponse>) {
    // TODO: Evaluate ACL decision using ACL engine
    let response = AclTestResponse {
        user: request.user,
        destination: request.destination,
        port: request.port,
        protocol: request.protocol,
        decision: "allow".to_string(),
        matched_rule: None,
    };

    (StatusCode::OK, Json(response))
}

/// GET /metrics - Prometheus metrics endpoint
pub async fn get_metrics(
    State(state): State<ApiState>,
) -> (StatusCode, String) {
    let sessions = state.session_manager.get_all_sessions().await;

    let active_count = sessions.iter().filter(|s| s.status.as_str() == "active").count();
    let total_sessions = sessions.len();
    let total_bytes_sent: u64 = sessions.iter().map(|s| s.bytes_sent).sum();
    let total_bytes_received: u64 = sessions.iter().map(|s| s.bytes_received).sum();

    let metrics = format!(
        "# HELP rustsocks_active_sessions Active sessions\n\
         # TYPE rustsocks_active_sessions gauge\n\
         rustsocks_active_sessions {}\n\
         # HELP rustsocks_sessions_total Total sessions\n\
         # TYPE rustsocks_sessions_total counter\n\
         rustsocks_sessions_total {}\n\
         # HELP rustsocks_bytes_sent_total Total bytes sent\n\
         # TYPE rustsocks_bytes_sent_total counter\n\
         rustsocks_bytes_sent_total {}\n\
         # HELP rustsocks_bytes_received_total Total bytes received\n\
         # TYPE rustsocks_bytes_received_total counter\n\
         rustsocks_bytes_received_total {}\n",
        active_count, total_sessions, total_bytes_sent, total_bytes_received
    );

    (StatusCode::OK, metrics)
}
