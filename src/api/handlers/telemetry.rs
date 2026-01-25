use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;

use crate::api::handlers::sessions::ApiState;
use crate::api::types::{
    ActiveAlert, AlertHistoryItem, AlertThreshold, ConnectionMetrics, ErrorDestinationBreakdown,
    ErrorTypeBreakdown, LatencyMetrics, PoolMetrics, RecentError, SystemMetrics,
    TelemetryAlertsResponse, TelemetryErrorsResponse, TelemetryMetricsResponse,
    TelemetryQueryParams, ThroughputMetrics, UpdateThresholdsRequest, UpdateThresholdsResponse,
};
use crate::telemetry::TelemetryEvent;
use crate::telemetry::TelemetrySeverity;

/// Query parameters for telemetry list endpoint.
#[derive(Debug, Deserialize)]
pub struct TelemetryEventsQueryParams {
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

/// GET /api/telemetry/events - Get raw telemetry events (Logs tab)
pub async fn get_telemetry_events(
    State(state): State<ApiState>,
    Query(params): Query<TelemetryEventsQueryParams>,
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

/// GET /api/telemetry/metrics - Get aggregated metrics (Metrics tab)
pub async fn get_telemetry_metrics(
    State(state): State<ApiState>,
    Query(params): Query<TelemetryQueryParams>,
) -> (StatusCode, Json<TelemetryMetricsResponse>) {
    let minutes = params.minutes;

    // Get session stats
    let all_sessions = state.session_manager.get_all_sessions().await;
    let active_sessions = all_sessions
        .iter()
        .filter(|s| s.status.as_str() == "active")
        .count() as u64;
    let failed_sessions = all_sessions
        .iter()
        .filter(|s| s.status.as_str() == "failed")
        .count() as u64;
    let total_sessions = all_sessions.len() as u64;

    let success_rate = if total_sessions > 0 {
        ((total_sessions - failed_sessions) as f64 / total_sessions as f64) * 100.0
    } else {
        100.0
    };

    // Calculate throughput
    let total_bytes_sent: u64 = all_sessions.iter().map(|s| s.bytes_sent).sum();
    let total_bytes_received: u64 = all_sessions.iter().map(|s| s.bytes_received).sum();

    // Calculate connect latency from session setup times (DNS + TCP connect)
    let latencies: Vec<f64> = all_sessions
        .iter()
        .filter_map(|s| s.connect_latency_ms)
        .map(|latency| latency as f64)
        .collect();

    let latency = if !latencies.is_empty() {
        let mut sorted = latencies.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
        let p50 = percentile(&sorted, 50.0);
        let p95 = percentile(&sorted, 95.0);
        let p99 = percentile(&sorted, 99.0);
        let max = sorted.last().copied().unwrap_or(0.0);

        LatencyMetrics {
            avg_ms: avg,
            p50_ms: p50,
            p95_ms: p95,
            p99_ms: p99,
            max_ms: max,
        }
    } else {
        LatencyMetrics {
            avg_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            max_ms: 0.0,
        }
    };

    // Get pool stats
    let pool_stats = state.connection_pool.stats();
    let pool_total = pool_stats.pool_hits + pool_stats.pool_misses;
    let pool_hit_rate = if pool_total > 0 {
        (pool_stats.pool_hits as f64 / pool_total as f64) * 100.0
    } else {
        0.0
    };

    // Calculate bytes per second (rough estimate based on active sessions)
    let bytes_per_second = if active_sessions > 0 {
        (total_bytes_sent + total_bytes_received) as f64 / (minutes as f64 * 60.0)
    } else {
        0.0
    };

    // Try to get system metrics
    let system = get_system_metrics();

    let response = TelemetryMetricsResponse {
        timestamp: Utc::now().to_rfc3339(),
        period_minutes: minutes,
        connections: ConnectionMetrics {
            total: total_sessions,
            active: active_sessions,
            failed: failed_sessions,
            success_rate,
        },
        latency,
        throughput: ThroughputMetrics {
            bytes_sent: total_bytes_sent,
            bytes_received: total_bytes_received,
            bytes_per_second,
        },
        pool: PoolMetrics {
            hits: pool_stats.pool_hits,
            misses: pool_stats.pool_misses,
            hit_rate: pool_hit_rate,
            idle_connections: pool_stats.total_idle as u64,
            in_use_connections: pool_stats.connections_in_use,
        },
        system,
    };

    (StatusCode::OK, Json(response))
}

/// GET /api/telemetry/errors - Get error breakdown (Errors tab)
pub async fn get_telemetry_errors(
    State(state): State<ApiState>,
    Query(params): Query<TelemetryQueryParams>,
) -> (StatusCode, Json<TelemetryErrorsResponse>) {
    let minutes = params.minutes as i64;
    let limit = params.limit.unwrap_or(50);

    // Get error events from telemetry
    let events = if let Some(history) = state.telemetry_history.as_ref() {
        history.get_events_since(minutes).await
    } else {
        Vec::new()
    };

    // Filter to errors and warnings only
    let error_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.severity, TelemetrySeverity::Error | TelemetrySeverity::Warning))
        .collect();

