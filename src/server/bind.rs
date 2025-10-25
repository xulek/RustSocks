use crate::protocol::{Address, ReplyCode};
use crate::server::proxy::{proxy_data, TrafficUpdateConfig};
use crate::session::{ConnectionInfo, SessionManager, SessionProtocol, SessionStatus};
use crate::utils::error::{Result, RustSocksError};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// BIND waiting timeout (typically 5 minutes per RFC 1928)
const BIND_ACCEPT_TIMEOUT: Duration = Duration::from_secs(300);

/// Context for BIND command handling
pub struct BindContext {
    pub user: String,
    pub client_addr: SocketAddr,
    pub acl_decision: String,
    pub acl_rule: Option<String>,
}

/// Handle BIND command
/// Returns the bound address/port where server listens for incoming connection
pub async fn handle_bind(
    mut client_stream: TcpStream,
    dest_addr: &Address,
    dest_port: u16,
    session_manager: Arc<SessionManager>,
    bind_ctx: BindContext,
) -> Result<()> {
    let client_addr = bind_ctx.client_addr;
    let dest_string = dest_addr.to_string();

    // Bind TCP listener on ephemeral port (0 = random)
    let bind_listener = TcpListener::bind("0.0.0.0:0").await?;
    let bind_addr = bind_listener.local_addr()?;

    info!(
        "BIND: listening on {} for incoming connection to {}:{}",
        bind_addr, dest_string, dest_port
    );

    // Send first response with bind address/port
    send_bind_response(&mut client_stream, ReplyCode::Succeeded, bind_addr).await?;

    // Create session for this BIND
    let connection_info = ConnectionInfo {
        source_ip: client_addr.ip(),
        source_port: client_addr.port(),
        dest_ip: dest_string.clone(),
        dest_port,
        protocol: SessionProtocol::Tcp,
    };

    let session_id = session_manager
        .new_session(
            &bind_ctx.user,
            connection_info,
            bind_ctx.acl_decision.clone(),
            bind_ctx.acl_rule.clone(),
        )
        .await;

    // Wait for incoming connection with timeout
    let incoming_result = timeout(BIND_ACCEPT_TIMEOUT, bind_listener.accept()).await;

    match incoming_result {
        Ok(Ok((incoming_stream, peer_addr))) => {
            info!(
                "BIND: accepted incoming connection from {} for client {}",
                peer_addr, client_addr
            );

            // Send second response with peer address
            send_bind_response(&mut client_stream, ReplyCode::Succeeded, peer_addr).await?;

            // Proxy data between client and incoming connection
            match proxy_data(
                client_stream,
                incoming_stream,
                session_manager.clone(),
                session_id,
                TrafficUpdateConfig::default(),
            )
            .await
            {
                Ok(_) => {
                    session_manager
                        .close_session(
                            &session_id,
                            Some("BIND connection closed normally".to_string()),
                            SessionStatus::Closed,
                        )
                        .await;
                }
                Err(e) => {
                    let reason = format!("BIND proxy error: {}", e);
                    session_manager
                        .close_session(&session_id, Some(reason), SessionStatus::Failed)
                        .await;
                    return Err(e);
                }
            }
        }
        Ok(Err(e)) => {
            warn!("BIND: error accepting incoming connection: {}", e);
            send_bind_response(&mut client_stream, ReplyCode::GeneralFailure, client_addr).await?;
            session_manager
                .close_session(
                    &session_id,
                    Some(format!("Accept error: {}", e)),
                    SessionStatus::Failed,
                )
                .await;
            return Err(RustSocksError::Io(e));
        }
        Err(_) => {
            warn!("BIND: timeout waiting for incoming connection ({}s)", BIND_ACCEPT_TIMEOUT.as_secs());
            send_bind_response(&mut client_stream, ReplyCode::GeneralFailure, client_addr).await?;
            session_manager
                .close_session(
                    &session_id,
                    Some("BIND timeout waiting for connection".to_string()),
                    SessionStatus::Failed,
                )
                .await;
            return Err(RustSocksError::Protocol(
                "BIND: timeout waiting for connection".to_string(),
            ));
        }
    }

    Ok(())
}

/// Send BIND response (first or second)
/// RFC 1928: +----+-----+-------+------+----------+----------+
///           |VER | REP |  RSV  | ATYP | BND.ADDR | BND.PORT |
///           +----+-----+-------+------+----------+----------+
async fn send_bind_response(
    stream: &mut TcpStream,
    reply: ReplyCode,
    bind_addr: SocketAddr,
) -> Result<()> {
    use tokio::io::AsyncWriteExt as _;

    let mut response = vec![0x05, reply as u8, 0x00]; // version, reply, reserved

    // Add address type and address
    match bind_addr {
        SocketAddr::V4(addr) => {
            response.push(0x01); // IPv4
            response.extend_from_slice(&addr.ip().octets());
        }
        SocketAddr::V6(addr) => {
            response.push(0x04); // IPv6
            response.extend_from_slice(&addr.ip().octets());
        }
    }

    // Add port (big-endian)
    response.extend_from_slice(&bind_addr.port().to_be_bytes());

    stream.write_all(&response).await?;
    stream.flush().await?;

    debug!(
        "BIND: sent response with address {}, reply={:?}",
        bind_addr, reply
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_accept_timeout_is_reasonable() {
        // Verify timeout is at least 5 minutes as per RFC 1928
        assert!(BIND_ACCEPT_TIMEOUT.as_secs() >= 300);
        // But not more than 10 minutes
        assert!(BIND_ACCEPT_TIMEOUT.as_secs() <= 600);
    }
}
