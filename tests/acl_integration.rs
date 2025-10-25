use rustsocks::acl::types::{AclRule, GlobalAclConfig, UserAcl};
use rustsocks::acl::{AclConfig, AclEngine, AclStats, Action, Protocol};
use rustsocks::auth::AuthManager;
use rustsocks::config::AuthConfig;
use rustsocks::protocol::ReplyCode;
use rustsocks::server::handler::handle_client;
use rustsocks::server::proxy::TrafficUpdateConfig;
use rustsocks::session::{SessionManager, SessionStatus};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn blocking_acl_config() -> AclConfig {
    AclConfig {
        global: GlobalAclConfig {
            default_policy: Action::Allow,
        },
        users: vec![UserAcl {
            username: "anonymous".to_string(),
            groups: vec![],
            rules: vec![AclRule {
                action: Action::Block,
                description: "Block blocked.example.com".to_string(),
                destinations: vec!["blocked.example.com".to_string()],
                ports: vec!["*".to_string()],
                protocols: vec![Protocol::Tcp],
                priority: 1000,
            }],
        }],
        groups: vec![],
    }
}

fn allowing_acl_config() -> AclConfig {
    AclConfig {
        global: GlobalAclConfig {
            default_policy: Action::Allow,
        },
        users: vec![],
        groups: vec![],
    }
}

#[tokio::test]
async fn acl_blocks_connection_and_tracks_stats() {
    // Authentication set to no-auth for simplicity
    let auth_manager = Arc::new(
        AuthManager::new(&AuthConfig {
            method: "none".into(),
            users: Vec::new(),
        })
        .expect("auth manager"),
    );

    let acl_engine = Arc::new(AclEngine::new(blocking_acl_config()).expect("acl engine"));
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new(String::from("anonymous"));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("listener addr");
    let session_manager = Arc::new(SessionManager::new());

    let server_task = {
        let auth_manager = auth_manager.clone();
        let acl_engine = Some(acl_engine.clone());
        let acl_stats = acl_stats.clone();
        let anonymous_user = anonymous_user.clone();
        let session_manager = session_manager.clone();
        let traffic_config = TrafficUpdateConfig::default();

        tokio::spawn(async move {
            let (stream, client_addr) = listener.accept().await.expect("accept test client");
            handle_client(
                stream,
                auth_manager,
                acl_engine,
                acl_stats,
                anonymous_user,
                session_manager,
                traffic_config,
                client_addr,
            )
            .await
            .expect("handler should complete");
        })
    };

    let mut client = TcpStream::connect(addr).await.expect("connect to handler");

    // Greeting (method negotiation)
    client
        .write_all(&[0x05, 0x01, 0x00])
        .await
        .expect("send greeting");

    let mut response = [0u8; 2];
    client
        .read_exact(&mut response)
        .await
        .expect("read method selection");
    assert_eq!(
        response,
        [0x05, 0x00],
        "Server should accept NO AUTH method"
    );

    // Send CONNECT request to blocked domain
    let domain = "blocked.example.com";
    let mut request = Vec::new();
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x03]);
    request.push(domain.len() as u8);
    request.extend_from_slice(domain.as_bytes());
    request.extend_from_slice(&80u16.to_be_bytes());
    client
        .write_all(&request)
        .await
        .expect("send connect request");

    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.expect("read reply");

    assert_eq!(reply[0], 0x05);
    assert_eq!(reply[1], ReplyCode::ConnectionNotAllowed as u8);
    assert_eq!(reply[3], 0x01); // IPv4 zero address

    // Drop client connection so handler can exit cleanly
    drop(client);
    server_task.await.expect("handler finished");

    let totals = acl_stats.snapshot();
    assert_eq!(totals.allowed, 0);
    assert_eq!(totals.blocked, 1);

    let user_stats = acl_stats
        .user_snapshot("anonymous")
        .expect("anonymous user should have stats");
    assert_eq!(user_stats.blocked, 1);
    assert_eq!(user_stats.allowed, 0);

    let rejected = session_manager.rejected_snapshot();
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].user, "anonymous");
    assert_eq!(rejected[0].dest_ip, "blocked.example.com");
}

#[tokio::test]
async fn acl_allows_connection_and_creates_session() {
    let auth_manager = Arc::new(
        AuthManager::new(&AuthConfig {
            method: "none".into(),
            users: Vec::new(),
        })
        .expect("auth manager"),
    );

    let acl_engine = Arc::new(AclEngine::new(allowing_acl_config()).expect("acl engine"));
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new(String::from("anonymous"));

    let upstream_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream listener");
    let upstream_addr = upstream_listener.local_addr().unwrap();

    let upstream_task = tokio::spawn(async move {
        if let Ok((stream, _)) = upstream_listener.accept().await {
            drop(stream);
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("listener addr");
    let session_manager = Arc::new(SessionManager::new());

    let server_task = {
        let auth_manager = auth_manager.clone();
        let acl_engine = Some(acl_engine.clone());
        let acl_stats = acl_stats.clone();
        let anonymous_user = anonymous_user.clone();
        let session_manager = session_manager.clone();

        tokio::spawn(async move {
            if let Ok((stream, client_addr)) = listener.accept().await {
                handle_client(
                    stream,
                    auth_manager,
                    acl_engine,
                    acl_stats,
                    anonymous_user,
                    session_manager,
                    TrafficUpdateConfig::default(),
                    client_addr,
                )
                .await
                .expect("handler should complete");
            }
        })
    };

    let mut client = TcpStream::connect(addr).await.expect("connect to handler");

    // Greeting (method negotiation)
    client
        .write_all(&[0x05, 0x01, 0x00])
        .await
        .expect("send greeting");

    let mut response = [0u8; 2];
    client
        .read_exact(&mut response)
        .await
        .expect("read method selection");
    assert_eq!(response, [0x05, 0x00]);

    // CONNECT request targeting the upstream listener
    let octets = upstream_addr.ip().to_string();
    let ip: std::net::Ipv4Addr = octets.parse().expect("ipv4 parse");
    let mut request = Vec::new();
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x01]);
    request.extend_from_slice(&ip.octets());
    request.extend_from_slice(&upstream_addr.port().to_be_bytes());
    client
        .write_all(&request)
        .await
        .expect("send connect request");

    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.expect("read reply");
    assert_eq!(reply[1], ReplyCode::Succeeded as u8);

    client.shutdown().await.expect("client shutdown");
    let _ = server_task.await;
    let _ = upstream_task.await;

    assert!(acl_stats.snapshot().allowed >= 1);
    assert_eq!(session_manager.active_session_count(), 0);
    let closed = session_manager.closed_snapshot();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].dest_ip, ip.to_string());
    assert_eq!(closed[0].status, SessionStatus::Closed);
}
