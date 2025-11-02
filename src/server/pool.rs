use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{debug, trace};

/// Configuration for connection pool
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Enable connection pooling
    pub enabled: bool,
    /// Maximum idle connections per destination
    pub max_idle_per_dest: usize,
    /// Maximum total idle connections across all destinations
    pub max_total_idle: usize,
    /// How long to keep idle connections alive (seconds)
    pub idle_timeout_secs: u64,
    /// Timeout for establishing new connections (milliseconds)
    pub connect_timeout_ms: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_idle_per_dest: 4,
            max_total_idle: 100,
            idle_timeout_secs: 90,
            connect_timeout_ms: 5000,
        }
    }
}

/// A pooled TCP connection with metadata
struct PooledConnection {
    stream: TcpStream,
    created_at: Instant,
    last_used: Instant,
}

impl PooledConnection {
    fn new(stream: TcpStream) -> Self {
        let now = Instant::now();
        Self {
            stream,
            created_at: now,
            last_used: now,
        }
    }

    fn is_expired(&self, idle_timeout: Duration) -> bool {
        self.last_used.elapsed() > idle_timeout
    }
}

#[derive(Debug, Default)]
struct PoolMetrics {
    total_created: AtomicU64,
    total_reused: AtomicU64,
    pool_hits: AtomicU64,
    pool_misses: AtomicU64,
    dropped_full: AtomicU64,
    expired: AtomicU64,
    evicted: AtomicU64,
    connections_in_use: AtomicU64,
    pending_creates: AtomicU64,
}

#[derive(Debug, Clone, Default)]
struct DestinationMetrics {
    total_created: u64,
    total_reused: u64,
    pool_hits: u64,
    pool_misses: u64,
    drops: u64,
    evicted: u64,
    expired: u64,
    in_use: u64,
    last_activity: Option<SystemTime>,
    last_miss: Option<SystemTime>,
}

/// Snapshot of per-destination pool state
#[derive(Debug, Clone)]
pub struct DestinationPoolStats {
    pub destination: SocketAddr,
    pub idle_connections: usize,
    pub in_use: u64,
    pub total_created: u64,
    pub total_reused: u64,
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub drops: u64,
    pub evicted: u64,
    pub expired: u64,
    pub last_activity: Option<SystemTime>,
    pub last_miss: Option<SystemTime>,
}

/// Guidance for how a returned upstream connection should be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReuseHint {
    /// Stream is clean and can be handed to the next client as-is.
    Reuse,
    /// Drop the used stream and establish a fresh connection to keep the pool warm.
    Refresh,
}

/// Connection pool for upstream TCP connections
///
/// Manages a pool of idle TCP connections to upstream servers, enabling connection reuse
/// and reducing connection establishment overhead.
pub struct ConnectionPool {
    config: PoolConfig,
    /// Map: destination address -> Vec of idle connections
    pools: Arc<Mutex<HashMap<SocketAddr, Vec<PooledConnection>>>>,
    destination_metrics: Arc<Mutex<HashMap<SocketAddr, DestinationMetrics>>>,
    metrics: Arc<PoolMetrics>,
    active_counts: Arc<Mutex<HashMap<SocketAddr, usize>>>,
}

impl ConnectionPool {
    async fn update_destination_metrics<F>(&self, addr: SocketAddr, update: F)
    where
        F: FnOnce(&mut DestinationMetrics),
    {
        let mut metrics = self.destination_metrics.lock().await;
        let entry = metrics.entry(addr).or_default();
        update(entry);
    }

    async fn record_expired(&self, addr: SocketAddr, count: usize) {
        if count == 0 {
            return;
        }

        self.metrics
            .expired
            .fetch_add(count as u64, Ordering::Relaxed);

        self.update_destination_metrics(addr, |entry| {
            entry.expired += count as u64;
            entry.last_activity = Some(SystemTime::now());
        })
        .await;
    }

    async fn record_evicted(&self, addr: SocketAddr) {
        self.metrics.evicted.fetch_add(1, Ordering::Relaxed);

        self.update_destination_metrics(addr, |entry| {
            entry.evicted += 1;
            entry.last_activity = Some(SystemTime::now());
        })
        .await;
    }

