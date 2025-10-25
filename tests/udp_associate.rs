use rustsocks::acl::{load_acl_config_sync, AclEngine, AclStats};
use rustsocks::auth::AuthManager;
use rustsocks::config::AuthConfig;
use rustsocks::server::{handle_client, ClientHandlerContext, TrafficUpdateConfig};
use rustsocks::session::SessionManager;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn udp_associate_basic_flow() {
    // Setup server
    let auth_config = AuthConfig {
        method: "none".to_string(),
        users: vec![],
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

    // Send UDP ASSOCIATE request (destination doesn't matter for now)
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

    assert_eq!(response[0], 0x05); // SOCKS version
    assert_eq!(response[1], 0x00); // Succeeded (UDP relay started)
    assert_eq!(response[2], 0x00); // Reserved

    let atyp = response[3];
    assert_eq!(atyp, 0x01); // IPv4

    let port = u16::from_be_bytes([response[8], response[9]]);
    assert!(port > 0); // UDP relay should bind to non-zero port

    println!("UDP ASSOCIATE test completed - relay bound on port {}", port);

    // Close TCP connection (should terminate UDP session)
    drop(client);
}

#[tokio::test]
async fn udp_associate_with_acl_allow() {
    // Create ACL config that allows UDP to localhost
    let acl_toml = r#"
[global]
default_policy = "block"

[[users]]
username = "anonymous"
  [[users.rules]]
  action = "allow"
  description = "Allow UDP to localhost"
  destinations = ["127.0.0.1"]
  ports = ["*"]
  protocols = ["udp"]
  priority = 100
"#;

    let temp_dir = tempfile::tempdir().unwrap();
    let acl_path = temp_dir.path().join("acl.toml");
    std::fs::write(&acl_path, acl_toml).unwrap();

    let acl_config = load_acl_config_sync(&acl_path).unwrap();
    let acl_engine = Arc::new(AclEngine::new(acl_config).unwrap());

    // Setup server with ACL
    let auth_config = AuthConfig {
        method: "none".to_string(),
        users: vec![],
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

    // Send UDP ASSOCIATE request
    let request = [
        0x05, 0x03, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x00,
    ];
    client.write_all(&request).await.unwrap();

    // Read response - should succeed
    let mut response = vec![0u8; 10];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(response[1], 0x00); // Succeeded

    // Verify ACL stats
    let user_stats = acl_stats.user_snapshot("anonymous").unwrap();
    assert_eq!(user_stats.allowed, 1);
}

#[tokio::test]
async fn udp_associate_with_acl_block() {
    // Create ACL config that blocks UDP
    let acl_toml = r#"
[global]
default_policy = "block"

[[users]]
username = "anonymous"
  [[users.rules]]
  action = "block"
  description = "Block all UDP"
  destinations = ["*"]
  ports = ["*"]
  protocols = ["udp"]
  priority = 100
"#;

    let temp_dir = tempfile::tempdir().unwrap();
    let acl_path = temp_dir.path().join("acl.toml");
    std::fs::write(&acl_path, acl_toml).unwrap();

    let acl_config = load_acl_config_sync(&acl_path).unwrap();
    let acl_engine = Arc::new(AclEngine::new(acl_config).unwrap());

    // Setup server with ACL
    let auth_config = AuthConfig {
        method: "none".to_string(),
        users: vec![],
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

    // Send UDP ASSOCIATE request
    let request = [
        0x05, 0x03, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x00,
    ];
    client.write_all(&request).await.unwrap();

    // Read response - should be blocked
    let mut response = vec![0u8; 10];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(response[1], 0x02); // ConnectionNotAllowed

    // Verify ACL stats
    let user_stats = acl_stats.user_snapshot("anonymous").unwrap();
    assert_eq!(user_stats.blocked, 1);
}
