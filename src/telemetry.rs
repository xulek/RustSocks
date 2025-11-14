use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Severity level of telemetry events.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TelemetrySeverity {
    Info,
    Warning,
    Error,
}

/// Single telemetry event describing an operational observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub timestamp: DateTime<Utc>,
    pub severity: TelemetrySeverity,
    pub category: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// In-memory history of telemetry events that can be queried by the API or UI.
#[derive(Debug, Clone)]
pub struct TelemetryHistory {
    events: Arc<RwLock<VecDeque<TelemetryEvent>>>,
    max_events: usize,
    max_age: ChronoDuration,
}

impl TelemetryHistory {
    /// Create a new telemetry history buffer.
    pub fn new(max_events: usize, retention_hours: u64) -> Self {
        Self {
            events: Arc::new(RwLock::new(VecDeque::with_capacity(max_events.max(1)))),
            max_events: max_events.max(1),
            max_age: ChronoDuration::hours(retention_hours as i64),
        }
    }

    /// Append an event to the history, trimming by age and size.
    pub async fn add_event(&self, event: TelemetryEvent) {
        let mut events = self.events.write().await;

        // Drop expired events first.
        let cutoff = Utc::now() - self.max_age;
        while let Some(front) = events.front() {
            if front.timestamp < cutoff {
                events.pop_front();
            } else {
                break;
            }
        }

        events.push_back(event);

        while events.len() > self.max_events {
            events.pop_front();
        }
    }

    /// Convenience helper that fills in the timestamp for you.
    pub async fn record_event(
        &self,
        severity: TelemetrySeverity,
        category: impl Into<String>,
        message: impl Into<String>,
        details: Option<Value>,
    ) {
        let event = TelemetryEvent {
            timestamp: Utc::now(),
            severity,
            category: category.into(),
            message: message.into(),
            details,
        };
        self.add_event(event).await;
    }

    /// Return all retained events.
    pub async fn get_events(&self) -> Vec<TelemetryEvent> {
        let events = self.events.read().await;
        events.iter().cloned().collect()
    }

    /// Return events recorded within the last `minutes`.
    pub async fn get_events_since(&self, minutes: i64) -> Vec<TelemetryEvent> {
        let events = self.events.read().await;
        let cutoff = Utc::now() - ChronoDuration::minutes(minutes);

        events
            .iter()
            .filter(|event| event.timestamp >= cutoff)
            .cloned()
            .collect()
    }
}
