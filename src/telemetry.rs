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
#[derive(Debug)]
pub struct TelemetryHistory {
    events: Arc<RwLock<VecDeque<TelemetryEvent>>>,
    max_events: usize,
    max_age: ChronoDuration,
    dropped_events: std::sync::atomic::AtomicU64,
}

impl TelemetryHistory {
    /// Create a new telemetry history buffer.
    pub fn new(max_events: usize, retention_hours: u64) -> Self {
        Self {
            events: Arc::new(RwLock::new(VecDeque::with_capacity(max_events.max(1)))),
            max_events: max_events.max(1),
            max_age: ChronoDuration::hours(retention_hours as i64),
            dropped_events: std::sync::atomic::AtomicU64::new(0),
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

        let mut dropped = 0u64;
        while events.len() > self.max_events {
            events.pop_front();
            dropped += 1;
        }
        if dropped > 0 {
            self.dropped_events
                .fetch_add(dropped, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Get count of events dropped due to capacity limits.
    pub fn dropped_event_count(&self) -> u64 {
        self.dropped_events
            .load(std::sync::atomic::Ordering::Relaxed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event_with_offset(minutes_ago: i64, message: &str) -> TelemetryEvent {
        TelemetryEvent {
            timestamp: Utc::now() - ChronoDuration::minutes(minutes_ago),
            severity: TelemetrySeverity::Info,
            category: "tests".to_string(),
            message: message.to_string(),
            details: None,
        }
    }

    #[tokio::test]
    async fn add_event_prunes_expired_entries() {
        let history = TelemetryHistory::new(8, 1);
        history.add_event(event_with_offset(180, "expired")).await;
        history.add_event(event_with_offset(5, "fresh")).await;

        let events = history.get_events().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "fresh");
    }

    #[tokio::test]
    async fn add_event_tracks_capacity_drops() {
        let history = TelemetryHistory::new(2, 24);

        history.add_event(event_with_offset(0, "one")).await;
        history.add_event(event_with_offset(0, "two")).await;
        history.add_event(event_with_offset(0, "three")).await;

        let events = history.get_events().await;
        let messages: Vec<_> = events.into_iter().map(|event| event.message).collect();

        assert_eq!(messages, vec!["two".to_string(), "three".to_string()]);
        assert_eq!(history.dropped_event_count(), 1);
    }

    #[tokio::test]
    async fn record_event_sets_timestamp_and_details() {
        let history = TelemetryHistory::new(4, 24);

        history
            .record_event(
                TelemetrySeverity::Warning,
                "pool",
                "queue saturated",
                Some(json!({"destination": "127.0.0.1:443"})),
            )
            .await;

        let events = history.get_events().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].severity, TelemetrySeverity::Warning);
        assert_eq!(events[0].category, "pool");
        assert!(events[0].timestamp <= Utc::now());
        assert_eq!(
            events[0]
                .details
                .as_ref()
                .and_then(|value| value.get("destination")),
            Some(&json!("127.0.0.1:443"))
        );
    }

    #[tokio::test]
    async fn get_events_since_filters_by_cutoff() {
        let history = TelemetryHistory::new(8, 24);
        history.add_event(event_with_offset(120, "old")).await;
        history.add_event(event_with_offset(15, "recent")).await;

        let recent = history.get_events_since(60).await;
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].message, "recent");
    }
}
