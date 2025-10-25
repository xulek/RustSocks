use crate::acl::{AclDecision, AclEngine, AclStats, Protocol};
use crate::auth::AuthManager;
use crate::protocol::*;
use crate::server::proxy::{proxy_data, TrafficUpdateConfig};
use crate::server::resolver::resolve_address;
use crate::session::{ConnectionInfo, SessionManager, SessionProtocol, SessionStatus};
use crate::utils::error::{Result, RustSocksError};
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

pub async fn handle_client(
    mut client_stream: TcpStream,
    auth_manager: Arc<AuthManager>,
    acl_engine: Option<Arc<AclEngine>>,
    acl_stats: Arc<AclStats>,
    anonymous_user: Arc<String>,
    session_manager: Arc<SessionManager>,
    traffic_config: TrafficUpdateConfig,
    client_addr: std::net::SocketAddr,
) -> Result<()> {
    // Step 1: Method selection
    let greeting = parse_client_greeting(&mut client_stream).await?;

    debug!("Client offered methods: {:?}", greeting.methods);

    // Select auth method
    let server_method = if greeting.methods.contains(&auth_manager.get_method()) {
        auth_manager.get_method()
    } else if greeting.methods.contains(&AuthMethod::NoAuth)
        && auth_manager.supports(AuthMethod::NoAuth)
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
    let user = auth_manager
        .authenticate(&mut client_stream, server_method)
        .await?;

    if let Some(ref username) = user {
        info!("User authenticated: {}", username);
    }

    let acl_user = user
        .clone()
        .unwrap_or_else(|| anonymous_user.as_ref().clone());

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
    if let Some(engine) = acl_engine.as_ref() {
        let protocol = match request.command {
            Command::UdpAssociate => Protocol::Udp,
            _ => Protocol::Tcp,
        };

        let (decision, matched_rule) = engine
            .evaluate(&acl_user, &request.address, request.port, &protocol)
            .await;

        match decision {
            AclDecision::Block => {
                acl_stats.record_block(&acl_user);
                let rule = matched_rule.as_deref().unwrap_or("unknown rule");

                warn!(
                    user = %acl_user,
                    dest = %dest_string,
                    port = request.port,
                    rule,
                    "ACL blocked connection"
                );

                session_manager
                    .track_rejected_session(
                        &acl_user,
                        client_addr.ip(),
                        client_addr.port(),
                        dest_string.clone(),
                        request.port,
                        session_protocol,
                        matched_rule.clone(),
                    )
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
                acl_stats.record_allow(&acl_user);
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
            handle_connect(
                client_stream,
                &request.address,
                request.port,
                session_manager,
                acl_user,
                client_addr,
                acl_decision,
                acl_rule_match,
                session_protocol,
                traffic_config,
            )
            .await?;
        }
        Command::Bind => {
            warn!("BIND command not yet implemented");
            send_socks5_response(
                &mut client_stream,
                ReplyCode::CommandNotSupported,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
            return Err(RustSocksError::UnsupportedCommand(0x02));
        }
        Command::UdpAssociate => {
            warn!("UDP ASSOCIATE command not yet implemented");
            send_socks5_response(
                &mut client_stream,
                ReplyCode::CommandNotSupported,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
            return Err(RustSocksError::UnsupportedCommand(0x03));
        }
    }

    Ok(())
}

async fn handle_connect(
    mut client_stream: TcpStream,
    dest_addr: &Address,
    dest_port: u16,
    session_manager: Arc<SessionManager>,
    session_user: String,
    client_addr: std::net::SocketAddr,
    acl_decision: String,
    acl_rule_match: Option<String>,
    session_protocol: SessionProtocol,
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
                std::io::Error::new(std::io::ErrorKind::Other, "no reachable upstream addresses")
            })));
        }
    };

    let peer_display = upstream_stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| format!("{}:{}", dest_host, dest_port));

    // Session tracking
    let connection_info = ConnectionInfo {
        source_ip: client_addr.ip(),
        source_port: client_addr.port(),
        dest_ip: dest_host.clone(),
        dest_port,
        protocol: session_protocol,
    };

    let session_id = session_manager
        .new_session(
            &session_user,
            connection_info,
            acl_decision,
            acl_rule_match.clone(),
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
