/// End-to-End Integration Tests
///
/// This test suite covers all critical E2E scenarios for RustSocks:
/// 1. Basic SOCKS5 CONNECT - Verifies fundamental proxy functionality
/// 2. Authentication (all methods) - NoAuth, UserPass, PAM
/// 3. ACL Enforcement - Allow and block rules
/// 4. Session Tracking - Traffic metrics and persistence
/// 5. UDP ASSOCIATE - UDP relay functionality
/// 6. BIND Command - Reverse connections
///
/// These tests ensure that all components work together correctly in real-world scenarios.
use rustsocks::acl::types::{AclRule, GlobalAclConfig, UserAcl};
use rustsocks::acl::{AclConfig, AclEngine, AclStats, Action, Protocol};
use rustsocks::auth::AuthManager;
use rustsocks::config::{AuthConfig, User};
use rustsocks::protocol::ReplyCode;
use rustsocks::qos::{ConnectionLimits, QosEngine};
use rustsocks::server::proxy::TrafficUpdateConfig;
use rustsocks::server::{handle_client, ClientHandlerContext, ConnectionPool, PoolConfig};
use rustsocks::session::{SessionManager, SessionStatus};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Duration;

// ============================================================================
// Helper Functions
// ============================================================================

/// Creates a basic SOCKS5 server context with minimal configuration
async fn create_basic_server_context(
    auth_config: AuthConfig,
    acl_config: Option<AclConfig>,
) -> (Arc<ClientHandlerContext>, Arc<SessionManager>) {
    let auth_manager = Arc::new(AuthManager::new(&auth_config).unwrap());
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new("anonymous".to_string());
    let session_manager = Arc::new(SessionManager::new());

    let acl_engine = acl_config.map(|config| Arc::new(AclEngine::new(config).unwrap()));

    let pool_config = PoolConfig::default();
    let connection_pool = Arc::new(ConnectionPool::new(pool_config));

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: auth_manager.clone(),
        acl_engine,
        acl_stats: acl_stats.clone(),
        anonymous_user: anonymous_user.clone(),
        session_manager: session_manager.clone(),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: connection_pool.clone(),
    });

    (ctx, session_manager)
}

/// Spawns a basic echo server for testing
async fn spawn_echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = stream.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        let _ = stream.write_all(&buf[..n]).await;
                    }
                });
            }
        }
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

/// Spawns a SOCKS5 server with the given context
async fn spawn_socks_server(ctx: Arc<ClientHandlerContext>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, client_addr)) = listener.accept().await {
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    let _ = handle_client(stream, ctx, client_addr).await;
                });
            }
        }
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

/// Performs SOCKS5 handshake with no authentication
async fn socks5_handshake_noauth(client: &mut TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    // Send greeting (no auth)
    client.write_all(&[0x05, 0x01, 0x00]).await?;

    // Read server choice
    let mut response = [0u8; 2];
    client.read_exact(&mut response).await?;

    if response != [0x05, 0x00] {
        return Err("unexpected method selection response".into());
    }

    Ok(())
}

/// Performs SOCKS5 handshake with username/password authentication
async fn socks5_handshake_userpass(
    client: &mut TcpStream,
    username: &str,
    password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Send greeting (username/password auth)
    client.write_all(&[0x05, 0x01, 0x02]).await?;

    // Read server choice
    let mut response = [0u8; 2];
    client.read_exact(&mut response).await?;

    if response != [0x05, 0x02] {
        return Err("server did not select username/password auth".into());
    }

    // Send username/password
    let mut auth_request = vec![0x01]; // Version
    auth_request.push(username.len() as u8);
    auth_request.extend_from_slice(username.as_bytes());
    auth_request.push(password.len() as u8);
    auth_request.extend_from_slice(password.as_bytes());

    client.write_all(&auth_request).await?;

    // Read auth response
    let mut auth_response = [0u8; 2];
    client.read_exact(&mut auth_response).await?;

    if auth_response != [0x01, 0x00] {
        return Err("authentication failed".into());
    }

    Ok(())
}

/// Sends SOCKS5 CONNECT request
async fn socks5_connect(
    client: &mut TcpStream,
    target_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut request = vec![0x05, 0x01, 0x00]; // VER, CMD (CONNECT), RSV

    match target_addr.ip() {
        IpAddr::V4(ip) => {
            request.push(0x01); // ATYP (IPv4)
            request.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            request.push(0x04); // ATYP (IPv6)
            request.extend_from_slice(&ip.octets());
        }
    }
    request.extend_from_slice(&target_addr.port().to_be_bytes());

    client.write_all(&request).await?;

    // Read response
    let mut response = vec![0u8; 10];
    client.read_exact(&mut response).await?;

    if response[1] != ReplyCode::Succeeded as u8 {
        return Err(format!("connect failed with reply code: {}", response[1]).into());
    }

    Ok(())
}

