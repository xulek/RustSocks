use crate::session::{SessionManager, SessionStats};
use crate::utils::error::Result;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::result::Result as StdResult;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{error, info};

#[derive(Clone)]
struct StatsState {
    manager: Arc<SessionManager>,
    default_window: Duration,
}

#[derive(Debug, Default, Deserialize)]
struct StatsQuery {
    window_hours: Option<u64>,
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

fn build_router(state: StatsState) -> Router {
    Router::new()
        .route("/stats", get(handle_get_stats))
        .with_state(state)
}

async fn handle_get_stats(
    State(state): State<StatsState>,
    Query(query): Query<StatsQuery>,
) -> StdResult<Json<SessionStats>, (StatusCode, String)> {
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
    Ok(Json(stats))
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

        let Json(stats) = handle_get_stats(
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

        let error = handle_get_stats(
            ExtractState(state),
            Query(StatsQuery {
                window_hours: Some(0),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
    }
}