    let total_errors = error_events.len() as u64;

    // Get total events for error rate calculation
    let all_events = if let Some(history) = state.telemetry_history.as_ref() {
        history.get_events_since(minutes).await.len() as u64
    } else {
        0
    };

    let error_rate = if all_events > 0 {
        (total_errors as f64 / all_events as f64) * 100.0
    } else {
        0.0
    };

    // Group by error type (category)
    let mut type_counts: HashMap<String, u64> = HashMap::new();
    for event in &error_events {
        *type_counts.entry(event.category.clone()).or_insert(0) += 1;
    }

    let by_type: Vec<ErrorTypeBreakdown> = type_counts
        .into_iter()
        .map(|(error_type, count)| {
            let percentage = if total_errors > 0 {
                (count as f64 / total_errors as f64) * 100.0
            } else {
                0.0
            };
            ErrorTypeBreakdown {
                error_type,
                count,
                percentage,
            }
        })
        .collect();

    // Group by destination (from details)
    let mut dest_errors: HashMap<String, (u64, Option<String>)> = HashMap::new();
    for event in &error_events {
        if let Some(details) = &event.details {
            if let Some(dest) = details.get("destination").and_then(|d| d.as_str()) {
                let entry = dest_errors
                    .entry(dest.to_string())
                    .or_insert((0, None));
                entry.0 += 1;
                entry.1 = Some(event.message.clone());
            }
        }
    }

    let mut by_destination: Vec<ErrorDestinationBreakdown> = dest_errors
        .into_iter()
        .map(|(destination, (error_count, last_error))| ErrorDestinationBreakdown {
            destination,
            error_count,
            last_error,
        })
        .collect();
    by_destination.sort_by(|a, b| b.error_count.cmp(&a.error_count));
    by_destination.truncate(10);

    // Recent errors
    let recent_errors: Vec<RecentError> = error_events
        .into_iter()
        .take(limit)
        .map(|e| {
            let destination = e
                .details
                .as_ref()
                .and_then(|d| d.get("destination"))
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());

            RecentError {
                timestamp: e.timestamp.to_rfc3339(),
                error_type: e.category.clone(),
                message: e.message.clone(),
                destination,
                details: e.details.clone(),
            }
        })
        .collect();

    let response = TelemetryErrorsResponse {
        total_errors,
        error_rate,
        by_type,
        by_destination,
        recent_errors,
    };

    (StatusCode::OK, Json(response))
}

