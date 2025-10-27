use crate::acl::{AclDecision, AclEngine, AclStats, Protocol};
use crate::auth::AuthManager;
use crate::protocol::*;
use crate::qos::{ConnectionLimits, QosEngine};
use crate::server::bind::handle_bind as handle_bind_relay;
use crate::server::proxy::{proxy_data, TrafficUpdateConfig};
use crate::server::resolver::resolve_address;
use crate::server::udp::handle_udp_associate as handle_udp_relay;
use crate::session::{ConnectionInfo, SessionManager, SessionProtocol, SessionStatus};
use crate::utils::error::{Result, RustSocksError};
use std::net::IpAddr;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Optimize TCP socket for low-latency proxying
/// - Disables Nagle's algorithm (TCP_NODELAY) for lower latency
fn optimize_tcp_socket(stream: &TcpStream) -> Result<()> {
    // Disable Nagle's algorithm - improves latency for small packets
    // This is the single most impactful TCP optimization for proxy workloads
    stream.set_nodelay(true)?;

    Ok(())
}

/// Context for handling client connections
pub struct ClientHandlerContext {
    pub auth_manager: Arc<AuthManager>,
    pub acl_engine: Option<Arc<AclEngine>>,
    pub acl_stats: Arc<AclStats>,
    pub anonymous_user: Arc<String>,
    pub session_manager: Arc<SessionManager>,
    pub traffic_config: TrafficUpdateConfig,
    pub qos_engine: QosEngine,
    pub connection_limits: ConnectionLimits,
}

pub async fn handle_client(
    mut client_stream: TcpStream,
    ctx: Arc<ClientHandlerContext>,
    client_addr: std::net::SocketAddr,
) -> Result<()> {
    ctx.auth_manager
        .authenticate_client(client_addr.ip())
        .await?;

    let version = client_stream.read_u8().await?;

    match version {
        SOCKS_VERSION => handle_socks5(client_stream, ctx, client_addr, version).await,
        SOCKS4_VERSION => handle_socks4(client_stream, ctx, client_addr).await,
        _ => Err(RustSocksError::Protocol(format!(
            "Unsupported SOCKS version: 0x{:02x}",
            version
        ))),
    }
}

async fn send_socks_response(
    stream: &mut TcpStream,
    protocol: SocksProtocol,
    reply: ReplyCode,
    bind_addr: Address,
    bind_port: u16,
) -> Result<()> {
    match protocol {
        SocksProtocol::V5 => send_socks5_response(stream, reply, bind_addr, bind_port).await,
        SocksProtocol::V4 => {
            let socks4_reply = match reply {
                ReplyCode::Succeeded => Socks4Reply::Granted,
                _ => Socks4Reply::Rejected,
            };

            let addr_octets = match bind_addr {
                Address::IPv4(octets) => octets,
                _ => [0u8; 4],
            };

            let port = if reply == ReplyCode::Succeeded {
                bind_port
            } else {
                0
            };

            send_socks4_response(stream, socks4_reply, addr_octets, port).await
        }
    }
}

