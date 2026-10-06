use crate::acl::{
    AclDecision, AclEngine, AclMetrics, AclStats, PolicyEvaluationContext, PolicyUsageSnapshot,
    Protocol,
};
use crate::protocol::{parse_udp_packet, serialize_udp_packet, Address, UdpHeader, UdpPacket};
use crate::qos::QosEngine;
use crate::server::resolver::resolve_address;
use crate::session::{SessionManager, SessionStatus};
use crate::utils::error::{Result, RustSocksError};
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::broadcast;
use tokio::time::timeout;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// UDP session manager for tracking client-to-destination mappings
struct UdpSessionMap {
    // Map client address to (destination, session_id)
    sessions: DashMap<SocketAddr, (SocketAddr, Uuid)>,
    // Map destination address back to client for responses
    reverse: DashMap<SocketAddr, SocketAddr>,
}

#[derive(Clone)]
pub struct UdpRelayContext {
    pub user: Arc<str>,
    pub user_groups: Arc<Vec<String>>,
    pub acl_engine: Option<Arc<AclEngine>>,
    pub acl_stats: Arc<AclStats>,
    pub qos_engine: QosEngine,
    pub source_ip: std::net::IpAddr,
    pub auth_method: String,
}

impl UdpSessionMap {
    fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            reverse: DashMap::new(),
        }
    }

    fn insert(&self, client: SocketAddr, dest: SocketAddr, session_id: Uuid) {
        self.sessions.insert(client, (dest, session_id));
        self.reverse.insert(dest, client);
    }

    #[allow(dead_code)]
    fn get_destination(&self, client: &SocketAddr) -> Option<(SocketAddr, Uuid)> {
        self.sessions.get(client).map(|entry| *entry.value())
    }

    fn get_client(&self, dest: &SocketAddr) -> Option<SocketAddr> {
        self.reverse.get(dest).map(|entry| *entry.value())
    }

    #[allow(dead_code)]
    fn remove(&self, client: &SocketAddr) {
        if let Some((_, (dest, _))) = self.sessions.remove(client) {
            self.reverse.remove(&dest);
        }
    }
}

/// Handle UDP ASSOCIATE command
/// Returns the local address/port where the UDP relay is listening
pub async fn handle_udp_associate(
    client_addr: SocketAddr,
    expected_client_port: Option<u16>,
    session_manager: Arc<SessionManager>,
    session_id: Uuid,
    shutdown_rx: broadcast::Receiver<()>,
    udp_ctx: UdpRelayContext,
) -> Result<SocketAddr> {
    // Bind the relay in the same address family as the TCP control connection.
    let bind_target = if client_addr.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let udp_socket = UdpSocket::bind(bind_target).await?;
    let local_addr = udp_socket.local_addr()?;

    // Wildcard addresses are not useful to a remote client. Determine the local
    // interface selected by the route to the control peer and advertise that IP.
    let advertised_addr = if local_addr.ip().is_unspecified() {
        let probe = UdpSocket::bind(bind_target).await?;
        if probe.connect(client_addr).await.is_ok() {
            probe
                .local_addr()
                .map(|addr| SocketAddr::new(addr.ip(), local_addr.port()))
                .unwrap_or(local_addr)
        } else {
            local_addr
        }
    } else {
        local_addr
    };

    info!(
        "UDP ASSOCIATE: bound relay socket on {} (advertising {}) for client {}",
        local_addr, advertised_addr, client_addr
    );

    // Spawn UDP relay task
    tokio::spawn(async move {
        if let Err(e) = run_udp_relay(
            udp_socket,
            client_addr,
            expected_client_port,
            session_manager.clone(),
            session_id,
            shutdown_rx,
            udp_ctx,
        )
        .await
        {
            warn!("UDP relay error: {}", e);
            session_manager
                .close_session(
                    &session_id,
                    Some(format!("UDP relay error: {}", e)),
                    SessionStatus::Failed,
                )
                .await;
        }
    });

    Ok(advertised_addr)
}

