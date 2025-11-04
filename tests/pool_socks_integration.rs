/// Connection Pool Integration with SOCKS5 Handler
///
/// Tests verifying pool works correctly in real SOCKS5 scenarios
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
async fn pool_integrates_with_socks5_multiple_requests() {
    // Pool with aggressive limits to test eviction
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 2,
        max_total_idle: 5,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let connection_pool = Arc::new(ConnectionPool::new(pool_config));

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: Arc::new(AuthManager::new(&auth_config).unwrap()),
        acl_engine: None,
        acl_stats: Arc::new(AclStats::new()),
        anonymous_user: Arc::new("anonymous".to_string()),
        session_manager: Arc::new(SessionManager::new()),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: connection_pool.clone(),
    });

    // SOCKS server
    let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = socks_listener.local_addr().unwrap();

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

    // Upstream servers
    let mut upstream_servers = Vec::new();
    for _ in 0..3 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4];
                        if stream.read_exact(&mut buf).await.is_ok() {
                            stream.write_all(&buf).await.ok();
                        }
                    });
                }
            }
        });

        upstream_servers.push(addr);
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Make multiple SOCKS5 requests to same destination
    for i in 0..5 {
        let server = upstream_servers[i % upstream_servers.len()];
        let mut client = TcpStream::connect(socks_addr).await.unwrap();

        // SOCKS5 handshake
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut buf = [0u8; 2];
        client.read_exact(&mut buf).await.unwrap();

        // CONNECT
        let port = server.port();
        let request = [
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
        client.write_all(&request).await.unwrap();

        let mut response = [0u8; 10];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(response[1], 0x00, "SOCKS connect should succeed");

        // Use connection
        client.write_all(b"data").await.unwrap();
        let mut reply = [0u8; 4];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"data");

        drop(client);
    }

    // Check pool stats
    let stats = connection_pool.stats();
    assert!(stats.total_idle <= 5, "Pool should respect global limit");
    assert!(
        stats.destinations <= 3,
        "Should have connections to at most 3 destinations"
    );

    println!(
        "Pool after 5 SOCKS requests: {} idle in {} destinations",
        stats.total_idle, stats.destinations
    );
}

#[tokio::test]
async fn pool_handles_connection_failure_mid_request() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 5,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 1000,
    };
    let connection_pool = Arc::new(ConnectionPool::new(pool_config));

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: Arc::new(AuthManager::new(&auth_config).unwrap()),
        acl_engine: None,
        acl_stats: Arc::new(AclStats::new()),
        anonymous_user: Arc::new("anonymous".to_string()),
        session_manager: Arc::new(SessionManager::new()),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: connection_pool.clone(),
    });

    let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = socks_listener.local_addr().unwrap();

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

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Try to connect to non-existent server
    let mut client = TcpStream::connect(socks_addr).await.unwrap();

    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut buf = [0u8; 2];
    client.read_exact(&mut buf).await.unwrap();

    // Connect to unreachable address
    let request = [
        0x05, 0x01, 0x00, 0x01, 192, 0, 2, 1, // 192.0.2.1 (TEST-NET-1, non-routable)
        0x27, 0x0F, // Port 9999
    ];
    client.write_all(&request).await.unwrap();

    let mut response = [0u8; 10];
    let result = client.read_exact(&mut response).await;

    // Should get error response or connection close
    if result.is_ok() {
        assert_ne!(
            response[1], 0x00,
            "Should not succeed connecting to non-routable address"
        );
    }

    // Pool should not be affected by failed connections
    let stats = connection_pool.stats();
    assert_eq!(stats.total_idle, 0, "No connections should be pooled");
}

#[tokio::test]
async fn pool_with_pooling_disabled_still_works() {
    let pool_config = PoolConfig {
        enabled: false, // Disabled!
        max_idle_per_dest: 5,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let connection_pool = Arc::new(ConnectionPool::new(pool_config));

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: Arc::new(AuthManager::new(&auth_config).unwrap()),
        acl_engine: None,
        acl_stats: Arc::new(AclStats::new()),
        anonymous_user: Arc::new("anonymous".to_string()),
        session_manager: Arc::new(SessionManager::new()),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: connection_pool.clone(),
    });

    let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = socks_listener.local_addr().unwrap();

    let ctx_server = ctx.clone();
    tokio::spawn(async move {
        if let Ok((stream, client_addr)) = socks_listener.accept().await {
            handle_client(stream, ctx_server, client_addr).await.ok();
        }
    });

    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream_listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = upstream_listener.accept().await {
            let mut buf = [0u8; 4];
            if stream.read_exact(&mut buf).await.is_ok() {
                stream.write_all(&buf).await.ok();
            }
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Normal SOCKS request
    let mut client = TcpStream::connect(socks_addr).await.unwrap();
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut buf = [0u8; 2];
    client.read_exact(&mut buf).await.unwrap();

    let port = upstream_addr.port();
    let request = [
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
    client.write_all(&request).await.unwrap();

    let mut response = [0u8; 10];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(response[1], 0x00, "Should work even with pooling disabled");

    client.write_all(b"test").await.unwrap();
    let mut reply = [0u8; 4];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"test");

    drop(client);

    // Pool should be empty
    let stats = connection_pool.stats();
    assert_eq!(stats.total_idle, 0);
    assert_eq!(stats.destinations, 0);
}

#[tokio::test]
async fn pool_stats_reflect_real_usage() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 5,
        max_total_idle: 20,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let connection_pool = Arc::new(ConnectionPool::new(pool_config.clone()));

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: Arc::new(AuthManager::new(&auth_config).unwrap()),
        acl_engine: None,
        acl_stats: Arc::new(AclStats::new()),
        anonymous_user: Arc::new("anonymous".to_string()),
        session_manager: Arc::new(SessionManager::new()),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: connection_pool.clone(),
    });

    let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = socks_listener.local_addr().unwrap();

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

    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream_listener.local_addr().unwrap();

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

    // Make 3 requests
    for _ in 0..3 {
        let mut client = TcpStream::connect(socks_addr).await.unwrap();
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut buf = [0u8; 2];
        client.read_exact(&mut buf).await.unwrap();

        let port = upstream_addr.port();
        let request = [
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
        client.write_all(&request).await.unwrap();

        let mut response = [0u8; 10];
        client.read_exact(&mut response).await.unwrap();

        client.write_all(b"test").await.unwrap();
        let mut reply = [0u8; 4];
        client.read_exact(&mut reply).await.unwrap();

        drop(client);
    }

    // Give time for connections to be returned to pool
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let stats = connection_pool.stats();

    println!(
        "Pool stats: {} idle connections to {} destinations",
        stats.total_idle, stats.destinations
    );

    assert_eq!(stats.config.enabled, true);
    assert_eq!(
        stats.config.max_idle_per_dest,
        pool_config.max_idle_per_dest
    );
    assert_eq!(stats.config.max_total_idle, pool_config.max_total_idle);

    // May have some connections pooled
    assert!(stats.total_idle <= stats.config.max_total_idle);
    if stats.destinations > 0 {
        assert_eq!(stats.destinations, 1, "Should only have 1 destination");
    }
}
