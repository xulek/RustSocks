/// Integration tests for LDAP Groups functionality
///
/// These tests verify that:
/// 1. ACL correctly filters LDAP groups (only uses groups defined in ACL config)
/// 2. Case-insensitive group matching works
/// 3. Per-user overrides work with LDAP groups
/// 4. Default policy applies when no groups match
///
/// Note: These tests use mock LDAP groups (simulated arrays of strings).
/// Real LDAP integration would require NSS/SSSD configuration.

use rustsocks::acl::types::{AclRule, GlobalAclConfig, GroupAcl};
use rustsocks::acl::{AclConfig, AclEngine, Action, Protocol};
use rustsocks::protocol::Address;

fn create_test_acl_config() -> AclConfig {
    // Create ACL config with groups: "developers" and "admins"
    // Note: We're NOT defining thousands of LDAP groups, only the ones we care about
    AclConfig {
        global: GlobalAclConfig {
            default_policy: Action::Block, // Block if no group matches
        },
        groups: vec![
            // Developers group - allow access to internal dev servers
            GroupAcl {
                name: "developers".to_string(),
                rules: vec![AclRule {
                    action: Action::Allow,
                    description: "Developers internal access".to_string(),
                    destinations: vec!["10.0.0.0/8".to_string()],
                    ports: vec!["*".to_string()],
                    protocols: vec![Protocol::Tcp],
                    priority: 100,
                }],
            },
            // Admins group - full access
            GroupAcl {
                name: "admins".to_string(),
                rules: vec![AclRule {
                    action: Action::Allow,
                    description: "Admins full access".to_string(),
                    destinations: vec!["*".to_string()],
                    ports: vec!["*".to_string()],
                    protocols: vec![Protocol::Tcp, Protocol::Udp],
                    priority: 200,
                }],
            },
        ],
        users: vec![], // No per-user configs initially
    }
}

#[tokio::test]
async fn test_ldap_groups_only_defined_groups_are_checked() {
    // User "alice" has many LDAP groups, but only "developers" is in ACL config
    let ldap_groups = vec![
        "alice".to_string(),
        "developers".to_string(), // ← This one is in ACL config
        "engineering".to_string(),
        "team_foo".to_string(),
        "random_ldap_group_123".to_string(),
        // ... potentially thousands more
    ];

    let acl_config = create_test_acl_config();
    let engine = AclEngine::new(acl_config).unwrap();

    // Try to connect to 10.1.2.3 (in developers' allowed range)
    let dest = Address::IPv4([10, 1, 2, 3]);
    let (decision, matched_rule) = engine
        .evaluate_with_groups("alice", &ldap_groups, &dest, 80, &Protocol::Tcp)
        .await;

    // Should ALLOW because "developers" group matches and allows 10.0.0.0/8
    assert_eq!(decision, rustsocks::acl::AclDecision::Allow);
    assert!(matched_rule.is_some());
    assert!(matched_rule
        .unwrap()
        .contains("Developers internal access"));
}

#[tokio::test]
async fn test_ldap_groups_no_matching_groups_uses_default_policy() {
    // User "bob" has LDAP groups, but NONE of them are in ACL config
    let ldap_groups = vec![
        "bob".to_string(),
        "hr".to_string(),
        "finance".to_string(),
        "random_team".to_string(),
    ];

    let acl_config = create_test_acl_config();
    let engine = AclEngine::new(acl_config).unwrap();

    // Try to connect to 10.1.2.3
    let dest = Address::IPv4([10, 1, 2, 3]);
    let (decision, matched_rule) = engine
        .evaluate_with_groups("bob", &ldap_groups, &dest, 80, &Protocol::Tcp)
        .await;

    // Should BLOCK because no groups match and default_policy = Block
    assert_eq!(decision, rustsocks::acl::AclDecision::Block);
    assert!(matched_rule.is_some());
    let rule_desc = matched_rule.unwrap();
    assert!(
        rule_desc.contains("Default policy") || rule_desc.contains("no matching groups")
    );
}

#[tokio::test]
async fn test_ldap_groups_case_insensitive_matching() {
    // LDAP returns "Developers" (capital D), ACL config has "developers" (lowercase)
    let ldap_groups = vec![
        "alice".to_string(),
        "Developers".to_string(), // ← Different case!
    ];

    let acl_config = create_test_acl_config();
    let engine = AclEngine::new(acl_config).unwrap();

    // Try to connect to 10.1.2.3
    let dest = Address::IPv4([10, 1, 2, 3]);
    let (decision, matched_rule) = engine
        .evaluate_with_groups("alice", &ldap_groups, &dest, 80, &Protocol::Tcp)
        .await;

    // Should ALLOW because case-insensitive matching: "Developers" = "developers"
    assert_eq!(decision, rustsocks::acl::AclDecision::Allow);
    assert!(matched_rule.is_some());
    assert!(matched_rule
        .unwrap()
        .contains("Developers internal access"));
}

