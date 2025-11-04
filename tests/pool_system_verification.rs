/// Integration test for connection pooling verification at system level
///
/// This test verifies that:
/// 1. Upstream connections are created on first request
/// 2. Connections are returned to pool after use
/// 3. Connections are reused from pool on subsequent requests
/// 4. Pool statistics accurately reflect usage
use rustsocks::acl::AclStats;
use rustsocks::auth::AuthManager;
use rustsocks::config::AuthConfig;
use rustsocks::qos::{ConnectionLimits, QosEngine};
use rustsocks::server::handler::{handle_client, ClientHandlerContext};
use rustsocks::server::pool::{ConnectionPool, PoolConfig};
use rustsocks::server::proxy::TrafficUpdateConfig;
use rustsocks::session::SessionManager;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

/// Spawn a simple echo server that echoes back data
async fn spawn_echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = socket.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        if socket.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

/// Spawn SOCKS5 server with pooling enabled
async fn spawn_socks_with_pooling(
    pool_config: PoolConfig,
) -> (SocketAddr, Arc<ClientHandlerContext>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let auth_manager = Arc::new(AuthManager::new(&auth_config).unwrap());
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new("anonymous".to_string());
    let session_manager = Arc::new(SessionManager::new());
    let connection_pool = Arc::new(ConnectionPool::new(pool_config));

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: auth_manager.clone(),
        acl_engine: None,
        acl_stats: acl_stats.clone(),
        anonymous_user: anonymous_user.clone(),
        session_manager: session_manager.clone(),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: connection_pool.clone(),
    });

    let ctx_clone = Arc::clone(&ctx);
    tokio::spawn(async move {
        loop {
            if let Ok((stream, peer_addr)) = listener.accept().await {
                let ctx = Arc::clone(&ctx_clone);
                tokio::spawn(async move {
                    let _ = handle_client(stream, ctx, peer_addr).await;
                });
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, ctx)
}

/// Perform SOCKS5 handshake and CONNECT
async fn socks5_connect(
    stream: &mut TcpStream,
    target: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    // SOCKS5 greeting: no auth
    stream.write_all(&[0x05, 0x01, 0x00]).await?;

    // Read server choice
    let mut buf = [0u8; 2];
    stream.read_exact(&mut buf).await?;

    if buf[0] != 0x05 || buf[1] != 0x00 {
        return Err("SOCKS5 handshake failed".into());
    }

    // CONNECT request
    let mut request = vec![0x05, 0x01, 0x00]; // VER, CMD=CONNECT, RSV

    match target {
        SocketAddr::V4(addr) => {
            request.push(0x01); // ATYP=IPv4
            request.extend_from_slice(&addr.ip().octets());
            request.extend_from_slice(&addr.port().to_be_bytes());
        }
        SocketAddr::V6(addr) => {
            request.push(0x04); // ATYP=IPv6
            request.extend_from_slice(&addr.ip().octets());
            request.extend_from_slice(&addr.port().to_be_bytes());
        }
    }

    stream.write_all(&request).await?;

    // Read response
    let mut response = vec![0u8; 4];
    stream.read_exact(&mut response).await?;

    let atyp = response[3];
    let addr_len = match atyp {
        0x01 => 4,  // IPv4
        0x04 => 16, // IPv6
        _ => return Err("Unsupported address type".into()),
    };

    let mut addr_port = vec![0u8; addr_len + 2];
    stream.read_exact(&mut addr_port).await?;

    if response[1] != 0x00 {
        return Err(format!("SOCKS5 CONNECT failed with code {}", response[1]).into());
    }

    Ok(())
}

#[tokio::test]
async fn pool_reuses_upstream_connections() {
    // Setup echo server
    let echo_addr = spawn_echo_server().await;
    println!("✓ Echo server started at {}", echo_addr);

    // Setup SOCKS proxy with pooling
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 4,
        max_total_idle: 10,
        idle_timeout_secs: 30,
        connect_timeout_ms: 3000,
    };

    let (socks_addr, ctx) = spawn_socks_with_pooling(pool_config).await;
    println!(
        "✓ SOCKS proxy started at {} with pooling enabled",
        socks_addr
    );

    // Get initial pool stats
    let initial_stats = ctx.connection_pool.stats();
    println!("\n📊 Initial pool stats:");
    println!("  Total created: {}", initial_stats.total_created);
    println!("  Total reused: {}", initial_stats.total_reused);
    println!("  Pool hits: {}", initial_stats.pool_hits);
    println!("  Pool misses: {}", initial_stats.pool_misses);

    assert_eq!(
        initial_stats.total_created, 0,
        "No connections should exist initially"
    );
    assert_eq!(initial_stats.pool_hits, 0, "No pool hits initially");

    // Connection 1: Should create new upstream connection
    println!("\n🔌 Connection 1: Creating first connection (expect pool miss)");
    {
        let mut client = timeout(Duration::from_secs(5), TcpStream::connect(socks_addr))
            .await
            .expect("Timeout connecting to SOCKS")
            .expect("Failed to connect to SOCKS");

        socks5_connect(&mut client, echo_addr)
            .await
            .expect("SOCKS5 handshake failed");

        // Send test data
        client.write_all(b"Hello 1").await.expect("Write failed");
        let mut buf = vec![0u8; 7];
        client.read_exact(&mut buf).await.expect("Read failed");
        assert_eq!(&buf, b"Hello 1");

        println!("✓ Connection 1 completed successfully");
    } // Connection closed here

    // Wait for connection to be returned to pool
    tokio::time::sleep(Duration::from_millis(100)).await;

    let stats_after_conn1 = ctx.connection_pool.stats();
    println!("\n📊 Stats after connection 1:");
    println!("  Total created: {}", stats_after_conn1.total_created);
    println!("  Total reused: {}", stats_after_conn1.total_reused);
    println!("  Pool hits: {}", stats_after_conn1.pool_hits);
    println!("  Pool misses: {}", stats_after_conn1.pool_misses);
    println!("  Total idle: {}", stats_after_conn1.total_idle);
    println!("  In use: {}", stats_after_conn1.connections_in_use);

    assert!(
        stats_after_conn1.total_created >= 1,
        "Should have created at least 1 connection"
    );
    assert_eq!(
        stats_after_conn1.pool_misses, 1,
        "Should have 1 pool miss for first connection"
    );
    assert_eq!(
        stats_after_conn1.connections_in_use, 0,
        "No connections should be in use after closing"
    );

    // Connection 2: Should reuse from pool
    println!("\n♻️  Connection 2: Making second connection (expect pool hit)");
    {
        let mut client = timeout(Duration::from_secs(5), TcpStream::connect(socks_addr))
            .await
            .expect("Timeout connecting to SOCKS")
            .expect("Failed to connect to SOCKS");

        socks5_connect(&mut client, echo_addr)
            .await
            .expect("SOCKS5 handshake failed");

        // Send test data
        client.write_all(b"Hello 2").await.expect("Write failed");
        let mut buf = vec![0u8; 7];
        client.read_exact(&mut buf).await.expect("Read failed");
        assert_eq!(&buf, b"Hello 2");

        println!("✓ Connection 2 completed successfully");
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    let stats_after_conn2 = ctx.connection_pool.stats();
    println!("\n📊 Stats after connection 2:");
    println!("  Total created: {}", stats_after_conn2.total_created);
    println!("  Total reused: {}", stats_after_conn2.total_reused);
    println!("  Pool hits: {}", stats_after_conn2.pool_hits);
    println!("  Pool misses: {}", stats_after_conn2.pool_misses);
    println!("  Total idle: {}", stats_after_conn2.total_idle);
    let hit_rate = if stats_after_conn2.pool_hits + stats_after_conn2.pool_misses > 0 {
        100.0 * stats_after_conn2.pool_hits as f64
            / (stats_after_conn2.pool_hits + stats_after_conn2.pool_misses) as f64
    } else {
        0.0
    };
    println!("  Hit rate: {:.1}%", hit_rate);

    // Verify pooling worked
    assert!(
        stats_after_conn2.pool_hits >= 1,
        "Should have at least 1 pool hit (connection reused!)"
    );
    assert!(
        stats_after_conn2.total_reused >= 1,
        "Should have reused at least 1 connection"
    );
    let hit_rate_check = if stats_after_conn2.pool_hits + stats_after_conn2.pool_misses > 0 {
        100.0 * stats_after_conn2.pool_hits as f64
            / (stats_after_conn2.pool_hits + stats_after_conn2.pool_misses) as f64
    } else {
        0.0
    };
    assert!(hit_rate_check > 0.0, "Hit rate should be greater than 0%");

    // Connection 3: Another reuse
    println!("\n♻️  Connection 3: Third connection (expect another pool hit)");
    {
        let mut client = timeout(Duration::from_secs(5), TcpStream::connect(socks_addr))
            .await
            .expect("Timeout connecting to SOCKS")
            .expect("Failed to connect to SOCKS");

        socks5_connect(&mut client, echo_addr)
            .await
            .expect("SOCKS5 handshake failed");

        client.write_all(b"Hello 3").await.expect("Write failed");
        let mut buf = vec![0u8; 7];
        client.read_exact(&mut buf).await.expect("Read failed");
        assert_eq!(&buf, b"Hello 3");

        println!("✓ Connection 3 completed successfully");
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    let final_stats = ctx.connection_pool.stats();
    println!("\n📊 Final stats after connection 3:");
    println!("  Total created: {}", final_stats.total_created);
    println!("  Total reused: {}", final_stats.total_reused);
    println!("  Pool hits: {}", final_stats.pool_hits);
    println!("  Pool misses: {}", final_stats.pool_misses);
    let final_hit_rate = if final_stats.pool_hits + final_stats.pool_misses > 0 {
        100.0 * final_stats.pool_hits as f64
            / (final_stats.pool_hits + final_stats.pool_misses) as f64
    } else {
        0.0
    };
    println!("  Hit rate: {:.1}%", final_hit_rate);

    // Per-destination breakdown
    println!("\n📍 Per-destination stats:");
    for dest_stats in &final_stats.per_destination {
        println!("  Destination: {}", dest_stats.destination);
        println!("    Idle: {}", dest_stats.idle_connections);
        println!("    In use: {}", dest_stats.in_use);
        println!("    Created: {}", dest_stats.total_created);
        println!("    Reused: {}", dest_stats.total_reused);
        println!("    Hits: {}", dest_stats.pool_hits);
        println!("    Misses: {}", dest_stats.pool_misses);
    }

    assert!(
        final_stats.pool_hits >= 2,
        "Should have at least 2 pool hits total"
    );
    let final_rate_check = if final_stats.pool_hits + final_stats.pool_misses > 0 {
        100.0 * final_stats.pool_hits as f64
            / (final_stats.pool_hits + final_stats.pool_misses) as f64
    } else {
        0.0
    };
    assert!(
        final_rate_check >= 50.0,
        "Hit rate should be at least 50% (2 hits out of 3 connections)"
    );

    println!("\n✅ CONNECTION POOLING VERIFIED!");
    println!("   - First connection created new upstream (pool miss)");
    println!("   - Subsequent connections reused from pool (pool hits)");
    println!("   - Hit rate: {:.1}%", final_rate_check);
}

#[tokio::test]
async fn pool_handles_multiple_destinations() {
    let echo1_addr = spawn_echo_server().await;
    let echo2_addr = spawn_echo_server().await;
    println!(
        "✓ Two echo servers started: {} and {}",
        echo1_addr, echo2_addr
    );

    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 2,
        max_total_idle: 10,
        idle_timeout_secs: 30,
        connect_timeout_ms: 3000,
    };

    let (socks_addr, ctx) = spawn_socks_with_pooling(pool_config).await;

    // Connect to destination 1 twice
    for i in 1..=2 {
        let mut client = TcpStream::connect(socks_addr).await.unwrap();
        socks5_connect(&mut client, echo1_addr).await.unwrap();
        client
            .write_all(format!("D1-{}", i).as_bytes())
            .await
            .unwrap();
        let mut buf = vec![0u8; 4];
        client.read_exact(&mut buf).await.unwrap();
        drop(client);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Connect to destination 2 twice
    for i in 1..=2 {
        let mut client = TcpStream::connect(socks_addr).await.unwrap();
        socks5_connect(&mut client, echo2_addr).await.unwrap();
        client
            .write_all(format!("D2-{}", i).as_bytes())
            .await
            .unwrap();
        let mut buf = vec![0u8; 4];
        client.read_exact(&mut buf).await.unwrap();
        drop(client);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    let stats = ctx.connection_pool.stats();
    println!("\n📊 Multi-destination stats:");
    println!("  Destinations tracked: {}", stats.destinations);
    println!("  Total pool hits: {}", stats.pool_hits);
    println!("  Total pool misses: {}", stats.pool_misses);

    assert_eq!(stats.destinations, 2, "Should track 2 destinations");
    assert!(
        stats.pool_hits >= 2,
        "Should have hits for both destinations"
    );

    // Check per-destination stats
    for dest in &stats.per_destination {
        println!("\n  Destination {}:", dest.destination);
        println!("    Hits: {}, Misses: {}", dest.pool_hits, dest.pool_misses);
        assert!(
            dest.pool_hits >= 1,
            "Each destination should have at least 1 hit"
        );
    }

    println!("\n✅ MULTI-DESTINATION POOLING VERIFIED!");
}
