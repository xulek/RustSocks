use super::metrics::QosMetrics;
use super::token_bucket::TokenBucket;
use super::types::{HtbConfig, UserAllocation};
use crate::utils::error::{Result, RustSocksError};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, trace, warn};

/// Per-user bucket tracking
#[derive(Debug)]
struct UserBucket {
    /// Guaranteed bandwidth bucket (always available)
    guaranteed_bucket: Arc<TokenBucket>,

    /// Maximum bandwidth bucket (for borrowing)
    max_bucket: Arc<TokenBucket>,

    /// Current demand estimate (bytes per second)
    current_demand: AtomicU64,

    /// Last activity timestamp
    last_activity: Arc<tokio::sync::Mutex<Instant>>,

    /// Active connections count
    active_connections: AtomicUsize,

    /// Total bytes transferred (for statistics)
    total_bytes: AtomicU64,
}

impl UserBucket {
    fn new(guaranteed_rate: u64, max_rate: u64, burst_size: u64) -> Self {
        Self {
            guaranteed_bucket: Arc::new(TokenBucket::new(burst_size, guaranteed_rate)),
            max_bucket: Arc::new(TokenBucket::new(burst_size, max_rate)),
            current_demand: AtomicU64::new(0),
            last_activity: Arc::new(tokio::sync::Mutex::new(Instant::now())),
            active_connections: AtomicUsize::new(0),
            total_bytes: AtomicU64::new(0),
        }
    }

    /// Check if user is active
    async fn is_active(&self, idle_timeout: Duration) -> bool {
        if self.active_connections.load(Ordering::Relaxed) == 0 {
            return false;
        }

        let last_activity = self.last_activity.lock().await;
        last_activity.elapsed() < idle_timeout
    }

    /// Update activity timestamp
    async fn update_activity(&self) {
        let mut last_activity = self.last_activity.lock().await;
        *last_activity = Instant::now();
    }

    /// Increment connection count
    fn inc_connections(&self) -> usize {
        self.active_connections.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Decrement connection count
    fn dec_connections(&self) -> usize {
        self.active_connections
            .fetch_sub(1, Ordering::Relaxed)
            .saturating_sub(1)
    }

    /// Get connection count
    fn connection_count(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }
}

/// Hierarchical Token Bucket QoS Engine
#[derive(Clone)]
pub struct HtbQos {
    config: HtbConfig,

    /// Global bandwidth bucket
    global_bucket: Arc<TokenBucket>,

    /// Per-user buckets
    user_buckets: Arc<DashMap<String, Arc<UserBucket>>>,

    /// Total active connections
    total_connections: Arc<AtomicUsize>,

