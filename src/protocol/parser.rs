use super::types::*;
use crate::utils::error::{Result, RustSocksError};
use bytes::Bytes;
use smallvec::SmallVec;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{debug, trace};

/// Parse client greeting (method selection) for SOCKS5.
/// The caller must provide the already-read version byte.
pub async fn parse_socks5_client_greeting<S>(stream: &mut S, version: u8) -> Result<ClientGreeting>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    if version != SOCKS_VERSION {
        return Err(RustSocksError::Protocol(format!(
            "Unsupported SOCKS version: 0x{:02x}",
            version
        )));
    }

    let nmethods = stream.read_u8().await?;

    if nmethods == 0 {
        return Err(RustSocksError::Protocol(
            "No authentication methods provided".to_string(),
        ));
    }

    // Read methods - use SmallVec for stack allocation (SOCKS5 typically has 1-3 methods)
    let mut methods_buf = SmallVec::<[u8; 8]>::from_elem(0, nmethods as usize);
    stream.read_exact(&mut methods_buf).await?;

    let methods: Vec<AuthMethod> = methods_buf.into_iter().map(AuthMethod::from).collect();

    trace!("Parsed client greeting: {} methods", methods.len());

    Ok(ClientGreeting { methods })
}

/// Send server choice
#[inline(always)]
pub async fn send_server_choice<S>(stream: &mut S, method: AuthMethod) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let buf = [SOCKS_VERSION, method as u8];
    stream.write_all(&buf).await?;
    stream.flush().await?;

    trace!("Sent server choice: {:?}", method);

    Ok(())
}

/// Parse username/password authentication (RFC 1929)
pub async fn parse_userpass_auth<S>(stream: &mut S) -> Result<(String, String)>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    // Read version
    let version = stream.read_u8().await?;

    if version != 0x01 {
        return Err(RustSocksError::Protocol(format!(
            "Unsupported userpass version: 0x{:02x}",
            version
        )));
    }

    // Read username - use SmallVec for stack allocation (most usernames < 64 bytes)
    let username_len = stream.read_u8().await? as usize;
    let mut username_buf = SmallVec::<[u8; 64]>::from_elem(0, username_len);
    stream.read_exact(&mut username_buf).await?;
    let username = String::from_utf8(username_buf.to_vec())
        .map_err(|_| RustSocksError::Protocol("Invalid username encoding".to_string()))?;

    // Read password - use SmallVec for stack allocation (most passwords < 64 bytes)
    let password_len = stream.read_u8().await? as usize;
    let mut password_buf = SmallVec::<[u8; 64]>::from_elem(0, password_len);
    stream.read_exact(&mut password_buf).await?;
    let password = String::from_utf8(password_buf.to_vec())
        .map_err(|_| RustSocksError::Protocol("Invalid password encoding".to_string()))?;

    trace!("Parsed userpass auth for user: {}", username);

    Ok((username, password))
}

/// Send authentication response
#[inline(always)]
pub async fn send_auth_response<S>(stream: &mut S, success: bool) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let status = if success { 0x00 } else { 0x01 };
    let buf = [0x01, status];
    stream.write_all(&buf).await?;
    stream.flush().await?;

    trace!(
        "Sent auth response: {}",
        if success { "success" } else { "failure" }
    );

    Ok(())
}

/// Parse SOCKS5 request
pub async fn parse_socks5_request<S>(stream: &mut S) -> Result<Socks5Request>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    // Read fixed part: version, command, reserved, address type
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await?;

    let version = buf[0];
    let command = buf[1];
    let reserved = buf[2];
    let address_type = buf[3];

    if version != SOCKS_VERSION {
        return Err(RustSocksError::Protocol(format!(
            "Unsupported SOCKS version: 0x{:02x}",
            version
        )));
    }

    // RFC 1928: Reserved field MUST be 0x00
    if reserved != 0x00 {
        trace!(
            "Non-zero reserved field in SOCKS5 request: 0x{:02x} (expected 0x00)",
            reserved
        );
    }

    let command = Command::try_from(command)?;

    // Parse address
    let address = match address_type {
        0x01 => {
            // IPv4
            let mut addr = [0u8; 4];
            stream.read_exact(&mut addr).await?;
            Address::IPv4(addr)
        }
        0x03 => {
            // Domain name - use SmallVec for stack allocation (most domains < 128 bytes)
            let domain_len = stream.read_u8().await? as usize;
            let mut domain_buf = SmallVec::<[u8; 128]>::from_elem(0, domain_len);
            stream.read_exact(&mut domain_buf).await?;
            let domain = String::from_utf8(domain_buf.to_vec())
                .map_err(|_| RustSocksError::Protocol("Invalid domain encoding".to_string()))?;
            Address::Domain(domain)
        }
        0x04 => {
            // IPv6
            let mut addr = [0u8; 16];
            stream.read_exact(&mut addr).await?;
            Address::IPv6(addr)
        }
        _ => {
            return Err(RustSocksError::UnsupportedAddressType(address_type));
        }
    };

    // Read port (big-endian)
    let port = stream.read_u16().await?;

    debug!(
        "Parsed SOCKS5 request: command={:?}, address={}, port={}",
        command,
        address.to_string(),
        port
    );

    Ok(Socks5Request {
        command,
        address,
        port,
    })
}

