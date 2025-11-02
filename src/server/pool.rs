use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
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

/// Connection pool for upstream TCP connections
///
/// Manages a pool of idle TCP connections to upstream servers, enabling connection reuse
/// and reducing connection establishment overhead.
pub struct ConnectionPool {
    config: PoolConfig,
    /// Map: destination address -> Vec of idle connections
    pools: Arc<Mutex<HashMap<SocketAddr, Vec<PooledConnection>>>>,
}

impl ConnectionPool {
    /// Create a new connection pool with the given configuration
    pub fn new(config: PoolConfig) -> Self {
        let enabled = config.enabled;
        let pool = Self {
            config,
            pools: Arc::new(Mutex::new(HashMap::new())),
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
            trace!("Reusing pooled connection to {}", addr);
            return Ok(stream);
        }

        // Pool miss - create new connection
        debug!("Pool miss for {}, creating new connection", addr);
        self.connect_new(addr).await
    }

    /// Return a connection to the pool for potential reuse
    ///
    /// # Arguments
    /// * `addr` - The destination address
    /// * `stream` - The TCP stream to return to the pool
    pub async fn put(&self, addr: SocketAddr, stream: TcpStream) {
        if !self.config.enabled {
            // Pooling disabled - just drop the connection
            return;
        }

        let mut pools = self.pools.lock().await;

        // Check if we're at capacity for this destination
        if let Some(pool) = pools.get(&addr) {
            if pool.len() >= self.config.max_idle_per_dest {
                trace!("Pool for {} is full, discarding connection", addr);
                return;
            }
        }

        // Check global pool size and evict if needed
        let total_idle: usize = pools.values().map(|v| v.len()).sum();
        if total_idle >= self.config.max_total_idle {
            // Find and remove the oldest connection from any pool
            self.evict_oldest(&mut pools);
        }

        // Get or create the pool for this destination and add connection
        let pool = pools.entry(addr).or_insert_with(Vec::new);
        pool.push(PooledConnection::new(stream));
        trace!(
            "Returned connection to pool for {} (pool size: {})",
            addr,
            pool.len()
        );
    }

    /// Try to get a connection from the pool
    async fn try_get_from_pool(&self, addr: SocketAddr) -> Option<TcpStream> {
        let mut pools = self.pools.lock().await;

        let pool = pools.get_mut(&addr)?;
        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);

        // Remove and return the most recently used non-expired connection
        while let Some(mut conn) = pool.pop() {
            if conn.is_expired(idle_timeout) {
                trace!(
                    "Discarding expired connection to {} (idle: {:?})",
                    addr,
                    conn.last_used.elapsed()
                );
                continue;
            }

            // Update last_used time
            conn.last_used = Instant::now();
            return Some(conn.stream);
        }

        // No valid connections found
        if pool.is_empty() {
            pools.remove(&addr);
        }

        None
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
    fn evict_oldest(&self, pools: &mut HashMap<SocketAddr, Vec<PooledConnection>>) {
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
    }

    /// Clean up expired connections (called periodically)
    #[allow(dead_code)]
    async fn cleanup_expired(&self) {
        let mut pools = self.pools.lock().await;
        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
        let mut total_removed = 0;

        pools.retain(|addr, pool| {
            let original_len = pool.len();
            pool.retain(|conn| !conn.is_expired(idle_timeout));
            let removed = original_len - pool.len();

            if removed > 0 {
                trace!("Cleaned up {} expired connections to {}", removed, addr);
                total_removed += removed;
            }

            !pool.is_empty()
        });

        if total_removed > 0 {
            debug!("Cleanup removed {} expired connections", total_removed);
        }
    }

    /// Get pool statistics
    pub async fn stats(&self) -> PoolStats {
        let pools = self.pools.lock().await;
        let total_idle: usize = pools.values().map(|v| v.len()).sum();
        let destinations = pools.len();

        PoolStats {
            total_idle,
            destinations,
            config: self.config.clone(),
        }
    }

    /// Start background task to clean up expired connections
    fn start_cleanup_task(&self) {
        let pools = Arc::clone(&self.pools);
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
                    }

                    !pool.is_empty()
                });

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pool_creation_with_defaults() {
        let config = PoolConfig::default();
        let pool = ConnectionPool::new(config);
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
        let pool = ConnectionPool::new(config);

        // Bind a test server
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Get a connection (should create new)
        let conn_task = tokio::spawn(async move { pool.get(addr).await });

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
        pool.put(addr, client_stream1).await;

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
        let pool = ConnectionPool::new(config);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create 3 connections and add to pool
        for _ in 0..3 {
            let connect_task = tokio::spawn(async move { TcpStream::connect(addr).await });
            let (server_stream, _) = listener.accept().await.unwrap();
            let client_stream = connect_task.await.unwrap().unwrap();

            // Add to pool (but only 2 should be kept due to max_idle_per_dest)
            pool.put(addr, client_stream).await;

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
        let pool = ConnectionPool::new(config);

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
            pool.put(addr1, client_stream).await;
            drop(server_stream);
        }

        let stats = pool.stats().await;
        assert_eq!(stats.total_idle, 2);

        // Add connection to addr2 - should trigger eviction
        let connect_task = tokio::spawn(async move { TcpStream::connect(addr2).await });
        let (server_stream, _) = listener2.accept().await.unwrap();
        let client_stream = connect_task.await.unwrap().unwrap();
        pool.put(addr2, client_stream).await;
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
        let pool = ConnectionPool::new(config);

        // Try to connect to non-routable address (should timeout)
        let addr: SocketAddr = "192.0.2.1:9999".parse().unwrap(); // TEST-NET-1 (non-routable)
        let result = pool.get(addr).await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        }
    }
}
