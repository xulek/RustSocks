//! Comprehensive unit tests for ACL (Access Control List) subsystem
//!
//! This test suite covers:
//! - Matchers: IP, CIDR, Domain, Wildcard, Port matching
//! - Engine: Rule evaluation, priorities, group inheritance
//! - Loader: TOML parsing and validation
//! - Stats: Counters and per-user statistics
//! - Edge cases and security considerations

use rustsocks::acl::engine::AclEngine;
use rustsocks::acl::stats::AclStats;
use rustsocks::acl::types::{
    AclConfig, AclDecision, AclRule, Action, GlobalAclConfig, GroupAcl, Protocol, UserAcl,
};
use rustsocks::protocol::Address;
use std::sync::Arc;

// ============================================================================
// IP Matcher Tests
// ============================================================================

mod ip_matcher_tests {
    use super::*;

    #[tokio::test]
    async fn ipv4_exact_match() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow specific IPv4".to_string(),
            destinations: vec!["192.168.1.100".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match exact IP
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv4([192, 168, 1, 100]),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Should not match different IP
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv4([192, 168, 1, 101]),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block); // default policy
    }

    #[tokio::test]
    async fn ipv6_exact_match() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow specific IPv6".to_string(),
            destinations: vec!["2001:db8::1".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match exact IPv6
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv6([
                    0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
                ]),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Should not match different IPv6
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv6([
                    0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
                ]),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn ipv4_does_not_match_ipv6() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow IPv4".to_string(),
            destinations: vec!["192.168.1.1".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // IPv4 rule should not match IPv6 address
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv6([
                    0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
                ]),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }
}

// ============================================================================
// CIDR Matcher Tests
// ============================================================================

mod cidr_matcher_tests {
    use super::*;

    #[tokio::test]
    async fn ipv4_cidr_slash_8() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow 10.0.0.0/8".to_string(),
            destinations: vec!["10.0.0.0/8".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match all 10.x.x.x addresses
        let test_cases = vec![
            ([10, 0, 0, 1], true),
            ([10, 255, 255, 255], true),
            ([10, 123, 45, 67], true),
            ([11, 0, 0, 1], false),
            ([9, 255, 255, 255], false),
            ([192, 168, 1, 1], false),
        ];

        for (ip, should_match) in test_cases {
            let (decision, _) = engine
                .evaluate("alice", &Address::IPv4(ip), 80, &Protocol::Tcp)
                .await;
            assert_eq!(
                decision == AclDecision::Allow,
                should_match,
                "IP {:?} match failed",
                ip
            );
        }
    }

