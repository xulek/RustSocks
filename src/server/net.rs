use tokio::net::TcpStream;
use tracing::{debug, warn};

/// Tune TCP sockets for low-latency proxy traffic.
///
/// Best-effort: any failures are logged and ignored so connections still proceed.
pub(crate) fn tune_tcp_stream(stream: &TcpStream) {
    if let Err(err) = stream.set_nodelay(true) {
        warn!("Failed to set TCP_NODELAY on socket: {}", err);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "fuchsia",
        target_os = "cygwin",
    ))]
    {
        if let Err(err) = stream.set_quickack(true) {
            debug!("Failed to set TCP_QUICKACK on socket: {}", err);
        }
    }

    let sock_ref = socket2::SockRef::from(stream);
    let _ = sock_ref.set_recv_buffer_size(262_144); // 256 KB
    let _ = sock_ref.set_send_buffer_size(262_144); // 256 KB
}
