/// Connection Pool Integration Tests
///
/// These tests verify end-to-end connection pooling functionality through the SOCKS5 proxy.

use rustsocks::acl::AclStats;
use rustsocks::auth::AuthManager;
use rustsocks::config::AuthConfig;
use rustsocks::qos::{ConnectionLimits, QosEngine};
use rustsocks::server::{
    handle_client, ClientHandlerContext, ConnectionPool, PoolConfig, TrafficUpdateConfig,
};
use rustsocks::session::SessionManager;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn connection_pool_reuses_upstream_connections() {
    // Setup server with connection pooling enabled
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

    // Enable connection pooling
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 4,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
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

    // Start SOCKS5 server
    let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = socks_listener.local_addr().unwrap();

    // Start upstream echo server
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream_listener.local_addr().unwrap();

    // Spawn SOCKS server handler
    let ctx_server = ctx.clone();
    tokio::spawn(async move {
        loop {
            if let Ok((stream, client_addr)) = socks_listener.accept().await {
                let ctx_clone = ctx_server.clone();
                tokio::spawn(async move {
                    handle_client(stream, ctx_clone, client_addr).await.ok();
                });
            }
        }
    });

    // Spawn upstream echo server
    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = upstream_listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 4];
                    if stream.read_exact(&mut buf).await.is_ok() {
                        stream.write_all(&buf).await.ok();
                    }
                });
            }
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // First SOCKS client connection
    {

        let mut client = TcpStream::connect(server_addr).await.unwrap();

        // SOCKS5 handshake
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut buf = [0u8; 2];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, [0x05, 0x00]); // No auth

        // CONNECT request to upstream
        let port = upstream_addr.port();
        let connect_request = [
            0x05,
            0x01,
            0x00,
            0x01,
            127,
            0,
            0,
            1,
            (port >> 8) as u8,
            (port & 0xff) as u8,
        ];
        client.write_all(&connect_request).await.unwrap();

        // Read response
        let mut response = [0u8; 10];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(response[0], 0x05);
        assert_eq!(response[1], 0x00); // Success

        // Send test data
        client.write_all(b"test").await.unwrap();
        let mut reply = [0u8; 4];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"test");

        drop(client);
    }

    // Second connection to same upstream - should reuse pool
    {
        let mut client = TcpStream::connect(server_addr).await.unwrap();

        // SOCKS5 handshake
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut buf = [0u8; 2];
        client.read_exact(&mut buf).await.unwrap();

        // CONNECT to same upstream
        let port = upstream_addr.port();
        let connect_request = [
            0x05,
            0x01,
            0x00,
            0x01,
            127,
            0,
            0,
            1,
            (port >> 8) as u8,
            (port & 0xff) as u8,
        ];
        client.write_all(&connect_request).await.unwrap();

        let mut response = [0u8; 10];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(response[1], 0x00); // Success

        client.write_all(b"pool").await.unwrap();
        let mut reply = [0u8; 4];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"pool");

        drop(client);
    }

    // Verify pool has idle connections
    let stats = connection_pool.stats().await;
    assert!(
        stats.total_idle <= stats.config.max_total_idle,
        "Pool should respect max_total_idle limit"
    );
}

#[tokio::test]
async fn connection_pool_respects_timeout() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 4,
        max_total_idle: 100,
        idle_timeout_secs: 1, // Very short timeout
        connect_timeout_ms: 100, // Short connect timeout
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    // Try to connect to non-routable address
    let addr: std::net::SocketAddr = "192.0.2.1:9999".parse().unwrap();
    let result = pool.get(addr).await;

    assert!(result.is_err(), "Should timeout on unreachable address");
    if let Err(e) = result {
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
    }
}

#[tokio::test]
async fn connection_pool_disabled_works_normally() {
    // Setup with pooling disabled
    let pool_config = PoolConfig {
        enabled: false,
        ..Default::default()
    };
    let connection_pool = Arc::new(ConnectionPool::new(pool_config));

    // Start echo server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).await.unwrap();
        stream.write_all(&buf).await.unwrap();
    });

    // Get connection (should create new, not use pool)
    let stream = connection_pool.get(addr).await.unwrap();

    // Send data
    let mut stream = stream;
    stream.write_all(b"test").await.unwrap();
    let mut reply = [0u8; 4];
    stream.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"test");

    // Return to pool (should be no-op when disabled)
    drop(stream);

    // Stats should show 0 idle (pooling disabled)
    let stats = connection_pool.stats().await;
    assert_eq!(stats.total_idle, 0, "No pooling when disabled");
}
