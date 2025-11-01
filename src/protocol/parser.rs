use super::types::*;
use crate::utils::error::{Result, RustSocksError};
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

    // Read methods
    let mut methods_buf = vec![0u8; nmethods as usize];
    stream.read_exact(&mut methods_buf).await?;

    let methods: Vec<AuthMethod> = methods_buf.into_iter().map(AuthMethod::from).collect();

    trace!("Parsed client greeting: {} methods", methods.len());

    Ok(ClientGreeting { methods })
}

/// Send server choice
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

    // Read username
    let username_len = stream.read_u8().await? as usize;
    let mut username_buf = vec![0u8; username_len];
    stream.read_exact(&mut username_buf).await?;
    let username = String::from_utf8(username_buf)
        .map_err(|_| RustSocksError::Protocol("Invalid username encoding".to_string()))?;

    // Read password
    let password_len = stream.read_u8().await? as usize;
    let mut password_buf = vec![0u8; password_len];
    stream.read_exact(&mut password_buf).await?;
    let password = String::from_utf8(password_buf)
        .map_err(|_| RustSocksError::Protocol("Invalid password encoding".to_string()))?;

    trace!("Parsed userpass auth for user: {}", username);

    Ok((username, password))
}

/// Send authentication response
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
    let _reserved = buf[2];
    let address_type = buf[3];

    if version != SOCKS_VERSION {
        return Err(RustSocksError::Protocol(format!(
            "Unsupported SOCKS version: 0x{:02x}",
            version
        )));
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
            // Domain name
            let domain_len = stream.read_u8().await? as usize;
            let mut domain_buf = vec![0u8; domain_len];
            stream.read_exact(&mut domain_buf).await?;
            let domain = String::from_utf8(domain_buf)
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
pub async fn send_socks5_response<S>(
    stream: &mut S,
    reply: ReplyCode,
    bind_addr: Address,
    bind_port: u16,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    // Write version, reply, reserved
    let mut buf = vec![SOCKS_VERSION, reply as u8, 0x00];

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
    let mut bytes = Vec::new();

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

    String::from_utf8(bytes)
        .map_err(|_| RustSocksError::Protocol("Invalid string encoding".to_string()))
}

/// Parse UDP packet from raw bytes
/// Format: RSV(2) + FRAG(1) + ATYP(1) + DST.ADDR(var) + DST.PORT(2) + DATA
pub fn parse_udp_packet(buf: &[u8]) -> Result<UdpPacket> {
    if buf.len() < 10 {
        return Err(RustSocksError::Protocol("UDP packet too short".to_string()));
    }

    let mut pos = 0;

    // Skip RSV (2 bytes)
    pos += 2;

    // FRAG
    let frag = buf[pos];
    pos += 1;

    // Check if fragmentation is used (we don't support it)
    if frag != 0 {
        return Err(RustSocksError::Protocol(
            "UDP fragmentation not supported".to_string(),
        ));
    }

    // Address type
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
    if buf.len() < pos + 2 {
        return Err(RustSocksError::Protocol(
            "Invalid port in UDP packet".to_string(),
        ));
    }
    let port = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
    pos += 2;

    // Data (rest of packet)
    let data = buf[pos..].to_vec();

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
    let mut buf = Vec::new();

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
            buf.push(0x03);
            buf.push(domain.len() as u8);
            buf.extend_from_slice(domain.as_bytes());
        }
    }

    // Port (big-endian)
    buf.extend_from_slice(&packet.header.port.to_be_bytes());

    // Data
    buf.extend_from_slice(&packet.data);

    buf
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
