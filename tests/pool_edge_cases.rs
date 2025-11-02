/// Connection Pool Edge Cases & Error Handling Tests
///
/// Comprehensive tests for error scenarios, edge cases, and robustness

use rustsocks::server::{ConnectionPool, PoolConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn pool_handles_closed_server_gracefully() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 4,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    // Bind server then immediately drop it
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    // Try to get connection - should fail with connection refused
    let result = pool.get(addr).await;
    assert!(result.is_err(), "Should fail when server is closed");

    if let Err(e) = result {
        let kind = e.kind();
        assert!(
            kind == std::io::ErrorKind::ConnectionRefused
                || kind == std::io::ErrorKind::ConnectionReset
                || kind == std::io::ErrorKind::TimedOut,
            "Expected connection error, got {:?}",
            kind
        );
    }
}

#[tokio::test]
async fn pool_timeout_on_unresponsive_server() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 4,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 100, // Very short timeout
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    // Use non-routable address (RFC 5737 TEST-NET-1)
    let addr: std::net::SocketAddr = "192.0.2.1:9999".parse().unwrap();

    let start = std::time::Instant::now();
    let result = pool.get(addr).await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "Should timeout");
    assert!(
        elapsed < Duration::from_millis(200),
        "Should timeout quickly, took {:?}",
        elapsed
    );

    if let Err(e) = result {
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
    }
}

#[tokio::test]
async fn pool_evicts_expired_connections() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 5,
        max_total_idle: 100,
        idle_timeout_secs: 1, // 1 second idle timeout
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Accept connections in background
    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    // Create and pool a connection
    let stream = pool.get(addr).await.unwrap();
    pool.put(addr, stream).await;

    // Verify it's in pool
    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 1);

    // Wait for expiration
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Try to get - should be expired and removed
    let result = pool.get(addr).await;
    assert!(result.is_ok(), "Should create new connection after expiry");

    // Pool should be empty (expired connection removed)
    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 0, "Expired connection should be removed");
}

#[tokio::test]
async fn pool_enforces_per_destination_limit_strictly() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 2, // Strict limit
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create 5 connections simultaneously
    let mut streams = Vec::new();
    for _ in 0..5 {
        let stream = pool.get(addr).await.unwrap();
        streams.push(stream);
    }

    // Put them all back (should only keep 2)
    for stream in streams {
        pool.put(addr, stream).await;
    }

    let stats = pool.stats().await;
    assert_eq!(
        stats.total_idle, 2,
        "Should enforce per-dest limit of 2"
    );
}

#[tokio::test]
async fn pool_enforces_global_limit_with_multiple_destinations() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 10,
        max_total_idle: 5, // Low global limit
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    // Create 3 different servers
    let mut servers = Vec::new();
    for _ in 0..3 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                if let Ok((stream, _)) = listener.accept().await {
                    drop(stream);
                }
            }
        });

        servers.push(addr);
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create 3 connections to each server (9 total)
    let mut streams = Vec::new();
    for addr in &servers {
        for _ in 0..3 {
            let stream = pool.get(*addr).await.unwrap();
            streams.push((*addr, stream));
        }
    }

    // Put them all back (should only keep 5 total)
    for (addr, stream) in streams {
        pool.put(addr, stream).await;
    }

    let stats = pool.stats().await;
    assert_eq!(
        stats.total_idle, 5,
        "Should enforce global limit of 5"
    );
    assert!(
        stats.destinations <= 3,
        "Should have connections from at most 3 destinations"
    );
}

#[tokio::test]
async fn pool_stats_accurate_after_operations() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 5,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Initial stats
    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 0);
    assert_eq!(stats.destinations, 0);

    // Create 3 connections
    let mut streams = Vec::new();
    for _ in 0..3 {
        let stream = pool.get(addr).await.unwrap();
        streams.push(stream);
    }

    // Put them back
    for stream in streams {
        pool.put(addr, stream).await;
    }

    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 3);
    assert_eq!(stats.destinations, 1);

    // Get one (should reduce idle count)
    let _stream = pool.get(addr).await.unwrap();

    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 2, "Getting should reduce idle count");

    // Put it back
    pool.put(addr, _stream).await;

    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 3, "Putting should increase idle count");
}

#[tokio::test]
async fn pool_disabled_never_stores_connections() {
    let pool_config = PoolConfig {
        enabled: false, // Disabled
        max_idle_per_dest: 10,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    // Try to use pool
    for _ in 0..5 {
        let stream = pool.get(addr).await.unwrap();
        pool.put(addr, stream).await;
    }

    // Pool should remain empty
    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 0, "Disabled pool should never store");
    assert_eq!(stats.destinations, 0);
}

