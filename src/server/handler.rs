use crate::acl::{AclDecision, AclEngine, AclStats, Protocol};
use crate::auth::AuthManager;
use crate::protocol::*;
use crate::server::proxy::proxy_data;
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

    // Step 3: SOCKS5 request
    let request = parse_socks5_request(&mut client_stream).await?;

    info!(
        "SOCKS5 request: command={:?}, dest={}:{}",
        request.command,
        request.address.to_string(),
        request.port
    );

    // Step 3b: ACL enforcement (if enabled)
    if let Some(engine) = acl_engine.as_ref() {
        let acl_user = user
            .as_deref()
            .unwrap_or_else(|| anonymous_user.as_ref().as_str());
        let protocol = match request.command {
            Command::UdpAssociate => Protocol::Udp,
            _ => Protocol::Tcp,
        };

        let (decision, matched_rule) = engine
            .evaluate(acl_user, &request.address, request.port, &protocol)
            .await;

        match decision {
            AclDecision::Block => {
                acl_stats.record_block(acl_user);
                let rule = matched_rule.as_deref().unwrap_or("unknown rule");

                warn!(
                    user = acl_user,
                    dest = %request.address.to_string(),
                    port = request.port,
                    rule,
                    "ACL blocked connection"
                );

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
                acl_stats.record_allow(acl_user);
                if let Some(rule) = matched_rule {
                    debug!(
                        user = acl_user,
                        dest = %request.address.to_string(),
                        port = request.port,
                        rule,
                        "ACL allowed connection"
                    );
                } else {
                    debug!(
                        user = acl_user,
                        dest = %request.address.to_string(),
                        port = request.port,
                        "ACL allowed connection (default policy)"
                    );
                }
            }
        }
    }

    // Step 4: Handle command
    match request.command {
        Command::Connect => {
            handle_connect(client_stream, &request.address, request.port).await?;
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
) -> Result<()> {
    // Connect to upstream
    let dest = match dest_addr {
        Address::IPv4(octets) => {
            let ip = std::net::Ipv4Addr::from(*octets);
            format!("{}:{}", ip, dest_port)
        }
        Address::IPv6(octets) => {
            let ip = std::net::Ipv6Addr::from(*octets);
            format!("[{}]:{}", ip, dest_port)
        }
        Address::Domain(domain) => format!("{}:{}", domain, dest_port),
    };

    debug!("Connecting to upstream: {}", dest);

    let upstream_stream = match TcpStream::connect(&dest).await {
        Ok(stream) => stream,
        Err(e) => {
            warn!("Failed to connect to {}: {}", dest, e);
            send_socks5_response(
                &mut client_stream,
                ReplyCode::ConnectionRefused,
                Address::IPv4([0, 0, 0, 0]),
                0,
            )
            .await?;
            return Err(e.into());
        }
    };

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

    info!("Connected to {}, proxying data", dest);

    // Proxy data between client and upstream
    proxy_data(client_stream, upstream_stream).await?;

    Ok(())
}
