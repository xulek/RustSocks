/// Integration tests for ACL Management API
///
/// These tests verify that the REST API endpoints for ACL management work correctly,
/// including CRUD operations for groups, users, and global settings.

use rustsocks::acl::types::{AclConfig, Action, GlobalAclConfig, GroupAcl};
use rustsocks::acl::{load_config, save_config};
use tempfile::TempDir;

// Helper to create test ACL config
fn create_test_config() -> AclConfig {
    AclConfig {
        global: GlobalAclConfig {
            default_policy: Action::Block,
        },
        groups: vec![GroupAcl {
            name: "developers".to_string(),
            rules: vec![],
        }],
        users: vec![],
    }
}

#[tokio::test]
async fn test_add_group_rule() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("acl.toml");

    // Create initial config
    let config = create_test_config();
    save_config(&config, &config_path).await.unwrap();

    // Verify we can add a rule
    let mut config = load_config(&config_path).await.unwrap();
    let rule = rustsocks::acl::types::AclRule {
        action: Action::Allow,
        description: "Test rule".to_string(),
        destinations: vec!["*.example.com".to_string()],
        ports: vec!["443".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 100,
    };

    rustsocks::acl::crud::add_group_rule(&mut config, "developers", rule.clone()).unwrap();

    // Verify rule was added
    assert_eq!(config.groups[0].rules.len(), 1);
    assert_eq!(config.groups[0].rules[0].destinations[0], "*.example.com");
}

