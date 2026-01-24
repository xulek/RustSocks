use crate::acl::{AclDecision, AclEngine, AclStats, Protocol};
use crate::auth::AuthManager;
use crate::protocol::*;
use crate::qos::{ConnectionLimits, QosEngine};
use crate::server::bind::handle_bind as handle_bind_relay;
use crate::server::pool::{ConnectionPool, ReuseHint};
use crate::server::proxy::{proxy_data, ProxyContext, TrafficUpdateConfig};
use crate::server::resolver::resolve_address;
use crate::server::udp::{handle_udp_associate as handle_udp_relay, UdpRelayContext};
use crate::session::{ConnectionInfo, SessionManager, SessionProtocol, SessionStatus};
use crate::utils::error::{Result, RustSocksError};
use std::io::ErrorKind;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, BufReader};
use tokio::sync::broadcast;
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};

fn handshake_timeout_error() -> RustSocksError {
    RustSocksError::Io(std::io::Error::new(
        ErrorKind::TimedOut,
        "handshake timed out",
    ))
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
    pub connection_pool: Arc<ConnectionPool>,
    pub handshake_timeout: Duration,
}

pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T> IoStream for T where T: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

#[instrument(
    level = "debug",
    skip(client_stream, ctx),
    fields(client = %client_addr)
)]
pub async fn handle_client<S>(
    mut client_stream: S,
    ctx: Arc<ClientHandlerContext>,
    client_addr: std::net::SocketAddr,
) -> Result<()>
where
    S: IoStream,
{
    ctx.auth_manager
        .authenticate_client(client_addr.ip())
        .await?;

    let version = match timeout(ctx.handshake_timeout, client_stream.read_u8()).await {
        Ok(Ok(version)) => version,
        Ok(Err(err)) => return Err(RustSocksError::Io(err)),
        Err(_) => return Err(handshake_timeout_error()),
    };

    match version {
        SOCKS_VERSION => handle_socks5(client_stream, ctx, client_addr, version).await,
        SOCKS4_VERSION => handle_socks4(client_stream, ctx, client_addr).await,
        _ => Err(RustSocksError::Protocol(format!(
            "Unsupported SOCKS version: 0x{:02x}",
            version
        ))),
    }
}

