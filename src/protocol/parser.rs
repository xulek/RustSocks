use super::types::*;
use crate::utils::error::{Result, RustSocksError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, trace};

/// Parse client greeting (method selection)
pub async fn parse_client_greeting(stream: &mut TcpStream) -> Result<ClientGreeting> {
    // Read version and number of methods
    let mut buf = [0u8; 2];
    stream.read_exact(&mut buf).await?;

    let version = buf[0];
    let nmethods = buf[1];

    if version != SOCKS_VERSION {
        return Err(RustSocksError::Protocol(format!(
            "Unsupported SOCKS version: 0x{:02x}",
            version
        )));
    }

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
pub async fn send_server_choice(stream: &mut TcpStream, method: AuthMethod) -> Result<()> {
    let buf = [SOCKS_VERSION, method as u8];
    stream.write_all(&buf).await?;
    stream.flush().await?;

    trace!("Sent server choice: {:?}", method);

    Ok(())
}

/// Parse username/password authentication (RFC 1929)
pub async fn parse_userpass_auth(stream: &mut TcpStream) -> Result<(String, String)> {
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
pub async fn send_auth_response(stream: &mut TcpStream, success: bool) -> Result<()> {
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
pub async fn parse_socks5_request(stream: &mut TcpStream) -> Result<Socks5Request> {
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
pub async fn send_socks5_response(
    stream: &mut TcpStream,
    reply: ReplyCode,
    bind_addr: Address,
    bind_port: u16,
) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_greeting_parsing() {
        // Simulate client greeting: version 5, 2 methods (no auth, userpass)
        let data = vec![0x05, 0x02, 0x00, 0x02];

        // This test would need a mock TcpStream - simplified for now
        // In practice, use tokio-test or similar for testing async IO
    }
}