#[tokio::test]
async fn test_update_group_rule() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("acl.toml");

    // Create initial config with a rule
    let mut config = create_test_config();
    let rule1 = rustsocks::acl::types::AclRule {
        action: Action::Allow,
        description: "Original rule".to_string(),
        destinations: vec!["*.example.com".to_string()],
        ports: vec!["443".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 100,
    };
    rustsocks::acl::crud::add_group_rule(&mut config, "developers", rule1).unwrap();
    save_config(&config, &config_path).await.unwrap();

    // Update the rule
    let mut config = load_config(&config_path).await.unwrap();
    let identifier = rustsocks::acl::crud::RuleIdentifier {
        destinations: vec!["*.example.com".to_string()],
        ports: Some(vec!["443".to_string()]),
    };

    let rule2 = rustsocks::acl::types::AclRule {
        action: Action::Block,
        description: "Updated rule".to_string(),
        destinations: vec!["*.example.com".to_string()],
        ports: vec!["443".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 500,
    };

    let old_rule = rustsocks::acl::crud::update_group_rule(
        &mut config,
        "developers",
        &identifier,
        rule2.clone(),
    )
    .unwrap();

    // Verify rule was updated
    assert_eq!(old_rule.action, Action::Allow);
    assert_eq!(old_rule.description, "Original rule");
    assert_eq!(config.groups[0].rules[0].action, Action::Block);
    assert_eq!(config.groups[0].rules[0].description, "Updated rule");
    assert_eq!(config.groups[0].rules[0].priority, 500);
}

#[tokio::test]
async fn test_delete_group_rule() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("acl.toml");

    // Create initial config with a rule
    let mut config = create_test_config();
    let rule = rustsocks::acl::types::AclRule {
        action: Action::Allow,
        description: "Test rule".to_string(),
        destinations: vec!["*.example.com".to_string()],
        ports: vec!["443".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 100,
    };
    rustsocks::acl::crud::add_group_rule(&mut config, "developers", rule).unwrap();
    save_config(&config, &config_path).await.unwrap();

    // Delete the rule
    let mut config = load_config(&config_path).await.unwrap();
    let identifier = rustsocks::acl::crud::RuleIdentifier {
        destinations: vec!["*.example.com".to_string()],
        ports: Some(vec!["443".to_string()]),
    };

    let deleted = rustsocks::acl::crud::delete_group_rule(&mut config, "developers", &identifier)
        .unwrap();

    // Verify rule was deleted
    assert_eq!(deleted.destinations[0], "*.example.com");
    assert_eq!(config.groups[0].rules.len(), 0);
}

#[tokio::test]
async fn test_add_duplicate_rule_fails() {
    let _temp_dir = TempDir::new().unwrap();

    let mut config = create_test_config();
    let rule = rustsocks::acl::types::AclRule {
        action: Action::Allow,
        description: "Test rule".to_string(),
        destinations: vec!["*.example.com".to_string()],
        ports: vec!["443".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 100,
    };

    // Add first time - should succeed
    assert!(rustsocks::acl::crud::add_group_rule(&mut config, "developers", rule.clone()).is_ok());

    // Add second time - should fail
    assert!(rustsocks::acl::crud::add_group_rule(&mut config, "developers", rule).is_err());
}

#[tokio::test]
async fn test_update_nonexistent_rule_fails() {
    let mut config = create_test_config();

    let identifier = rustsocks::acl::crud::RuleIdentifier {
        destinations: vec!["*.nonexistent.com".to_string()],
        ports: Some(vec!["443".to_string()]),
    };

    let new_rule = rustsocks::acl::types::AclRule {
        action: Action::Allow,
        description: "New rule".to_string(),
        destinations: vec!["*.nonexistent.com".to_string()],
        ports: vec!["443".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 100,
    };

    let result =
        rustsocks::acl::crud::update_group_rule(&mut config, "developers", &identifier, new_rule);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_search_rules() {
    let mut config = create_test_config();

    // Add multiple rules
    let rule1 = rustsocks::acl::types::AclRule {
        action: Action::Allow,
        description: "SSH to prod".to_string(),
        destinations: vec!["*.prod.com".to_string()],
        ports: vec!["22".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 100,
    };

    let rule2 = rustsocks::acl::types::AclRule {
        action: Action::Block,
        description: "HTTPS to prod".to_string(),
        destinations: vec!["*.prod.com".to_string()],
        ports: vec!["443".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 200,
    };

    rustsocks::acl::crud::add_group_rule(&mut config, "developers", rule1).unwrap();
    rustsocks::acl::crud::add_group_rule(&mut config, "developers", rule2).unwrap();

    // Search by destination
    let criteria = rustsocks::acl::crud::RuleSearchCriteria {
        destination: Some("prod.com".to_string()),
        port: None,
        action: None,
    };
    let results = rustsocks::acl::crud::search_rules(&config, &criteria);
    assert_eq!(results.len(), 2);

    // Search by action
    let criteria = rustsocks::acl::crud::RuleSearchCriteria {
        destination: None,
        port: None,
        action: Some("block".to_string()),
    };
    let results = rustsocks::acl::crud::search_rules(&config, &criteria);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].owner, "developers");
}

#[tokio::test]
async fn test_create_and_delete_group() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("acl.toml");

    let mut config = create_test_config();
    save_config(&config, &config_path).await.unwrap();

    // Add new group
    config.groups.push(GroupAcl {
        name: "admins".to_string(),
        rules: vec![],
    });

    assert_eq!(config.groups.len(), 2);
    assert_eq!(config.groups[1].name, "admins");

    // Delete group
    let deleted = rustsocks::acl::crud::delete_group(&mut config, "admins").unwrap();
    assert_eq!(deleted.name, "admins");
    assert_eq!(config.groups.len(), 1);
}

#[tokio::test]
async fn test_add_user_rule() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("acl.toml");

    let mut config = create_test_config();
    save_config(&config, &config_path).await.unwrap();

    // Add user rule
    let rule = rustsocks::acl::types::AclRule {
        action: Action::Block,
        description: "Alice blocked from admin".to_string(),
        destinations: vec!["admin.example.com".to_string()],
        ports: vec!["*".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 1000,
    };

    rustsocks::acl::crud::add_user_rule(&mut config, "alice", rule.clone()).unwrap();

    // Verify user and rule were added
    assert_eq!(config.users.len(), 1);
    assert_eq!(config.users[0].username, "alice");
    assert_eq!(config.users[0].rules.len(), 1);
    assert_eq!(config.users[0].rules[0].destinations[0], "admin.example.com");
}

#[tokio::test]
async fn test_persistence_atomic_write() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("acl.toml");

    // Write first config
    let config1 = create_test_config();
    save_config(&config1, &config_path).await.unwrap();
    assert!(config_path.exists());

    // Write second config (should create backup)
    let mut config2 = create_test_config();
    config2.global.default_policy = Action::Allow;
    save_config(&config2, &config_path).await.unwrap();

    // Load and verify
    let loaded = load_config(&config_path).await.unwrap();
    assert_eq!(loaded.global.default_policy, Action::Allow);

    // Backup should be cleaned up
    let backup_path = temp_dir.path().join("acl.toml.backup");
    assert!(!backup_path.exists());
}

#[tokio::test]
async fn test_rule_identifier_matching() {
    let rule = rustsocks::acl::types::AclRule {
        action: Action::Allow,
        description: "Test".to_string(),
        destinations: vec!["*.example.com".to_string()],
        ports: vec!["443".to_string()],
        protocols: vec![rustsocks::acl::Protocol::Tcp],
        priority: 100,
    };

    // Match with ports
    let id1 = rustsocks::acl::crud::RuleIdentifier {
        destinations: vec!["*.example.com".to_string()],
        ports: Some(vec!["443".to_string()]),
    };
    assert!(id1.matches(&rule));

    // Match without ports (destination only)
    let id2 = rustsocks::acl::crud::RuleIdentifier {
        destinations: vec!["*.example.com".to_string()],
        ports: None,
    };
    assert!(id2.matches(&rule));

    // No match - different destination
    let id3 = rustsocks::acl::crud::RuleIdentifier {
        destinations: vec!["*.different.com".to_string()],
        ports: Some(vec!["443".to_string()]),
    };
    assert!(!id3.matches(&rule));

    // No match - different port
    let id4 = rustsocks::acl::crud::RuleIdentifier {
        destinations: vec!["*.example.com".to_string()],
        ports: Some(vec!["80".to_string()]),
    };
    assert!(!id4.matches(&rule));
}
