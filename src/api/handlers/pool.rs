use crate::api::handlers::sessions::ApiState;
use crate::api::types::PoolStatsResponse;
use axum::{extract::State, http::StatusCode, Json};

/// GET /api/pool/stats - connection pooling telemetry snapshot
pub async fn get_pool_stats(
    State(state): State<ApiState>,
) -> (StatusCode, Json<PoolStatsResponse>) {
    let stats = state.connection_pool.stats();
    let response = PoolStatsResponse::from(stats);
    (StatusCode::OK, Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::server::pool::{ConnectionPool, PoolConfig};
    use crate::session::SessionManager;
    use std::sync::Arc;

    fn test_state(pool: Arc<ConnectionPool>) -> ApiState {
        ApiState {
            session_manager: Arc::new(SessionManager::new()),
            acl_engine: None,
            acl_config_path: None,
            connection_pool: pool,
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
    async fn returns_pool_stats_snapshot() {
        let pool = Arc::new(ConnectionPool::new(PoolConfig {
            enabled: true,
            max_idle_per_dest: 3,
            max_total_idle: 9,
            idle_timeout_secs: 30,
            connect_timeout_ms: 500,
        }));

        let (status, Json(response)) = get_pool_stats(State(test_state(pool))).await;

        assert_eq!(status, StatusCode::OK);
        assert!(response.enabled);
        assert_eq!(response.total_idle, 0);
        assert_eq!(response.config.max_idle_per_dest, 3);
        assert_eq!(response.config.max_total_idle, 9);
        assert_eq!(response.hit_rate, 0.0);
    }
}
