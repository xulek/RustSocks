use crate::acl::{AclEngine, PolicyEvaluationContext, Protocol};
use crate::protocol::{Address, ReplyCode};
use crate::qos::QosEngine;
use crate::server::handler::IoStream;
use crate::server::pool::ConnectionPool;
use crate::server::proxy::{proxy_data, ProxyContext, TrafficUpdateConfig};
use crate::server::resolver::resolve_address;
use crate::session::{ConnectionInfo, SessionManager, SessionProtocol, SessionStatus};
use crate::utils::error::{Result, RustSocksError};
use chrono::Utc;
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// BIND waiting timeout (typically 5 minutes per RFC 1928)
const BIND_ACCEPT_TIMEOUT: Duration = Duration::from_secs(300);

/// Context for BIND command handling
pub struct BindContext {
    pub user: Arc<str>,
    pub client_addr: SocketAddr,
    pub acl_decision: String,
    pub acl_rule: Option<String>,
    pub qos_engine: QosEngine,
    pub connection_pool: Arc<ConnectionPool>,
    pub acl_engine: Option<Arc<AclEngine>>,
    pub user_groups: Vec<String>,
    pub source_ip: IpAddr,
    pub auth_method: String,
}

/// Handle BIND command
/// Returns the bound address/port where server listens for incoming connection
pub async fn handle_bind<S>(
    mut client_stream: S,
    dest_addr: &Address,
    dest_port: u16,
    session_manager: Arc<SessionManager>,
    bind_ctx: BindContext,
) -> Result<()>
where
    S: IoStream,
{
    let client_addr = bind_ctx.client_addr;
    let dest_string = dest_addr.to_string();

    // Resolve the expected peer once and validate the exact incoming connection.
    // A wildcard request address/port intentionally leaves that component unrestricted.
    let wildcard_address = match dest_addr {
        Address::IPv4(octets) => *octets == [0, 0, 0, 0],
        Address::IPv6(octets) => *octets == [0; 16],
        Address::Domain(_) => false,
    };
    let expected_ips: Option<HashSet<IpAddr>> = if wildcard_address {
        None
    } else {
        let resolved = resolve_address(dest_addr, dest_port).await?;
        Some(resolved.into_iter().map(|addr| addr.ip()).collect())
    };

    // Preserve the control connection address family.
    let bind_target = if client_addr.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let bind_listener = TcpListener::bind(bind_target).await?;
    let bind_addr = bind_listener.local_addr()?;
    let advertised_bind_addr = if bind_addr.ip().is_unspecified() {
        let probe = UdpSocket::bind(bind_target).await?;
        if probe.connect(client_addr).await.is_ok() {
            probe
                .local_addr()
                .map(|addr| SocketAddr::new(addr.ip(), bind_addr.port()))
                .unwrap_or(bind_addr)
        } else {
            bind_addr
        }
    } else {
        bind_addr
    };

    info!(
        "BIND: listening on {} (advertising {}) for incoming connection to {}:{}",
        bind_addr, advertised_bind_addr, dest_string, dest_port
    );

    // Send first response with a usable local address instead of a wildcard when possible.
    send_bind_response(
        &mut client_stream,
        ReplyCode::Succeeded,
        advertised_bind_addr,
    )
    .await?;

    // Create session for this BIND
    let connection_info = ConnectionInfo {
        source_ip: client_addr.ip(),
        source_port: client_addr.port(),
        dest_ip: dest_string.clone(),
        dest_port,
        protocol: SessionProtocol::Tcp,
    };

    let (session_id, cancel_token) = session_manager
        .new_session_with_control(
            bind_ctx.user.as_ref(),
            connection_info,
            bind_ctx.acl_decision.clone(),
            bind_ctx.acl_rule.clone(),
            None,
            None,
        )
        .await;

    // Wait for the expected incoming connection with timeout. Unexpected peers are
    // dropped instead of being accepted as the BIND target.
    let incoming_result = timeout(BIND_ACCEPT_TIMEOUT, async {
        loop {
            let (stream, peer_addr) = bind_listener.accept().await?;
            let ip_matches = expected_ips
                .as_ref()
                .map(|ips| ips.contains(&peer_addr.ip()))
                .unwrap_or(true);
            let port_matches = dest_port == 0 || peer_addr.port() == dest_port;
            if ip_matches && port_matches {
                break Ok((stream, peer_addr));
            }
            warn!(
                peer = %peer_addr,
                expected = %dest_string,
                expected_port = dest_port,
                "BIND: rejected unexpected incoming peer"
            );
            drop(stream);
        }
    })
    .await;

    match incoming_result {
        Ok(Ok((incoming_stream, peer_addr))) => {
            if let Some(engine) = bind_ctx.acl_engine.as_ref() {
                let peer_address = match peer_addr.ip() {
                    IpAddr::V4(ip) => Address::IPv4(ip.octets()),
                    IpAddr::V6(ip) => Address::IPv6(ip.octets()),
                };
                let mut usage = session_manager
                    .policy_usage_snapshot(bind_ctx.user.as_ref())
                    .await;
                usage.active_connections = usage.active_connections.saturating_sub(1);
                usage.connections_last_minute = usage.connections_last_minute.saturating_sub(1);
                if let Some(rule) = engine
                    .is_explicitly_blocked_with_policy_context(PolicyEvaluationContext {
                        user: bind_ctx.user.as_ref(),
                        groups: &bind_ctx.user_groups,
                        source_ip: bind_ctx.source_ip,
                        auth_method: &bind_ctx.auth_method,
                        destination: &peer_address,
                        port: peer_addr.port(),
                        protocol: &Protocol::Tcp,
                        now: Utc::now(),
                        usage,
                    })
                    .await
                {
                    warn!(peer = %peer_addr, rule = %rule, "BIND peer blocked by ACL");
                    send_bind_response(
                        &mut client_stream,
                        ReplyCode::ConnectionNotAllowed,
                        peer_addr,
                    )
                    .await?;
                    session_manager
                        .close_session(
                            &session_id,
                            Some(format!("BIND peer blocked by ACL: {}", rule)),
                            SessionStatus::Failed,
                        )
                        .await;
                    return Err(RustSocksError::AuthFailed(
                        "BIND peer blocked by ACL".to_string(),
                    ));
                }
            }
            info!(
                "BIND: accepted incoming connection from {} for client {}",
                peer_addr, client_addr
            );

            // Send second response with peer address
            send_bind_response(&mut client_stream, ReplyCode::Succeeded, peer_addr).await?;

            // Proxy data between client and incoming connection
            let proxy_ctx = ProxyContext {
                session_manager: session_manager.clone(),
                session_id,
                cancel_token,
                update_config: TrafficUpdateConfig::default(),
                qos_engine: bind_ctx.qos_engine.clone(),
                user: Arc::clone(&bind_ctx.user),
            };
            match proxy_data(client_stream, incoming_stream, proxy_ctx).await {
                Ok(Some(reuse)) => {
                    drop(reuse.stream);
                    session_manager
                        .close_session(
                            &session_id,
                            Some("BIND connection closed normally".to_string()),
                            SessionStatus::Closed,
                        )
                        .await;
                }
                Ok(None) => {
                    session_manager
                        .close_session(
                            &session_id,
                            Some("BIND connection closed normally".to_string()),
                            SessionStatus::Closed,
                        )
                        .await;
                }
                Err(RustSocksError::ConnectionClosed) => {
                    session_manager
                        .close_session(
                            &session_id,
                            Some("BIND connection closed by client".to_string()),
                            SessionStatus::Closed,
                        )
                        .await;
                    info!("BIND session closed by client {}", client_addr);
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
            warn!(
                "BIND: timeout waiting for incoming connection ({}s)",
                BIND_ACCEPT_TIMEOUT.as_secs()
            );
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
async fn send_bind_response<S>(
    stream: &mut S,
    reply: ReplyCode,
    bind_addr: SocketAddr,
) -> Result<()>
where
    S: IoStream,
{
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
