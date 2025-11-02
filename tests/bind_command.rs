use rustsocks::acl::{load_acl_config_sync, AclEngine, AclStats};
use rustsocks::auth::AuthManager;
use rustsocks::config::{AuthConfig, PamSettings};
use rustsocks::qos::{ConnectionLimits, QosEngine};
use rustsocks::server::{
    handle_client, ClientHandlerContext, ConnectionPool, PoolConfig, TrafficUpdateConfig,
};
use rustsocks::session::SessionManager;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn bind_basic_handshake() {
    // Setup server
    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: PamSettings::default(),
    };
    let auth_manager = Arc::new(AuthManager::new(&auth_config).unwrap());
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new("anonymous".to_string());
    let session_manager = Arc::new(SessionManager::new());

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: auth_manager.clone(),
        acl_engine: None,
        acl_stats: acl_stats.clone(),
        anonymous_user: anonymous_user.clone(),
        session_manager: session_manager.clone(),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
    });

    // Start SOCKS5 server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (stream, client_addr) = listener.accept().await.unwrap();
        handle_client(stream, ctx, client_addr).await.ok();
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Client connects
    let mut client = TcpStream::connect(server_addr).await.unwrap();

    // Send greeting (no auth)
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();

    // Read server choice
    let mut buf = [0u8; 2];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf[0], 0x05); // SOCKS version
    assert_eq!(buf[1], 0x00); // NoAuth

    // Send BIND request (to any destination)
    let request = [
        0x05, // SOCKS version
        0x02, // BIND command
        0x00, // Reserved
        0x01, // IPv4
        0, 0, 0, 0, // 0.0.0.0
        0x00, 0x00, // port 0
    ];
    client.write_all(&request).await.unwrap();

    // Read first response (bind address)
    let mut response = vec![0u8; 10];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(response[0], 0x05); // SOCKS version
    assert_eq!(response[1], 0x00); // Succeeded
    assert_eq!(response[2], 0x00); // Reserved

    let atyp = response[3];
    assert_eq!(atyp, 0x01); // IPv4

    let bind_port = u16::from_be_bytes([response[8], response[9]]);
    assert!(bind_port > 0); // BIND should bind to non-zero port

    println!(
        "BIND handshake test completed - server listening on port {}",
        bind_port
    );

    // Close client connection
    drop(client);
}

#[tokio::test]
async fn bind_with_incoming_connection() {
    // Setup server
    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: PamSettings::default(),
    };
    let auth_manager = Arc::new(AuthManager::new(&auth_config).unwrap());
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new("anonymous".to_string());
    let session_manager = Arc::new(SessionManager::new());

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: auth_manager.clone(),
        acl_engine: None,
        acl_stats: acl_stats.clone(),
        anonymous_user: anonymous_user.clone(),
        session_manager: session_manager.clone(),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
    });

    // Start SOCKS5 server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (stream, client_addr) = listener.accept().await.unwrap();
        handle_client(stream, ctx, client_addr).await.ok();
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Client connects
    let mut client = TcpStream::connect(server_addr).await.unwrap();

    // Send greeting (no auth)
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();

    // Read server choice
    let mut buf = [0u8; 2];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf[0], 0x05);
    assert_eq!(buf[1], 0x00);

    // Send BIND request
    let request = [0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x00];
    client.write_all(&request).await.unwrap();

    // Read first response (bind address)
    let mut response = vec![0u8; 10];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(response[0], 0x05);
    assert_eq!(response[1], 0x00); // Succeeded

    let bind_addr = format!(
        "{}.{}.{}.{}",
        response[4], response[5], response[6], response[7]
    );
    let bind_port = u16::from_be_bytes([response[8], response[9]]);

    println!(
        "BIND command received - server listening on {}:{}",
        bind_addr, bind_port
    );

    // Simulate incoming connection from remote server
    let bind_socket = format!("127.0.0.1:{}", bind_port);
    let incoming = TcpStream::connect(bind_socket).await.unwrap();

    // Client should receive second response with incoming connection address
    let mut second_response = vec![0u8; 10];
    let read_result = tokio::time::timeout(
        tokio::time::Duration::from_secs(2),
        client.read_exact(&mut second_response),
    )
    .await;

    assert!(
        read_result.is_ok(),
        "Should receive second response within timeout"
    );

    assert_eq!(second_response[0], 0x05); // SOCKS version
    assert_eq!(second_response[1], 0x00); // Succeeded

    println!(
        "BIND command test completed - received second response with incoming connection info"
    );

    // Close connections
    drop(client);
    drop(incoming);
}

