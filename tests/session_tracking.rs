use rustsocks::qos::QosEngine;
use rustsocks::server::proxy::{proxy_data, TrafficUpdateConfig};
use rustsocks::session::{ConnectionInfo, SessionManager, SessionProtocol, SessionStatus};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn proxy_updates_session_traffic_on_shutdown_flush() {
    let session_manager = Arc::new(SessionManager::new());

    // Prepare client-side connection
    let client_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind client listener");
    let client_addr = client_listener.local_addr().unwrap();

    let client_connect = TcpStream::connect(client_addr);
    let client_accept = client_listener.accept();
    let (client_peer, server_side_client) = tokio::join!(client_connect, client_accept);
    let mut client_peer = client_peer.expect("client connect");
    let (server_client_stream, _) = server_side_client.expect("server accept client");

    // Prepare upstream-side connection
    let upstream_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream listener");
    let upstream_addr = upstream_listener.local_addr().unwrap();

    let upstream_connect = TcpStream::connect(upstream_addr);
    let upstream_accept = upstream_listener.accept();
    let (server_side_upstream, upstream_peer_tuple) =
        tokio::join!(upstream_connect, upstream_accept);
    let upstream_stream = server_side_upstream.expect("proxy connect upstream");
    let (mut upstream_peer, _) = upstream_peer_tuple.expect("upstream accept proxy");

    let source_addr = server_client_stream.peer_addr().unwrap();
    let dest_addr = upstream_listener.local_addr().unwrap();
    let connection_info = ConnectionInfo {
        source_ip: source_addr.ip(),
        source_port: source_addr.port(),
        dest_ip: dest_addr.ip().to_string(),
        dest_port: dest_addr.port(),
        protocol: SessionProtocol::Tcp,
    };

    let (session_id, cancel_token) = session_manager
        .new_session_with_control("integration-user", connection_info, "allow", None, None)
        .await;

    let proxy_task = tokio::spawn(proxy_data(
        server_client_stream,
        upstream_stream,
        session_manager.clone(),
        session_id,
        cancel_token,
        TrafficUpdateConfig::new(10),
        QosEngine::None,
        "integration-user".to_string(),
    ));

    // Client -> Upstream payload (forces flush on close, not threshold)
    let upload_payload = b"hello-proxy-upload";
    client_peer
        .write_all(upload_payload)
        .await
        .expect("client write");

    let mut upstream_buffer = vec![0u8; upload_payload.len()];
    upstream_peer
        .read_exact(&mut upstream_buffer)
        .await
        .expect("upstream read upload");
    assert_eq!(upload_payload, upstream_buffer.as_slice());

    // Upstream -> Client payload
    let download_payload = b"reply-payload";
    upstream_peer
        .write_all(download_payload)
        .await
        .expect("upstream write");

    let mut client_buffer = vec![0u8; download_payload.len()];
    client_peer
        .read_exact(&mut client_buffer)
        .await
        .expect("client read download");
    assert_eq!(download_payload, client_buffer.as_slice());

    // Close both directions to trigger final flush
    client_peer.shutdown().await.expect("client shutdown");
    upstream_peer.shutdown().await.expect("upstream shutdown");

    proxy_task
        .await
        .expect("proxy task join")
        .expect("proxy task result");

    let session_arc = session_manager
        .get_session(&session_id)
        .expect("session still active");
    let session = session_arc.read().await;

    assert_eq!(session.bytes_sent, upload_payload.len() as u64);
    assert_eq!(session.bytes_received, download_payload.len() as u64);
    assert!(
        session.packets_sent >= 1,
        "expected at least one packet sent"
    );
    assert!(
        session.packets_received >= 1,
        "expected at least one packet received"
    );

    drop(session);
    session_manager
        .close_session(
            &session_id,
            Some("test completion".into()),
            SessionStatus::Closed,
        )
        .await;
}
