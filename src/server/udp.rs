use crate::protocol::{parse_udp_packet, serialize_udp_packet, Address, UdpHeader, UdpPacket};
use crate::server::resolver::resolve_address;
use crate::session::{SessionManager, SessionStatus};
use crate::utils::error::{Result, RustSocksError};
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
    session_manager: Arc<SessionManager>,
    session_id: Uuid,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<SocketAddr> {
    // Bind UDP socket on any available port
    let udp_socket = UdpSocket::bind("0.0.0.0:0").await?;
    let local_addr = udp_socket.local_addr()?;

    info!(
        "UDP ASSOCIATE: bound relay socket on {} for client {}",
        local_addr, client_addr
    );

    // Spawn UDP relay task
    tokio::spawn(async move {
        if let Err(e) = run_udp_relay(
            udp_socket,
            client_addr,
            session_manager.clone(),
            session_id,
            shutdown_rx,
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

    Ok(local_addr)
}

/// Run the UDP relay loop
async fn run_udp_relay(
    socket: UdpSocket,
    client_addr: SocketAddr,
    session_manager: Arc<SessionManager>,
    session_id: Uuid,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    let socket = Arc::new(socket);
    let session_map = Arc::new(UdpSessionMap::new());

    let mut buf = vec![0u8; 65535]; // Max UDP packet size
    let udp_timeout = Duration::from_secs(120); // 2 minutes idle timeout

    loop {
        // Wait for packet or shutdown signal
        tokio::select! {
            result = timeout(udp_timeout, socket.recv_from(&mut buf)) => {
                match result {
                    Ok(Ok((len, peer_addr))) => {
                        let packet_data = &buf[..len];

                        // Determine if this is from client or from destination
                        if peer_addr.ip() == client_addr.ip() {
                            // Packet from client to destination
                            if let Err(e) = handle_client_packet(
                                &socket,
                                packet_data,
                                peer_addr,
                                &session_map,
                                &session_manager,
                                &session_id,
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
    packet_data: &[u8],
    client_addr: SocketAddr,
    session_map: &Arc<UdpSessionMap>,
    session_manager: &Arc<SessionManager>,
    session_id: &Uuid,
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

    // Resolve destination address
    let dest_candidates = resolve_address(&packet.header.address, packet.header.port).await?;

    // Try to connect to first available destination
    let dest_addr = dest_candidates
        .first()
        .ok_or_else(|| RustSocksError::Protocol("No destination address resolved".to_string()))?;

    // Store session mapping
    session_map.insert(client_addr, *dest_addr, *session_id);

    // Forward raw data to destination (without SOCKS5 header)
    let sent = socket.send_to(&packet.data, dest_addr).await?;

    // Update session traffic
    session_manager
        .update_traffic(session_id, sent as u64, 0, 1, 0)
        .await;

    debug!(
        "Forwarded {} bytes from client {} to destination {}",
        sent, client_addr, dest_addr
    );

    Ok(())
}

/// Handle packet from destination (forward back to client)
async fn handle_destination_packet(
    socket: &Arc<UdpSocket>,
    packet_data: &[u8],
    dest_addr: SocketAddr,
    session_map: &Arc<UdpSessionMap>,
    session_manager: &Arc<SessionManager>,
    session_id: &Uuid,
) -> Result<()> {
    // Find client address from reverse mapping
    let client_addr = session_map.get_client(&dest_addr).ok_or_else(|| {
        RustSocksError::Protocol(format!("No client mapping for destination {}", dest_addr))
    })?;

    debug!(
        "UDP destination packet: {} -> {} ({} bytes)",
        dest_addr,
        client_addr,
        packet_data.len()
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
        data: packet_data.to_vec(),
    };

    let response_bytes = serialize_udp_packet(&response_packet);

    // Send to client
    let sent = socket.send_to(&response_bytes, client_addr).await?;

    // Update session traffic
    session_manager
        .update_traffic(session_id, 0, packet_data.len() as u64, 0, 1)
        .await;

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