/// GET /api/telemetry/alerts - Get active alerts and history (Alerts tab)
pub async fn get_telemetry_alerts(
    State(state): State<ApiState>,
    Query(params): Query<TelemetryQueryParams>,
) -> (StatusCode, Json<TelemetryAlertsResponse>) {
    // Get current metrics for threshold checking
    let metrics_response = get_telemetry_metrics(State(state.clone()), Query(params.clone())).await;
    let metrics = metrics_response.1 .0;

    // Default thresholds (in production, these would come from database)
    let thresholds = get_default_thresholds();

    // Check current values against thresholds
    let mut active_alerts = Vec::new();
    let now = Utc::now();

    // Check latency thresholds
    check_threshold(
        &thresholds,
        "avg_latency_ms",
        metrics.latency.avg_ms,
        &mut active_alerts,
        &now,
    );
    check_threshold(
        &thresholds,
        "p95_latency_ms",
        metrics.latency.p95_ms,
        &mut active_alerts,
        &now,
    );
    check_threshold(
        &thresholds,
        "p99_latency_ms",
        metrics.latency.p99_ms,
        &mut active_alerts,
        &now,
    );

    // Check error rate
    let error_rate = if metrics.connections.total > 0 {
        (metrics.connections.failed as f64 / metrics.connections.total as f64) * 100.0
    } else {
        0.0
    };
    check_threshold(&thresholds, "error_rate", error_rate, &mut active_alerts, &now);

    // Check pool hit rate (inverted - lower is worse)
    check_threshold(
        &thresholds,
        "pool_hit_rate",
        metrics.pool.hit_rate,
        &mut active_alerts,
        &now,
    );

    // Check system metrics if available
    if let Some(system) = &metrics.system {
        check_threshold(
            &thresholds,
            "memory_usage_percent",
            system.memory_usage_percent,
            &mut active_alerts,
            &now,
        );
        check_threshold(
            &thresholds,
            "cpu_usage_percent",
            system.cpu_usage_percent,
            &mut active_alerts,
            &now,
        );
    }

    // Check connection utilization
    let max_connections = state.config_snapshot.server.max_connections as f64;
    let connection_percent = if max_connections > 0.0 {
        (metrics.connections.active as f64 / max_connections) * 100.0
    } else {
        0.0
    };
    check_threshold(
        &thresholds,
        "active_connections_percent",
        connection_percent,
        &mut active_alerts,
        &now,
    );

    // For now, alert history is empty (would come from database in production)
    let alert_history: Vec<AlertHistoryItem> = Vec::new();

    let response = TelemetryAlertsResponse {
        active_alerts,
        alert_history,
        thresholds,
    };

    (StatusCode::OK, Json(response))
}

/// GET /api/telemetry/alerts/config - Get alert threshold configuration
pub async fn get_alert_thresholds(
    State(_state): State<ApiState>,
) -> (StatusCode, Json<Vec<AlertThreshold>>) {
    let thresholds = get_default_thresholds();
    (StatusCode::OK, Json(thresholds))
}

/// PUT /api/telemetry/alerts/config - Update alert thresholds
pub async fn update_alert_thresholds(
    State(_state): State<ApiState>,
    Json(request): Json<UpdateThresholdsRequest>,
) -> (StatusCode, Json<UpdateThresholdsResponse>) {
    // In production, this would persist to database
    // For now, just acknowledge the request
    let updated_count = request.thresholds.len();

    let response = UpdateThresholdsResponse {
        success: true,
        message: format!("Updated {} threshold(s)", updated_count),
        updated_count,
    };

    (StatusCode::OK, Json(response))
}

/// POST /api/telemetry/alerts/{id}/acknowledge - Acknowledge an alert
pub async fn acknowledge_alert(
    State(_state): State<ApiState>,
    Path(alert_id): Path<i64>,
) -> (StatusCode, Json<serde_json::Value>) {
    // In production, this would update the database
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "message": format!("Alert {} acknowledged", alert_id)
        })),
    )
}

// ============================================================================
// Helper Functions
// ============================================================================

fn percentile(sorted_data: &[f64], p: f64) -> f64 {
    if sorted_data.is_empty() {
        return 0.0;
    }
    let idx = (p / 100.0 * (sorted_data.len() - 1) as f64).round() as usize;
    sorted_data[idx.min(sorted_data.len() - 1)]
}