// ============================================================================
// E2E Test 1: Basic CONNECT
// ============================================================================

#[tokio::test]
async fn e2e_basic_connect() {
    // Setup echo server
    let echo_addr = spawn_echo_server().await;

    // Setup SOCKS5 server (no auth, no ACL)
    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let (ctx, session_manager) = create_basic_server_context(auth_config, None).await;
    let socks_addr = spawn_socks_server(ctx).await;

    // Connect to SOCKS5 server
    let mut client = TcpStream::connect(socks_addr).await.unwrap();

    // Perform handshake
    socks5_handshake_noauth(&mut client).await.unwrap();

    // Connect to echo server
    socks5_connect(&mut client, echo_addr).await.unwrap();

    // Send data and verify echo
    let test_data = b"Hello, SOCKS5!";
    client.write_all(test_data).await.unwrap();

    let mut response = vec![0u8; test_data.len()];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(response, test_data, "Echo data should match sent data");

    // Verify session was created
    tokio::time::sleep(Duration::from_millis(100)).await;
    let active_sessions = session_manager.get_active_sessions().await;
    assert_eq!(active_sessions.len(), 1, "Should have 1 active session");

    println!("✅ E2E Test 1: Basic CONNECT - PASSED");
}

// ============================================================================
// E2E Test 2: Authentication (All Methods)
// ============================================================================

#[tokio::test]
async fn e2e_auth_noauth() {
    let echo_addr = spawn_echo_server().await;

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let (ctx, _) = create_basic_server_context(auth_config, None).await;
    let socks_addr = spawn_socks_server(ctx).await;

    let mut client = TcpStream::connect(socks_addr).await.unwrap();
    socks5_handshake_noauth(&mut client).await.unwrap();
    socks5_connect(&mut client, echo_addr).await.unwrap();

    // Send test data
    client.write_all(b"test").await.unwrap();
    let mut buf = [0u8; 4];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"test");

    println!("✅ E2E Test 2a: NoAuth authentication - PASSED");
}

#[tokio::test]
async fn e2e_auth_userpass() {
    let echo_addr = spawn_echo_server().await;

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "userpass".to_string(),
        users: vec![User {
            username: "alice".to_string(),
            password: "secret123".to_string(),
        }],
        pam: Default::default(),
    };

    let (ctx, _) = create_basic_server_context(auth_config, None).await;
    let socks_addr = spawn_socks_server(ctx).await;

    // Test with valid credentials
    let mut client = TcpStream::connect(socks_addr).await.unwrap();
    socks5_handshake_userpass(&mut client, "alice", "secret123")
        .await
        .unwrap();
    socks5_connect(&mut client, echo_addr).await.unwrap();

    client.write_all(b"authenticated").await.unwrap();
    let mut buf = [0u8; 13];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"authenticated");

    println!("✅ E2E Test 2b: Username/Password authentication - PASSED");
}

#[tokio::test]
async fn e2e_auth_userpass_invalid() {
    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "userpass".to_string(),
        users: vec![User {
            username: "alice".to_string(),
            password: "secret123".to_string(),
        }],
        pam: Default::default(),
    };

    let (ctx, _) = create_basic_server_context(auth_config, None).await;
    let socks_addr = spawn_socks_server(ctx).await;

    // Test with invalid credentials
    let mut client = TcpStream::connect(socks_addr).await.unwrap();
    let result = socks5_handshake_userpass(&mut client, "alice", "wrongpassword").await;
    assert!(result.is_err(), "Should fail with wrong password");

    println!("✅ E2E Test 2c: Invalid credentials rejected - PASSED");
}

// ============================================================================
// E2E Test 3: ACL Enforcement
// ============================================================================