/// Run the UDP relay loop
async fn run_udp_relay(
    socket: UdpSocket,
    client_addr: SocketAddr,
    expected_client_port: Option<u16>,
    session_manager: Arc<SessionManager>,
    session_id: Uuid,
    mut shutdown_rx: broadcast::Receiver<()>,
    udp_ctx: UdpRelayContext,
) -> Result<()> {
    let socket = Arc::new(socket);
    let session_map = Arc::new(UdpSessionMap::new());
    let udp_ctx = Arc::new(udp_ctx);
    let mut client_udp_addr =
        expected_client_port.map(|port| SocketAddr::new(client_addr.ip(), port));

    const MAX_DATAGRAM: usize = 65_535;
    let mut buf = BytesMut::with_capacity(MAX_DATAGRAM);
    let udp_timeout = Duration::from_secs(120); // 2 minutes idle timeout

    loop {
        if buf.capacity() < MAX_DATAGRAM {
            buf.reserve(MAX_DATAGRAM - buf.capacity());
        }
        buf.clear();

        // Wait for packet or shutdown signal
        tokio::select! {
            result = timeout(udp_timeout, socket.recv_buf_from(&mut buf)) => {
                match result {
                    Ok(Ok((len, peer_addr))) => {
                        if len == 0 {
                            continue;
                        }

                        let packet_data = buf.split().freeze();

                        // Determine if this is from the associated client or a destination.
                        // When the request supplied a UDP source port, only that exact endpoint
                        // may use the relay. For wildcard-port requests, bind to the first *valid*
                        // SOCKS5 UDP datagram from the TCP control peer's IP.
                        let is_client = match client_udp_addr {
                            Some(addr) => addr == peer_addr,
                            None => peer_addr.ip() == client_addr.ip(),
                        };

                        if is_client {
                            if client_udp_addr.is_none() {
                                if parse_udp_packet(packet_data.clone()).is_err() {
                                    warn!(
                                        peer = %peer_addr,
                                        "Ignoring malformed UDP packet while waiting for client endpoint"
                                    );
                                    continue;
                                }
                                client_udp_addr = Some(peer_addr);
                                debug!(client_udp = %peer_addr, "Bound UDP association to client endpoint");
                            }
                            // Packet from client to destination
                            if let Err(e) = handle_client_packet(
                                &socket,
                                packet_data,
                                peer_addr,
                                &session_map,
                                &session_manager,
                                &session_id,
                                &udp_ctx,
                            )
                            .await
                            {
                                warn!("Error handling client UDP packet: {}", e);
                            }
                        } else {
                            // Packet from destination back to client
                            if let Err(e) = handle_destination_packet(
                                &socket,
                                packet_data,
                                peer_addr,
                                &session_map,
                                &session_manager,
                                &session_id,
                                &udp_ctx,
                            )
                            .await
                            {
                                warn!("Error handling destination UDP packet: {}", e);
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        warn!("UDP socket error: {}", e);
                        return Err(RustSocksError::Io(e));
                    }
                    Err(_) => {
                        // Timeout - close session
                        info!("UDP session timeout after {} seconds", udp_timeout.as_secs());
                        session_manager
                            .close_session(
                                &session_id,
                                Some("UDP session timeout".to_string()),
                                SessionStatus::Closed,
                            )
                            .await;
                        return Ok(());
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                info!("UDP relay shutting down");
                session_manager
                    .close_session(
                        &session_id,
                        Some("Server shutdown".to_string()),
                        SessionStatus::Closed,
                    )
                    .await;
                return Ok(());
            }
        }
    }
}

/// Handle packet from client (forward to destination)
async fn handle_client_packet(
    socket: &Arc<UdpSocket>,
    packet_data: Bytes,
    client_addr: SocketAddr,
    session_map: &Arc<UdpSessionMap>,
    session_manager: &Arc<SessionManager>,
    session_id: &Uuid,
    udp_ctx: &Arc<UdpRelayContext>,
) -> Result<()> {
    // Parse SOCKS5 UDP packet
    let packet = parse_udp_packet(packet_data)?;

    debug!(
        "UDP client packet: {} -> {}:{} ({} bytes)",
        client_addr,
        packet.header.address,
        packet.header.port,
        packet.data.len()
    );

    if let Some(engine) = udp_ctx.acl_engine.as_ref() {
        let outcome = engine
            .evaluate_policy_for_traffic(PolicyEvaluationContext {
                user: udp_ctx.user.as_ref(),
                groups: udp_ctx.user_groups.as_ref(),
                source_ip: udp_ctx.source_ip,
                auth_method: &udp_ctx.auth_method,
                destination: &packet.header.address,
                port: packet.header.port,
                protocol: &Protocol::Udp,
                now: Utc::now(),
                // Admission limits are evaluated when UDP ASSOCIATE is opened.
                usage: PolicyUsageSnapshot::default(),
            })
            .await;
        AclMetrics::record_outcome(&outcome);

        match outcome.decision {
            AclDecision::Block => {
                udp_ctx.acl_stats.record_block(udp_ctx.user.as_ref());
                let rule = outcome.matched_rule.as_deref().unwrap_or("unknown rule");
                warn!(
                    user = %udp_ctx.user.as_ref(),
                    dest = %packet.header.address,
                    port = packet.header.port,
                    rule,
                    "Policy engine blocked UDP packet"
                );
                return Ok(());
            }
            AclDecision::Allow => udp_ctx.acl_stats.record_allow(udp_ctx.user.as_ref()),
        }
    }

    if let Err(err) = udp_ctx
        .qos_engine
        .allocate_bandwidth_arc(&udp_ctx.user, packet.data.len() as u64)
        .await
    {
        warn!(
            user = %udp_ctx.user.as_ref(),
            error = %err,
            "QoS allocation failed for UDP upload"
        );
        return Ok(());
    }

    // Resolve destination address and recheck explicit IP/CIDR deny rules
    // against the exact address that will receive the datagram.
    let dest_candidates = resolve_address(&packet.header.address, packet.header.port).await?;
    let mut dest_addr = None;
    for candidate in dest_candidates {
        let resolved = match candidate.ip() {
            std::net::IpAddr::V4(ip) => Address::IPv4(ip.octets()),
            std::net::IpAddr::V6(ip) => Address::IPv6(ip.octets()),
        };
        if let Some(engine) = udp_ctx.acl_engine.as_ref() {
            if let Some(rule) = engine
                .is_explicitly_blocked_with_policy_context(PolicyEvaluationContext {
                    user: udp_ctx.user.as_ref(),
                    groups: udp_ctx.user_groups.as_ref(),
                    source_ip: udp_ctx.source_ip,
                    auth_method: &udp_ctx.auth_method,
                    destination: &resolved,
                    port: packet.header.port,
                    protocol: &Protocol::Udp,
                    now: Utc::now(),
                    usage: PolicyUsageSnapshot::default(),
                })
                .await
            {
                warn!(
                    user = %udp_ctx.user.as_ref(),
                    domain = %packet.header.address,
                    resolved_ip = %candidate.ip(),
                    rule = %rule,
                    "Resolved UDP destination blocked by ACL"
                );
                continue;
            }
        }
        dest_addr = Some(candidate);
        break;
    }

    let Some(dest_addr) = dest_addr else {
        udp_ctx.acl_stats.record_block(udp_ctx.user.as_ref());
        return Ok(());
    };

    // Store session mapping
    session_map.insert(client_addr, dest_addr, *session_id);

    // Forward raw data to destination (without SOCKS5 header)
    let sent = socket.send_to(packet.data.as_ref(), dest_addr).await?;

    session_manager.queue_traffic_update(session_id, sent as u64, 0, 1, 0);

    debug!(
        "Forwarded {} bytes from client {} to destination {}",
        sent, client_addr, dest_addr
    );

    Ok(())
}

/// Handle packet from destination (forward back to client)
async fn handle_destination_packet(
    socket: &Arc<UdpSocket>,
    packet_data: Bytes,
    dest_addr: SocketAddr,
    session_map: &Arc<UdpSessionMap>,
    session_manager: &Arc<SessionManager>,
    session_id: &Uuid,
    udp_ctx: &Arc<UdpRelayContext>,
) -> Result<()> {
    // Find client address from reverse mapping
    let client_addr = session_map.get_client(&dest_addr).ok_or_else(|| {
        RustSocksError::Protocol(format!("No client mapping for destination {}", dest_addr))
    })?;

    let packet_len = packet_data.len();

    debug!(
        "UDP destination packet: {} -> {} ({} bytes)",
        dest_addr, client_addr, packet_len
    );

    // Wrap response in SOCKS5 UDP header
    let response_packet = UdpPacket {
        header: UdpHeader {
            frag: 0,
            address: match dest_addr {
                SocketAddr::V4(addr) => Address::IPv4(addr.ip().octets()),
                SocketAddr::V6(addr) => Address::IPv6(addr.ip().octets()),
            },
            port: dest_addr.port(),
        },
        data: packet_data.clone(),
    };

    let response_bytes = serialize_udp_packet(&response_packet)?;

    // Send to client
    if let Err(err) = udp_ctx
        .qos_engine
        .allocate_bandwidth_arc(&udp_ctx.user, packet_len as u64)
        .await
    {
        warn!(
            user = %udp_ctx.user.as_ref(),
            error = %err,
            "QoS allocation failed for UDP download"
        );
        return Ok(());
    }

    let sent = socket.send_to(&response_bytes, client_addr).await?;

    session_manager.queue_traffic_update(session_id, 0, packet_len as u64, 0, 1);

    debug!(
        "Forwarded {} bytes from destination {} to client {}",
        sent, dest_addr, client_addr
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_session_map() {
        let map = UdpSessionMap::new();
        let client = "127.0.0.1:1234".parse().unwrap();
        let dest = "8.8.8.8:53".parse().unwrap();
        let session_id = Uuid::new_v4();

        map.insert(client, dest, session_id);

        assert_eq!(map.get_destination(&client), Some((dest, session_id)));
        assert_eq!(map.get_client(&dest), Some(client));

        map.remove(&client);
        assert!(map.get_destination(&client).is_none());
        assert!(map.get_client(&dest).is_none());
    }
}