    fn decrement_in_use(&self) {
        loop {
            let current = self.metrics.connections_in_use.load(Ordering::Relaxed);
            if current == 0 {
                break;
            }
            if self
                .metrics
                .connections_in_use
                .compare_exchange(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    async fn increment_active(&self, addr: SocketAddr) {
        let mut guard = self.active_counts.lock().await;
        *guard.entry(addr).or_insert(0) += 1;
    }

    async fn decrement_active(&self, addr: SocketAddr) {
        let mut guard = self.active_counts.lock().await;
        if let Some(count) = guard.get_mut(&addr) {
            if *count > 1 {
                *count -= 1;
            } else {
                guard.remove(&addr);
            }
        }
    }

    /// Create a new connection pool with the given configuration
    pub fn new(config: PoolConfig) -> Self {
        let enabled = config.enabled;
        let pools = Arc::new(Mutex::new(HashMap::new()));
        let destination_metrics = Arc::new(Mutex::new(HashMap::new()));
        let metrics = Arc::new(PoolMetrics::default());
        let pool = Self {
            config,
            pools,
            destination_metrics,
            metrics,
            active_counts: Arc::new(Mutex::new(HashMap::new())),
        };

        // Start background cleanup task if pooling is enabled
        if enabled {
            pool.start_cleanup_task();
        }

        pool
    }

    /// Get a connection from the pool or create a new one
    ///
    /// # Arguments
    /// * `addr` - The destination socket address
    ///
    /// # Returns
    /// A TCP stream to the destination, either from the pool or newly created
    pub async fn get(&self, addr: SocketAddr) -> std::io::Result<TcpStream> {
        if !self.config.enabled {
            // Pooling disabled - create new connection
            return self.connect_new(addr).await;
        }

        // Try to get from pool first
        if let Some(stream) = self.try_get_from_pool(addr).await {
            debug!("♻️  Reusing pooled connection to {}", addr);
            self.metrics.pool_hits.fetch_add(1, Ordering::Relaxed);
            self.metrics.total_reused.fetch_add(1, Ordering::Relaxed);
            self.metrics
                .connections_in_use
                .fetch_add(1, Ordering::Relaxed);

            let now = SystemTime::now();
            self.update_destination_metrics(addr, |entry| {
                entry.pool_hits += 1;
                entry.total_reused += 1;
                entry.in_use += 1;
                entry.last_activity = Some(now);
            })
            .await;

            self.increment_active(addr).await;

            return Ok(stream);
        }

        // Pool miss - create new connection
        debug!("🔌 Pool miss for {}, creating new connection", addr);
        self.metrics.pool_misses.fetch_add(1, Ordering::Relaxed);
        let miss_time = SystemTime::now();
        self.update_destination_metrics(addr, |entry| {
            entry.pool_misses += 1;
            entry.last_miss = Some(miss_time);
        })
        .await;

        self.metrics.pending_creates.fetch_add(1, Ordering::Relaxed);
        let result = self.connect_new(addr).await;
        self.metrics.pending_creates.fetch_sub(1, Ordering::Relaxed);

        if result.is_ok() {
            self.metrics.total_created.fetch_add(1, Ordering::Relaxed);
            self.metrics
                .connections_in_use
                .fetch_add(1, Ordering::Relaxed);

            let now = SystemTime::now();
            self.update_destination_metrics(addr, |entry| {
                entry.total_created += 1;
                entry.in_use += 1;
                entry.last_activity = Some(now);
            })
            .await;

            self.increment_active(addr).await;
        }

        result
    }

    /// Return a connection to the pool and decide whether to reuse or refresh it.
    pub async fn put(self: &Arc<Self>, addr: SocketAddr, stream: TcpStream, hint: ReuseHint) {
        if !self.config.enabled {
            // Pooling disabled - just drop the connection
            return;
        }

        self.decrement_in_use();

        match hint {
            ReuseHint::Reuse => {
                let (inserted, dropped_for_capacity, evicted_addr) =
                    self.insert_stream(addr, stream).await;

                if dropped_for_capacity {
                    self.metrics.dropped_full.fetch_add(1, Ordering::Relaxed);
                    self.update_destination_metrics(addr, |entry| {
                        entry.drops += 1;
                        entry.in_use = entry.in_use.saturating_sub(1);
                    })
                    .await;
                } else if inserted {
                    self.update_destination_metrics(addr, |entry| {
                        entry.in_use = entry.in_use.saturating_sub(1);
                        entry.last_activity = Some(SystemTime::now());
                    })
                    .await;
                } else {
                    self.update_destination_metrics(addr, |entry| {
                        entry.in_use = entry.in_use.saturating_sub(1);
                    })
                    .await;
                }

                if let Some(evicted) = evicted_addr {
                    self.record_evicted(evicted).await;
                }

                self.decrement_active(addr).await;
            }
            ReuseHint::Refresh => {
                let mut stream = stream;
                if let Err(e) = stream.shutdown().await {
                    trace!("Failed to shutdown used upstream connection: {}", e);
                }

                self.update_destination_metrics(addr, |entry| {
                    entry.in_use = entry.in_use.saturating_sub(1);
                })
                .await;

                self.decrement_active(addr).await;

                let pool = Arc::clone(self);
                tokio::spawn(async move {
                    if let Err(e) = pool.refresh_connection(addr).await {
                        trace!(
                            target = "rustsocks::server::pool",
                            error = %e,
                            "Failed to refresh pooled connection for {}",
                            addr
                        );
                    }
                });
            }
        }
    }

    /// Release a connection that cannot be returned to the idle pool.
    pub async fn release(self: &Arc<Self>, addr: SocketAddr, hint: ReuseHint) {
        if !self.config.enabled {
            return;
        }

        self.decrement_in_use();

        self.update_destination_metrics(addr, |entry| {
            entry.in_use = entry.in_use.saturating_sub(1);
        })
        .await;

        self.decrement_active(addr).await;

        if matches!(hint, ReuseHint::Refresh) {
            let pool = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(e) = pool.refresh_connection(addr).await {
                    trace!(
                        target = "rustsocks::server::pool",
                        error = %e,
                        "Failed to refresh pooled connection for {}",
                        addr
                    );
                }
            });
        }
    }

    /// Try to get a connection from the pool
    async fn try_get_from_pool(&self, addr: SocketAddr) -> Option<TcpStream> {
        let mut pools = self.pools.lock().await;

        let pool = pools.get_mut(&addr)?;
        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
        let mut expired = 0usize;
        let mut stream: Option<TcpStream> = None;

        while let Some(mut conn) = pool.pop() {
            if conn.is_expired(idle_timeout) {
                trace!(
                    "Discarding expired connection to {} (idle: {:?})",
                    addr,
                    conn.last_used.elapsed()
                );
                expired += 1;
                continue;
            }

            // Update last_used time
            conn.last_used = Instant::now();
            stream = Some(conn.stream);
            break;
        }

        // No valid connections found
        if pool.is_empty() {
            pools.remove(&addr);
        }

        drop(pools);

        if expired > 0 {
            self.record_expired(addr, expired).await;
        }

        stream
    }

    /// Insert a connection into the idle pool, returning whether it was stored.
    async fn insert_stream(
        &self,
        addr: SocketAddr,
        stream: TcpStream,
    ) -> (bool, bool, Option<SocketAddr>) {
        let mut pools = self.pools.lock().await;
        let mut dropped_for_capacity = false;
        let mut inserted = false;
        let mut evicted_addr = None;

        if let Some(pool) = pools.get(&addr) {
            if pool.len() >= self.config.max_idle_per_dest {
                trace!("Pool for {} is full, discarding connection", addr);
                dropped_for_capacity = true;
            }
        }

        if !dropped_for_capacity {
            let total_idle: usize = pools.values().map(|v| v.len()).sum();
            if total_idle >= self.config.max_total_idle {
                evicted_addr = self.evict_oldest(&mut pools);
            }

            let pool = pools.entry(addr).or_insert_with(Vec::new);
            pool.push(PooledConnection::new(stream));
            debug!(
                "💾 Returned connection to pool for {} (pool size: {})",
                addr,
                pool.len()
            );
            inserted = true;
        } else {
            drop(stream);
        }

        drop(pools);
        (inserted, dropped_for_capacity, evicted_addr)
    }

    /// Establish a fresh upstream connection and add it to the pool.
    async fn refresh_connection(&self, addr: SocketAddr) -> std::io::Result<()> {
        self.metrics.pending_creates.fetch_add(1, Ordering::Relaxed);
        let result = self.connect_new(addr).await;
        self.metrics.pending_creates.fetch_sub(1, Ordering::Relaxed);

        let stream = result?;

        self.metrics.total_created.fetch_add(1, Ordering::Relaxed);

        let now = SystemTime::now();
        self.update_destination_metrics(addr, |entry| {
            entry.total_created += 1;
            entry.last_activity = Some(now);
        })
        .await;

        let (inserted, dropped_for_capacity, evicted_addr) = self.insert_stream(addr, stream).await;

        if dropped_for_capacity {
            self.metrics.dropped_full.fetch_add(1, Ordering::Relaxed);
            self.update_destination_metrics(addr, |entry| {
                entry.drops += 1;
            })
            .await;
        } else if !inserted {
            trace!(
                "Refresh connection for {} was not inserted due to concurrent updates",
                addr
            );
        }

        if let Some(evicted) = evicted_addr {
            self.record_evicted(evicted).await;
        }

        Ok(())
    }

    /// Create a new TCP connection with timeout
    async fn connect_new(&self, addr: SocketAddr) -> std::io::Result<TcpStream> {
        let connect_timeout = Duration::from_millis(self.config.connect_timeout_ms);

        match timeout(connect_timeout, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => Ok(stream),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "Connection to {} timed out after {:?}",
                    addr, connect_timeout
                ),
            )),
        }
    }

    /// Evict the oldest connection from all pools
    fn evict_oldest(
        &self,
        pools: &mut HashMap<SocketAddr, Vec<PooledConnection>>,
    ) -> Option<SocketAddr> {
        let mut oldest_addr: Option<SocketAddr> = None;
        let mut oldest_time = Instant::now();

        // Find the oldest connection across all pools
        for (addr, pool) in pools.iter() {
            if let Some(conn) = pool.first() {
                if conn.created_at < oldest_time {
                    oldest_time = conn.created_at;
                    oldest_addr = Some(*addr);
                }
            }
        }

        // Remove the oldest connection
        if let Some(addr) = oldest_addr {
            if let Some(pool) = pools.get_mut(&addr) {
                pool.remove(0);
                if pool.is_empty() {
                    pools.remove(&addr);
                }
                trace!("Evicted oldest connection to {}", addr);
            }
        }

        oldest_addr
    }

    /// Clean up expired connections (called periodically)
    #[allow(dead_code)]
    async fn cleanup_expired(&self) {
        let mut pools = self.pools.lock().await;
        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
        let mut total_removed = 0;
        let mut removed_by_addr = Vec::new();

        pools.retain(|addr, pool| {
            let original_len = pool.len();
            pool.retain(|conn| !conn.is_expired(idle_timeout));
            let removed = original_len - pool.len();

            if removed > 0 {
                trace!("Cleaned up {} expired connections to {}", removed, addr);
                total_removed += removed;
                removed_by_addr.push((*addr, removed));
            }

            !pool.is_empty()
        });

        drop(pools);

        for (addr, removed) in removed_by_addr {
            self.record_expired(addr, removed).await;
        }

        if total_removed > 0 {
            debug!("Cleanup removed {} expired connections", total_removed);
        }
    }

    /// Get pool statistics
    pub async fn stats(&self) -> PoolStats {
        let pools_guard = self.pools.lock().await;
        let mut idle_counts: HashMap<SocketAddr, usize> = HashMap::new();
        let mut total_idle: usize = 0;

        for (addr, pool) in pools_guard.iter() {
            total_idle += pool.len();
            idle_counts.insert(*addr, pool.len());
        }

        drop(pools_guard);

        let metrics_guard = self.destination_metrics.lock().await;
        let mut all_addresses: HashSet<SocketAddr> = idle_counts.keys().copied().collect();
        all_addresses.extend(metrics_guard.keys().copied());

        let mut per_destination = Vec::with_capacity(all_addresses.len());
        for addr in all_addresses {
            let entry = metrics_guard.get(&addr).cloned().unwrap_or_default();
            let idle_connections = idle_counts.get(&addr).copied().unwrap_or(0);
            per_destination.push(DestinationPoolStats {
                destination: addr,
                idle_connections,
                in_use: entry.in_use,
                total_created: entry.total_created,
                total_reused: entry.total_reused,
                pool_hits: entry.pool_hits,
                pool_misses: entry.pool_misses,
                drops: entry.drops,
                evicted: entry.evicted,
                expired: entry.expired,
                last_activity: entry.last_activity,
                last_miss: entry.last_miss,
            });
        }
        drop(metrics_guard);

        per_destination.sort_by(|a, b| {
            b.in_use
                .cmp(&a.in_use)
                .then_with(|| b.idle_connections.cmp(&a.idle_connections))
                .then_with(|| b.total_created.cmp(&a.total_created))
        });

        let destinations = per_destination.len();

        let active_counts = self.active_counts.lock().await;
        let connections_in_use: u64 = active_counts.values().map(|&v| v as u64).sum();
        drop(active_counts);

        PoolStats {
            total_idle,
            destinations,
            config: self.config.clone(),
            total_created: self.metrics.total_created.load(Ordering::Relaxed),
            total_reused: self.metrics.total_reused.load(Ordering::Relaxed),
            pool_hits: self.metrics.pool_hits.load(Ordering::Relaxed),
            pool_misses: self.metrics.pool_misses.load(Ordering::Relaxed),
            dropped_full: self.metrics.dropped_full.load(Ordering::Relaxed),
            expired: self.metrics.expired.load(Ordering::Relaxed),
            evicted: self.metrics.evicted.load(Ordering::Relaxed),
            connections_in_use,
            pending_creates: self.metrics.pending_creates.load(Ordering::Relaxed),
            per_destination,
        }
    }

    /// Start background task to clean up expired connections
    fn start_cleanup_task(&self) {
        let pools = Arc::clone(&self.pools);
        let destination_metrics = Arc::clone(&self.destination_metrics);
        let metrics = Arc::clone(&self.metrics);
        let idle_timeout_secs = self.config.idle_timeout_secs;

        tokio::spawn(async move {
            // Run cleanup every idle_timeout/2 seconds
            let cleanup_interval =
                Duration::from_secs(idle_timeout_secs / 2).max(Duration::from_secs(30));
            let mut interval = tokio::time::interval(cleanup_interval);

            loop {
                interval.tick().await;

                let mut pools_guard = pools.lock().await;
                let idle_timeout = Duration::from_secs(idle_timeout_secs);
                let mut total_removed = 0;
                let mut removed_by_addr = Vec::new();

                pools_guard.retain(|addr, pool| {
                    let original_len = pool.len();
                    pool.retain(|conn| !conn.is_expired(idle_timeout));
                    let removed = original_len - pool.len();

                    if removed > 0 {
                        trace!(
                            "Cleanup: removed {} expired connections to {}",
                            removed,
                            addr
                        );
                        total_removed += removed;
                        removed_by_addr.push((*addr, removed));
                    }

                    !pool.is_empty()
                });

                drop(pools_guard);

                if !removed_by_addr.is_empty() {
                    let mut dest_guard = destination_metrics.lock().await;
                    for (addr, removed) in removed_by_addr {
                        metrics.expired.fetch_add(removed as u64, Ordering::Relaxed);
                        let entry = dest_guard.entry(addr).or_default();
                        entry.expired += removed as u64;
                        entry.last_activity = Some(SystemTime::now());
                    }
                }

                if total_removed > 0 {
                    debug!(
                        "Periodic cleanup removed {} expired connections",
                        total_removed
                    );
                }
            }
        });
    }
}