    /// Rebalancing task handle
    rebalance_handle: Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>,
}

impl HtbQos {
    /// Create new HTB QoS engine
    pub fn new(config: HtbConfig) -> Self {
        let global_bucket = Arc::new(TokenBucket::new(
            config.burst_size_bytes,
            config.global_bandwidth_bytes_per_sec,
        ));

        Self {
            config,
            global_bucket,
            user_buckets: Arc::new(DashMap::new()),
            total_connections: Arc::new(AtomicUsize::new(0)),
            rebalance_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Start the rebalancing task
    pub async fn start(&self) {
        if !self.config.fair_sharing_enabled {
            debug!("Fair sharing disabled, skipping rebalancing task");
            return;
        }

        let htb = self.clone();
        let mut handle_guard = self.rebalance_handle.lock().await;

        if handle_guard.is_none() {
            let handle = tokio::spawn(async move {
                htb.rebalancing_task().await;
            });
            *handle_guard = Some(handle);
            debug!("HTB rebalancing task started");
        }
    }

    /// Stop the rebalancing task
    pub async fn stop(&self) {
        let mut handle_guard = self.rebalance_handle.lock().await;
        if let Some(handle) = handle_guard.take() {
            handle.abort();
            debug!("HTB rebalancing task stopped");
        }
    }

    /// Allocate bandwidth for a user to transfer bytes
    ///
    /// This is the main entry point called from proxy loop
    pub async fn allocate_bandwidth(&self, user: &str, bytes: u64) -> Result<()> {
        // Enforce global bandwidth limit
        if self.global_bucket.try_consume(bytes).is_err() {
            let wait_start = Instant::now();
            self.global_bucket
                .consume(bytes)
                .await
                .map_err(RustSocksError::Io)?;
            QosMetrics::observe_wait(wait_start.elapsed().as_secs_f64());
        }

        // Get or create user bucket
        let user_bucket = self.get_or_create_user_bucket(user);

        // Update activity
        user_bucket.update_activity().await;

        // Update total bytes
        user_bucket
            .total_bytes
            .fetch_add(bytes, Ordering::Relaxed);

        // Try guaranteed bucket first (always available)
        if user_bucket.guaranteed_bucket.try_consume(bytes).is_ok() {
            trace!(
                user = %user,
                bytes = bytes,
                "Consumed from guaranteed bucket"
            );
            return Ok(());
        }

        // Try borrowing from max bucket (may block)
        if user_bucket.max_bucket.try_consume(bytes).is_ok() {
            trace!(
                user = %user,
                bytes = bytes,
                "Consumed from borrowed bucket"
            );
            return Ok(());
        }

        // Need to wait for tokens
        trace!(
            user = %user,
            bytes = bytes,
            "Waiting for tokens"
        );

        let wait_start = Instant::now();
        user_bucket
            .max_bucket
            .consume(bytes)
            .await
            .map_err(RustSocksError::Io)?;
        QosMetrics::observe_wait(wait_start.elapsed().as_secs_f64());

        Ok(())
    }

    /// Increment user connection count
    pub fn inc_user_connections(&self, user: &str) -> Result<usize> {
        let user_bucket = self.get_or_create_user_bucket(user);
        let count = user_bucket.inc_connections();

        // Also increment global
        let global_count = self.total_connections.fetch_add(1, Ordering::Relaxed) + 1;

        trace!(
            user = %user,
            user_connections = count,
            total_connections = global_count,
            "Connection established"
        );

        Ok(count)
    }

    /// Decrement user connection count
    pub fn dec_user_connections(&self, user: &str) -> usize {
        if let Some(user_bucket) = self.user_buckets.get(user) {
            let count = user_bucket.dec_connections();
            self.total_connections.fetch_sub(1, Ordering::Relaxed);

            trace!(
                user = %user,
                remaining_connections = count,
                "Connection closed"
            );

            count
        } else {
            0
        }
    }

    /// Get user connection count
    pub fn get_user_connections(&self, user: &str) -> usize {
        self.user_buckets
            .get(user)
            .map(|b| b.connection_count())
            .unwrap_or(0)
    }

    /// Get total connection count
    pub fn get_total_connections(&self) -> usize {
        self.total_connections.load(Ordering::Relaxed)
    }

    /// Get current user allocations (for monitoring/API)
    pub async fn get_user_allocations(&self) -> Vec<UserAllocation> {
        let mut allocations = Vec::new();
        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);

        for entry in self.user_buckets.iter() {
            let user = entry.key().clone();
            let bucket = entry.value();

            let is_active = bucket.is_active(idle_timeout).await;
            let current_demand = bucket.current_demand.load(Ordering::Relaxed);

            allocations.push(UserAllocation {
                user,
                allocated_bandwidth: bucket.max_bucket.refill_rate(),
                guaranteed_bandwidth: bucket.guaranteed_bucket.refill_rate(),
                max_bandwidth: self.config.max_bandwidth_bytes_per_sec,
                current_demand,
                is_active,
                active_connections: bucket.connection_count(),
            });
        }

        allocations
    }

    /// Get or create user bucket
    fn get_or_create_user_bucket(&self, user: &str) -> Arc<UserBucket> {
        self.user_buckets
            .entry(user.to_string())
            .or_insert_with(|| {
                Arc::new(UserBucket::new(
                    self.config.guaranteed_bandwidth_bytes_per_sec,
                    self.config.max_bandwidth_bytes_per_sec,
                    self.config.burst_size_bytes,
                ))
            })
            .clone()
    }

    /// Periodic rebalancing task - recalculate fair shares
    async fn rebalancing_task(&self) {
        let mut ticker = interval(Duration::from_millis(self.config.rebalance_interval_ms));
        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);

        loop {
            ticker.tick().await;

            if let Err(e) = self.rebalance_bandwidth(idle_timeout).await {
                warn!("Rebalancing error: {}", e);
            }
        }
    }