#[tokio::test]
async fn bind_with_acl_allow() {
    // Create ACL config that allows BIND to localhost
    let acl_toml = r#"
[global]
default_policy = "block"

[[users]]
username = "anonymous"
  [[users.rules]]
  action = "allow"
  description = "Allow BIND to localhost"
  destinations = ["127.0.0.1"]
  ports = ["*"]
  protocols = ["tcp"]
  priority = 100
"#;

    let temp_dir = tempfile::tempdir().unwrap();
    let acl_path = temp_dir.path().join("acl.toml");
    std::fs::write(&acl_path, acl_toml).unwrap();

    let acl_config = load_acl_config_sync(&acl_path).unwrap();
    let acl_engine = Arc::new(AclEngine::new(acl_config).unwrap());

    // Setup server with ACL
    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: PamSettings::default(),
    };
    let auth_manager = Arc::new(AuthManager::new(&auth_config).unwrap());
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new("anonymous".to_string());
    let session_manager = Arc::new(SessionManager::new());

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: auth_manager.clone(),
        acl_engine: Some(acl_engine),
        acl_stats: acl_stats.clone(),
        anonymous_user: anonymous_user.clone(),
        session_manager: session_manager.clone(),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
    });

    // Start SOCKS5 server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (stream, client_addr) = listener.accept().await.unwrap();
        handle_client(stream, ctx, client_addr).await.ok();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Client connects
    let mut client = TcpStream::connect(server_addr).await.unwrap();

    // Send greeting
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut buf = [0u8; 2];
    client.read_exact(&mut buf).await.unwrap();

    // Send BIND request to allowed destination
    let request = [0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x00];
    client.write_all(&request).await.unwrap();

    // Read response - should succeed
    let mut response = vec![0u8; 10];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(response[1], 0x00); // Succeeded

    // Verify ACL stats
    let user_stats = acl_stats.user_snapshot("anonymous").unwrap();
    assert_eq!(user_stats.allowed, 1);

    println!("BIND with ACL allow test completed");
}

#[tokio::test]
async fn bind_with_acl_block() {
    // Create ACL config that blocks BIND
    let acl_toml = r#"
[global]
default_policy = "block"

[[users]]
username = "anonymous"
  [[users.rules]]
  action = "block"
  description = "Block all BIND"
  destinations = ["*"]  # Match all
  ports = ["*"]  # Match all
  protocols = ["tcp"]
  priority = 100
"#;

    let temp_dir = tempfile::tempdir().unwrap();
    let acl_path = temp_dir.path().join("acl.toml");
    std::fs::write(&acl_path, acl_toml).unwrap();

    let acl_config = load_acl_config_sync(&acl_path).unwrap();
    let acl_engine = Arc::new(AclEngine::new(acl_config).unwrap());

    // Setup server with ACL
    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: PamSettings::default(),
    };
    let auth_manager = Arc::new(AuthManager::new(&auth_config).unwrap());
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new("anonymous".to_string());
    let session_manager = Arc::new(SessionManager::new());

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: auth_manager.clone(),
        acl_engine: Some(acl_engine),
        acl_stats: acl_stats.clone(),
        anonymous_user: anonymous_user.clone(),
        session_manager: session_manager.clone(),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
    });

    // Start SOCKS5 server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (stream, client_addr) = listener.accept().await.unwrap();
        handle_client(stream, ctx, client_addr).await.ok();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Client connects
    let mut client = TcpStream::connect(server_addr).await.unwrap();

    // Send greeting
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut buf = [0u8; 2];
    client.read_exact(&mut buf).await.unwrap();

    // Send BIND request - should be blocked
    let request = [0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x00];
    client.write_all(&request).await.unwrap();

    // Read response - should be blocked
    let mut response = vec![0u8; 10];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(response[1], 0x02); // ConnectionNotAllowed

    // Verify ACL stats
    let user_stats = acl_stats.user_snapshot("anonymous").unwrap();
    assert_eq!(user_stats.blocked, 1);

    println!("BIND with ACL block test completed");
}
