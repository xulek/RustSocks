use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::api::handlers::sessions::ApiState;
use crate::telemetry::TelemetryEvent;
use crate::telemetry::TelemetrySeverity;

/// Query parameters for telemetry list endpoint.
#[derive(Debug, Deserialize)]
pub struct TelemetryQueryParams {
    #[serde(default)]
    pub minutes: Option<u32>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub severity: Option<TelemetrySeverityFilter>,
    #[serde(default)]
    pub category: Option<String>,
}

/// Helper enum for filtering by severity.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TelemetrySeverityFilter {
    Info,
    Warning,
    Error,
}

impl From<TelemetrySeverityFilter> for TelemetrySeverity {
    fn from(filter: TelemetrySeverityFilter) -> Self {
        match filter {
            TelemetrySeverityFilter::Info => TelemetrySeverity::Info,
            TelemetrySeverityFilter::Warning => TelemetrySeverity::Warning,
            TelemetrySeverityFilter::Error => TelemetrySeverity::Error,
        }
    }
}

/// GET /api/telemetry/events
pub async fn get_telemetry_events(
    State(state): State<ApiState>,
    Query(params): Query<TelemetryQueryParams>,
) -> (StatusCode, Json<Vec<TelemetryEvent>>) {
    let mut events = if let Some(history) = state.telemetry_history.as_ref() {
        if let Some(minutes) = params.minutes {
            history.get_events_since(minutes as i64).await
        } else {
            history.get_events().await
        }
    } else {
        Vec::new()
    };

    if let Some(severity_filter) = params.severity {
        let severity: TelemetrySeverity = severity_filter.into();
        events.retain(|event| event.severity == severity);
    }

    if let Some(category) = params.category {
        let normalized = category.to_lowercase();
        events.retain(|event| event.category.eq_ignore_ascii_case(&normalized));
    }

    events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    let limit = params.limit.unwrap_or(100).clamp(1, 500);
    events.truncate(limit);

    (StatusCode::OK, Json(events))
}