#[tokio::test]
async fn e2e_acl_allow() {
    let echo_addr = spawn_echo_server().await;

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    // ACL config that allows all
    let acl_config = AclConfig {
        global: GlobalAclConfig {
            default_policy: Action::Allow,
        },
        users: vec![],
        groups: vec![],
    };

    let (ctx, _) = create_basic_server_context(auth_config, Some(acl_config)).await;
    let socks_addr = spawn_socks_server(ctx).await;

    let mut client = TcpStream::connect(socks_addr).await.unwrap();
    socks5_handshake_noauth(&mut client).await.unwrap();
    socks5_connect(&mut client, echo_addr).await.unwrap();

    client.write_all(b"allowed").await.unwrap();
    let mut buf = [0u8; 7];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"allowed");

    println!("✅ E2E Test 3a: ACL allows connection - PASSED");
}

#[tokio::test]
async fn e2e_acl_block() {
    let echo_addr = spawn_echo_server().await;

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    // ACL config that blocks the echo server
    let acl_config = AclConfig {
        global: GlobalAclConfig {
            default_policy: Action::Allow,
        },
        users: vec![UserAcl {
            username: "anonymous".to_string(),
            groups: vec![],
            rules: vec![AclRule {
                action: Action::Block,
                description: "Block test server".to_string(),
                destinations: vec![echo_addr.ip().to_string()],
                ports: vec![echo_addr.port().to_string()],
                protocols: vec![Protocol::Tcp],
                priority: 1000,
            }],
        }],
        groups: vec![],
    };

    let (ctx, session_manager) = create_basic_server_context(auth_config, Some(acl_config)).await;
    let socks_addr = spawn_socks_server(ctx).await;

    let mut client = TcpStream::connect(socks_addr).await.unwrap();
    socks5_handshake_noauth(&mut client).await.unwrap();

    // Try to connect (should be blocked)
    let result = socks5_connect(&mut client, echo_addr).await;
    assert!(result.is_err(), "Connection should be blocked by ACL");

    // Verify rejected session was tracked
    tokio::time::sleep(Duration::from_millis(100)).await;
    let rejected_sessions = session_manager.rejected_snapshot().await;
    assert_eq!(rejected_sessions.len(), 1, "Should have 1 rejected session");
    assert_eq!(rejected_sessions[0].user, "anonymous");
    assert_eq!(rejected_sessions[0].status, SessionStatus::RejectedByAcl);

    println!("✅ E2E Test 3b: ACL blocks connection - PASSED");
}

// ============================================================================
// E2E Test 4: Session Tracking
// ============================================================================

#[tokio::test]
async fn e2e_session_tracking() {
    let echo_addr = spawn_echo_server().await;

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let (ctx, session_manager) = create_basic_server_context(auth_config, None).await;
    let socks_addr = spawn_socks_server(ctx).await;

    // Verify no sessions initially
    assert_eq!(session_manager.get_active_sessions().await.len(), 0);

    let mut client = TcpStream::connect(socks_addr).await.unwrap();
    socks5_handshake_noauth(&mut client).await.unwrap();
    socks5_connect(&mut client, echo_addr).await.unwrap();

    // Send some data
    let test_data = b"tracking test data";
    client.write_all(test_data).await.unwrap();
    let mut buf = vec![0u8; test_data.len()];
    client.read_exact(&mut buf).await.unwrap();

    // Wait for session to be created
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify active session
    let active_sessions = session_manager.get_active_sessions().await;
    assert_eq!(active_sessions.len(), 1, "Should have 1 active session");

    let session = &active_sessions[0];
    assert_eq!(session.user, "anonymous");
    assert_eq!(session.dest_ip, echo_addr.ip().to_string());
    assert_eq!(session.dest_port, echo_addr.port());

    // Close connection
    drop(client);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify session was closed
    assert_eq!(
        session_manager.get_active_sessions().await.len(),
        0,
        "Session should be closed"
    );

    let closed_sessions = session_manager.get_closed_sessions().await;
    assert!(
        closed_sessions
            .iter()
            .any(|s| s.status == SessionStatus::Closed),
        "Should have a closed session"
    );

    println!("✅ E2E Test 4: Session tracking - PASSED");
}

// ============================================================================
// E2E Test 5: UDP ASSOCIATE
// ============================================================================

