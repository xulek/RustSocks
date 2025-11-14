use rustsocks::qos::{ConnectionLimits, HtbConfig, QosConfig, QosEngine};
use rustsocks::server::proxy::{proxy_data, TrafficUpdateConfig};
use rustsocks::session::{ConnectionInfo, SessionManager, SessionProtocol, SessionStatus};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, Instant};

#[tokio::test]
async fn bandwidth_throttling_enforced_by_proxy() {
    let session_manager = Arc::new(SessionManager::new());

    let client_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind client listener");
    let client_addr = client_listener.local_addr().unwrap();

    let client_connect = TcpStream::connect(client_addr);
    let (client_peer, server_client_pair) = tokio::join!(client_connect, client_listener.accept());
    let mut client_peer = client_peer.expect("client connect");
    let (server_client_stream, _) = server_client_pair.expect("accept client stream");

    let upstream_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream listener");
    let upstream_addr = upstream_listener.local_addr().unwrap();

    let upstream_connect = TcpStream::connect(upstream_addr);
    let (upstream_stream, upstream_peer_pair) =
        tokio::join!(upstream_connect, upstream_listener.accept());
    let upstream_stream = upstream_stream.expect("connect upstream stream");
    let (mut upstream_peer, _) = upstream_peer_pair.expect("accept upstream peer");

    client_peer.set_nodelay(true).expect("set TCP_NODELAY");
    upstream_peer
        .set_nodelay(true)
        .expect("set TCP_NODELAY upstream");

    let connection_info = ConnectionInfo {
        source_ip: server_client_stream.peer_addr().unwrap().ip(),
        source_port: server_client_stream.peer_addr().unwrap().port(),
        dest_ip: upstream_addr.ip().to_string(),
        dest_port: upstream_addr.port(),
        protocol: SessionProtocol::Tcp,
    };

    let (session_id, cancel_token) = session_manager
        .new_session_with_control("throttle-user", connection_info, "allow", None, None)
        .await;

    let qos_config = QosConfig {
        enabled: true,
        htb: HtbConfig {
            global_bandwidth_bytes_per_sec: 200_000,
            guaranteed_bandwidth_bytes_per_sec: 65_536,
            max_bandwidth_bytes_per_sec: 65_536,
            burst_size_bytes: 65_536,
            refill_interval_ms: 10,
            fair_sharing_enabled: true,
            rebalance_interval_ms: 20,
            idle_timeout_secs: 30,
        },
        connection_limits: ConnectionLimits {
            max_connections_per_user: 10,
            max_connections_global: 100,
        },
        ..QosConfig::default()
    };

    let qos_engine = QosEngine::from_config(qos_config.clone())
        .await
        .expect("create QoS engine");
    qos_engine
        .check_and_inc_connection("throttle-user", &qos_config.connection_limits)
        .expect("increment connection count");

    let qos_clone = qos_engine.clone();
    let proxy_task = tokio::spawn(proxy_data(
        server_client_stream,
        upstream_stream,
        session_manager.clone(),
        session_id,
        cancel_token,
        TrafficUpdateConfig::new(10),
        qos_clone,
        Arc::<str>::from("throttle-user"),
    ));

    let chunk = vec![0xAB; 65_536];
    let mut total_sent = 0usize;
    let start = Instant::now();

    for _ in 0..3 {
        client_peer.write_all(&chunk).await.expect("client write");
        total_sent += chunk.len();
    }

    client_peer.shutdown().await.expect("shutdown client");

    let mut received = vec![0u8; total_sent];
    upstream_peer
        .read_exact(&mut received)
        .await
        .expect("read proxied data");
    upstream_peer.shutdown().await.expect("shutdown upstream");

    let _ = proxy_task.await.expect("join proxy task");

    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(400),
        "expected throttling delay, got {:?}",
        elapsed
    );

    session_manager
        .close_session(
            &session_id,
            Some("throttle test completed".into()),
            SessionStatus::Closed,
        )
        .await;

    qos_engine.dec_user_connection("throttle-user");
}

#[tokio::test]
async fn fair_sharing_allocations_even_between_users() {
    let qos_config = QosConfig {
        enabled: true,
        htb: HtbConfig {
            global_bandwidth_bytes_per_sec: 100_000,
            guaranteed_bandwidth_bytes_per_sec: 20_000,
            max_bandwidth_bytes_per_sec: 80_000,
            burst_size_bytes: 40_000,
            refill_interval_ms: 10,
            fair_sharing_enabled: true,
            rebalance_interval_ms: 20,
            idle_timeout_secs: 30,
        },
        connection_limits: ConnectionLimits {
            max_connections_per_user: 10,
            max_connections_global: 100,
        },
        ..QosConfig::default()
    };

    let qos_engine = QosEngine::from_config(qos_config.clone())
        .await
        .expect("create QoS engine");

    for user in ["alice", "bob"] {
        qos_engine
            .check_and_inc_connection(user, &qos_config.connection_limits)
            .expect("increment connection");
    }

    for user in ["alice", "bob"] {
        qos_engine
            .allocate_bandwidth(user, qos_config.htb.burst_size_bytes)
            .await
            .expect("allocate initial traffic");
    }

    tokio::time::sleep(Duration::from_millis(
        qos_config.htb.rebalance_interval_ms * 3,
    ))
    .await;

    let allocations = qos_engine.get_user_allocations().await;
    let alice = allocations
        .iter()
        .find(|a| a.user == "alice")
        .expect("alice allocation");
    let bob = allocations
        .iter()
        .find(|a| a.user == "bob")
        .expect("bob allocation");

    let diff = alice.allocated_bandwidth.abs_diff(bob.allocated_bandwidth);

    assert!(
        diff <= qos_config.htb.guaranteed_bandwidth_bytes_per_sec / 2,
        "expected similar allocations, got alice={} bob={}",
        alice.allocated_bandwidth,
        bob.allocated_bandwidth
    );

    for user in ["alice", "bob"] {
        qos_engine.dec_user_connection(user);
    }
}
