use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

use crate::session::SessionManager;
use crate::api::handlers::sessions::ApiState;
use crate::api::handlers::{
    sessions::{get_active_sessions, get_session_detail, get_session_history, get_session_stats, get_user_sessions},
    management::{health_check, reload_acl, get_acl_rules, test_acl_decision, get_metrics},
};
use crate::api::types::ApiConfig;

/// Start the REST API server
pub async fn start_api_server(
    config: ApiConfig,
    session_manager: Arc<SessionManager>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !config.enable_api {
        info!("API server disabled");
        return Ok(());
    }

    let state = ApiState { session_manager };

    // Build router with all endpoints
    let app = Router::new()
        // Health and metrics
        .route("/health", get(health_check))
        .route("/metrics", get(get_metrics))

        // Session endpoints
        .route("/api/sessions/active", get(get_active_sessions))
        .route("/api/sessions/history", get(get_session_history))
        .route("/api/sessions/stats", get(get_session_stats))
        .route("/api/sessions/:id", get(get_session_detail))
        .route("/api/users/:user/sessions", get(get_user_sessions))

        // Management endpoints
        .route("/api/admin/reload-acl", post(reload_acl))
        .route("/api/acl/rules", get(get_acl_rules))
        .route("/api/acl/test", post(test_acl_decision))

        // Layer with state and body limit
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1MB max body
        .with_state(state);

    // Bind and listen
    let addr: SocketAddr = format!("{}:{}", config.bind_address, config.bind_port)
        .parse()
        .map_err(|e| format!("Invalid bind address: {}", e))?;

    let listener = TcpListener::bind(&addr).await?;
    info!("API server listening on http://{}", addr);

    // Run server
    let server = axum::serve(listener, app);
    server.await?;

    Ok(())
}
