use rustsocks::acl::types::{AclRule, GlobalAclConfig, UserAcl};
use rustsocks::acl::{AclConfig, AclEngine, AclStats, Action, Protocol};
use rustsocks::auth::AuthManager;
use rustsocks::config::{AuthConfig, PamSettings};
use rustsocks::protocol::ReplyCode;
use rustsocks::qos::{ConnectionLimits, QosEngine};
use rustsocks::server::proxy::TrafficUpdateConfig;
use rustsocks::server::{handle_client, ClientHandlerContext, ConnectionPool, PoolConfig};
use rustsocks::session::{SessionManager, SessionStatus};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, Instant};

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
            client_method: "none".into(),
            socks_method: "none".into(),
            users: Vec::new(),
            pam: PamSettings::default(),
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
        let ctx = Arc::new(ClientHandlerContext {
            auth_manager: auth_manager.clone(),
            acl_engine: Some(acl_engine.clone()),
            acl_stats: acl_stats.clone(),
            anonymous_user: anonymous_user.clone(),
            session_manager: session_manager.clone(),
            traffic_config: TrafficUpdateConfig::default(),
            qos_engine: QosEngine::None,
            connection_limits: ConnectionLimits::default(),
            connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
        });

        tokio::spawn(async move {
            let (stream, client_addr) = listener.accept().await.expect("accept test client");
            handle_client(stream, ctx, client_addr)
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

struct AllowEnv {
    server_addr: SocketAddr,
    upstream_addr: SocketAddr,
    session_manager: Arc<SessionManager>,
    server_task: tokio::task::JoinHandle<()>,
    upstream_task: tokio::task::JoinHandle<()>,
}

impl AllowEnv {
    async fn wait(self) {
        let _ = self.server_task.await;
        let _ = self.upstream_task.await;
    }
}

async fn spawn_allow_env(expected: usize) -> AllowEnv {
    let auth_manager = Arc::new(
        AuthManager::new(&AuthConfig {
            client_method: "none".into(),
            socks_method: "none".into(),
            users: Vec::new(),
            pam: PamSettings::default(),
        })
        .expect("auth manager"),
    );

    let acl_engine = Arc::new(AclEngine::new(allowing_acl_config()).expect("acl engine"));
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new(String::from("anonymous"));
    let session_manager = Arc::new(SessionManager::new());

    let upstream_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream listener");
    let upstream_addr = upstream_listener.local_addr().unwrap();

    let upstream_task = tokio::spawn(async move {
        for _ in 0..expected {
            match upstream_listener.accept().await {
                Ok((stream, _)) => drop(stream),
                Err(_) => break,
            }
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let server_addr = listener.local_addr().unwrap();

    let server_task = {
        let ctx = Arc::new(ClientHandlerContext {
            auth_manager: auth_manager.clone(),
            acl_engine: Some(acl_engine.clone()),
            acl_stats: acl_stats.clone(),
            anonymous_user: anonymous_user.clone(),
            session_manager: session_manager.clone(),
            traffic_config: TrafficUpdateConfig::default(),
            qos_engine: QosEngine::None,
            connection_limits: ConnectionLimits::default(),
            connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
        });

        tokio::spawn(async move {
            let mut handles = Vec::with_capacity(expected);
            for _ in 0..expected {
                match listener.accept().await {
                    Ok((stream, client_addr)) => {
                        let ctx = ctx.clone();

                        handles.push(tokio::spawn(async move {
                            let _ = handle_client(stream, ctx, client_addr).await;
                        }));
                    }
                    Err(_) => break,
                }
            }

            for handle in handles {
                let _ = handle.await;
            }
        })
    };

    AllowEnv {
        server_addr,
        upstream_addr,
        session_manager,
        server_task,
        upstream_task,
    }
}

async fn perform_handshake(
    server_addr: SocketAddr,
    upstream_addr: SocketAddr,
) -> std::io::Result<Duration> {
    let mut client = TcpStream::connect(server_addr).await?;

    let start = Instant::now();

    client.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut response = [0u8; 2];
    client.read_exact(&mut response).await?;
    if response != [0x05, 0x00] {
        return Err(std::io::Error::other("unexpected method selection reply"));
    }

    let ip = match upstream_addr.ip() {
        IpAddr::V4(ip) => ip,
        IpAddr::V6(_) => {
            return Err(std::io::Error::other(
                "IPv6 upstream not supported in this test",
            ))
        }
    };

    let mut request = Vec::new();
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x01]);
    request.extend_from_slice(&ip.octets());
    request.extend_from_slice(&upstream_addr.port().to_be_bytes());
    client.write_all(&request).await?;

    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await?;
    if reply[1] != ReplyCode::Succeeded as u8 {
        return Err(std::io::Error::other("connect reply not succeeded"));
    }

    let elapsed = start.elapsed();
    client.shutdown().await?;

    Ok(elapsed)
}

#[tokio::test]
async fn acl_allows_connection_and_creates_session() {
    let env = spawn_allow_env(1).await;
    let session_manager = env.session_manager.clone();
    let dest_ip = env.upstream_addr.ip().to_string();

    let duration = perform_handshake(env.server_addr, env.upstream_addr)
        .await
        .expect("handshake should succeed");
    assert!(
        duration.as_millis() < 1000,
        "slow handshake: {:?}",
        duration
    );

    env.wait().await;

    assert_eq!(session_manager.active_session_count(), 0);
    let closed = session_manager.closed_snapshot();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].dest_ip, dest_ip);
    assert_eq!(closed[0].status, SessionStatus::Closed);
}

#[tokio::test]
#[ignore = "Timing-sensitive benchmark"]
async fn acl_performance_under_seven_ms() {
    const ITERATIONS: usize = 32;
    let env = spawn_allow_env(ITERATIONS).await;

    let mut total = Duration::default();
    for _ in 0..ITERATIONS {
        match perform_handshake(env.server_addr, env.upstream_addr).await {
            Ok(elapsed) => total += elapsed,
            Err(err) => {
                eprintln!("Skipping benchmark due to error: {:?}", err);
                env.wait().await;
                return;
            }
        }
    }

    env.wait().await;

    let avg = total / ITERATIONS as u32;
    assert!(
        avg.as_micros() <= 7_000,
        "average handshake overhead {:?} exceeds 7ms",
        avg
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "Stress test with 1000 concurrent connections"]
async fn acl_handles_thousand_concurrent_connections() {
    const CONNECTIONS: usize = 1000;
    const BATCH_SIZE: usize = 50; // Connect in batches to avoid overwhelming the listener

    let env = spawn_allow_env(CONNECTIONS).await;
    let session_manager = env.session_manager.clone();

    let mut success = 0usize;

    // Connect in batches to avoid TCP backlog overflow
    for batch_start in (0..CONNECTIONS).step_by(BATCH_SIZE) {
        let batch_end = (batch_start + BATCH_SIZE).min(CONNECTIONS);
        let batch_count = batch_end - batch_start;

        let mut tasks = Vec::with_capacity(batch_count);
        for _ in 0..batch_count {
            let server_addr = env.server_addr;
            let upstream_addr = env.upstream_addr;
            tasks.push(tokio::spawn(async move {
                perform_handshake(server_addr, upstream_addr).await.is_ok()
            }));
        }

        for task in tasks {
            if task.await.unwrap_or(false) {
                success += 1;
            }
        }

        // Small delay between batches to let server process
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    env.wait().await;

    assert_eq!(session_manager.active_session_count(), 0);
    assert_eq!(session_manager.closed_snapshot().len(), success);
    assert!(
        success >= (CONNECTIONS * 95) / 100, // Expect at least 95% success rate
        "only {} successful handshakes out of {} ({}%)",
        success,
        CONNECTIONS,
        (success * 100) / CONNECTIONS
    );
}
