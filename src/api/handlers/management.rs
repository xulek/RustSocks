use crate::api::handlers::sessions::ApiState;
use crate::api::types::{AclTestRequest, AclTestResponse, HealthResponse};
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

/// GET /health - Health check endpoint
pub async fn health_check(State(_state): State<ApiState>) -> (StatusCode, Json<HealthResponse>) {
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
pub async fn reload_acl(State(state): State<ApiState>) -> (StatusCode, Json<ReloadResponse>) {
    // Check if ACL is enabled
    let Some(ref acl_engine) = state.acl_engine else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ReloadResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
            }),
        );
    };

    let Some(ref config_path) = state.acl_config_path else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ReloadResponse {
                success: false,
                message: "ACL config path not set".to_string(),
            }),
        );
    };

    // Load new config from file
    let new_config = match crate::acl::load_acl_config(config_path).await {
        Ok(config) => config,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ReloadResponse {
                    success: false,
                    message: format!("Failed to load ACL config: {}", e),
                }),
            );
        }
    };

    // Reload ACL engine
    match acl_engine.reload(new_config).await {
        Ok(()) => (
            StatusCode::OK,
            Json(ReloadResponse {
                success: true,
                message: "ACL reloaded successfully".to_string(),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ReloadResponse {
                success: false,
                message: format!("Failed to reload ACL: {}", e),
            }),
        ),
    }
}

#[derive(Serialize, Deserialize)]
pub struct AclRulesResponse {
    pub user_count: usize,
    pub group_count: usize,
    pub message: String,
}

/// GET /api/acl/rules - Get current ACL rules summary
pub async fn get_acl_rules(State(state): State<ApiState>) -> (StatusCode, Json<AclRulesResponse>) {
    let Some(ref acl_engine) = state.acl_engine else {
        return (
            StatusCode::BAD_REQUEST,
            Json(AclRulesResponse {
                user_count: 0,
                group_count: 0,
                message: "ACL is not enabled".to_string(),
            }),
        );
    };

    let user_count = acl_engine.get_user_count().await;
    let group_count = acl_engine.get_group_count().await;

    let response = AclRulesResponse {
        user_count,
        group_count,
        message: format!(
            "ACL has {} users and {} groups configured",
            user_count, group_count
        ),
    };

    (StatusCode::OK, Json(response))
}

/// POST /api/acl/test - Test ACL decision for a connection
pub async fn test_acl_decision(
    State(state): State<ApiState>,
    Json(request): Json<AclTestRequest>,
) -> (StatusCode, Json<AclTestResponse>) {
    let Some(ref acl_engine) = state.acl_engine else {
        return (
            StatusCode::BAD_REQUEST,
            Json(AclTestResponse {
                user: request.user,
                destination: request.destination,
                port: request.port,
                protocol: request.protocol,
                decision: "error".to_string(),
                matched_rule: Some("ACL is not enabled".to_string()),
            }),
        );
    };

    // Parse protocol
    let protocol = match request.protocol.to_lowercase().as_str() {
        "tcp" => crate::acl::Protocol::Tcp,
        "udp" => crate::acl::Protocol::Udp,
        "both" => crate::acl::Protocol::Both,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(AclTestResponse {
                    user: request.user,
                    destination: request.destination,
                    port: request.port,
                    protocol: request.protocol,
                    decision: "error".to_string(),
                    matched_rule: Some("Invalid protocol (use: tcp, udp, or both)".to_string()),
                }),
            );
        }
    };

    // Parse destination as Address (IP or domain)
    let address = match request.destination.parse::<std::net::IpAddr>() {
        Ok(ip) => match ip {
            std::net::IpAddr::V4(ipv4) => crate::protocol::Address::IPv4(ipv4.octets()),
            std::net::IpAddr::V6(ipv6) => crate::protocol::Address::IPv6(ipv6.octets()),
        },
        Err(_) => crate::protocol::Address::Domain(request.destination.clone()),
    };

    // Evaluate ACL
    let (decision, matched_rule) = acl_engine
        .evaluate(&request.user, &address, request.port, &protocol)
        .await;

    // Convert decision to string
    let decision_str = match decision {
        crate::acl::AclDecision::Allow => "allow",
        crate::acl::AclDecision::Block => "block",
    };

    let response = AclTestResponse {
        user: request.user,
        destination: request.destination,
        port: request.port,
        protocol: request.protocol,
        decision: decision_str.to_string(),
        matched_rule,
    };

    (StatusCode::OK, Json(response))
}

/// GET /metrics - Prometheus metrics endpoint
pub async fn get_metrics(State(state): State<ApiState>) -> (StatusCode, String) {
    let sessions = state.session_manager.get_all_sessions().await;

    let active_count = sessions
        .iter()
        .filter(|s| s.status.as_str() == "active")
        .count();
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
