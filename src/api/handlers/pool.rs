use crate::api::handlers::sessions::ApiState;
use crate::api::types::PoolStatsResponse;
use axum::{extract::State, http::StatusCode, Json};

/// GET /api/pool/stats - connection pooling telemetry snapshot
pub async fn get_pool_stats(
    State(state): State<ApiState>,
) -> (StatusCode, Json<PoolStatsResponse>) {
    let stats = state.connection_pool.stats().await;
    let response = PoolStatsResponse::from(stats);
    (StatusCode::OK, Json(response))
}