/// Send SOCKS5 response
#[inline(always)]
pub async fn send_socks5_response<S>(
    stream: &mut S,
    reply: ReplyCode,
    bind_addr: Address,
    bind_port: u16,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    // Write version, reply, reserved - use SmallVec for stack allocation (response < 256 bytes)
    let mut buf = SmallVec::<[u8; 256]>::new();
    buf.push(SOCKS_VERSION);
    buf.push(reply as u8);
    buf.push(0x00);

    // Write address type and address
    match &bind_addr {
        Address::IPv4(octets) => {
            buf.push(0x01);
            buf.extend_from_slice(octets);
        }
        Address::IPv6(octets) => {
            buf.push(0x04);
            buf.extend_from_slice(octets);
        }
        Address::Domain(domain) => {
            // RFC 1928: Domain name length is u8 (max 255 octets)
            if domain.len() > 255 {
                return Err(RustSocksError::Protocol(format!(
                    "Domain name too long: {} octets (max 255)",
                    domain.len()
                )));
            }
            buf.push(0x03);
            buf.push(domain.len() as u8);
            buf.extend_from_slice(domain.as_bytes());
        }
    }

    // Write port (big-endian)
    buf.extend_from_slice(&bind_port.to_be_bytes());

    stream.write_all(&buf).await?;
    stream.flush().await?;

    debug!(
        "Sent SOCKS5 response: reply={:?}, bind_addr={}, bind_port={}",
        reply,
        bind_addr.to_string(),
        bind_port
    );

    Ok(())
}

/// Parse SOCKS4/4a request (SOCKS version byte must be consumed by caller)
pub async fn parse_socks4_request<S>(stream: &mut S) -> Result<Socks4Request>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let command_byte = stream.read_u8().await?;
    let command = Command::try_from(command_byte)?;

    let port = stream.read_u16().await?;

    let mut ip_octets = [0u8; 4];
    stream.read_exact(&mut ip_octets).await?;

    let user_id = read_null_terminated_string(stream).await?;
    let user_id = if user_id.is_empty() {
        None
    } else {
        Some(user_id)
    };

    let address =
        if ip_octets[0] == 0 && ip_octets[1] == 0 && ip_octets[2] == 0 && ip_octets[3] != 0 {
            let domain = read_null_terminated_string(stream).await?;
            if domain.is_empty() {
                return Err(RustSocksError::Protocol(
                    "SOCKS4a domain name missing".to_string(),
                ));
            }
            Address::Domain(domain)
        } else {
            Address::IPv4(ip_octets)
        };

    debug!(
        "Parsed SOCKS4 request: command={:?}, address={}, port={}, user_id={:?}",
        command,
        address.to_string(),
        port,
        user_id
    );

    Ok(Socks4Request {
        command,
        address,
        port,
        user_id,
    })
}

/// Send SOCKS4 response
pub async fn send_socks4_response<S>(
    stream: &mut S,
    reply: Socks4Reply,
    bind_addr: [u8; 4],
    bind_port: u16,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut buf = Vec::with_capacity(8);
    buf.push(0x00);
    buf.push(reply as u8);
    buf.extend_from_slice(&bind_port.to_be_bytes());
    buf.extend_from_slice(&bind_addr);

    stream.write_all(&buf).await?;
    stream.flush().await?;

    debug!(
        "Sent SOCKS4 response: reply={:?}, bind_addr={}.{}.{}.{}, bind_port={}",
        reply, bind_addr[0], bind_addr[1], bind_addr[2], bind_addr[3], bind_port
    );

    Ok(())
}