/// Statistics about the connection pool
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Total number of idle connections
    pub total_idle: usize,
    /// Number of unique destinations with pooled connections
    pub destinations: usize,
    /// Pool configuration
    pub config: PoolConfig,
    /// Connections currently checked out
    pub connections_in_use: u64,
    /// Total connections ever created
    pub total_created: u64,
    /// Total times a pooled connection was reused
    pub total_reused: u64,
    /// Number of times a pooled connection was returned successfully
    pub pool_hits: u64,
    /// Number of times pool lookup failed
    pub pool_misses: u64,
    /// Connections dropped due to per-destination cap
    pub dropped_full: u64,
    /// Connections expired due to idle timeout
    pub expired: u64,
    /// Connections evicted due to global cap
    pub evicted: u64,
    /// Connections currently being created
    pub pending_creates: u64,
    /// Detailed stats per destination
    pub per_destination: Vec<DestinationPoolStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pool_creation_with_defaults() {
        let config = PoolConfig::default();
        let pool = Arc::new(ConnectionPool::new(config));
        let stats = pool.stats().await;

        assert_eq!(stats.total_idle, 0);
        assert_eq!(stats.destinations, 0);
    }

    #[tokio::test]
    async fn pool_disabled_creates_new_connections() {
        let config = PoolConfig {
            enabled: false,
            ..Default::default()
        };
        let pool = Arc::new(ConnectionPool::new(config));

        // Bind a test server
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Get a connection (should create new)
        let pool_clone = Arc::clone(&pool);
        let conn_task = tokio::spawn(async move { pool_clone.get(addr).await });

        // Accept the connection
        let (_, _) = listener.accept().await.unwrap();

        let stream = conn_task.await.unwrap().unwrap();
        assert!(stream.peer_addr().is_ok());
    }

    #[tokio::test]
    async fn pool_reuses_connections() {
        let config = PoolConfig {
            enabled: true,
            max_idle_per_dest: 2,
            ..Default::default()
        };
        let pool = Arc::new(ConnectionPool::new(config));

        // Bind a test server
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create first connection
        let pool_clone = Arc::clone(&pool);
        let conn1_task = tokio::spawn(async move { pool_clone.get(addr).await });
        let (_stream1, _) = listener.accept().await.unwrap();
        let client_stream1 = conn1_task.await.unwrap().unwrap();

        // Return to pool
        pool.put(addr, client_stream1, ReuseHint::Reuse).await;

        // Stats should show 1 idle connection
        let stats = pool.stats().await;
        assert_eq!(stats.total_idle, 1);
        assert_eq!(stats.destinations, 1);

        // Get again - should reuse
        let pool_clone = Arc::clone(&pool);
        let conn2_task = tokio::spawn(async move { pool_clone.get(addr).await });

        let reused_stream = conn2_task.await.unwrap().unwrap();
        assert!(reused_stream.peer_addr().is_ok());

        // Stats should show 0 idle (connection was taken from pool)
        let stats = pool.stats().await;
        assert_eq!(stats.total_idle, 0);
    }

    #[tokio::test]
    async fn pool_respects_max_idle_per_dest() {
        let config = PoolConfig {
            enabled: true,
            max_idle_per_dest: 2,
            max_total_idle: 100,
            ..Default::default()
        };
        let pool = Arc::new(ConnectionPool::new(config));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create 3 connections and add to pool
        for _ in 0..3 {
            let connect_task = tokio::spawn(async move { TcpStream::connect(addr).await });
            let (server_stream, _) = listener.accept().await.unwrap();
            let client_stream = connect_task.await.unwrap().unwrap();

            // Add to pool (but only 2 should be kept due to max_idle_per_dest)
            pool.put(addr, client_stream, ReuseHint::Reuse).await;

            // Drop server stream
            drop(server_stream);
        }

        // Should only have 2 idle connections (max_idle_per_dest)
        let stats = pool.stats().await;
        assert_eq!(stats.total_idle, 2);
    }

    #[tokio::test]
    async fn pool_evicts_on_global_limit() {
        let config = PoolConfig {
            enabled: true,
            max_idle_per_dest: 10,
            max_total_idle: 2, // Low global limit
            ..Default::default()
        };
        let pool = Arc::new(ConnectionPool::new(config));

        // Create two listeners for different destinations
        let listener1 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr1 = listener1.local_addr().unwrap();

        let listener2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr2 = listener2.local_addr().unwrap();

        // Add 2 connections to addr1
        for _ in 0..2 {
            let connect_task = tokio::spawn(async move { TcpStream::connect(addr1).await });
            let (server_stream, _) = listener1.accept().await.unwrap();
            let client_stream = connect_task.await.unwrap().unwrap();
            pool.put(addr1, client_stream, ReuseHint::Reuse).await;
            drop(server_stream);
        }

        let stats = pool.stats().await;
        assert_eq!(stats.total_idle, 2);

        // Add connection to addr2 - should trigger eviction
        let connect_task = tokio::spawn(async move { TcpStream::connect(addr2).await });
        let (server_stream, _) = listener2.accept().await.unwrap();
        let client_stream = connect_task.await.unwrap().unwrap();
        pool.put(addr2, client_stream, ReuseHint::Reuse).await;
        drop(server_stream);

        let stats = pool.stats().await;
        assert_eq!(stats.total_idle, 2); // Should still be 2 (oldest evicted)
    }

    #[tokio::test]
    async fn connection_timeout_works() {
        let config = PoolConfig {
            enabled: true,
            connect_timeout_ms: 100, // Very short timeout
            ..Default::default()
        };
        let pool = Arc::new(ConnectionPool::new(config));

        // Try to connect to non-routable address (should timeout)
        let addr: SocketAddr = "192.0.2.1:9999".parse().unwrap(); // TEST-NET-1 (non-routable)
        let result = pool.get(addr).await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        }
    }
}
