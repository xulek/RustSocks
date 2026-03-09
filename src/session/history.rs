use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::debug;
#[cfg(feature = "database")]
use tracing::warn;

use super::SessionManager;
#[cfg(feature = "database")]
use super::SessionStore;

/// Single metrics snapshot at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub timestamp: DateTime<Utc>,
    pub active_sessions: u64,
    pub total_sessions: u64,
    pub bandwidth: u64, // total bytes sent + received
}

/// Thread-safe storage for metrics history
#[derive(Debug, Clone)]
pub struct MetricsHistory {
    snapshots: Arc<RwLock<VecDeque<MetricsSnapshot>>>,
    max_snapshots: usize,
    max_age: ChronoDuration,
}

impl MetricsHistory {
    /// Create new history storage
    ///
    /// # Arguments
    /// * `max_snapshots` - Maximum number of snapshots to keep (e.g., 1440 for 2h @ 5s interval)
    /// * `max_age_hours` - Maximum age of snapshots in hours
    pub fn new(max_snapshots: usize, max_age_hours: i64) -> Self {
        Self {
            snapshots: Arc::new(RwLock::new(VecDeque::with_capacity(max_snapshots))),
            max_snapshots,
            max_age: ChronoDuration::hours(max_age_hours),
        }
    }

    /// Add a new snapshot
    pub async fn add_snapshot(&self, snapshot: MetricsSnapshot) {
        let mut snapshots = self.snapshots.write().await;

        // Remove old snapshots beyond max_age
        let cutoff = Utc::now() - self.max_age;
        while let Some(front) = snapshots.front() {
            if front.timestamp < cutoff {
                snapshots.pop_front();
            } else {
                break;
            }
        }

        // Add new snapshot
        snapshots.push_back(snapshot);

        // Trim to max_snapshots if needed
        while snapshots.len() > self.max_snapshots {
            snapshots.pop_front();
        }
    }

    /// Get all snapshots (for API response)
    pub async fn get_snapshots(&self) -> Vec<MetricsSnapshot> {
        let snapshots = self.snapshots.read().await;
        snapshots.iter().cloned().collect()
    }

    /// Get snapshots within a time range
    pub async fn get_snapshots_since(&self, minutes: i64) -> Vec<MetricsSnapshot> {
        let snapshots = self.snapshots.read().await;
        let cutoff = Utc::now() - ChronoDuration::minutes(minutes);

        snapshots
            .iter()
            .filter(|s| s.timestamp >= cutoff)
            .cloned()
            .collect()
    }
}

/// Background task that collects metrics periodically
pub async fn start_metrics_collector(
    session_manager: Arc<SessionManager>,
    history: Arc<MetricsHistory>,
    #[cfg(feature = "database")] store: Option<Arc<SessionStore>>,
    interval_secs: u64,
) {
    let mut ticker = interval(Duration::from_secs(interval_secs));

    debug!("Starting metrics collector (interval: {}s)", interval_secs);

    loop {
        ticker.tick().await;

        // Collect current stats (24 hour lookback)
        let stats = session_manager
            .get_stats(StdDuration::from_secs(24 * 60 * 60))
            .await;

        let snapshot = MetricsSnapshot {
            timestamp: Utc::now(),
            active_sessions: stats.active_sessions as u64,
            total_sessions: stats.total_sessions as u64,
            bandwidth: stats.total_bytes,
        };

        // Add to in-memory history
        history.add_snapshot(snapshot.clone()).await;

        // Persist to database if available
        #[cfg(feature = "database")]
        if let Some(ref db) = store {
            if let Err(e) = db
                .insert_metric(
                    &snapshot.timestamp,
                    snapshot.active_sessions,
                    snapshot.total_sessions,
                    snapshot.bandwidth,
                )
                .await
            {
                warn!(error = %e, "Failed to persist metrics snapshot to database");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with_offset(hours_ago: i64, total_sessions: u64) -> MetricsSnapshot {
        MetricsSnapshot {
            timestamp: Utc::now() - ChronoDuration::hours(hours_ago),
            active_sessions: total_sessions / 2,
            total_sessions,
            bandwidth: total_sessions * 100,
        }
    }

    #[tokio::test]
    async fn add_snapshot_prunes_expired_entries() {
        let history = MetricsHistory::new(8, 1);
        history.add_snapshot(snapshot_with_offset(4, 10)).await;
        history.add_snapshot(snapshot_with_offset(0, 20)).await;

        let snapshots = history.get_snapshots().await;
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].total_sessions, 20);
    }

    #[tokio::test]
    async fn add_snapshot_enforces_max_capacity() {
        let history = MetricsHistory::new(2, 24);
        history.add_snapshot(snapshot_with_offset(0, 10)).await;
        history.add_snapshot(snapshot_with_offset(0, 20)).await;
        history.add_snapshot(snapshot_with_offset(0, 30)).await;

        let snapshots = history.get_snapshots().await;
        let totals: Vec<_> = snapshots
            .into_iter()
            .map(|snapshot| snapshot.total_sessions)
            .collect();
        assert_eq!(totals, vec![20, 30]);
    }

    #[tokio::test]
    async fn get_snapshots_since_filters_recent_values() {
        let history = MetricsHistory::new(8, 24);
        history
            .add_snapshot(MetricsSnapshot {
                timestamp: Utc::now() - ChronoDuration::minutes(90),
                active_sessions: 1,
                total_sessions: 10,
                bandwidth: 100,
            })
            .await;
        history
            .add_snapshot(MetricsSnapshot {
                timestamp: Utc::now() - ChronoDuration::minutes(10),
                active_sessions: 2,
                total_sessions: 20,
                bandwidth: 200,
            })
            .await;

        let snapshots = history.get_snapshots_since(60).await;
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].total_sessions, 20);
    }
}
