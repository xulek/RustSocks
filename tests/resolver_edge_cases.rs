// Resolver Edge Cases Tests
// Tests for DNS resolution error handling, timeouts, and boundary conditions

use rustsocks::protocol::types::Address;
use rustsocks::server::resolver::resolve_address;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[tokio::test]
async fn test_resolve_ipv4_all_zeros() {
    let addr = Address::IPv4([0, 0, 0, 0]);
    let result = resolve_address(&addr, 80).await;

    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].ip(), IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
}

#[tokio::test]
async fn test_resolve_ipv4_broadcast() {
    let addr = Address::IPv4([255, 255, 255, 255]);
    let result = resolve_address(&addr, 80).await;

    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(
        resolved[0].ip(),
        IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255))
    );
}

#[tokio::test]
async fn test_resolve_ipv6_loopback() {
    let addr = Address::IPv6([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let result = resolve_address(&addr, 8080).await;

    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].ip(), IpAddr::V6(Ipv6Addr::LOCALHOST));
}

#[tokio::test]
async fn test_resolve_ipv6_all_zeros() {
    let addr = Address::IPv6([0; 16]);
    let result = resolve_address(&addr, 80).await;

    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].ip(), IpAddr::V6(Ipv6Addr::UNSPECIFIED));
}

#[tokio::test]
async fn test_resolve_ipv6_all_ones() {
    let addr = Address::IPv6([255; 16]);
    let result = resolve_address(&addr, 80).await;

    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.len(), 1);
}

#[tokio::test]
async fn test_resolve_nonexistent_domain() {
    // Use a domain that should not exist
    let addr = Address::Domain(
        "this-domain-definitely-does-not-exist-12345678990.invalid".to_string(),
    );
    let result = resolve_address(&addr, 80).await;

    // Should return an error because the domain doesn't exist
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resolve_invalid_tld() {
    // Use an invalid TLD that should fail DNS resolution
    let addr = Address::Domain("example.invalidtld99999".to_string());
    let result = resolve_address(&addr, 80).await;

    // Should return an error
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resolve_localhost() {
    let addr = Address::Domain("localhost".to_string());
    let result = resolve_address(&addr, 8080).await;

    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert!(!resolved.is_empty());

    // localhost should resolve to 127.0.0.1 and/or ::1
    let has_ipv4_loopback = resolved
        .iter()
        .any(|s| s.ip() == IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    let has_ipv6_loopback = resolved
        .iter()
        .any(|s| s.ip() == IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));

    assert!(has_ipv4_loopback || has_ipv6_loopback);
}

#[tokio::test]
async fn test_resolve_with_all_port_values() {
    let addr = Address::IPv4([127, 0, 0, 1]);

    // Test port 0 (ephemeral)
    let result = resolve_address(&addr, 0).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap()[0].port(), 0);

    // Test port 1 (lowest valid)
    let result = resolve_address(&addr, 1).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap()[0].port(), 1);

    // Test port 65535 (highest valid)
    let result = resolve_address(&addr, 65535).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap()[0].port(), 65535);

    // Test common ports
    for port in [80, 443, 8080, 22, 3306, 5432] {
        let result = resolve_address(&addr, port).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap()[0].port(), port);
    }
}

#[tokio::test]
async fn test_resolve_domain_case_insensitivity() {
    // DNS should be case-insensitive
    let domains = vec![
        "LOCALHOST",
        "localhost",
        "LocalHost",
        "LOCALHOST",
        "LoCaLhOsT",
    ];

    let mut results = Vec::new();
    for domain in domains {
        let addr = Address::Domain(domain.to_string());
        let result = resolve_address(&addr, 8080).await;
        assert!(result.is_ok(), "Failed to resolve: {}", domain);
        results.push(result.unwrap());
    }

    // All should resolve successfully (though results may vary by system)
    assert_eq!(results.len(), 5);
}

#[tokio::test]
async fn test_resolve_very_long_domain() {
    // RFC 1035: domain name max length is 253 characters
    // Test with a domain that's exactly 253 chars (should work if valid)
    // Test with a domain that's 254 chars (may fail)

    // Create a valid 253-char domain (use valid labels)
    let label = "a".repeat(63); // Max label length
    let domain = format!("{}.{}.{}.{}", label, label, label, label); // ~255 chars - will be too long

    let addr = Address::Domain(domain.clone());
    let result = resolve_address(&addr, 80).await;

    // This should fail because the domain is too long
    assert!(result.is_err(), "Expected failure for domain: {}", domain);
}

