use crate::protocol::types::Address;
use crate::utils::error::{Result, RustSocksError};
use dashmap::DashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::instrument;

#[derive(Clone)]
struct ResolverRuntimeConfig {
    timeout: Duration,
    cache_ttl: Duration,
    cache_max_entries: usize,
    lookup_limit: std::sync::Arc<Semaphore>,
}

impl Default for ResolverRuntimeConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            cache_ttl: Duration::from_secs(30),
            cache_max_entries: 4_096,
            lookup_limit: std::sync::Arc::new(Semaphore::new(128)),
        }
    }
}

fn resolver_config() -> &'static RwLock<ResolverRuntimeConfig> {
    static CONFIG: OnceLock<RwLock<ResolverRuntimeConfig>> = OnceLock::new();
    CONFIG.get_or_init(|| RwLock::new(ResolverRuntimeConfig::default()))
}

pub fn configure_resolver(settings: &crate::config::ResolverSettings) {
    let new_config = ResolverRuntimeConfig {
        timeout: Duration::from_millis(settings.timeout_ms),
        cache_ttl: Duration::from_secs(settings.cache_ttl_secs),
        cache_max_entries: settings.cache_max_entries,
        lookup_limit: std::sync::Arc::new(Semaphore::new(settings.max_concurrent_lookups)),
    };
    *resolver_config()
        .write()
        .expect("resolver configuration lock poisoned") = new_config;
    dns_cache().clear();
}

fn current_config() -> ResolverRuntimeConfig {
    resolver_config()
        .read()
        .expect("resolver configuration lock poisoned")
        .clone()
}

#[derive(Clone)]
struct CacheEntry {
    ips: Vec<IpAddr>,
    inserted_at: Instant,
}

fn dns_cache() -> &'static DashMap<String, CacheEntry> {
    static CACHE: OnceLock<DashMap<String, CacheEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn cached_ips(domain: &str, cache_ttl: Duration) -> Option<Vec<IpAddr>> {
    let key = domain.to_ascii_lowercase();
    let cache = dns_cache();
    if let Some(entry) = cache.get(&key) {
        if entry.value().inserted_at.elapsed() <= cache_ttl {
            return Some(entry.ips.clone());
        }
        drop(entry);
        cache.remove(&key);
    }
    None
}

fn cache_ips(domain: &str, ips: Vec<IpAddr>, cache_ttl: Duration, cache_max_entries: usize) {
    if ips.is_empty() {
        return;
    }

    let cache = dns_cache();
    if cache.len() >= cache_max_entries {
        let expired: Vec<String> = cache
            .iter()
            .filter(|entry| entry.value().inserted_at.elapsed() > cache_ttl)
            .map(|entry| entry.key().clone())
            .collect();
        for key in expired {
            cache.remove(&key);
        }
    }

    if cache.len() >= cache_max_entries {
        if let Some(oldest_key) = cache
            .iter()
            .min_by_key(|entry| entry.value().inserted_at)
            .map(|entry| entry.key().clone())
        {
            cache.remove(&oldest_key);
        }
    }

    cache.insert(
        domain.to_ascii_lowercase(),
        CacheEntry {
            ips,
            inserted_at: Instant::now(),
        },
    );
}

/// Resolve a SOCKS address into a list of socket addresses.
///
/// Domain lookups have a hard timeout and a small bounded positive cache. The
/// returned SocketAddr values are the exact addresses callers should authorize
/// and connect to, which avoids a second DNS lookup/TOCTOU window.
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
            let runtime = current_config();
            let ips = if let Some(ips) = cached_ips(domain, runtime.cache_ttl) {
                ips
            } else {
                let permit = timeout(
                    runtime.timeout,
                    runtime.lookup_limit.clone().acquire_owned(),
                )
                .await
                .map_err(|_| {
                    RustSocksError::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "DNS resolver concurrency limit timed out",
                    ))
                })?
                .map_err(|_| {
                    RustSocksError::Io(std::io::Error::other("DNS resolver semaphore closed"))
                })?;

                let lookup = timeout(
                    runtime.timeout,
                    tokio::net::lookup_host((domain.as_str(), port)),
                )
                .await
                .map_err(|_| {
                    RustSocksError::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "DNS resolution timed out",
                    ))
                })?
                .map_err(RustSocksError::Io)?;
                drop(permit);

                let mut ips: Vec<IpAddr> = lookup.map(|addr| addr.ip()).collect();
                ips.sort_unstable();
                ips.dedup();
                cache_ips(
                    domain,
                    ips.clone(),
                    runtime.cache_ttl,
                    runtime.cache_max_entries,
                );
                ips
            };
            ips.into_iter()
                .map(|ip| SocketAddr::new(ip, port))
                .collect()
        }
    };

    // Prefer IPv6, then IPv4, while preserving order inside each category.
    if targets.len() > 1 {
        targets.sort_by_key(|addr| match addr.ip() {
            IpAddr::V6(_) => 0,
            IpAddr::V4(_) => 1,
        });
    }

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
        if resolved
            .iter()
            .any(|socket| matches!(socket.ip(), IpAddr::V6(_)))
        {
            assert!(matches!(resolved[0].ip(), IpAddr::V6(_)));
        }
    }
}
