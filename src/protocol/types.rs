use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

/// SOCKS5 Version
pub const SOCKS_VERSION: u8 = 0x05;

/// Authentication methods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AuthMethod {
    NoAuth = 0x00,
    Gssapi = 0x01,
    UserPass = 0x02,
    NoAcceptable = 0xFF,
}

impl From<u8> for AuthMethod {
    fn from(value: u8) -> Self {
        match value {
            0x00 => AuthMethod::NoAuth,
            0x01 => AuthMethod::Gssapi,
            0x02 => AuthMethod::UserPass,
            _ => AuthMethod::NoAcceptable,
        }
    }
}

/// SOCKS5 commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    Connect = 0x01,
    Bind = 0x02,
    UdpAssociate = 0x03,
}

impl TryFrom<u8> for Command {
    type Error = crate::utils::error::RustSocksError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Command::Connect),
            0x02 => Ok(Command::Bind),
            0x03 => Ok(Command::UdpAssociate),
            _ => Err(crate::utils::error::RustSocksError::UnsupportedCommand(
                value,
            )),
        }
    }
}

/// Address types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    IPv4([u8; 4]),
    IPv6([u8; 16]),
    Domain(String),
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Address::IPv4(octets) => write!(f, "{}", Ipv4Addr::from(*octets)),
            Address::IPv6(octets) => write!(f, "{}", Ipv6Addr::from(*octets)),
            Address::Domain(domain) => write!(f, "{}", domain),
        }
    }
}

/// SOCKS5 reply codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReplyCode {
    Succeeded = 0x00,
    GeneralFailure = 0x01,
    ConnectionNotAllowed = 0x02,
    NetworkUnreachable = 0x03,
    HostUnreachable = 0x04,
    ConnectionRefused = 0x05,
    TtlExpired = 0x06,
    CommandNotSupported = 0x07,
    AddressTypeNotSupported = 0x08,
}

/// Client greeting message
#[derive(Debug)]
pub struct ClientGreeting {
    pub methods: Vec<AuthMethod>,
}

/// Server choice message
#[derive(Debug)]
pub struct ServerChoice {
    pub method: AuthMethod,
}

/// SOCKS5 request
#[derive(Debug)]
pub struct Socks5Request {
    pub command: Command,
    pub address: Address,
    pub port: u16,
}

/// SOCKS5 response
#[derive(Debug)]
pub struct Socks5Response {
    pub reply: ReplyCode,
    pub address: Address,
    pub port: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_method_conversion() {
        assert_eq!(AuthMethod::from(0x00), AuthMethod::NoAuth);
        assert_eq!(AuthMethod::from(0x02), AuthMethod::UserPass);
        assert_eq!(AuthMethod::from(0xFF), AuthMethod::NoAcceptable);
    }

    #[test]
    fn test_command_conversion() {
        assert_eq!(Command::try_from(0x01).unwrap(), Command::Connect);
        assert_eq!(Command::try_from(0x02).unwrap(), Command::Bind);
        assert_eq!(Command::try_from(0x03).unwrap(), Command::UdpAssociate);
        assert!(Command::try_from(0x04).is_err());
    }

    #[test]
    fn test_address_to_string() {
        let ipv4 = Address::IPv4([192, 168, 1, 1]);
        assert_eq!(ipv4.to_string(), "192.168.1.1");

        let domain = Address::Domain("example.com".to_string());
        assert_eq!(domain.to_string(), "example.com");
    }
}