async fn handle_socks5(
    mut client_stream: TcpStream,
    ctx: Arc<ClientHandlerContext>,
    client_addr: std::net::SocketAddr,
    version: u8,
) -> Result<()> {
    // Step 1: Method selection
    let greeting = parse_socks5_client_greeting(&mut client_stream, version).await?;

    debug!("Client offered methods: {:?}", greeting.methods);

    // Select auth method
    let server_method = if greeting.methods.contains(&ctx.auth_manager.get_method()) {
        ctx.auth_manager.get_method()
    } else if greeting.methods.contains(&AuthMethod::NoAuth)
        && ctx.auth_manager.supports(AuthMethod::NoAuth)
    {
        AuthMethod::NoAuth
    } else {
        send_server_choice(&mut client_stream, AuthMethod::NoAcceptable).await?;
        return Err(RustSocksError::AuthFailed(
            "No acceptable auth method".to_string(),
        ));
    };

    send_server_choice(&mut client_stream, server_method).await?;

    // Step 2: Authentication
    let user = ctx
        .auth_manager
        .authenticate(&mut client_stream, server_method, client_addr.ip())
        .await?;

    if let Some(ref username) = user {
        info!("User authenticated: {}", username);
    }

    let acl_user = user
        .clone()
        .unwrap_or_else(|| ctx.anonymous_user.as_ref().clone());

    // Step 2b: Check connection limits (QoS)
    if let Err(e) = ctx
        .qos_engine
        .check_and_inc_connection(&acl_user, &ctx.connection_limits)
    {
        warn!(
            user = %acl_user,
            error = %e,
            "Connection limit exceeded"
        );
        send_socks_response(
            &mut client_stream,
            SocksProtocol::V5,
            ReplyCode::ConnectionNotAllowed,
            Address::IPv4([0, 0, 0, 0]),
            0,
        )
        .await?;
        return Err(e);
    }

    // Ensure connection is decremented on drop
    let _connection_guard = ConnectionGuard {
        qos_engine: ctx.qos_engine.clone(),
        user: acl_user.clone(),
    };

    // Step 3: SOCKS5 request
    let request = parse_socks5_request(&mut client_stream).await?;

    let dest_string = request.address.to_string();
    info!(
        "SOCKS5 request: command={:?}, dest={}:{}",
        request.command, dest_string, request.port
    );

    let session_protocol = match request.command {
        Command::UdpAssociate => SessionProtocol::Udp,
        _ => SessionProtocol::Tcp,
    };

    let mut acl_rule_match: Option<String> = None;
    let mut acl_decision = "allow".to_string();

    // Step 3b: ACL enforcement (if enabled)
    if let Some(engine) = ctx.acl_engine.as_ref() {
        let protocol = match request.command {
            Command::UdpAssociate => Protocol::Udp,
            _ => Protocol::Tcp,
        };

        let (decision, matched_rule) = engine
            .evaluate(&acl_user, &request.address, request.port, &protocol)
            .await;

        match decision {
            AclDecision::Block => {
                ctx.acl_stats.record_block(&acl_user);
                let rule = matched_rule.as_deref().unwrap_or("unknown rule");

                warn!(
                    user = %acl_user,
                    dest = %dest_string,
                    port = request.port,
                    rule,
                    "ACL blocked connection"
                );

                let conn_info = ConnectionInfo {
                    source_ip: client_addr.ip(),
                    source_port: client_addr.port(),
                    dest_ip: dest_string.clone(),
                    dest_port: request.port,
                    protocol: session_protocol,
                };
                ctx.session_manager
                    .track_rejected_session(&acl_user, conn_info, matched_rule.clone())
                    .await;

                send_socks_response(
                    &mut client_stream,
                    SocksProtocol::V5,
                    ReplyCode::ConnectionNotAllowed,
                    Address::IPv4([0, 0, 0, 0]),
                    0,
                )
                .await?;

                return Ok(());
            }
            AclDecision::Allow => {
                ctx.acl_stats.record_allow(&acl_user);
                acl_rule_match = matched_rule.clone();
                acl_decision = "allow".to_string();

                match matched_rule.as_deref() {
                    Some(rule) => debug!(
                        user = %acl_user,
                        dest = %dest_string,
                        port = request.port,
                        rule,
                        "ACL allowed connection"
                    ),
                    None => debug!(
                        user = %acl_user,
                        dest = %dest_string,
                        port = request.port,
                        "ACL allowed connection (default policy)"
                    ),
                }
            }
        }
    }

    // Step 4: Handle command
    match request.command {
        Command::Connect => {
            let session_ctx = SessionContext {
                user: acl_user.clone(),
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
                protocol: session_protocol,
                qos_engine: ctx.qos_engine.clone(),
            };
            handle_connect(
                client_stream,
                &request.address,
                request.port,
                ctx.session_manager.clone(),
                session_ctx,
                ctx.traffic_config,
                SocksProtocol::V5,
            )
            .await?;
        }
        Command::Bind => {
            let bind_ctx = crate::server::bind::BindContext {
                user: acl_user,
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
                qos_engine: ctx.qos_engine.clone(),
            };

            handle_bind_relay(
                client_stream,
                &request.address,
                request.port,
                ctx.session_manager.clone(),
                bind_ctx,
            )
            .await?;
        }
        Command::UdpAssociate => {
            let session_ctx = SessionContext {
                user: acl_user,
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
                protocol: session_protocol,
                qos_engine: ctx.qos_engine.clone(),
            };
            handle_udp_associate(
                client_stream,
                &request.address,
                request.port,
                ctx.session_manager.clone(),
                session_ctx,
            )
            .await?;
        }
    }

    Ok(())
}