async fn read_null_terminated_string<S>(stream: &mut S) -> Result<String>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    const MAX_LEN: usize = 255;
    let mut bytes = SmallVec::<[u8; 256]>::new();

    loop {
        let byte = stream.read_u8().await?;
        if byte == 0x00 {
            break;
        }

        if bytes.len() >= MAX_LEN {
            return Err(RustSocksError::Protocol(
                "SOCKS4 field exceeds maximum length".to_string(),
            ));
        }

        bytes.push(byte);
    }

    String::from_utf8(bytes.to_vec())
        .map_err(|_| RustSocksError::Protocol("Invalid string encoding".to_string()))
}

/// Parse UDP packet from raw bytes
/// Format: RSV(2) + FRAG(1) + ATYP(1) + DST.ADDR(var) + DST.PORT(2) + DATA
pub fn parse_udp_packet(buf: Bytes) -> Result<UdpPacket> {
    if buf.len() < 10 {
        return Err(RustSocksError::Protocol("UDP packet too short".to_string()));
    }

    let mut pos = 0;
    let len = buf.len();

    // Read and validate RSV (2 bytes) - RFC 1928: MUST be 0x0000
    let rsv = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
    if rsv != 0x0000 {
        trace!(
            "Non-zero reserved field in UDP packet: 0x{:04x} (expected 0x0000)",
            rsv
        );
    }
    pos += 2;

    // FRAG
    if len < pos + 1 {
        return Err(RustSocksError::Protocol(
            "Malformed UDP packet (missing frag byte)".to_string(),
        ));
    }
    let frag = buf[pos];
    pos += 1;

    // RFC 1928: "If an implementation does not support fragmentation, it MUST drop
    // any datagram whose FRAG field is other than X'00'."
    if frag != 0 {
        trace!(
            "Dropping UDP packet with FRAG={} (fragmentation not supported)",
            frag
        );
        return Err(RustSocksError::Protocol(
            "UDP fragmentation not supported - packet dropped per RFC 1928".to_string(),
        ));
    }

    // Address type
    if len < pos + 1 {
        return Err(RustSocksError::Protocol(
            "Malformed UDP packet (missing address type)".to_string(),
        ));
    }
    let address_type = buf[pos];
    pos += 1;

    // Parse address
    let address = match address_type {
        0x01 => {
            // IPv4
            if buf.len() < pos + 4 {
                return Err(RustSocksError::Protocol(
                    "Invalid IPv4 in UDP packet".to_string(),
                ));
            }
            let addr = [buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]];
            pos += 4;
            Address::IPv4(addr)
        }
        0x03 => {
            // Domain
            if buf.len() < pos + 1 {
                return Err(RustSocksError::Protocol(
                    "Invalid domain in UDP packet".to_string(),
                ));
            }
            let domain_len = buf[pos] as usize;
            pos += 1;
            if buf.len() < pos + domain_len {
                return Err(RustSocksError::Protocol(
                    "Invalid domain in UDP packet".to_string(),
                ));
            }
            let domain = String::from_utf8(buf[pos..pos + domain_len].to_vec()).map_err(|_| {
                RustSocksError::Protocol("Invalid domain encoding in UDP packet".to_string())
            })?;
            pos += domain_len;
            Address::Domain(domain)
        }
        0x04 => {
            // IPv6
            if buf.len() < pos + 16 {
                return Err(RustSocksError::Protocol(
                    "Invalid IPv6 in UDP packet".to_string(),
                ));
            }
            let mut addr = [0u8; 16];
            addr.copy_from_slice(&buf[pos..pos + 16]);
            pos += 16;
            Address::IPv6(addr)
        }
        _ => {
            return Err(RustSocksError::UnsupportedAddressType(address_type));
        }
    };

    // Port (big-endian)
    if len < pos + 2 {
        return Err(RustSocksError::Protocol(
            "Invalid port in UDP packet".to_string(),
        ));
    }
    let port = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
    pos += 2;

    // Data (rest of packet)
    if pos > len {
        return Err(RustSocksError::Protocol(
            "Malformed UDP packet (missing payload)".to_string(),
        ));
    }
    let data = buf.slice(pos..);

    Ok(UdpPacket {
        header: UdpHeader {
            frag,
            address,
            port,
        },
        data,
    })
}