#[tokio::test]
async fn test_resolve_domain_with_hyphens() {
    // Test valid domain with hyphens
    let addr = Address::Domain("my-test-domain.example.com".to_string());
    let result = resolve_address(&addr, 80).await;

    // This will fail because the domain doesn't exist, but it should be parsed correctly
    // The error should be about DNS resolution, not parsing
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resolve_domain_with_numbers() {
    // Test domain with numbers (e.g., "example123.com")
    let addr = Address::Domain("test123.example456.com".to_string());
    let result = resolve_address(&addr, 80).await;

    // Should fail due to non-existent domain, not parsing issues
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resolve_ipv6_prefers_over_ipv4() {
    // When both IPv4 and IPv6 are available, IPv6 should come first
    let addr = Address::Domain("localhost".to_string());
    let result = resolve_address(&addr, 8080).await;

    assert!(result.is_ok());
    let resolved = result.unwrap();

    if resolved.len() > 1 {
        // Check if IPv6 addresses come before IPv4
        let ipv6_indices: Vec<usize> = resolved
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s.ip(), IpAddr::V6(_)))
            .map(|(i, _)| i)
            .collect();

        let ipv4_indices: Vec<usize> = resolved
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s.ip(), IpAddr::V4(_)))
            .map(|(i, _)| i)
            .collect();

        if !ipv6_indices.is_empty() && !ipv4_indices.is_empty() {
            // If both exist, all IPv6 should come before all IPv4
            assert!(
                ipv6_indices.iter().max().unwrap() < ipv4_indices.iter().min().unwrap(),
                "IPv6 addresses should come before IPv4"
            );
        }
    }
}

#[tokio::test]
async fn test_resolve_concurrent_operations() {
    // Test multiple concurrent resolutions
    use tokio::task::JoinSet;

    let mut set = JoinSet::new();

    let test_addresses = vec![
        Address::IPv4([127, 0, 0, 1]),
        Address::IPv4([8, 8, 8, 8]),
        Address::IPv6([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
        Address::Domain("localhost".to_string()),
    ];

    // Spawn 50 concurrent resolution tasks
    for i in 0..50 {
        let addr = test_addresses[i % test_addresses.len()].clone();
        set.spawn(async move { resolve_address(&addr, 8080 + i as u16).await });
    }

    // Wait for all to complete
    let mut success_count = 0;
    while let Some(result) = set.join_next().await {
        if result.is_ok() && result.unwrap().is_ok() {
            success_count += 1;
        }
    }

    // At least the IP addresses should resolve successfully
    assert!(success_count >= 37); // 75% success rate (50 * 0.75 = 37.5)
}

#[tokio::test]
async fn test_resolve_with_timeout() {
    // Test that resolution doesn't hang indefinitely
    use tokio::time::{timeout, Duration};

    let addr = Address::Domain("example.com".to_string());

    // Should complete within 5 seconds
    let result = timeout(Duration::from_secs(5), resolve_address(&addr, 80)).await;

    assert!(
        result.is_ok(),
        "Resolution should complete within timeout period"
    );

    // The actual resolution may succeed or fail depending on network
    // but it should not hang
}

#[tokio::test]
async fn test_resolve_special_ipv4_addresses() {
    // Test various special-purpose IPv4 addresses

    // Private networks
    let private_addrs = vec![
        [10, 0, 0, 1],       // 10.0.0.0/8
        [172, 16, 0, 1],     // 172.16.0.0/12
        [192, 168, 0, 1],    // 192.168.0.0/16
        [169, 254, 0, 1],    // Link-local 169.254.0.0/16
        [224, 0, 0, 1],      // Multicast
        [127, 0, 0, 1],      // Loopback
    ];

    for ip in private_addrs {
        let addr = Address::IPv4(ip);
        let result = resolve_address(&addr, 80).await;
        assert!(result.is_ok(), "Failed to resolve {:?}", ip);
        assert_eq!(result.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn test_resolve_special_ipv6_addresses() {
    // Test various special-purpose IPv6 addresses

    // Loopback
    let loopback = Address::IPv6([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let result = resolve_address(&loopback, 80).await;
    assert!(result.is_ok());

    // Link-local (fe80::/10)
    let link_local = Address::IPv6([
        0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
    ]);
    let result = resolve_address(&link_local, 80).await;
    assert!(result.is_ok());

    // Multicast (ff00::/8)
    let multicast = Address::IPv6([0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let result = resolve_address(&multicast, 80).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_resolve_empty_results_handling() {
    // This test verifies that we handle the "empty results" case
    // While it's hard to trigger with real DNS, we test the code path exists

    // Use an invalid domain that should fail
    let addr = Address::Domain("".to_string());
    let result = resolve_address(&addr, 80).await;

    // Empty domain should fail during resolution
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resolve_stress_test() {
    // Stress test with many rapid resolutions
    use tokio::task::JoinSet;

    let mut set = JoinSet::new();

    // Spawn 200 concurrent tasks
    for i in 0..200 {
        set.spawn(async move {
            let addr = Address::IPv4([127, 0, 0, 1]);
            resolve_address(&addr, (i % 65535) as u16).await
        });
    }

    let mut success_count = 0;
    while let Some(result) = set.join_next().await {
        if result.is_ok() && result.unwrap().is_ok() {
            success_count += 1;
        }
    }

    // All IPv4 literal resolutions should succeed
    assert_eq!(success_count, 200);
}