    #[tokio::test]
    async fn ipv4_cidr_slash_16() {
        let rule = AclRule {
            action: Action::Block,
            description: "Block 192.168.0.0/16".to_string(),
            destinations: vec!["192.168.0.0/16".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config_with_policy("alice", vec![rule], Action::Allow);
        let engine = AclEngine::new(config).unwrap();

        // Should block all 192.168.x.x addresses
        let test_cases = vec![
            ([192, 168, 0, 1], true),
            ([192, 168, 1, 1], true),
            ([192, 168, 255, 255], true),
            ([192, 169, 0, 1], false),
            ([192, 167, 255, 255], false),
            ([10, 0, 0, 1], false),
        ];

        for (ip, should_block) in test_cases {
            let (decision, _) = engine
                .evaluate("alice", &Address::IPv4(ip), 80, &Protocol::Tcp)
                .await;
            assert_eq!(
                decision == AclDecision::Block,
                should_block,
                "IP {:?} block failed",
                ip
            );
        }
    }

    #[tokio::test]
    async fn ipv4_cidr_slash_24() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow 172.16.50.0/24".to_string(),
            destinations: vec!["172.16.50.0/24".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match 172.16.50.0-255 only
        let test_cases = vec![
            ([172, 16, 50, 0], true),
            ([172, 16, 50, 128], true),
            ([172, 16, 50, 255], true),
            ([172, 16, 49, 255], false),
            ([172, 16, 51, 0], false),
            ([172, 17, 50, 1], false),
        ];

        for (ip, should_match) in test_cases {
            let (decision, _) = engine
                .evaluate("alice", &Address::IPv4(ip), 80, &Protocol::Tcp)
                .await;
            assert_eq!(
                decision == AclDecision::Allow,
                should_match,
                "IP {:?} match failed",
                ip
            );
        }
    }

    #[tokio::test]
    async fn ipv4_cidr_slash_32() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow 10.0.0.1/32".to_string(),
            destinations: vec!["10.0.0.1/32".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // /32 should match only one specific IP
        let (decision, _) = engine
            .evaluate("alice", &Address::IPv4([10, 0, 0, 1]), 80, &Protocol::Tcp)
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate("alice", &Address::IPv4([10, 0, 0, 2]), 80, &Protocol::Tcp)
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn ipv6_cidr_slash_32() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow 2001:db8::/32".to_string(),
            destinations: vec!["2001:db8::/32".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match 2001:db8::* range
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv6([
                    0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
                ]),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv6([
                    0x20, 0x01, 0x0d, 0xb8, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff,
                ]),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Should not match different prefix
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv6([
                    0x20, 0x01, 0x0d, 0xb9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
                ]),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn multiple_cidr_ranges() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow private ranges".to_string(),
            destinations: vec![
                "10.0.0.0/8".to_string(),
                "172.16.0.0/12".to_string(),
                "192.168.0.0/16".to_string(),
            ],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match all private ranges
        let test_cases = vec![
            ([10, 1, 2, 3], true),
            ([172, 16, 0, 1], true),
            ([172, 31, 255, 255], true),
            ([192, 168, 1, 1], true),
            ([8, 8, 8, 8], false),
            ([1, 1, 1, 1], false),
        ];

        for (ip, should_match) in test_cases {
            let (decision, _) = engine
                .evaluate("alice", &Address::IPv4(ip), 80, &Protocol::Tcp)
                .await;
            assert_eq!(
                decision == AclDecision::Allow,
                should_match,
                "IP {:?} match failed",
                ip
            );
        }
    }
}

// ============================================================================
// Domain Matcher Tests
// ============================================================================

mod domain_matcher_tests {
    use super::*;

    #[tokio::test]
    async fn exact_domain_match() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow example.com".to_string(),
            destinations: vec!["example.com".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match exact domain (case-insensitive)
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("EXAMPLE.COM".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Should not match subdomain
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("www.example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);

        // Should not match different domain
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("test.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn wildcard_subdomain_match() {
        let rule = AclRule {
            action: Action::Block,
            description: "Block *.malware.com".to_string(),
            destinations: vec!["*.malware.com".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config_with_policy("alice", vec![rule], Action::Allow);
        let engine = AclEngine::new(config).unwrap();

        // Should match subdomains
        let test_cases = vec![
            ("api.malware.com", true),
            ("www.malware.com", true),
            ("cdn.malware.com", true),
            ("a.malware.com", true),
            ("malware.com", false),          // No subdomain
            ("api.test.malware.com", false), // Too many levels
            ("notmalware.com", false),
            ("malware.com.evil.com", false),
        ];

        for (domain, should_block) in test_cases {
            let (decision, _) = engine
                .evaluate(
                    "alice",
                    &Address::Domain(domain.to_string()),
                    80,
                    &Protocol::Tcp,
                )
                .await;
            assert_eq!(
                decision == AclDecision::Block,
                should_block,
                "Domain {} block failed",
                domain
            );
        }
    }

    #[tokio::test]
    async fn wildcard_middle_segment() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow api.*.company.com".to_string(),
            destinations: vec!["api.*.company.com".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match pattern
        let test_cases = vec![
            ("api.dev.company.com", true),
            ("api.prod.company.com", true),
            ("api.test.company.com", true),
            ("api.company.com", false),          // Missing segment
            ("www.dev.company.com", false),      // Wrong prefix
            ("api.dev.test.company.com", false), // Too many segments
        ];

        for (domain, should_match) in test_cases {
            let (decision, _) = engine
                .evaluate(
                    "alice",
                    &Address::Domain(domain.to_string()),
                    80,
                    &Protocol::Tcp,
                )
                .await;
            assert_eq!(
                decision == AclDecision::Allow,
                should_match,
                "Domain {} match failed",
                domain
            );
        }
    }

    #[tokio::test]
    async fn multiple_wildcards_pattern() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow *.*.example.com".to_string(),
            destinations: vec!["*.*.example.com".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match two-level subdomains
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("api.v1.example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("cdn.prod.example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Should not match single-level subdomain
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("api.example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn domain_with_special_characters() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow domains with hyphens and numbers".to_string(),
            destinations: vec![
                "api-server.example.com".to_string(),
                "server123.test.com".to_string(),
                "my-app-v2.example.org".to_string(),
            ],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match domains with hyphens and numbers
        let test_cases = vec![
            ("api-server.example.com", true),
            ("API-SERVER.EXAMPLE.COM", true),
            ("server123.test.com", true),
            ("my-app-v2.example.org", true),
            ("apiserver.example.com", false),
            ("server124.test.com", false),
        ];

        for (domain, should_match) in test_cases {
            let (decision, _) = engine
                .evaluate(
                    "alice",
                    &Address::Domain(domain.to_string()),
                    80,
                    &Protocol::Tcp,
                )
                .await;
            assert_eq!(
                decision == AclDecision::Allow,
                should_match,
                "Domain {} match failed",
                domain
            );
        }
    }
}

// ============================================================================
// Port Matcher Tests
// ============================================================================

mod port_matcher_tests {
    use super::*;

    #[tokio::test]
    async fn single_port_match() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow HTTPS only".to_string(),
            destinations: vec!["*".to_string()], // Empty = match all
            ports: vec!["443".to_string()],
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn port_range_match() {
        let rule = AclRule {
            action: Action::Block,
            description: "Block high ports".to_string(),
            destinations: vec!["*".to_string()], // Empty = match all
            ports: vec!["49152-65535".to_string()],
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config_with_policy("alice", vec![rule], Action::Allow);
        let engine = AclEngine::new(config).unwrap();

        // Should block ports in range
        let test_cases = vec![
            (49152, true),
            (50000, true),
            (65535, true),
            (49151, false),
            (1024, false),
            (80, false),
        ];

        for (port, should_block) in test_cases {
            let (decision, _) = engine
                .evaluate(
                    "alice",
                    &Address::Domain("example.com".to_string()),
                    port,
                    &Protocol::Tcp,
                )
                .await;
            assert_eq!(
                decision == AclDecision::Block,
                should_block,
                "Port {} block failed",
                port
            );
        }
    }

    #[tokio::test]
    async fn multiple_ports_match() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow common web ports".to_string(),
            destinations: vec!["*".to_string()], // Empty = match all
            ports: vec!["80,443,8080,8443".to_string()],
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match specified ports
        let test_cases = vec![
            (80, true),
            (443, true),
            (8080, true),
            (8443, true),
            (8081, false),
            (22, false),
            (3000, false),
        ];

        for (port, should_match) in test_cases {
            let (decision, _) = engine
                .evaluate(
                    "alice",
                    &Address::Domain("example.com".to_string()),
                    port,
                    &Protocol::Tcp,
                )
                .await;
            assert_eq!(
                decision == AclDecision::Allow,
                should_match,
                "Port {} match failed",
                port
            );
        }
    }

    #[tokio::test]
    async fn any_port_match() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow all ports".to_string(),
            destinations: vec!["example.com".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match any port
        for port in [1, 80, 443, 1024, 8080, 65535] {
            let (decision, _) = engine
                .evaluate(
                    "alice",
                    &Address::Domain("example.com".to_string()),
                    port,
                    &Protocol::Tcp,
                )
                .await;
            assert_eq!(decision, AclDecision::Allow, "Port {} should match", port);
        }
    }

    #[tokio::test]
    async fn combined_port_rules() {
        let rules = vec![
            AclRule {
                action: Action::Block,
                description: "Block SSH".to_string(),
                destinations: vec!["*".to_string()], // Empty = match all
                ports: vec!["22".to_string()],
                protocols: vec![Protocol::Both],
                priority: 200,
            },
            AclRule {
                action: Action::Allow,
                description: "Allow web ports".to_string(),
                destinations: vec!["*".to_string()], // Empty = match all
                ports: vec!["80,443".to_string()],
                protocols: vec![Protocol::Both],
                priority: 100,
            },
        ];

        let config = create_test_config("alice", rules);
        let engine = AclEngine::new(config).unwrap();

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                22,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }
}

// ============================================================================
// Protocol Matcher Tests
// ============================================================================

mod protocol_matcher_tests {
    use super::*;

    #[tokio::test]
    async fn tcp_only_match() {
        let rule = AclRule {
            action: Action::Allow,
            description: "TCP only".to_string(),
            destinations: vec!["*".to_string()], // Empty = match all
            ports: vec!["*".to_string()],        // Empty = match all
            protocols: vec![Protocol::Tcp],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                53,
                &Protocol::Udp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn udp_only_match() {
        let rule = AclRule {
            action: Action::Allow,
            description: "UDP only".to_string(),
            destinations: vec!["*".to_string()], // Empty = match all
            ports: vec!["*".to_string()],        // Empty = match all
            protocols: vec![Protocol::Udp],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                53,
                &Protocol::Udp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn both_protocols_match() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Both protocols".to_string(),
            destinations: vec!["*".to_string()], // Empty = match all
            ports: vec!["*".to_string()],        // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                53,
                &Protocol::Udp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn star_protocol_matches_all() {
        // Test that "*" in TOML is parsed as Protocol::Both
        let rule = AclRule {
            action: Action::Allow,
            description: "Star protocol".to_string(),
            destinations: vec!["*".to_string()],
            ports: vec!["*".to_string()],
            protocols: vec![Protocol::Both], // "*" is alias for "both"
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match TCP
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Should match UDP
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                53,
                &Protocol::Udp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn empty_protocol_list_matches_nothing() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Empty protocols".to_string(),
            destinations: vec!["*".to_string()],
            ports: vec!["*".to_string()],
            protocols: vec![], // Empty = match nothing
            priority: 100,
        };

        let config = create_test_config_with_policy("alice", vec![rule], Action::Block);
        let engine = AclEngine::new(config).unwrap();

        // Should NOT match TCP (falls back to default Block)
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);

        // Should NOT match UDP (falls back to default Block)
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                53,
                &Protocol::Udp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn protocol_specific_rules() {
        let rules = vec![
            AclRule {
                action: Action::Block,
                description: "Block UDP DNS".to_string(),
                destinations: vec!["*".to_string()], // Empty = match all
                ports: vec!["53".to_string()],
                protocols: vec![Protocol::Udp],
                priority: 200,
            },
            AclRule {
                action: Action::Allow,
                description: "Allow all TCP".to_string(),
                destinations: vec!["*".to_string()], // Empty = match all
                ports: vec!["*".to_string()],        // Empty = match all
                protocols: vec![Protocol::Tcp],
                priority: 100,
            },
        ];

        let config = create_test_config("alice", rules);
        let engine = AclEngine::new(config).unwrap();

        // TCP DNS should be allowed
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("8.8.8.8".to_string()),
                53,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // UDP DNS should be blocked
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("8.8.8.8".to_string()),
                53,
                &Protocol::Udp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }
}

// ============================================================================
// ACL Engine Priority Tests
// ============================================================================

mod priority_tests {
    use super::*;

    #[tokio::test]
    async fn higher_priority_wins() {
        let rules = vec![
            AclRule {
                action: Action::Block,
                description: "High priority block".to_string(),
                destinations: vec!["evil.com".to_string()],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 1000,
            },
            AclRule {
                action: Action::Allow,
                description: "Low priority allow".to_string(),
                destinations: vec!["*.com".to_string()],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 100,
            },
        ];

        let config = create_test_config("alice", rules);
        let engine = AclEngine::new(config).unwrap();

        // Higher priority block should win
        let (decision, desc) = engine
            .evaluate(
                "alice",
                &Address::Domain("evil.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
        assert!(desc.unwrap().contains("High priority block"));
    }

    #[tokio::test]
    async fn block_action_takes_precedence_over_allow() {
        let rules = vec![
            AclRule {
                action: Action::Allow,
                description: "Allow all".to_string(),
                destinations: vec!["*".to_string()], // Empty = match all
                ports: vec!["*".to_string()],        // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 100,
            },
            AclRule {
                action: Action::Block,
                description: "Block specific".to_string(),
                destinations: vec!["blocked.com".to_string()],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 100,
            },
        ];

        let config = create_test_config("alice", rules);
        let engine = AclEngine::new(config).unwrap();

        // Block should be evaluated first (even with same priority)
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("blocked.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn first_matching_rule_wins() {
        // BLOCK rules are always evaluated first (security-first policy)
        // So even though ALLOW has higher priority number, BLOCK will win
        let rules = vec![
            AclRule {
                action: Action::Allow,
                description: "High priority allow".to_string(),
                destinations: vec!["example.com".to_string()],
                ports: vec!["80".to_string()],
                protocols: vec![Protocol::Tcp],
                priority: 200,
            },
            AclRule {
                action: Action::Block,
                description: "Lower priority block (but wins due to BLOCK-first)".to_string(),
                destinations: vec!["example.com".to_string()],
                ports: vec!["80".to_string()],
                protocols: vec![Protocol::Tcp],
                priority: 100,
            },
        ];

        let config = create_test_config("alice", rules);
        let engine = AclEngine::new(config).unwrap();

        let (decision, desc) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        // BLOCK wins because BLOCK rules are always checked first
        assert_eq!(decision, AclDecision::Block);
        assert!(desc.unwrap().contains("block"));
    }

    #[tokio::test]
    async fn priority_ordering_with_multiple_rules() {
        let rules = vec![
            AclRule {
                action: Action::Allow,
                description: "Priority 50".to_string(),
                destinations: vec!["low.example.com".to_string()],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 50,
            },
            AclRule {
                action: Action::Block,
                description: "Priority 500".to_string(),
                destinations: vec!["high.example.com".to_string()],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 500,
            },
            AclRule {
                action: Action::Allow,
                description: "Priority 100".to_string(),
                destinations: vec!["mid.example.com".to_string()],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 100,
            },
        ];

        let config = create_test_config("alice", rules);
        let engine = AclEngine::new(config).unwrap();

        // Block rule should be checked first (BLOCK > ALLOW)
        let (decision, desc) = engine
            .evaluate(
                "alice",
                &Address::Domain("high.example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
        assert!(desc.unwrap().contains("Priority 500"));
    }
}

// ============================================================================
// Group Inheritance Tests
// ============================================================================

mod group_inheritance_tests {
    use super::*;

    #[tokio::test]
    async fn user_inherits_group_rules() {
        let config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec!["developers".to_string()],
                rules: vec![],
            }],
            groups: vec![GroupAcl {
                name: "developers".to_string(),
                rules: vec![AclRule {
                    action: Action::Allow,
                    description: "Devs can access dev servers".to_string(),
                    destinations: vec!["*.dev.company.com".to_string()],
                    ports: vec!["*".to_string()], // Empty = match all
                    protocols: vec![Protocol::Both],
                    priority: 100,
                }],
            }],
        };

        let engine = AclEngine::new(config).unwrap();

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("api.dev.company.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn user_rules_combined_with_group_rules() {
        let config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec!["developers".to_string()],
                rules: vec![AclRule {
                    action: Action::Block,
                    description: "Alice blocks social media".to_string(),
                    destinations: vec!["*.facebook.com".to_string(), "*.twitter.com".to_string()],
                    ports: vec!["*".to_string()], // Empty = match all
                    protocols: vec![Protocol::Both],
                    priority: 500,
                }],
            }],
            groups: vec![GroupAcl {
                name: "developers".to_string(),
                rules: vec![AclRule {
                    action: Action::Allow,
                    description: "Allow all internet".to_string(),
                    destinations: vec!["*".to_string()], // Empty = match all
                    ports: vec!["*".to_string()],        // Empty = match all
                    protocols: vec![Protocol::Both],
                    priority: 100,
                }],
            }],
        };

        let engine = AclEngine::new(config).unwrap();

        // User's block rule should take precedence
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("www.facebook.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);

        // Group's allow rule should apply for other sites
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("github.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn multiple_group_membership() {
        let config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec!["developers".to_string(), "admins".to_string()],
                rules: vec![],
            }],
            groups: vec![
                GroupAcl {
                    name: "developers".to_string(),
                    rules: vec![AclRule {
                        action: Action::Allow,
                        description: "Dev access".to_string(),
                        destinations: vec!["*.dev.company.com".to_string()],
                        ports: vec!["*".to_string()], // Empty = match all
                        protocols: vec![Protocol::Both],
                        priority: 100,
                    }],
                },
                GroupAcl {
                    name: "admins".to_string(),
                    rules: vec![AclRule {
                        action: Action::Allow,
                        description: "Admin access".to_string(),
                        destinations: vec!["*.prod.company.com".to_string()],
                        ports: vec!["*".to_string()], // Empty = match all
                        protocols: vec![Protocol::Both],
                        priority: 100,
                    }],
                },
            ],
        };

        let engine = AclEngine::new(config).unwrap();

        // Should have access from developers group
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("api.dev.company.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Should have access from admins group
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("db.prod.company.com".to_string()),
                5432,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }
}

// ============================================================================
// Default Policy Tests
// ============================================================================

mod default_policy_tests {
    use super::*;

    #[tokio::test]
    async fn default_allow_policy() {
        let config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Allow,
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec![],
                rules: vec![],
            }],
            groups: vec![],
        };

        let engine = AclEngine::new(config).unwrap();

        // No rules, default should be allow
        let (decision, desc) = engine
            .evaluate(
                "alice",
                &Address::Domain("anything.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
        assert!(desc.unwrap().contains("Default policy"));
    }

    #[tokio::test]
    async fn default_block_policy() {
        let config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec![],
                rules: vec![],
            }],
            groups: vec![],
        };

        let engine = AclEngine::new(config).unwrap();

        // No rules, default should be block
        let (decision, desc) = engine
            .evaluate(
                "alice",
                &Address::Domain("anything.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
        assert!(desc.unwrap().contains("Default policy"));
    }

    #[tokio::test]
    async fn unknown_user_gets_default_policy() {
        let config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec![],
                rules: vec![AclRule {
                    action: Action::Allow,
                    description: "Alice can access".to_string(),
                    destinations: vec!["*.com".to_string()],
                    ports: vec!["*".to_string()], // Empty = match all
                    protocols: vec![Protocol::Both],
                    priority: 100,
                }],
            }],
            groups: vec![],
        };

        let engine = AclEngine::new(config).unwrap();

        // Unknown user should get default policy
        let (decision, desc) = engine
            .evaluate(
                "unknown_user",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
        assert!(desc.unwrap().contains("Default policy"));
    }

    #[tokio::test]
    async fn default_policy_when_no_rules_match() {
        let config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec![],
                rules: vec![AclRule {
                    action: Action::Allow,
                    description: "Only example.com".to_string(),
                    destinations: vec!["example.com".to_string()],
                    ports: vec!["443".to_string()],
                    protocols: vec![Protocol::Tcp],
                    priority: 100,
                }],
            }],
            groups: vec![],
        };

        let engine = AclEngine::new(config).unwrap();

        // Matching rule
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // No matching rule - wrong port
        let (decision, desc) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
        assert!(desc.unwrap().contains("Default policy"));

        // No matching rule - wrong domain
        let (decision, desc) = engine
            .evaluate(
                "alice",
                &Address::Domain("other.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
        assert!(desc.unwrap().contains("Default policy"));
    }
}

// ============================================================================
// ACL Stats Tests
// ============================================================================

mod acl_stats_tests {
    use super::*;

    #[test]
    fn stats_initialization() {
        let stats = AclStats::new();
        let snapshot = stats.snapshot();

        assert_eq!(snapshot.allowed, 0);
        assert_eq!(snapshot.blocked, 0);
    }

    #[test]
    fn record_allow_increments_counters() {
        let stats = AclStats::new();

        stats.record_allow("alice");
        stats.record_allow("alice");
        stats.record_allow("bob");

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.allowed, 3);
        assert_eq!(snapshot.blocked, 0);

        assert_eq!(stats.user_snapshot("alice").map(|s| s.allowed), Some(2));
        assert_eq!(stats.user_snapshot("alice").map(|s| s.blocked), Some(0));
        assert_eq!(stats.user_snapshot("bob").map(|s| s.allowed), Some(1));
    }

    #[test]
    fn record_block_increments_counters() {
        let stats = AclStats::new();

        stats.record_block("alice");
        stats.record_block("alice");
        stats.record_block("alice");
        stats.record_block("bob");

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.allowed, 0);
        assert_eq!(snapshot.blocked, 4);

        assert_eq!(stats.user_snapshot("alice").map(|s| s.blocked), Some(3));
        assert_eq!(stats.user_snapshot("bob").map(|s| s.blocked), Some(1));
    }

    #[test]
    fn mixed_allow_and_block() {
        let stats = AclStats::new();

        stats.record_allow("alice");
        stats.record_block("alice");
        stats.record_allow("alice");
        stats.record_block("bob");

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.allowed, 2);
        assert_eq!(snapshot.blocked, 2);

        let alice_stats = stats.user_snapshot("alice").unwrap();
        assert_eq!(alice_stats.allowed, 2);
        assert_eq!(alice_stats.blocked, 1);

        let bob_stats = stats.user_snapshot("bob").unwrap();
        assert_eq!(bob_stats.allowed, 0);
        assert_eq!(bob_stats.blocked, 1);
    }

    #[test]
    fn concurrent_stat_updates() {
        let stats = Arc::new(AclStats::new());
        let mut handles = vec![];

        for i in 0..10 {
            let stats_clone = stats.clone();
            let handle = std::thread::spawn(move || {
                for _ in 0..100 {
                    if i % 2 == 0 {
                        stats_clone.record_allow(format!("user{}", i));
                    } else {
                        stats_clone.record_block(format!("user{}", i));
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.allowed, 500); // 5 users * 100
        assert_eq!(snapshot.blocked, 500); // 5 users * 100
    }
}

// ============================================================================
// Complex Scenario Tests
// ============================================================================

mod complex_scenarios {
    use super::*;

    #[tokio::test]
    async fn corporate_network_policy() {
        let config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![
                UserAcl {
                    username: "developer".to_string(),
                    groups: vec!["engineering".to_string()],
                    rules: vec![AclRule {
                        action: Action::Block,
                        description: "Devs cannot access production DB".to_string(),
                        destinations: vec!["prod-db.company.com".to_string()],
                        ports: vec!["5432".to_string()],
                        protocols: vec![Protocol::Tcp],
                        priority: 1000,
                    }],
                },
                UserAcl {
                    username: "admin".to_string(),
                    groups: vec!["engineering".to_string(), "ops".to_string()],
                    rules: vec![],
                },
            ],
            groups: vec![
                GroupAcl {
                    name: "engineering".to_string(),
                    rules: vec![
                        AclRule {
                            action: Action::Allow,
                            description: "Access dev environment".to_string(),
                            destinations: vec!["*.dev.company.com".to_string()],
                            ports: vec!["*".to_string()], // Empty = match all
                            protocols: vec![Protocol::Both],
                            priority: 100,
                        },
                        AclRule {
                            action: Action::Allow,
                            description: "Access GitHub".to_string(),
                            destinations: vec![
                                "github.com".to_string(),
                                "*.github.com".to_string(),
                            ],
                            ports: vec!["443".to_string()],
                            protocols: vec![Protocol::Tcp],
                            priority: 100,
                        },
                    ],
                },
                GroupAcl {
                    name: "ops".to_string(),
                    rules: vec![AclRule {
                        action: Action::Allow,
                        description: "Full production access".to_string(),
                        destinations: vec![
                            "*.prod.company.com".to_string(),
                            "prod-db.company.com".to_string(), // Exact match for prod DB
                        ],
                        ports: vec!["*".to_string()], // Empty = match all
                        protocols: vec![Protocol::Both],
                        priority: 200,
                    }],
                },
            ],
        };

        let engine = AclEngine::new(config).unwrap();

        // Developer: Can access dev environment
        let (decision, _) = engine
            .evaluate(
                "developer",
                &Address::Domain("api.dev.company.com".to_string()),
                8080,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Developer: Cannot access production DB (blocked by user rule)
        let (decision, _) = engine
            .evaluate(
                "developer",
                &Address::Domain("prod-db.company.com".to_string()),
                5432,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);

        // Admin: Can access production DB (ops group)
        let (decision, _) = engine
            .evaluate(
                "admin",
                &Address::Domain("prod-db.company.com".to_string()),
                5432,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Both: Can access GitHub
        let (decision, _) = engine
            .evaluate(
                "developer",
                &Address::Domain("github.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn security_policy_with_exceptions() {
        let rules = vec![
            // Block all social media
            AclRule {
                action: Action::Block,
                description: "Block social media".to_string(),
                destinations: vec![
                    "*.facebook.com".to_string(),
                    "*.twitter.com".to_string(),
                    "*.instagram.com".to_string(),
                    "*.tiktok.com".to_string(),
                ],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 900,
            },
            // Block torrent ports
            AclRule {
                action: Action::Block,
                description: "Block torrents".to_string(),
                destinations: vec!["*".to_string()], // Empty = match all
                ports: vec!["6881-6889".to_string()],
                protocols: vec![Protocol::Both],
                priority: 800,
            },
            // Allow HTTPS to anywhere
            AclRule {
                action: Action::Allow,
                description: "Allow HTTPS".to_string(),
                destinations: vec!["*".to_string()], // Empty = match all
                ports: vec!["443".to_string()],
                protocols: vec![Protocol::Tcp],
                priority: 100,
            },
            // Allow HTTP
            AclRule {
                action: Action::Allow,
                description: "Allow HTTP".to_string(),
                destinations: vec!["*".to_string()], // Empty = match all
                ports: vec!["80".to_string()],
                protocols: vec![Protocol::Tcp],
                priority: 100,
            },
        ];

        let config = create_test_config("user", rules);
        let engine = AclEngine::new(config).unwrap();

        // Should block Facebook even on HTTPS (higher priority)
        let (decision, _) = engine
            .evaluate(
                "user",
                &Address::Domain("www.facebook.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);

        // Should block torrent ports
        let (decision, _) = engine
            .evaluate(
                "user",
                &Address::Domain("tracker.example.com".to_string()),
                6881,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);

        // Should allow regular HTTPS
        let (decision, _) = engine
            .evaluate(
                "user",
                &Address::Domain("google.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn geo_blocking_simulation() {
        let rules = vec![
            AclRule {
                action: Action::Block,
                description: "Block China IP ranges".to_string(),
                destinations: vec!["1.0.1.0/24".to_string(), "1.0.2.0/23".to_string()],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 500,
            },
            AclRule {
                action: Action::Block,
                description: "Block Russia IP ranges".to_string(),
                destinations: vec!["5.8.0.0/16".to_string()],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 500,
            },
            AclRule {
                action: Action::Allow,
                description: "Allow all other IPs".to_string(),
                destinations: vec!["*".to_string()], // Empty = match all
                ports: vec!["*".to_string()],        // Empty = match all
                protocols: vec![Protocol::Both],
                priority: 100,
            },
        ];

        let config = create_test_config("user", rules);
        let engine = AclEngine::new(config).unwrap();

        // Should block China IPs
        let (decision, _) = engine
            .evaluate("user", &Address::IPv4([1, 0, 1, 100]), 80, &Protocol::Tcp)
            .await;
        assert_eq!(decision, AclDecision::Block);

        // Should block Russia IPs
        let (decision, _) = engine
            .evaluate("user", &Address::IPv4([5, 8, 0, 1]), 80, &Protocol::Tcp)
            .await;
        assert_eq!(decision, AclDecision::Block);

        // Should allow other IPs
        let (decision, _) = engine
            .evaluate("user", &Address::IPv4([8, 8, 8, 8]), 80, &Protocol::Tcp)
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }
}

// ============================================================================
// Edge Cases and Security Tests
// ============================================================================

mod edge_cases {
    use super::*;

    #[tokio::test]
    async fn star_destination_matches_all() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Star matches all".to_string(),
            destinations: vec!["*".to_string()], // "*" = match all
            ports: vec!["443".to_string()],
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match any destination with port 443
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("anything.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate("alice", &Address::IPv4([1, 2, 3, 4]), 443, &Protocol::Tcp)
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn empty_destination_list_matches_nothing() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Empty destinations".to_string(),
            destinations: vec![], // Empty = match nothing
            ports: vec!["443".to_string()],
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config_with_policy("alice", vec![rule], Action::Block);
        let engine = AclEngine::new(config).unwrap();

        // Should NOT match anything (empty list = match nothing)
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("anything.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block); // Falls back to default policy

        let (decision, _) = engine
            .evaluate("alice", &Address::IPv4([1, 2, 3, 4]), 443, &Protocol::Tcp)
            .await;
        assert_eq!(decision, AclDecision::Block); // Falls back to default policy
    }

    #[tokio::test]
    async fn star_port_matches_all() {
        let rule = AclRule {
            action: Action::Block,
            description: "Star ports".to_string(),
            destinations: vec!["blocked.com".to_string()],
            ports: vec!["*".to_string()], // "*" = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should match any port
        for port in [80, 443, 8080, 65535] {
            let (decision, _) = engine
                .evaluate(
                    "alice",
                    &Address::Domain("blocked.com".to_string()),
                    port,
                    &Protocol::Tcp,
                )
                .await;
            assert_eq!(decision, AclDecision::Block, "Port {} should match", port);
        }
    }

    #[tokio::test]
    async fn empty_port_list_matches_nothing() {
        let rule = AclRule {
            action: Action::Block,
            description: "Empty ports".to_string(),
            destinations: vec!["blocked.com".to_string()],
            ports: vec![], // Empty = match nothing
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config_with_policy("alice", vec![rule], Action::Allow);
        let engine = AclEngine::new(config).unwrap();

        // Should NOT match any port (empty list = match nothing)
        for port in [80, 443, 8080, 65535] {
            let (decision, _) = engine
                .evaluate(
                    "alice",
                    &Address::Domain("blocked.com".to_string()),
                    port,
                    &Protocol::Tcp,
                )
                .await;
            assert_eq!(
                decision,
                AclDecision::Allow,
                "Port {} should not match, falls back to default Allow",
                port
            );
        }
    }

    #[tokio::test]
    async fn case_insensitive_domain_matching() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Domain".to_string(),
            destinations: vec!["ExAmPlE.cOm".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        let test_cases = vec!["example.com", "EXAMPLE.COM", "Example.Com", "eXaMpLe.CoM"];

        for domain in test_cases {
            let (decision, _) = engine
                .evaluate(
                    "alice",
                    &Address::Domain(domain.to_string()),
                    80,
                    &Protocol::Tcp,
                )
                .await;
            assert_eq!(
                decision,
                AclDecision::Allow,
                "Domain {} should match",
                domain
            );
        }
    }

    #[tokio::test]
    async fn wildcard_does_not_match_empty_segment() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Wildcard".to_string(),
            destinations: vec!["*.example.com".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Should NOT match the base domain without subdomain
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);

        // Should match with subdomain
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("www.example.com".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn port_zero_handling() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Any port".to_string(),
            destinations: vec!["example.com".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // Port 0 should match with wildcard
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                0,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn max_port_65535_handling() {
        let rule = AclRule {
            action: Action::Block,
            description: "Block max port".to_string(),
            destinations: vec!["*".to_string()], // Empty = match all
            ports: vec!["65535".to_string()],
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config_with_policy("alice", vec![rule], Action::Allow);
        let engine = AclEngine::new(config).unwrap();

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                65535,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                65534,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow); // Default allow
    }

    #[tokio::test]
    async fn ip_as_domain_string_matches_cidr() {
        let rule = AclRule {
            action: Action::Block,
            description: "Block private IPs".to_string(),
            destinations: vec!["192.168.0.0/16".to_string()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        // IP as domain string should match CIDR
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("192.168.1.1".to_string()),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn long_domain_name_handling() {
        let long_domain = format!("{}.com", "a".repeat(200));
        let rule = AclRule {
            action: Action::Allow,
            description: "Long domain".to_string(),
            destinations: vec![long_domain.clone()],
            ports: vec!["*".to_string()], // Empty = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config("alice", vec![rule]);
        let engine = AclEngine::new(config).unwrap();

        let (decision, _) = engine
            .evaluate("alice", &Address::Domain(long_domain), 80, &Protocol::Tcp)
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn many_rules_performance() {
        // Create 100 rules
        let mut rules = vec![];
        for i in 0..100 {
            rules.push(AclRule {
                action: if i % 2 == 0 {
                    Action::Allow
                } else {
                    Action::Block
                },
                description: format!("Rule {}", i),
                destinations: vec![format!("domain{}.com", i)],
                ports: vec!["*".to_string()], // Empty = match all
                protocols: vec![Protocol::Both],
                priority: i as u32,
            });
        }

        let config = create_test_config("alice", rules);
        let engine = AclEngine::new(config).unwrap();

        // Should still evaluate quickly
        use std::time::Instant;
        let start = Instant::now();

        for i in 0..100 {
            engine
                .evaluate(
                    "alice",
                    &Address::Domain(format!("domain{}.com", i)),
                    80,
                    &Protocol::Tcp,
                )
                .await;
        }

        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 100,
            "100 evaluations took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn empty_destinations_and_ports_allows_all_ips() {
        // Exact scenario from user: default_policy = Block, but Allow rule with ["*"] should work
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow all destinations and ports".to_string(),
            destinations: vec!["*".to_string()], // "*" = match all
            ports: vec!["*".to_string()],        // "*" = match all
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config_with_policy("alice", vec![rule], Action::Block);
        let engine = AclEngine::new(config).unwrap();

        // Test various IP addresses that should all be allowed
        // This is the exact scenario from user: 192.168.55.220:22
        let (decision, rule_desc) = engine
            .evaluate(
                "alice",
                &Address::IPv4([192, 168, 55, 220]),
                22,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(
            decision,
            AclDecision::Allow,
            "Star destinations should match IPv4 192.168.55.220:22, got {:?}",
            rule_desc
        );

        // Test more IPv4 addresses
        let (decision, _) = engine
            .evaluate("alice", &Address::IPv4([10, 0, 0, 1]), 80, &Protocol::Tcp)
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv4([172, 16, 0, 1]),
                443,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Test IPv6
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv6([
                    0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x01,
                ]),
                22,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Test domain
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::Domain("example.com".to_string()),
                8080,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Test UDP protocol
        let (decision, _) = engine
            .evaluate("alice", &Address::IPv4([8, 8, 8, 8]), 53, &Protocol::Udp)
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn toml_config_with_empty_destinations_works() {
        // Test that TOML parsing handles ["*"] correctly (match all)
        use rustsocks::acl::loader::load_acl_config;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let toml_content = r#"
[global]
default_policy = "block"

[[users]]
username = "alice"
groups = []

  [[users.rules]]
  action = "allow"
  description = "Allow all"
  destinations = ["*"]  # "*" = match all
  ports = ["*"]         # "*" = match all
  protocols = ["both"]
  priority = 100
"#;

        let mut temp_file = NamedTempFile::new().expect("create temp file");
        temp_file
            .write_all(toml_content.as_bytes())
            .expect("write TOML");
        temp_file.flush().expect("flush");

        let config = load_acl_config(temp_file.path())
            .await
            .expect("load ACL config from TOML");

        let engine = AclEngine::new(config).unwrap();

        // Test the exact scenario from user
        let (decision, rule_desc) = engine
            .evaluate(
                "alice",
                &Address::IPv4([192, 168, 55, 220]),
                22,
                &Protocol::Tcp,
            )
            .await;

        assert_eq!(
            decision,
            AclDecision::Allow,
            "TOML with [\"*\"] destinations should allow all IPs, got {:?}",
            rule_desc
        );
    }

    #[tokio::test]
    async fn toml_config_with_omitted_destinations_matches_nothing() {
        // Test that TOML parsing handles omitted destinations correctly (serde default = empty list = match nothing)
        use rustsocks::acl::loader::load_acl_config;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let toml_content = r#"
[global]
default_policy = "block"

[[users]]
username = "alice"
groups = []

  [[users.rules]]
  action = "allow"
  description = "Allow nothing (omitted fields)"
  # destinations field omitted - defaults to empty list = match nothing
  # ports field omitted - defaults to empty list = match nothing
  protocols = ["both"]
  priority = 100
"#;

        let mut temp_file = NamedTempFile::new().expect("create temp file");
        temp_file
            .write_all(toml_content.as_bytes())
            .expect("write TOML");
        temp_file.flush().expect("flush");

        let config = load_acl_config(temp_file.path())
            .await
            .expect("load ACL config from TOML");

        let engine = AclEngine::new(config).unwrap();

        // Test the exact scenario from user
        let (decision, rule_desc) = engine
            .evaluate(
                "alice",
                &Address::IPv4([192, 168, 55, 220]),
                22,
                &Protocol::Tcp,
            )
            .await;

        assert_eq!(
            decision,
            AclDecision::Block,
            "TOML with omitted destinations should match nothing (falls back to default Block), got {:?}",
            rule_desc
        );
    }

    #[tokio::test]
    async fn toml_config_with_star_protocol() {
        // Test that "*" in TOML is parsed correctly for protocols
        use rustsocks::acl::loader::load_acl_config;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let toml_content = r#"
[global]
default_policy = "block"

[[users]]
username = "alice"
groups = []

  [[users.rules]]
  action = "allow"
  description = "Allow all with star protocol"
  destinations = ["*"]
  ports = ["*"]
  protocols = ["*"]  # "*" = alias for "both"
  priority = 100
"#;

        let mut temp_file = NamedTempFile::new().expect("create temp file");
        temp_file
            .write_all(toml_content.as_bytes())
            .expect("write TOML");
        temp_file.flush().expect("flush");

        let config = load_acl_config(temp_file.path())
            .await
            .expect("load ACL config from TOML");

        let engine = AclEngine::new(config).unwrap();

        // Should match TCP
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv4([192, 168, 1, 1]),
                80,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        // Should match UDP
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv4([192, 168, 1, 1]),
                53,
                &Protocol::Udp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }

    #[tokio::test]
    async fn cidr_ranges_allow_all_ips() {
        // Compare: using CIDR ranges 0.0.0.0/0 and ::/0 should also work
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow all via CIDR".to_string(),
            destinations: vec!["0.0.0.0/0".to_string(), "::/0".to_string()],
            ports: vec!["*".to_string()],
            protocols: vec![Protocol::Both],
            priority: 100,
        };

        let config = create_test_config_with_policy("alice", vec![rule], Action::Block);
        let engine = AclEngine::new(config).unwrap();

        // Same tests as above
        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv4([192, 168, 55, 220]),
                22,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);

        let (decision, _) = engine
            .evaluate(
                "alice",
                &Address::IPv6([
                    0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x01,
                ]),
                22,
                &Protocol::Tcp,
            )
            .await;
        assert_eq!(decision, AclDecision::Allow);
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_test_config(username: &str, rules: Vec<AclRule>) -> AclConfig {
    create_test_config_with_policy(username, rules, Action::Block)
}

fn create_test_config_with_policy(
    username: &str,
    rules: Vec<AclRule>,
    default_policy: Action,
) -> AclConfig {
    AclConfig {
        global: GlobalAclConfig { default_policy },
        users: vec![UserAcl {
            username: username.to_string(),
            groups: vec![],
            rules,
        }],
        groups: vec![],
    }
}
