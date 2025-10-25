use rustsocks::protocol::types::Address;
use rustsocks::server::resolve_address;
use std::net::IpAddr;
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn domain_resolution_reaches_ipv6_localhost() {
    let listener = match TcpListener::bind("[::1]:0").await {
        Ok(l) => l,
        Err(_) => {
            // Environment without IPv6 loopback support; skip test.
            return;
        }
    };

    let port = listener.local_addr().unwrap().port();
    let addr = Address::Domain("localhost".to_string());
    let resolved = resolve_address(&addr, port).await.unwrap();
    if !resolved
        .iter()
        .any(|socket| matches!(socket.ip(), IpAddr::V6(_)))
    {
        // Nothing to test if resolver cannot find IPv6 entries in this environment.
        return;
    }

    let accept_task = tokio::spawn(async move {
        listener.accept().await.ok();
    });

    let mut connected_stream = None;
    for target in resolved {
        if matches!(target.ip(), IpAddr::V6(_)) {
            if let Ok(stream) = TcpStream::connect(target).await {
                connected_stream = Some(stream);
                break;
            }
        }
    }

    assert!(
        connected_stream.is_some(),
        "expected to connect to ::1 via domain resolution"
    );

    drop(connected_stream);
    let _ = accept_task.await;
}