    /// Rebalance bandwidth allocation among active users
    async fn rebalance_bandwidth(&self, idle_timeout: Duration) -> Result<()> {
        // Collect active users and their demands
        let mut active_users: Vec<(String, Arc<UserBucket>, u64)> = Vec::new();

        for entry in self.user_buckets.iter() {
            let user = entry.key();
            let bucket = entry.value();

            if bucket.is_active(idle_timeout).await {
                let demand = self.estimate_user_demand(bucket.as_ref()).await;
                active_users.push((user.clone(), bucket.clone(), demand));
            }
        }

        if active_users.is_empty() {
            return Ok(());
        }

        // Calculate fair shares
        let allocations = self.calculate_fair_shares(&active_users);

        // Apply new rates
        for (user, bucket, new_rate) in allocations {
            bucket.max_bucket.set_refill_rate(new_rate).await;
            bucket.current_demand.store(new_rate, Ordering::Relaxed);

            trace!(
                user = %user,
                new_rate = new_rate,
                "Updated bandwidth allocation"
            );
        }

        debug!(
            active_users = active_users.len(),
            "Rebalanced bandwidth allocations"
        );

        Ok(())
    }

    /// Calculate fair shares using HTB algorithm
    fn calculate_fair_shares(
        &self,
        active_users: &[(String, Arc<UserBucket>, u64)],
    ) -> Vec<(String, Arc<UserBucket>, u64)> {
        let mut allocations = Vec::new();
        let mut remaining = self.config.global_bandwidth_bytes_per_sec;

        // Phase 1: Allocate guaranteed bandwidth to all active users
        for (user, bucket, _demand) in active_users {
            let guaranteed = self.config.guaranteed_bandwidth_bytes_per_sec;
            allocations.push((user.clone(), bucket.clone(), guaranteed));
            remaining = remaining.saturating_sub(guaranteed);
        }

        if remaining == 0 || active_users.is_empty() {
            return allocations;
        }

        // Phase 2: Fair share of remaining bandwidth based on demand
        let total_demand: u64 = active_users.iter().map(|(_, _, demand)| demand).sum();

        if total_demand > 0 {
            for (idx, (_user, _bucket, demand)) in active_users.iter().enumerate() {
                let guaranteed = self.config.guaranteed_bandwidth_bytes_per_sec;

                // Calculate proportional share
                let share = if total_demand > remaining {
                    // Oversubscribed: proportional allocation
                    ((*demand as f64 / total_demand as f64) * remaining as f64) as u64
                } else {
                    // Enough for everyone: give what they need
                    *demand
                };

                // Cap at max_bandwidth
                let capped_share =
                    std::cmp::min(share, self.config.max_bandwidth_bytes_per_sec - guaranteed);

                // Update allocation
                allocations[idx].2 = guaranteed + capped_share;
            }
        } else {
            // No demand info, split equally
            let equal_share = remaining / active_users.len() as u64;

            for (idx, _) in active_users.iter().enumerate() {
                let guaranteed = self.config.guaranteed_bandwidth_bytes_per_sec;
                let capped_share =
                    std::cmp::min(equal_share, self.config.max_bandwidth_bytes_per_sec - guaranteed);
                allocations[idx].2 = guaranteed + capped_share;
            }
        }

        allocations
    }

