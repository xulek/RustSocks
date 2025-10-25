use crate::acl::{AclDecision, AclEngine, AclStats, Protocol};
use crate::auth::AuthManager;
use crate::protocol::*;
use crate::server::bind::handle_bind as handle_bind_relay;
use crate::server::proxy::{proxy_data, TrafficUpdateConfig};
use crate::server::resolver::resolve_address;
use crate::server::udp::handle_udp_associate as handle_udp_relay;
use crate::session::{ConnectionInfo, SessionManager, SessionProtocol, SessionStatus};
use crate::utils::error::{Result, RustSocksError};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Context for handling client connections
pub struct ClientHandlerContext {
    pub auth_manager: Arc<AuthManager>,
    pub acl_engine: Option<Arc<AclEngine>>,
    pub acl_stats: Arc<AclStats>,
    pub anonymous_user: Arc<String>,
    pub session_manager: Arc<SessionManager>,
    pub traffic_config: TrafficUpdateConfig,
}

pub async fn handle_client(
    mut client_stream: TcpStream,
    ctx: Arc<ClientHandlerContext>,
    client_addr: std::net::SocketAddr,
) -> Result<()> {
    // Step 1: Method selection
    let greeting = parse_client_greeting(&mut client_stream).await?;

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
        .authenticate(&mut client_stream, server_method)
        .await?;

    if let Some(ref username) = user {
        info!("User authenticated: {}", username);
    }

    let acl_user = user
        .clone()
        .unwrap_or_else(|| ctx.anonymous_user.as_ref().clone());

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

                send_socks5_response(
                    &mut client_stream,
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
                user: acl_user,
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
                protocol: session_protocol,
            };
            handle_connect(
                client_stream,
                &request.address,
                request.port,
                ctx.session_manager.clone(),
                session_ctx,
                ctx.traffic_config,
            )
            .await?;
        }
        Command::Bind => {
            let bind_ctx = crate::server::bind::BindContext {
                user: acl_user,
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
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

struct SessionContext {
    user: String,
    client_addr: std::net::SocketAddr,
    acl_decision: String,
    acl_rule: Option<String>,
    protocol: SessionProtocol,
}

async fn handle_connect(
    mut client_stream: TcpStream,
    dest_addr: &Address,
    dest_port: u16,
    session_manager: Arc<SessionManager>,
    session_ctx: SessionContext,
    traffic_config: TrafficUpdateConfig,
) -> Result<()> {
    let dest_host = match dest_addr {
        Address::IPv4(octets) => std::net::Ipv4Addr::from(*octets).to_string(),
        Address::IPv6(octets) => std::net::Ipv6Addr::from(*octets).to_string(),
        Address::Domain(domain) => domain.clone(),
    };

    let candidates = match resolve_address(dest_addr, dest_port).await {
        Ok(list) => list,
        Err(e) => {
            warn!(
                "Destination resolution failed for {}:{}: {}",
                dest_host, dest_port, e
            );
            send_socks5_response(
                &mut client_stream,
                ReplyCode::HostUnreachable,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
            return Err(e);
        }
    };

    let mut last_err: Option<std::io::Error> = None;
    let mut upstream_stream_opt = None;

    for target in candidates {
        debug!("Attempting upstream connection to {}", target);
        match TcpStream::connect(target).await {
            Ok(stream) => {
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
            send_socks5_response(
                &mut client_stream,
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

    let session_id = session_manager
        .new_session(
            &session_ctx.user,
            connection_info,
            session_ctx.acl_decision.clone(),
            session_ctx.acl_rule.clone(),
        )
        .await;

    // Get local address for response
    let local_addr = upstream_stream.local_addr()?;
    let bind_addr = match local_addr {
        std::net::SocketAddr::V4(addr) => Address::IPv4(addr.ip().octets()),
        std::net::SocketAddr::V6(addr) => Address::IPv6(addr.ip().octets()),
    };
    let bind_port = local_addr.port();

    // Send success response
    send_socks5_response(
        &mut client_stream,
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
        traffic_config,
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

    // Create session
    let connection_info = ConnectionInfo {
        source_ip: session_ctx.client_addr.ip(),
        source_port: session_ctx.client_addr.port(),
        dest_ip: dest_host.clone(),
        dest_port: 0,
        protocol: session_ctx.protocol,
    };

    let session_id = session_manager
        .new_session(
            &session_ctx.user,
            connection_info,
            session_ctx.acl_decision.clone(),
            session_ctx.acl_rule.clone(),
        )
        .await;

    // Create shutdown channel for UDP relay
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

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
    match tokio::io::AsyncReadExt::read(&mut client_stream, &mut buf).await {
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

    Ok(())
}