/// Serialize UDP packet to bytes
/// Format: RSV(2) + FRAG(1) + ATYP(1) + DST.ADDR(var) + DST.PORT(2) + DATA
pub fn serialize_udp_packet(packet: &UdpPacket) -> Vec<u8> {
    // Pre-calculate capacity to avoid reallocations
    let header_size =
        4 + match &packet.header.address {
            Address::IPv4(_) => 4,
            Address::IPv6(_) => 16,
            Address::Domain(d) => 1 + d.len().min(255),
        } + 2; // port
    let mut buf = Vec::with_capacity(header_size + packet.data.len());

    // RSV (2 bytes)
    buf.extend_from_slice(&[0x00, 0x00]);

    // FRAG
    buf.push(packet.header.frag);

    // Address type and address
    match &packet.header.address {
        Address::IPv4(octets) => {
            buf.push(0x01);
            buf.extend_from_slice(octets);
        }
        Address::IPv6(octets) => {
            buf.push(0x04);
            buf.extend_from_slice(octets);
        }
        Address::Domain(domain) => {
            // RFC 1928: Domain name length is u8 (max 255 octets)
            // Note: This should not panic in normal operation as we validate on parse
            let domain_len = domain.len().min(255);
            buf.push(0x03);
            buf.push(domain_len as u8);
            buf.extend_from_slice(&domain.as_bytes()[..domain_len]);
        }
    }

    // Port (big-endian)
    buf.extend_from_slice(&packet.header.port.to_be_bytes());

    // Data
    buf.extend_from_slice(packet.data.as_ref());

    buf
}

/// Parse GSS-API message (RFC 1961)
/// Format: +------+------+------+.......................+
///         | ver  | mtyp | len  |       token           |
///         +------+------+------+.......................+
///         | 0x01 | 0x?? | 0x02 | up to 2^16-1 octets  |
pub async fn parse_gssapi_message<S>(stream: &mut S) -> Result<GssApiMessage>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    // Read version (1 byte)
    let version = stream.read_u8().await?;

    if version != 0x01 {
        return Err(RustSocksError::Protocol(format!(
            "Unsupported GSS-API message version: 0x{:02x}",
            version
        )));
    }

    // Read message type (1 byte)
    let mtyp = stream.read_u8().await?;
    let message_type = GssApiMessageType::from(mtyp);

    // Check for abort message (no length/token)
    if message_type == GssApiMessageType::Abort {
        trace!("Received GSS-API abort message");
        return Ok(GssApiMessage {
            version,
            message_type,
            token: Vec::new(),
        });
    }

    // Read token length (2 bytes, big-endian)
    let token_len = stream.read_u16().await? as usize;

    // Read token
    let mut token = vec![0u8; token_len];
    stream.read_exact(&mut token).await?;

    trace!(
        "Parsed GSS-API message: version={}, type={:?}, token_len={}",
        version,
        message_type,
        token_len
    );

    Ok(GssApiMessage {
        version,
        message_type,
        token,
    })
}

/// Send GSS-API message (RFC 1961)
pub async fn send_gssapi_message<S>(
    stream: &mut S,
    message_type: GssApiMessageType,
    token: &[u8],
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    // Pre-allocate for header (4 bytes) + token
    let mut buf = Vec::with_capacity(4 + token.len());

    // Version (1 byte)
    buf.push(0x01);

    // Message type (1 byte)
    buf.push(message_type as u8);

    // For abort messages, don't send length/token
    if message_type != GssApiMessageType::Abort {
        // Token length (2 bytes, big-endian)
        let token_len = token.len() as u16;
        buf.extend_from_slice(&token_len.to_be_bytes());

        // Token
        buf.extend_from_slice(token);
    }

    stream.write_all(&buf).await?;
    stream.flush().await?;

    trace!(
        "Sent GSS-API message: type={:?}, token_len={}",
        message_type,
        token.len()
    );

    Ok(())
}

/// Send GSS-API abort message (RFC 1961)
pub async fn send_gssapi_abort<S>(stream: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    send_gssapi_message(stream, GssApiMessageType::Abort, &[]).await
}

#[cfg(test)]
mod tests {
    use super::{parse_socks5_client_greeting, AuthMethod, SOCKS_VERSION};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn test_client_greeting_parsing() {
        // Simulate client greeting: version 5, 2 methods (no auth, userpass)
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut server_stream, _) = listener.accept().await.unwrap();
            let version = server_stream.read_u8().await.unwrap();
            assert_eq!(version, SOCKS_VERSION);
            parse_socks5_client_greeting(&mut server_stream, version)
                .await
                .unwrap()
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(&[0x05, 0x02, 0x00, 0x02]).await.unwrap();

        let greeting = server.await.unwrap();
        assert_eq!(
            greeting.methods,
            vec![AuthMethod::NoAuth, AuthMethod::UserPass]
        );
    }
}