#[tokio::test]
async fn test_ldap_groups_multiple_matching_groups() {
    // User "charlie" belongs to both "developers" and "admins"
    let ldap_groups = vec![
        "charlie".to_string(),
        "developers".to_string(), // Priority 100
        "admins".to_string(),     // Priority 200 (higher)
    ];

    let acl_config = create_test_acl_config();
    let engine = AclEngine::new(acl_config).unwrap();

    // Try to connect to 192.168.1.1 (NOT in developers range, but admins allow *)
    let dest = Address::IPv4([192, 168, 1, 1]);
    let (decision, matched_rule) = engine
        .evaluate_with_groups("charlie", &ldap_groups, &dest, 80, &Protocol::Tcp)
        .await;

    // Should ALLOW via "admins" group (higher priority)
    assert_eq!(decision, rustsocks::acl::AclDecision::Allow);
    assert!(matched_rule.is_some());
    assert!(matched_rule.unwrap().contains("Admins full access"));
}

#[tokio::test]
async fn test_ldap_groups_with_per_user_override() {
    use rustsocks::acl::types::UserAcl;

    // User "alice" has LDAP group "developers", but also per-user BLOCK rule
    let ldap_groups = vec!["alice".to_string(), "developers".to_string()];

    let mut acl_config = create_test_acl_config();
    acl_config.users = vec![UserAcl {
        username: "alice".to_string(),
        groups: vec![], // Groups come from LDAP, not config
        rules: vec![AclRule {
            action: Action::Block,
            description: "Alice blocked from 10.1.2.3".to_string(),
            destinations: vec!["10.1.2.3".to_string()], // Specific IP
            ports: vec!["*".to_string()],
            protocols: vec![Protocol::Tcp],
            priority: 1000, // Higher than group rules
        }],
    }];

    let engine = AclEngine::new(acl_config).unwrap();

    // Try to connect to 10.1.2.3
    let dest = Address::IPv4([10, 1, 2, 3]);
    let (decision, matched_rule) = engine
        .evaluate_with_groups("alice", &ldap_groups, &dest, 80, &Protocol::Tcp)
        .await;

    // Should BLOCK because per-user rule has higher priority
    assert_eq!(decision, rustsocks::acl::AclDecision::Block);
    assert!(matched_rule.is_some());
    assert!(matched_rule.unwrap().contains("Alice blocked"));

    // Try to connect to 10.1.2.4 (different IP, not blocked)
    let dest2 = Address::IPv4([10, 1, 2, 4]);
    let (decision2, matched_rule2) = engine
        .evaluate_with_groups("alice", &ldap_groups, &dest2, 80, &Protocol::Tcp)
        .await;

    // Should ALLOW via "developers" group rule
    assert_eq!(decision2, rustsocks::acl::AclDecision::Allow);
    assert!(matched_rule2.is_some());
    assert!(matched_rule2
        .unwrap()
        .contains("Developers internal access"));
}

#[tokio::test]
async fn test_ldap_groups_empty_groups_list() {
    // User has empty groups list (no LDAP groups)
    let ldap_groups = vec![];

    let acl_config = create_test_acl_config();
    let engine = AclEngine::new(acl_config).unwrap();

    // Try to connect
    let dest = Address::IPv4([10, 1, 2, 3]);
    let (decision, matched_rule) = engine
        .evaluate_with_groups("alice", &ldap_groups, &dest, 80, &Protocol::Tcp)
        .await;

    // Should BLOCK (default policy, no groups)
    assert_eq!(decision, rustsocks::acl::AclDecision::Block);
    assert!(matched_rule.is_some());
}

#[tokio::test]
async fn test_ldap_groups_mixed_case_variations() {
    // Test various case combinations
    let test_cases = vec![
        ("developers", vec!["DEVELOPERS"]),
        ("developers", vec!["Developers"]),
        ("developers", vec!["developers"]),
        ("developers", vec!["dEvElOpErS"]),
        ("admins", vec!["ADMINS"]),
    ];

    for (acl_group, ldap_groups_input) in test_cases {
        let ldap_groups: Vec<String> = ldap_groups_input
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let acl_config = create_test_acl_config();
        let engine = AclEngine::new(acl_config).unwrap();

        let dest = if acl_group == "admins" {
            Address::IPv4([192, 168, 1, 1]) // admins allow *
        } else {
            Address::IPv4([10, 1, 2, 3]) // developers allow 10.0.0.0/8
        };

        let (decision, _) = engine
            .evaluate_with_groups("testuser", &ldap_groups, &dest, 80, &Protocol::Tcp)
            .await;

        assert_eq!(
            decision,
            rustsocks::acl::AclDecision::Allow,
            "Failed for ACL group '{}' with LDAP groups {:?}",
            acl_group,
            ldap_groups
        );
    }
}