    /// Estimate user's bandwidth demand from recent activity
    async fn estimate_user_demand(&self, bucket: &UserBucket) -> u64 {
        // Simple estimation: if bucket is being depleted, user has high demand
        let guaranteed_available = bucket.guaranteed_bucket.available_tokens();
        let max_available = bucket.max_bucket.available_tokens();

        // If tokens are low, assume user wants maximum
        if guaranteed_available < bucket.guaranteed_bucket.capacity() / 4
            || max_available < bucket.max_bucket.capacity() / 4
        {
            return self.config.max_bandwidth_bytes_per_sec;
        }

        // Otherwise assume they want guaranteed
        self.config.guaranteed_bandwidth_bytes_per_sec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use tokio::time::{Duration, Instant};

    #[tokio::test]
    async fn test_htb_creation() {
        let config = HtbConfig::default();
        let htb = HtbQos::new(config);

        assert_eq!(htb.get_total_connections(), 0);
    }

    #[tokio::test]
    async fn test_connection_counting() {
        let config = HtbConfig::default();
        let htb = HtbQos::new(config);

        htb.inc_user_connections("alice").unwrap();
        htb.inc_user_connections("alice").unwrap();
        htb.inc_user_connections("bob").unwrap();

        assert_eq!(htb.get_user_connections("alice"), 2);
        assert_eq!(htb.get_user_connections("bob"), 1);
        assert_eq!(htb.get_total_connections(), 3);

        let _ = htb.dec_user_connections("alice");
        assert_eq!(htb.get_user_connections("alice"), 1);
        assert_eq!(htb.get_total_connections(), 2);
    }

    #[tokio::test]
    async fn test_bandwidth_allocation() {
        let config = HtbConfig {
            guaranteed_bandwidth_bytes_per_sec: 1_000_000, // 1 MB/s
            max_bandwidth_bytes_per_sec: 10_000_000,       // 10 MB/s
            burst_size_bytes: 100_000,                     // 100 KB
            ..Default::default()
        };
        let htb = HtbQos::new(config);

        // Should succeed immediately (burst available)
        let result = htb.allocate_bandwidth("alice", 50_000).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fair_sharing_calculation() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 1_000_000,    // 1 MB/s total
            guaranteed_bandwidth_bytes_per_sec: 100_000,  // 100 KB/s per user
            max_bandwidth_bytes_per_sec: 1_000_000,       // 1 MB/s max
            ..Default::default()
        };
        let htb = HtbQos::new(config);

        // Create 3 active users with varying demands
        let bucket1 = Arc::new(UserBucket::new(100_000, 1_000_000, 10_000));
        let bucket2 = Arc::new(UserBucket::new(100_000, 1_000_000, 10_000));
        let bucket3 = Arc::new(UserBucket::new(100_000, 1_000_000, 10_000));

        let active_users = vec![
            ("alice".to_string(), bucket1, 500_000),
            ("bob".to_string(), bucket2, 300_000),
            ("charlie".to_string(), bucket3, 200_000),
        ];

        let allocations = htb.calculate_fair_shares(&active_users);

        // Each should get guaranteed (100KB) + fair share of remaining (700KB)
        assert_eq!(allocations.len(), 3);

        // Alice should get most (highest demand)
        assert!(allocations[0].2 > allocations[1].2);
        assert!(allocations[1].2 > allocations[2].2);
    }

    #[tokio::test]
    async fn test_allocate_bandwidth_throttles_after_burst() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 1_000_000,
            guaranteed_bandwidth_bytes_per_sec: 200_000,
            max_bandwidth_bytes_per_sec: 400_000,
            burst_size_bytes: 100_000,
            ..Default::default()
        };
        let htb = HtbQos::new(config);

        // Consume guaranteed and borrowed buckets
        htb.allocate_bandwidth("alice", 100_000).await.unwrap();
        htb.allocate_bandwidth("alice", 100_000).await.unwrap();

        // Third allocation should require waiting for refill
        let start = Instant::now();
        htb.allocate_bandwidth("alice", 100_000).await.unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(50),
            "expected throttling delay, got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_rebalance_prefers_higher_demand_users() {
        let mut config = HtbConfig {
            global_bandwidth_bytes_per_sec: 800_000,
            guaranteed_bandwidth_bytes_per_sec: 100_000,
            max_bandwidth_bytes_per_sec: 600_000,
            burst_size_bytes: 200_000,
            ..Default::default()
        };
        config.rebalance_interval_ms = 50;
        config.idle_timeout_secs = 60;

        let htb = HtbQos::new(config);

        // Make buckets active with different demand levels
        let alice_bucket = htb.get_or_create_user_bucket("alice");
        let bob_bucket = htb.get_or_create_user_bucket("bob");

        alice_bucket
            .max_bucket
            .try_consume(alice_bucket.max_bucket.capacity())
            .ok();
        bob_bucket
            .max_bucket
            .try_consume(bob_bucket.max_bucket.capacity() / 4)
            .ok();

        alice_bucket
            .guaranteed_bucket
            .try_consume(alice_bucket.guaranteed_bucket.capacity())
            .ok();

        alice_bucket
            .active_connections
            .store(1, Ordering::Relaxed);
        bob_bucket
            .active_connections
            .store(1, Ordering::Relaxed);

        alice_bucket.update_activity().await;
        bob_bucket.update_activity().await;

        // Run rebalance manually to observe allocation change
        htb.rebalance_bandwidth(Duration::from_secs(1))
            .await
            .unwrap();

        let alice_rate = alice_bucket.max_bucket.refill_rate();
        let bob_rate = bob_bucket.max_bucket.refill_rate();

        assert!(
            alice_rate > bob_rate,
            "expected higher allocation for high-demand user (alice: {}, bob: {})",
            alice_rate,
            bob_rate
        );
    }
}