fn get_system_metrics() -> Option<SystemMetrics> {
    // Try to read from /proc on Linux
    #[cfg(target_os = "linux")]
    {
        use std::fs;

        let memory_info = fs::read_to_string("/proc/meminfo").ok()?;
        let mut mem_total: u64 = 0;
        let mut mem_available: u64 = 0;

        for line in memory_info.lines() {
            if line.starts_with("MemTotal:") {
                mem_total = parse_meminfo_value(line);
            } else if line.starts_with("MemAvailable:") {
                mem_available = parse_meminfo_value(line);
            }
        }

        let memory_usage_bytes = mem_total.saturating_sub(mem_available);
        let memory_usage_percent = if mem_total > 0 {
            (memory_usage_bytes as f64 / mem_total as f64) * 100.0
        } else {
            0.0
        };

        // CPU usage would require sampling over time, return 0 for now
        let cpu_usage_percent = 0.0;

        // Get open file descriptors for current process
        let fd_count = fs::read_dir("/proc/self/fd").ok()?.count() as u64;

        Some(SystemMetrics {
            memory_usage_bytes,
            memory_usage_percent,
            cpu_usage_percent,
            open_file_descriptors: Some(fd_count),
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn parse_meminfo_value(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .map(|kb| kb * 1024) // Convert from kB to bytes
        .unwrap_or(0)
}

fn get_default_thresholds() -> Vec<AlertThreshold> {
    vec![
        // Performance thresholds
        AlertThreshold {
            metric_name: "avg_latency_ms".to_string(),
            display_name: "Average Connect Latency".to_string(),
            category: "performance".to_string(),
            warning_threshold: Some(100.0),
            error_threshold: Some(500.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("ms".to_string()),
            description: Some("Average connect latency (DNS + TCP setup)".to_string()),
        },
        AlertThreshold {
            metric_name: "p95_latency_ms".to_string(),
            display_name: "P95 Connect Latency".to_string(),
            category: "performance".to_string(),
            warning_threshold: Some(200.0),
            error_threshold: Some(1000.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("ms".to_string()),
            description: Some("95th percentile connect latency".to_string()),
        },
        AlertThreshold {
            metric_name: "p99_latency_ms".to_string(),
            display_name: "P99 Connect Latency".to_string(),
            category: "performance".to_string(),
            warning_threshold: Some(500.0),
            error_threshold: Some(2000.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("ms".to_string()),
            description: Some("99th percentile connect latency".to_string()),
        },
        // Error thresholds
        AlertThreshold {
            metric_name: "error_rate".to_string(),
            display_name: "Error Rate".to_string(),
            category: "errors".to_string(),
            warning_threshold: Some(5.0),
            error_threshold: Some(15.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("%".to_string()),
            description: Some("Percentage of failed connections".to_string()),
        },
        AlertThreshold {
            metric_name: "auth_failure_rate".to_string(),
            display_name: "Auth Failure Rate".to_string(),
            category: "errors".to_string(),
            warning_threshold: Some(10.0),
            error_threshold: Some(25.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("%".to_string()),
            description: Some("Authentication failure rate".to_string()),
        },
        AlertThreshold {
            metric_name: "timeout_rate".to_string(),
            display_name: "Timeout Rate".to_string(),
            category: "errors".to_string(),
            warning_threshold: Some(5.0),
            error_threshold: Some(10.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("%".to_string()),
            description: Some("Connection timeout rate".to_string()),
        },
        // Resource thresholds
        AlertThreshold {
            metric_name: "memory_usage_percent".to_string(),
            display_name: "Memory Usage".to_string(),
            category: "resources".to_string(),
            warning_threshold: Some(70.0),
            error_threshold: Some(90.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("%".to_string()),
            description: Some("Memory usage percentage".to_string()),
        },
        AlertThreshold {
            metric_name: "cpu_usage_percent".to_string(),
            display_name: "CPU Usage".to_string(),
            category: "resources".to_string(),
            warning_threshold: Some(70.0),
            error_threshold: Some(90.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("%".to_string()),
            description: Some("CPU usage percentage".to_string()),
        },
        AlertThreshold {
            metric_name: "fd_usage_percent".to_string(),
            display_name: "File Descriptors".to_string(),
            category: "resources".to_string(),
            warning_threshold: Some(70.0),
            error_threshold: Some(90.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("%".to_string()),
            description: Some("File descriptor usage percentage".to_string()),
        },
        // Limit thresholds
        AlertThreshold {
            metric_name: "active_connections_percent".to_string(),
            display_name: "Active Connections".to_string(),
            category: "limits".to_string(),
            warning_threshold: Some(70.0),
            error_threshold: Some(90.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("%".to_string()),
            description: Some("Percentage of max connections in use".to_string()),
        },
        AlertThreshold {
            metric_name: "pool_utilization".to_string(),
            display_name: "Pool Utilization".to_string(),
            category: "limits".to_string(),
            warning_threshold: Some(80.0),
            error_threshold: Some(95.0),
            comparison: "gt".to_string(),
            enabled: true,
            unit: Some("%".to_string()),
            description: Some("Connection pool utilization".to_string()),
        },
        AlertThreshold {
            metric_name: "pool_hit_rate".to_string(),
            display_name: "Pool Hit Rate".to_string(),
            category: "limits".to_string(),
            warning_threshold: Some(50.0),
            error_threshold: Some(30.0),
            comparison: "lt".to_string(),
            enabled: true,
            unit: Some("%".to_string()),
            description: Some("Connection pool hit rate (lower is worse)".to_string()),
        },
    ]
}

fn check_threshold(
    thresholds: &[AlertThreshold],
    metric_name: &str,
    current_value: f64,
    active_alerts: &mut Vec<ActiveAlert>,
    now: &chrono::DateTime<Utc>,
) {
    let Some(threshold) = thresholds.iter().find(|t| t.metric_name == metric_name && t.enabled)
    else {
        return;
    };

    let is_lt = threshold.comparison == "lt" || threshold.comparison == "lte";

    // Check error threshold first
    if let Some(error_thresh) = threshold.error_threshold {
        let triggered = if is_lt {
            current_value < error_thresh
        } else {
            current_value > error_thresh
        };

        if triggered {
            active_alerts.push(ActiveAlert {
                id: active_alerts.len() as i64 + 1,
                metric_name: metric_name.to_string(),
                display_name: threshold.display_name.clone(),
                severity: "error".to_string(),
                current_value,
                threshold_value: error_thresh,
                message: format!(
                    "{} is {} (threshold: {}{})",
                    threshold.display_name,
                    format_value(current_value, threshold.unit.as_deref()),
                    format_value(error_thresh, threshold.unit.as_deref()),
                    if is_lt { " min" } else { " max" }
                ),
                triggered_at: now.to_rfc3339(),
                duration_seconds: 0,
            });
            return;
        }
    }

    // Check warning threshold
    if let Some(warn_thresh) = threshold.warning_threshold {
        let triggered = if is_lt {
            current_value < warn_thresh
        } else {
            current_value > warn_thresh
        };

        if triggered {
            active_alerts.push(ActiveAlert {
                id: active_alerts.len() as i64 + 1,
                metric_name: metric_name.to_string(),
                display_name: threshold.display_name.clone(),
                severity: "warning".to_string(),
                current_value,
                threshold_value: warn_thresh,
                message: format!(
                    "{} is {} (threshold: {}{})",
                    threshold.display_name,
                    format_value(current_value, threshold.unit.as_deref()),
                    format_value(warn_thresh, threshold.unit.as_deref()),
                    if is_lt { " min" } else { " max" }
                ),
                triggered_at: now.to_rfc3339(),
                duration_seconds: 0,
            });
        }
    }
}

fn format_value(value: f64, unit: Option<&str>) -> String {
    match unit {
        Some("%") => format!("{:.1}%", value),
        Some("ms") => format!("{:.0}ms", value),
        Some("bytes") => format_bytes(value as u64),
        _ => format!("{:.2}", value),
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