async fn send_socks_response<S>(
    stream: &mut S,
    protocol: SocksProtocol,
    reply: ReplyCode,
    bind_addr: Address,
    bind_port: u16,
) -> Result<()>
where
    S: IoStream,
{
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

#[instrument(
    level = "debug",
    skip(client_stream, ctx),
    fields(client = %client_addr, version)
)]
async fn handle_socks5<S>(
    client_stream: S,
    ctx: Arc<ClientHandlerContext>,
    client_addr: std::net::SocketAddr,
    version: u8,
) -> Result<()>
where
    S: IoStream,
{
    let handshake_timeout = ctx.handshake_timeout;
    let handshake = timeout(handshake_timeout, async {
        // Optimization: Wrap stream in BufReader to reduce syscalls during protocol parsing
        // This reduces 3 separate read() calls to 1 buffered read
        // BufReader will be unwrapped before data proxying phase
        let mut buffered_stream = BufReader::with_capacity(4096, client_stream);

        // Step 1: Method selection
        let greeting = parse_socks5_client_greeting(&mut buffered_stream, version).await?;

        debug!("Client offered methods: {:?}", greeting.methods);

        // Select auth method
        let server_method = if greeting.methods.contains(&ctx.auth_manager.get_method()) {
            ctx.auth_manager.get_method()
        } else if greeting.methods.contains(&AuthMethod::NoAuth)
            && ctx.auth_manager.supports(AuthMethod::NoAuth)
        {
            AuthMethod::NoAuth
        } else {
            // Use get_mut() to access underlying stream for write operations
            send_server_choice(buffered_stream.get_mut(), AuthMethod::NoAcceptable).await?;
            return Err(RustSocksError::AuthFailed(
                "No acceptable auth method".to_string(),
            ));
        };

        send_server_choice(buffered_stream.get_mut(), server_method).await?;

        // Step 2: Authentication (reads buffered, writes through get_mut())
        let auth_result = ctx
            .auth_manager
            .authenticate(&mut buffered_stream, server_method, client_addr.ip())
            .await?;

        // Extract username and groups from authentication result
        let (user, user_groups) = match auth_result {
            Some((username, groups)) => {
                info!(
                    user = %username,
                    group_count = groups.len(),
                    "User authenticated with groups from LDAP"
                );
                (Some(username), groups)
            }
            None => {
                debug!("No authentication (anonymous user)");
                (None, Vec::new())
            }
        };

        let acl_user: Arc<str> = user
            .map(|username| Arc::from(username.into_boxed_str()))
            .unwrap_or_else(|| Arc::from(ctx.anonymous_user.as_str()));

        // Step 2b: Check connection limits (QoS)
        let connection_guard = check_connection_limits(
            buffered_stream.get_mut(),
            SocksProtocol::V5,
            &ctx,
            &acl_user,
        )
        .await?;

        // Step 3: SOCKS5 request (buffered read for final handshake message)
        let request = parse_socks5_request(&mut buffered_stream).await?;

        info!(
            user = %acl_user.as_ref(),
            "SOCKS5 request: command={:?}, dest={}:{}",
            request.command, request.address, request.port
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

            // Use evaluate_with_groups() for dynamic LDAP group matching
            let (decision, matched_rule) = engine
                .evaluate_with_groups(
                    acl_user.as_ref(),
                    &user_groups,
                    &request.address,
                    request.port,
                    &protocol,
                )
                .await;

            match decision {
                AclDecision::Block => {
                    let dest_string = request.address.to_string();
                    let block_ctx = AclBlockContext {
                        user: &acl_user,
                        dest_string: &dest_string,
                        port: request.port,
                        client_addr,
                        session_protocol,
                        matched_rule: matched_rule.clone(),
                    };
                    handle_acl_block(
                        buffered_stream.get_mut(),
                        SocksProtocol::V5,
                        &ctx,
                        block_ctx,
                    )
                    .await?;

                    return Ok(None);
                }
                AclDecision::Allow => {
                    ctx.acl_stats.record_allow(acl_user.as_ref());
                    acl_rule_match = matched_rule.clone();
                    acl_decision = "allow".to_string();

                    match matched_rule.as_deref() {
                        Some(rule) => debug!(
                            user = %acl_user.as_ref(),
                            dest = %request.address,
                            port = request.port,
                            rule,
                            "ACL allowed connection"
                        ),
                        None => debug!(
                            user = %acl_user.as_ref(),
                            dest = %request.address,
                            port = request.port,
                            "ACL allowed connection (default policy)"
                        ),
                    }
                }
            }
        }

        Ok(Some((
            buffered_stream,
            request,
            acl_user,
            acl_decision,
            acl_rule_match,
            session_protocol,
            connection_guard,
            user_groups,
        )))
    })
    .await;

    let (
        buffered_stream,
        request,
        acl_user,
        acl_decision,
        acl_rule_match,
        session_protocol,
        _connection_guard,
        user_groups,
    ) = match handshake {
        Ok(Ok(Some(values))) => values,
        Ok(Ok(None)) => return Ok(()),
        Ok(Err(err)) => return Err(err),
        Err(_) => {
            warn!(
                client = %client_addr,
                "SOCKS5 handshake timed out"
            );
            return Err(handshake_timeout_error());
        }
    };

    // Step 4: Handle command
    // Unwrap BufReader to get raw stream for data transfer phase
    // BufReader was only needed for protocol parsing (3 reads) - now we proxy data directly
    let client_stream = buffered_stream.into_inner();

    match request.command {
        Command::Connect => {
            let session_ctx = SessionContext {
                user: Arc::clone(&acl_user),
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
                protocol: session_protocol,
                qos_engine: ctx.qos_engine.clone(),
            };
            let connect_ctx = ConnectHandlerContext {
                session_manager: ctx.session_manager.clone(),
                traffic_config: ctx.traffic_config,
                protocol: SocksProtocol::V5,
                connection_pool: ctx.connection_pool.clone(),
            };
            handle_connect(
                client_stream,
                &request.address,
                request.port,
                connect_ctx,
                session_ctx,
            )
            .await?;
        }
        Command::Bind => {
            let bind_ctx = crate::server::bind::BindContext {
                user: Arc::clone(&acl_user),
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
                qos_engine: ctx.qos_engine.clone(),
                connection_pool: ctx.connection_pool.clone(),
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
                user: Arc::clone(&acl_user),
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
                protocol: session_protocol,
                qos_engine: ctx.qos_engine.clone(),
            };
            let udp_ctx = UdpRelayContext {
                user: Arc::clone(&acl_user),
                user_groups: Arc::new(user_groups),
                acl_engine: ctx.acl_engine.clone(),
                acl_stats: ctx.acl_stats.clone(),
                qos_engine: ctx.qos_engine.clone(),
            };
            handle_udp_associate(
                client_stream,
                &request.address,
                request.port,
                ctx.session_manager.clone(),
                session_ctx,
                udp_ctx,
            )
            .await?;
        }
    }

    Ok(())
}