async fn handle_socks4(
    mut client_stream: TcpStream,
    ctx: Arc<ClientHandlerContext>,
    client_addr: std::net::SocketAddr,
) -> Result<()> {
    if !ctx.auth_manager.supports(AuthMethod::NoAuth) {
        warn!(
            client = %client_addr,
            "SOCKS4 request rejected: server requires authentication"
        );
        send_socks_response(
            &mut client_stream,
            SocksProtocol::V4,
            ReplyCode::ConnectionNotAllowed,
            Address::IPv4([0, 0, 0, 0]),
            0,
        )
        .await?;
        return Err(RustSocksError::AuthFailed(
            "SOCKS4 requires no-auth configuration".to_string(),
        ));
    }

    // Perform no-auth path to allow future auth hooks (e.g., PAM address)
    let _ = ctx
        .auth_manager
        .authenticate(&mut client_stream, AuthMethod::NoAuth, client_addr.ip())
        .await?;

    let request = parse_socks4_request(&mut client_stream).await?;

    let dest_string = request.address.to_string();
    info!(
        "SOCKS4 request: command={:?}, dest={}:{} user_id={:?}",
        request.command, dest_string, request.port, request.user_id
    );

    let user = request.user_id.clone();
    let acl_user = user
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ctx.anonymous_user.as_ref().clone());

    if let Some(ref username) = user {
        if !username.is_empty() {
            info!("SOCKS4 user identifier received: {}", username);
        }
    }

    if let Err(e) = ctx
        .qos_engine
        .check_and_inc_connection(&acl_user, &ctx.connection_limits)
    {
        warn!(
            user = %acl_user,
            error = %e,
            "Connection limit exceeded (SOCKS4)"
        );
        send_socks_response(
            &mut client_stream,
            SocksProtocol::V4,
            ReplyCode::ConnectionNotAllowed,
            Address::IPv4([0, 0, 0, 0]),
            0,
        )
        .await?;
        return Err(e);
    }

    let _connection_guard = ConnectionGuard {
        qos_engine: ctx.qos_engine.clone(),
        user: acl_user.clone(),
    };

    let session_protocol = SessionProtocol::Tcp;
    let mut acl_rule_match: Option<String> = None;
    let mut acl_decision = "allow".to_string();

    if let Some(engine) = ctx.acl_engine.as_ref() {
        let (decision, matched_rule) = engine
            .evaluate(&acl_user, &request.address, request.port, &Protocol::Tcp)
            .await;

        match decision {
            AclDecision::Block => {
                ctx.acl_stats.record_block(&acl_user);
                let rule = matched_rule.as_deref().unwrap_or("unknown rule");

                warn!(
                    user = %acl_user,
                    dest = %dest_string,
                    port = request.port,
                    rule,
                    "ACL blocked SOCKS4 connection"
                );

                let conn_info = ConnectionInfo {
                    source_ip: client_addr.ip(),
                    source_port: client_addr.port(),
                    dest_ip: dest_string.clone(),
                    dest_port: request.port,
                    protocol: session_protocol,
                };
                ctx.session_manager
                    .track_rejected_session(&acl_user, conn_info, matched_rule.clone())
                    .await;

                send_socks_response(
                    &mut client_stream,
                    SocksProtocol::V4,
                    ReplyCode::ConnectionNotAllowed,
                    Address::IPv4([0, 0, 0, 0]),
                    0,
                )
                .await?;

                return Ok(());
            }
            AclDecision::Allow => {
                ctx.acl_stats.record_allow(&acl_user);
                acl_rule_match = matched_rule.clone();
                acl_decision = "allow".to_string();
            }
        }
    }

    match request.command {
        Command::Connect => {
            let session_ctx = SessionContext {
                user: acl_user.clone(),
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
                protocol: session_protocol,
                qos_engine: ctx.qos_engine.clone(),
            };

            handle_connect(
                client_stream,
                &request.address,
                request.port,
                ctx.session_manager.clone(),
                session_ctx,
                ctx.traffic_config,
                SocksProtocol::V4,
            )
            .await?;
        }
        Command::Bind => {
            warn!(
                "SOCKS4 BIND not supported for dest {}:{}",
                dest_string, request.port
            );
            send_socks_response(
                &mut client_stream,
                SocksProtocol::V4,
                ReplyCode::CommandNotSupported,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
        }
        _ => {
            warn!(
                "Unsupported SOCKS4 command {:?} for dest {}:{}",
                request.command, dest_string, request.port
            );
            send_socks_response(
                &mut client_stream,
                SocksProtocol::V4,
                ReplyCode::CommandNotSupported,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
        }
    }

    Ok(())
}

struct SessionContext {
    user: String,
    client_addr: std::net::SocketAddr,
    acl_decision: String,
    acl_rule: Option<String>,
    protocol: SessionProtocol,
    qos_engine: QosEngine,
}

async fn handle_connect(
    mut client_stream: TcpStream,
    dest_addr: &Address,
    dest_port: u16,
    session_manager: Arc<SessionManager>,
    session_ctx: SessionContext,
    traffic_config: TrafficUpdateConfig,
    protocol: SocksProtocol,
) -> Result<()> {
    let dest_host = match dest_addr {
        Address::IPv4(octets) => std::net::Ipv4Addr::from(*octets).to_string(),
        Address::IPv6(octets) => std::net::Ipv6Addr::from(*octets).to_string(),
        Address::Domain(domain) => domain.clone(),
    };

    let mut candidates = match resolve_address(dest_addr, dest_port).await {
        Ok(list) => list,
        Err(e) => {
            warn!(
                "Destination resolution failed for {}:{}: {}",
                dest_host, dest_port, e
            );
            send_socks_response(
                &mut client_stream,
                protocol,
                ReplyCode::HostUnreachable,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
            return Err(e);
        }
    };

    if matches!(protocol, SocksProtocol::V4) {
        candidates.retain(|addr| matches!(addr.ip(), IpAddr::V4(_)));
        if candidates.is_empty() {
            warn!(
                "SOCKS4 request {}:{} resolved to non-IPv4 addresses",
                dest_host, dest_port
            );
            send_socks_response(
                &mut client_stream,
                protocol,
                ReplyCode::HostUnreachable,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
            return Err(RustSocksError::Protocol(
                "SOCKS4 requires IPv4 destination".to_string(),
            ));
        }
    }

    let mut last_err: Option<std::io::Error> = None;
    let mut upstream_stream_opt = None;

    for target in candidates {
        debug!("Attempting upstream connection to {}", target);
        match TcpStream::connect(target).await {
            Ok(stream) => {
                // Optimize TCP socket for low latency and high throughput
                if let Err(e) = optimize_tcp_socket(&stream) {
                    warn!("Failed to optimize upstream TCP socket: {}", e);
                }
                upstream_stream_opt = Some(stream);
                break;
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    let upstream_stream = match upstream_stream_opt {
        Some(stream) => stream,
        None => {
            if let Some(ref err) = last_err {
                warn!("Failed to connect to {}:{}: {}", dest_host, dest_port, err);
            }
            send_socks_response(
                &mut client_stream,
                protocol,
                ReplyCode::HostUnreachable,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
            return Err(RustSocksError::Io(last_err.unwrap_or_else(|| {
                std::io::Error::other("no reachable upstream addresses")
            })));
        }
    };

    let peer_display = upstream_stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| format!("{}:{}", dest_host, dest_port));

    // Session tracking
    let connection_info = ConnectionInfo {
        source_ip: session_ctx.client_addr.ip(),
        source_port: session_ctx.client_addr.port(),
        dest_ip: dest_host.clone(),
        dest_port,
        protocol: session_ctx.protocol,
    };

    let (session_id, cancel_token) = session_manager
        .new_session_with_control(
            &session_ctx.user,
            connection_info,
            session_ctx.acl_decision.clone(),
            session_ctx.acl_rule.clone(),
            None,
        )
        .await;

    // Get local address for response
    let local_addr = upstream_stream.local_addr()?;
    let bind_addr = match local_addr {
        std::net::SocketAddr::V4(addr) => Address::IPv4(addr.ip().octets()),
        std::net::SocketAddr::V6(addr) => Address::IPv6(addr.ip().octets()),
    };
    let bind_port = local_addr.port();

    if matches!(protocol, SocksProtocol::V4) && !matches!(&bind_addr, Address::IPv4(_)) {
        warn!(
            "SOCKS4 client received non-IPv4 bind address {}:{}",
            dest_host, dest_port
        );
        send_socks_response(
            &mut client_stream,
            protocol,
            ReplyCode::AddressTypeNotSupported,
            Address::IPv4([0, 0, 0, 0]),
            0,
        )
        .await?;
        return Err(RustSocksError::UnsupportedAddressType(0x04));
    }

    // Send success response
    send_socks_response(
        &mut client_stream,
        protocol,
        ReplyCode::Succeeded,
        bind_addr,
        bind_port,
    )
    .await?;

    info!("Connected to {}, proxying data", peer_display);

    // Proxy data between client and upstream
    match proxy_data(
        client_stream,
        upstream_stream,
        session_manager.clone(),
        session_id,
        cancel_token,
        traffic_config,
        session_ctx.qos_engine.clone(),
        session_ctx.user.clone(),
    )
    .await
    {
        Ok(_) => {
            session_manager
                .close_session(
                    &session_id,
                    Some("Connection closed normally".to_string()),
                    SessionStatus::Closed,
                )
                .await;
            Ok(())
        }
        Err(RustSocksError::ConnectionClosed) => {
            debug!(session = %session_id, "Session cancelled");
            Ok(())
        }
        Err(e) => {
            let reason = format!("Proxy error: {}", e);
            session_manager
                .close_session(&session_id, Some(reason), SessionStatus::Failed)
                .await;
            Err(e)
        }
    }
}

async fn handle_udp_associate(
    mut client_stream: TcpStream,
    _dest_addr: &Address,
    _dest_port: u16,
    session_manager: Arc<SessionManager>,
    session_ctx: SessionContext,
) -> Result<()> {
    let dest_host = "0.0.0.0".to_string(); // UDP ASSOCIATE doesn't specify real destination yet

    // Create shutdown channel for UDP relay
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

    // Create session
    let connection_info = ConnectionInfo {
        source_ip: session_ctx.client_addr.ip(),
        source_port: session_ctx.client_addr.port(),
        dest_ip: dest_host.clone(),
        dest_port: 0,
        protocol: session_ctx.protocol,
    };

    let (session_id, cancel_token) = session_manager
        .new_session_with_control(
            &session_ctx.user,
            connection_info,
            session_ctx.acl_decision.clone(),
            session_ctx.acl_rule.clone(),
            Some(shutdown_tx.clone()),
        )
        .await;

    // Start UDP relay
    let udp_relay_addr = match handle_udp_relay(
        session_ctx.client_addr,
        session_manager.clone(),
        session_id,
        shutdown_rx,
    )
    .await
    {
        Ok(addr) => addr,
        Err(e) => {
            warn!("Failed to start UDP relay: {}", e);
            send_socks5_response(
                &mut client_stream,
                ReplyCode::GeneralFailure,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
            session_manager
                .close_session(
                    &session_id,
                    Some(format!("UDP relay start failed: {}", e)),
                    SessionStatus::Failed,
                )
                .await;
            return Err(e);
        }
    };

    // Send success response with UDP relay address
    let bind_addr = match udp_relay_addr {
        std::net::SocketAddr::V4(addr) => Address::IPv4(addr.ip().octets()),
        std::net::SocketAddr::V6(addr) => Address::IPv6(addr.ip().octets()),
    };
    let bind_port = udp_relay_addr.port();

    send_socks5_response(
        &mut client_stream,
        ReplyCode::Succeeded,
        bind_addr,
        bind_port,
    )
    .await?;

    info!(
        "UDP ASSOCIATE established: relay on {}, client {}",
        udp_relay_addr, session_ctx.client_addr
    );

    // Keep TCP connection alive - when it closes, UDP session ends
    // Read from stream to detect disconnect
    let mut buf = [0u8; 1];
    tokio::select! {
        result = tokio::io::AsyncReadExt::read(&mut client_stream, &mut buf) => {
            match result {
                Ok(0) | Err(_) => {
                    debug!("TCP control connection closed, terminating UDP session");
                    let _ = shutdown_tx.send(());
                    session_manager
                        .close_session(
                            &session_id,
                            Some("TCP control connection closed".to_string()),
                            SessionStatus::Closed,
                        )
                        .await;
                }
                Ok(_) => {
                    debug!("Unexpected data on TCP control connection");
                }
            }
        }
        _ = cancel_token.cancelled() => {
            debug!("UDP session cancelled via ACL update");
        }
    }

    Ok(())
}

/// RAII guard to ensure connection count is decremented on drop
struct ConnectionGuard {
    qos_engine: QosEngine,
    user: String,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.qos_engine.dec_user_connection(&self.user);
    }
}
