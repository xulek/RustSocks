use crate::session::SessionManager;
use crate::utils::error::Result;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::result::Result as StdResult;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{error, info};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

#[derive(Clone)]
struct StatsState {
    manager: Arc<SessionManager>,
    default_window: Duration,
}

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
struct StatsQuery {
    /// Time window in hours (default: 24)
    #[serde(default)]
    window_hours: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct HealthResponse {
    status: String,
    version: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct AclDecisionStatsDto {
    allowed: u64,
    blocked: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct UserSessionStatDto {
    user: String,
    sessions: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct DestinationStatDto {
    dest_ip: String,
    connections: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct SessionStatsDto {
    /// Timestamp when stats were generated
    generated_at: String,
    /// Number of currently active sessions
    active_sessions: usize,
    /// Total number of sessions in the time window
    total_sessions: usize,
    /// Total bytes transferred in the time window
    total_bytes: u64,
    /// Top users by session count
    top_users: Vec<UserSessionStatDto>,
    /// Top destinations by connection count
    top_destinations: Vec<DestinationStatDto>,
    /// ACL decision statistics
    acl: AclDecisionStatsDto,
}

pub async fn start_stats_server(
    bind_addr: &str,
    session_manager: Arc<SessionManager>,
    default_window: Duration,
) -> Result<JoinHandle<()>> {
    let listener = TcpListener::bind(bind_addr).await?;
    let local_addr = listener.local_addr()?;

    let state = StatsState {
        manager: session_manager,
        default_window,
    };
    let router = build_router(state);

    info!("Session stats API listening on {}", local_addr);

    let server = axum::serve(listener, router.into_make_service());

    let handle = tokio::spawn(async move {
        if let Err(err) = server.await {
            error!("Session stats API error: {}", err);
        }
    });

    Ok(handle)
}

/// OpenAPI documentation generator
#[derive(OpenApi)]
#[openapi(
    paths(health_check, get_session_stats),
    components(schemas(
        HealthResponse,
        SessionStatsDto,
        UserSessionStatDto,
        DestinationStatDto,
        AclDecisionStatsDto
    ))
)]
struct ApiDoc;

fn build_router(state: StatsState) -> Router {
    let mut openapi = ApiDoc::openapi();

    // Configure OpenAPI info
    openapi.info.title = "RustSocks Session API".into();
    openapi.info.version = env!("CARGO_PKG_VERSION").into();
    openapi.info.description = Some(
        "Session tracking, statistics, and monitoring API for RustSocks SOCKS5 proxy server"
            .into(),
    );

    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", openapi))
        .route("/health", get(health_check))
        .route("/stats", get(get_session_stats))
        .with_state(state)
}

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Server is healthy", body = HealthResponse),
    ),
    tag = "Health"
)]
async fn health_check() -> (StatusCode, Json<HealthResponse>) {
    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    (StatusCode::OK, Json(response))
}

/// Get session statistics
#[utoipa::path(
    get,
    path = "/stats",
    params(StatsQuery),
    responses(
        (status = 200, description = "Session statistics retrieved successfully", body = SessionStatsDto),
        (status = 400, description = "Invalid window_hours parameter"),
        (status = 500, description = "Internal server error"),
    ),
    tag = "Statistics"
)]
async fn get_session_stats(
    State(state): State<StatsState>,
    Query(query): Query<StatsQuery>,
) -> StdResult<Json<SessionStatsDto>, (StatusCode, String)> {
    let lookback = match query.window_hours {
        Some(0) => {
            return Err((
                StatusCode::BAD_REQUEST,
                "window_hours must be greater than 0".to_string(),
            ))
        }
        Some(hours) => {
            let seconds = hours.checked_mul(3600).ok_or((
                StatusCode::BAD_REQUEST,
                "window_hours is too large".to_string(),
            ))?;
            Duration::from_secs(seconds)
        }
        None => state.default_window,
    };

    if lookback.is_zero() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "stats window is misconfigured".to_string(),
        ));
    }

    let stats = state.manager.get_stats(lookback).await;

    // Convert SessionStats to SessionStatsDto
    let dto = SessionStatsDto {
        generated_at: stats.generated_at.to_rfc3339(),
        active_sessions: stats.active_sessions,
        total_sessions: stats.total_sessions,
        total_bytes: stats.total_bytes,
        top_users: stats.top_users.iter().map(|u| UserSessionStatDto {
            user: u.user.clone(),
            sessions: u.sessions,
        }).collect(),
        top_destinations: stats.top_destinations.iter().map(|d| DestinationStatDto {
            dest_ip: d.dest_ip.clone(),
            connections: d.connections,
        }).collect(),
        acl: AclDecisionStatsDto {
            allowed: stats.acl.allowed,
            blocked: stats.acl.blocked,
        },
    };

    Ok(Json(dto))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Query, State as ExtractState};
    use std::time::Duration as StdDuration;

    #[tokio::test]
    async fn stats_endpoint_returns_json() {
        let manager = Arc::new(SessionManager::new());
        let state = StatsState {
            manager,
            default_window: StdDuration::from_secs(24 * 3600),
        };

        let Json(stats) = get_session_stats(
            ExtractState(state.clone()),
            Query(StatsQuery { window_hours: None }),
        )
        .await
        .unwrap();

        assert_eq!(stats.active_sessions, 0);
    }

    #[tokio::test]
    async fn stats_endpoint_rejects_zero_window() {
        let manager = Arc::new(SessionManager::new());
        let state = StatsState {
            manager,
            default_window: StdDuration::from_secs(24 * 3600),
        };

        let error = get_session_stats(
            ExtractState(state),
            Query(StatsQuery {
                window_hours: Some(0),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn health_response_has_version() {
        let response = HealthResponse {
            status: "healthy".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        assert!(!response.version.is_empty());
    }
}
