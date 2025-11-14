use crate::protocol::types::Address;
use crate::utils::error::{Result, RustSocksError};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tracing::instrument;

/// Resolve a SOCKS5 address into a list of socket addresses, preferring IPv6 entries first.
#[instrument(level = "debug", fields(port = port, address = ?address))]
pub async fn resolve_address(address: &Address, port: u16) -> Result<Vec<SocketAddr>> {
    let mut targets = match address {
        Address::IPv4(octets) => {
            let ip = IpAddr::V4(Ipv4Addr::from(*octets));
            vec![SocketAddr::new(ip, port)]
        }
        Address::IPv6(octets) => {
            let ip = IpAddr::V6(Ipv6Addr::from(*octets));
            vec![SocketAddr::new(ip, port)]
        }
        Address::Domain(domain) => {
            let lookup = tokio::net::lookup_host((domain.as_str(), port))
                .await
                .map_err(RustSocksError::Io)?;
            lookup.collect()
        }
    };

    // Prefer IPv6, then IPv4, while preserving order inside each category.
    targets.sort_by_key(|addr| match addr.ip() {
        IpAddr::V6(_) => 0,
        IpAddr::V4(_) => 1,
    });

    if targets.is_empty() {
        return Err(RustSocksError::Io(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "no addresses found for destination",
        )));
    }

    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_ipv4_literal() {
        let addr = Address::IPv4([127, 0, 0, 1]);
        let resolved = resolve_address(&addr, 8080).await.unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0], SocketAddr::from(([127, 0, 0, 1], 8080)));
    }

    #[tokio::test]
    async fn resolves_ipv6_literal() {
        let addr = Address::IPv6([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        let resolved = resolve_address(&addr, 8080).await.unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0],
            SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], 8080))
        );
    }

    #[tokio::test]
    async fn resolves_domain_prefers_ipv6() {
        let addr = Address::Domain("localhost".to_string());
        let resolved = resolve_address(&addr, 8080).await.unwrap();
        assert!(!resolved.is_empty());
        // first entry should be IPv6 when available
        if resolved
            .iter()
            .any(|socket| matches!(socket.ip(), IpAddr::V6(_)))
        {
            assert!(matches!(resolved[0].ip(), IpAddr::V6(_)));
        }
    }
}