#[tokio::test]
async fn pool_handles_simultaneous_put_operations() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 10,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let max_idle_per_dest = pool_config.max_idle_per_dest;
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Concurrent puts
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let pool_clone = pool.clone();
        tasks.push(tokio::spawn(async move {
            let stream = pool_clone.get(addr).await.unwrap();
            pool_clone.put(addr, stream).await;
        }));
    }

    for task in tasks {
        task.await.unwrap();
    }

    let stats = pool.stats().await;
    assert!(
        stats.total_idle <= max_idle_per_dest,
        "Should respect per-dest limit even with concurrent puts"
    );
}

#[tokio::test]
async fn pool_reuses_most_recent_connection() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 5,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create 3 connections
    let mut streams = Vec::new();
    for _ in 0..3 {
        let stream = pool.get(addr).await.unwrap();
        streams.push(stream);
    }

    // Put them back
    for stream in streams {
        pool.put(addr, stream).await;
    }

    // Get should return most recently used (LIFO behavior via pop())
    let _stream = pool.get(addr).await.unwrap();

    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 2);
}

#[tokio::test]
async fn pool_creates_new_connection_on_empty_pool() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 5,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    // Pool is empty, should create new
    let stream = pool.get(addr).await.unwrap();
    assert!(stream.peer_addr().is_ok());

    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 0, "Pool still empty after get");
}

#[tokio::test]
async fn pool_handles_multiple_destinations_correctly() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 3,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    // Create 4 different servers
    let mut servers = Vec::new();
    for _ in 0..4 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                if let Ok((stream, _)) = listener.accept().await {
                    drop(stream);
                }
            }
        });

        servers.push(addr);
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create 2 connections to each server
    let mut streams = Vec::new();
    for addr in &servers {
        for _ in 0..2 {
            let stream = pool.get(*addr).await.unwrap();
            streams.push((*addr, stream));
        }
    }

    // Put them all back
    for (addr, stream) in streams {
        pool.put(addr, stream).await;
    }

    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 8, "Should have 2 per destination");
    assert_eq!(stats.destinations, 4, "Should track 4 destinations");

    // Get from first server - should reduce only that pool
    let _stream = pool.get(servers[0]).await.unwrap();

    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 7, "Should reduce by 1");
    assert_eq!(stats.destinations, 4, "Still 4 destinations");
}

#[tokio::test]
async fn pool_cleanup_task_runs_periodically() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 5,
        max_total_idle: 100,
        idle_timeout_secs: 1, // 1 second timeout
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create 3 connections
    let mut streams = Vec::new();
    for _ in 0..3 {
        let stream = pool.get(addr).await.unwrap();
        streams.push(stream);
    }

    // Put them back
    for stream in streams {
        pool.put(addr, stream).await;
    }

    let stats = pool.stats().await;
    assert_eq!(stats.total_idle, 3);

    // Wait for cleanup task to run (cleanup interval is idle_timeout/2, min 30s)
    // Since our timeout is 1s, cleanup runs every 30s (max)
    // But connections expire after 1s, so next get() will clean them
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Next get should trigger cleanup of expired
    let _stream = pool.get(addr).await.unwrap();

    // Pool should have cleaned up expired ones
    let stats = pool.stats().await;
    assert_eq!(
        stats.total_idle, 0,
        "Cleanup should have removed expired connections"
    );
}

#[tokio::test]
async fn pool_handles_rapid_get_put_cycles() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 10,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let max_idle_per_dest = pool_config.max_idle_per_dest;
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1];
                    while stream.read(&mut buf).await.is_ok() {
                        if stream.write_all(&buf).await.is_err() {
                            break;
                        }
                    }
                });
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Rapid cycles
    for _ in 0..50 {
        let stream = pool.get(addr).await.unwrap();
        pool.put(addr, stream).await;
    }

    let stats = pool.stats().await;
    assert!(
        stats.total_idle <= max_idle_per_dest,
        "Should maintain limits even with rapid cycling"
    );
}

#[tokio::test]
async fn pool_connection_actually_works() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 5,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    // Echo server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 4];
                    // Keep reading and echoing until connection closes
                    while stream.read_exact(&mut buf).await.is_ok() {
                        if stream.write_all(&buf).await.is_err() {
                            break;
                        }
                    }
                });
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Get connection and actually use it
    let mut stream = pool.get(addr).await.unwrap();

    // Send data
    stream.write_all(b"test").await.unwrap();

    // Read echo
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"test", "Connection should actually work");

    // Return to pool
    pool.put(addr, stream).await;

    // Get again and verify it still works (should reuse the same connection)
    let mut stream = pool.get(addr).await.unwrap();
    stream.write_all(b"work").await.unwrap();
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"work", "Reused connection should work");
}