#[instrument(
    level = "debug",
    skip(client_stream, ctx),
    fields(client = %client_addr)
)]
async fn handle_socks4<S>(
    mut client_stream: S,
    ctx: Arc<ClientHandlerContext>,
    client_addr: std::net::SocketAddr,
) -> Result<()>
where
    S: IoStream,
{
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

    let handshake_timeout = ctx.handshake_timeout;
    let handshake = timeout(handshake_timeout, async {
        // Perform no-auth path to allow future auth hooks (e.g., PAM address)
        let auth_result = ctx
            .auth_manager
            .authenticate(&mut client_stream, AuthMethod::NoAuth, client_addr.ip())
            .await?;

        // Extract groups if any (usually None for SOCKS4 no-auth)
        let user_groups = match auth_result {
            Some((_, groups)) => groups,
            None => Vec::new(),
        };

        let request = parse_socks4_request(&mut client_stream).await?;

        let dest_string = request.address.to_string();
        info!(
            "SOCKS4 request: command={:?}, dest={}:{} user_id={:?}",
            request.command, dest_string, request.port, request.user_id
        );

        let user = request.user_id.clone();
        let acl_user: Arc<str> = user
            .clone()
            .filter(|s| !s.is_empty())
            .map(|username| Arc::from(username.into_boxed_str()))
            .unwrap_or_else(|| Arc::from(ctx.anonymous_user.as_str()));

        if let Some(ref username) = user {
            if !username.is_empty() {
                info!("SOCKS4 user identifier received: {}", username);
            }
        }

        let connection_guard = check_connection_limits(
            &mut client_stream,
            SocksProtocol::V4,
            &ctx,
            &acl_user,
        )
        .await?;

        let session_protocol = SessionProtocol::Tcp;
        let mut acl_rule_match: Option<String> = None;
        let mut acl_decision = "allow".to_string();

        if let Some(engine) = ctx.acl_engine.as_ref() {
            // Use evaluate_with_groups() for dynamic LDAP group matching
            let (decision, matched_rule) = engine
                .evaluate_with_groups(
                    acl_user.as_ref(),
                    &user_groups,
                    &request.address,
                    request.port,
                    &Protocol::Tcp,
                )
                .await;

            match decision {
                AclDecision::Block => {
                    let block_ctx = AclBlockContext {
                        user: &acl_user,
                        dest_string: &dest_string,
                        port: request.port,
                        client_addr,
                        session_protocol,
                        matched_rule: matched_rule.clone(),
                    };
                    handle_acl_block(
                        &mut client_stream,
                        SocksProtocol::V4,
                        &ctx,
                        block_ctx,
                    )
                    .await?;

                    return Ok(None);
                }
                AclDecision::Allow => {
                    ctx.acl_stats.record_allow(acl_user.as_ref());
                    acl_rule_match = matched_rule.clone();
                    acl_decision = "allow".to_string();
                }
            }
        }

        Ok(Some((
            client_stream,
            request,
            acl_user,
            acl_decision,
            acl_rule_match,
            session_protocol,
            connection_guard,
        )))
    })
    .await;

    let (
        client_stream,
        request,
        acl_user,
        acl_decision,
        acl_rule_match,
        session_protocol,
        _connection_guard,
    ) = match handshake {
        Ok(Ok(Some(values))) => values,
        Ok(Ok(None)) => return Ok(()),
        Ok(Err(err)) => return Err(err),
        Err(_) => {
            warn!(
                client = %client_addr,
                "SOCKS4 handshake timed out"
            );
            return Err(handshake_timeout_error());
        }
    };

    let dest_string = request.address.to_string();
    let mut client_stream = client_stream;

    match request.command {
        Command::Connect => {
            let session_ctx = SessionContext {
                user: Arc::clone(&acl_user),
                client_addr,
                acl_decision,
                acl_rule: acl_rule_match,
                protocol: session_protocol,
                qos_engine: ctx.qos_engine.clone(),
            };

            let connect_ctx = ConnectHandlerContext {
                session_manager: ctx.session_manager.clone(),
                traffic_config: ctx.traffic_config,
                protocol: SocksProtocol::V4,
                connection_pool: ctx.connection_pool.clone(),
            };
            handle_connect(
                client_stream,
                &request.address,
                request.port,
                connect_ctx,
                session_ctx,
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
    user: Arc<str>,
    client_addr: std::net::SocketAddr,
    acl_decision: String,
    acl_rule: Option<String>,
    protocol: SessionProtocol,
    qos_engine: QosEngine,
}

struct ConnectHandlerContext {
    session_manager: Arc<SessionManager>,
    traffic_config: TrafficUpdateConfig,
    protocol: SocksProtocol,
    connection_pool: Arc<ConnectionPool>,
}

#[instrument(
    level = "debug",
    skip(client_stream, connect_ctx, session_ctx),
    fields(port = dest_port)
)]
async fn handle_connect<S>(
    mut client_stream: S,
    dest_addr: &Address,
    dest_port: u16,
    connect_ctx: ConnectHandlerContext,
    session_ctx: SessionContext,
) -> Result<()>
where
    S: IoStream,
{
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
                connect_ctx.protocol,
                ReplyCode::HostUnreachable,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
            return Err(e);
        }
    };

    if matches!(connect_ctx.protocol, SocksProtocol::V4) {
        candidates.retain(|addr| matches!(addr.ip(), IpAddr::V4(_)));
        if candidates.is_empty() {
            warn!(
                "SOCKS4 request {}:{} resolved to non-IPv4 addresses",
                dest_host, dest_port
            );
            send_socks_response(
                &mut client_stream,
                connect_ctx.protocol,
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
        match connect_ctx.connection_pool.get(target).await {
            Ok(stream) => {
                upstream_stream_opt = Some((stream, target));
                break;
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    let (upstream_stream, upstream_addr) = match upstream_stream_opt {
        Some((stream, addr)) => (stream, addr),
        None => {
            if let Some(ref err) = last_err {
                warn!("Failed to connect to {}:{}: {}", dest_host, dest_port, err);
            }
            send_socks_response(
                &mut client_stream,
                connect_ctx.protocol,
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

    // Get local address for response
    let local_addr = upstream_stream.local_addr()?;
    let bind_addr = match local_addr {
        std::net::SocketAddr::V4(addr) => Address::IPv4(addr.ip().octets()),
        std::net::SocketAddr::V6(addr) => Address::IPv6(addr.ip().octets()),
    };
    let bind_port = local_addr.port();

    if matches!(connect_ctx.protocol, SocksProtocol::V4) && !matches!(&bind_addr, Address::IPv4(_))
    {
        warn!(
            "SOCKS4 client received non-IPv4 bind address {}:{}",
            dest_host, dest_port
        );
        send_socks_response(
            &mut client_stream,
            connect_ctx.protocol,
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
        connect_ctx.protocol,
        ReplyCode::Succeeded,
        bind_addr,
        bind_port,
    )
    .await?;

    // Session tracking (after response to minimize handshake latency)
    let connection_info = ConnectionInfo {
        source_ip: session_ctx.client_addr.ip(),
        source_port: session_ctx.client_addr.port(),
        dest_ip: dest_host.clone(),
        dest_port,
        protocol: session_ctx.protocol,
    };

    let (session_id, cancel_token) = connect_ctx
        .session_manager
        .new_session_with_control(
            session_ctx.user.as_ref(),
            connection_info,
            session_ctx.acl_decision.clone(),
            session_ctx.acl_rule.clone(),
            None,
        )
        .await;

    if tracing::enabled!(tracing::Level::INFO) {
        let peer_display = upstream_stream
            .peer_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|_| format!("{}:{}", dest_host, dest_port));
        info!("Connected to {}, proxying data", peer_display);
    }

    // Proxy data between client and upstream
    let proxy_ctx = ProxyContext {
        session_manager: connect_ctx.session_manager.clone(),
        session_id,
        cancel_token,
        update_config: connect_ctx.traffic_config,
        qos_engine: session_ctx.qos_engine.clone(),
        user: Arc::clone(&session_ctx.user),
    };
    match proxy_data(client_stream, upstream_stream, proxy_ctx).await
    {
        Ok(reusable_stream) => {
            if let Some(reuse) = reusable_stream {
                connect_ctx
                    .connection_pool
                    .put(upstream_addr, reuse.stream, reuse.hint)
                    .await;
            } else {
                connect_ctx
                    .connection_pool
                    .release(upstream_addr, ReuseHint::Refresh)
                    .await;
            }
            connect_ctx
                .session_manager
                .close_session(
                    &session_id,
                    Some("Connection closed normally".to_string()),
                    SessionStatus::Closed,
                )
                .await;
            Ok(())
        }
        Err(RustSocksError::ConnectionClosed) => {
            connect_ctx
                .connection_pool
                .release(upstream_addr, ReuseHint::Refresh)
                .await;
            connect_ctx
                .session_manager
                .close_session(
                    &session_id,
                    Some("Connection closed by client".to_string()),
                    SessionStatus::Closed,
                )
                .await;
            debug!(session = %session_id, "Session closed by client");
            Ok(())
        }
        Err(e) => {
            let reason = format!("Proxy error: {}", e);
            connect_ctx
                .connection_pool
                .release(upstream_addr, ReuseHint::Refresh)
                .await;
            connect_ctx
                .session_manager
                .close_session(&session_id, Some(reason), SessionStatus::Failed)
                .await;
            Err(e)
        }
    }
}

#[instrument(
    level = "debug",
    skip(client_stream, session_manager, session_ctx, udp_ctx)
)]
async fn handle_udp_associate<S>(
    mut client_stream: S,
    _dest_addr: &Address,
    _dest_port: u16,
    session_manager: Arc<SessionManager>,
    session_ctx: SessionContext,
    udp_ctx: UdpRelayContext,
) -> Result<()>
where
    S: IoStream,
{
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
            session_ctx.user.as_ref(),
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
        udp_ctx,
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
    user: Arc<str>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.qos_engine.dec_user_connection_arc(&self.user);
    }
}

/// Context for handling ACL block responses (reduces code duplication)
struct AclBlockContext<'a> {
    user: &'a Arc<str>,
    dest_string: &'a str,
    port: u16,
    client_addr: std::net::SocketAddr,
    session_protocol: SessionProtocol,
    matched_rule: Option<String>,
}

/// Check connection limits and send error response if exceeded
/// Returns ConnectionGuard on success for RAII cleanup
async fn check_connection_limits<S>(
    stream: &mut S,
    protocol: SocksProtocol,
    ctx: &Arc<ClientHandlerContext>,
    user: &Arc<str>,
) -> Result<ConnectionGuard>
where
    S: IoStream,
{
    if let Err(e) = ctx
        .qos_engine
        .check_and_inc_connection_arc(user, &ctx.connection_limits)
    {
        warn!(
            user = %user.as_ref(),
            error = %e,
            "Connection limit exceeded"
        );
        send_socks_response(
            stream,
            protocol,
            ReplyCode::ConnectionNotAllowed,
            Address::IPv4([0, 0, 0, 0]),
            0,
        )
        .await?;
        return Err(e);
    }

    Ok(ConnectionGuard {
        qos_engine: ctx.qos_engine.clone(),
        user: Arc::clone(user),
    })
}

/// Handle ACL block response - extracted to reduce duplication between SOCKS4/5
async fn handle_acl_block<S>(
    stream: &mut S,
    protocol: SocksProtocol,
    ctx: &Arc<ClientHandlerContext>,
    block_ctx: AclBlockContext<'_>,
) -> Result<()>
where
    S: IoStream,
{
    ctx.acl_stats.record_block(block_ctx.user.as_ref());
    let rule = block_ctx.matched_rule.as_deref().unwrap_or("unknown rule");

    warn!(
        user = %block_ctx.user.as_ref(),
        dest = %block_ctx.dest_string,
        port = block_ctx.port,
        rule,
        "ACL blocked connection"
    );

    let conn_info = ConnectionInfo {
        source_ip: block_ctx.client_addr.ip(),
        source_port: block_ctx.client_addr.port(),
        dest_ip: block_ctx.dest_string.to_string(),
        dest_port: block_ctx.port,
        protocol: block_ctx.session_protocol,
    };
    ctx.session_manager
        .track_rejected_session(block_ctx.user.as_ref(), conn_info, block_ctx.matched_rule)
        .await;

    send_socks_response(
        stream,
        protocol,
        ReplyCode::ConnectionNotAllowed,
        Address::IPv4([0, 0, 0, 0]),
        0,
    )
    .await?;

    Ok(())
}