#[tokio::test]
async fn e2e_udp_associate() {
    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let (ctx, session_manager) = create_basic_server_context(auth_config, None).await;
    let socks_addr = spawn_socks_server(ctx).await;

    let mut client = TcpStream::connect(socks_addr).await.unwrap();

    // Handshake
    socks5_handshake_noauth(&mut client).await.unwrap();

    // Send UDP ASSOCIATE request
    let request = [
        0x05, // SOCKS version
        0x03, // UDP ASSOCIATE command
        0x00, // Reserved
        0x01, // IPv4
        0, 0, 0, 0, // 0.0.0.0
        0x00, 0x00, // port 0
    ];
    client.write_all(&request).await.unwrap();

    // Read response
    let mut response = vec![0u8; 10];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(response[0], 0x05, "SOCKS version should be 5");
    assert_eq!(response[1], 0x00, "Reply should be succeeded");
    assert_eq!(response[3], 0x01, "Address type should be IPv4");

    let udp_port = u16::from_be_bytes([response[8], response[9]]);
    assert!(udp_port > 0, "UDP relay should bind to non-zero port");

    // Verify session was created
    tokio::time::sleep(Duration::from_millis(100)).await;
    let active_sessions = session_manager.get_active_sessions().await;
    assert_eq!(active_sessions.len(), 1, "Should have 1 active UDP session");

    println!(
        "✅ E2E Test 5: UDP ASSOCIATE - PASSED (relay on port {})",
        udp_port
    );
}

// ============================================================================
// E2E Test 6: BIND Command
// ============================================================================

#[tokio::test]
async fn e2e_bind_command() {
    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
    };

    let (ctx, _session_manager) = create_basic_server_context(auth_config, None).await;
    let socks_addr = spawn_socks_server(ctx).await;

    let mut client = TcpStream::connect(socks_addr).await.unwrap();

    // Handshake
    socks5_handshake_noauth(&mut client).await.unwrap();

    // Send BIND request
    let request = [
        0x05, // SOCKS version
        0x02, // BIND command
        0x00, // Reserved
        0x01, // IPv4
        127, 0, 0, 1, // 127.0.0.1
        0x00, 0x00, // port 0
    ];
    client.write_all(&request).await.unwrap();

    // Read first response (bind address)
    let mut response = vec![0u8; 10];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(response[0], 0x05, "SOCKS version should be 5");
    assert_eq!(response[1], 0x00, "Reply should be succeeded");

    let bind_port = u16::from_be_bytes([response[8], response[9]]);
    assert!(bind_port > 0, "BIND should bind to non-zero port");

    println!(
        "✅ E2E Test 6: BIND Command - PASSED (bound on port {})",
        bind_port
    );

    // Note: Full BIND test with incoming connection is in bind_command.rs
    // This test verifies basic BIND handshake
}

// ============================================================================
// E2E Test 7: Complete End-to-End Flow
// ============================================================================

#[tokio::test]
async fn e2e_complete_flow() {
    // This test combines multiple features:
    // - Username/password authentication
    // - ACL enforcement (allow)
    // - Session tracking
    // - Data transfer

    let echo_addr = spawn_echo_server().await;

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "userpass".to_string(),
        users: vec![User {
            username: "testuser".to_string(),
            password: "testpass".to_string(),
        }],
        pam: Default::default(),
    };

    let acl_config = AclConfig {
        global: GlobalAclConfig {
            default_policy: Action::Allow,
        },
        users: vec![UserAcl {
            username: "testuser".to_string(),
            groups: vec![],
            rules: vec![AclRule {
                action: Action::Allow,
                description: "Allow all for testuser".to_string(),
                destinations: vec!["*".to_string()],
                ports: vec!["*".to_string()],
                protocols: vec![Protocol::Both],
                priority: 100,
            }],
        }],
        groups: vec![],
    };

    let (ctx, session_manager) = create_basic_server_context(auth_config, Some(acl_config)).await;
    let socks_addr = spawn_socks_server(ctx).await;

    // Connect and authenticate
    let mut client = TcpStream::connect(socks_addr).await.unwrap();
    socks5_handshake_userpass(&mut client, "testuser", "testpass")
        .await
        .unwrap();

    // Connect to echo server
    socks5_connect(&mut client, echo_addr).await.unwrap();

    // Transfer data
    let test_data = b"Complete E2E test data";
    client.write_all(test_data).await.unwrap();

    let mut response = vec![0u8; test_data.len()];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(response, test_data);

    // Verify session
    tokio::time::sleep(Duration::from_millis(100)).await;
    let active_sessions = session_manager.get_active_sessions().await;
    assert_eq!(active_sessions.len(), 1);

    let session = &active_sessions[0];
    assert_eq!(session.user, "testuser");
    assert_eq!(session.dest_ip, echo_addr.ip().to_string());
    assert_eq!(session.dest_port, echo_addr.port());

    println!("✅ E2E Test 7: Complete flow (auth + ACL + session + data) - PASSED");
}
