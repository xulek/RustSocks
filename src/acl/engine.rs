use super::matcher::CompiledAclRule;
use super::types::{AclConfig, AclDecision, Action, GlobalAclConfig, Protocol};
use crate::protocol::Address;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// ACL Engine - evaluates ACL rules for connections
pub struct AclEngine {
    config: Arc<RwLock<CompiledAclConfig>>,
}

/// Compiled ACL configuration for efficient evaluation
#[derive(Debug, Clone)]
struct CompiledAclConfig {
    global: GlobalAclConfig,
    users: std::collections::HashMap<String, CompiledUserAcl>,
    groups: std::collections::HashMap<String, CompiledGroupAcl>,
}

#[derive(Debug, Clone)]
struct CompiledUserAcl {
    #[allow(dead_code)]
    username: String,
    groups: Vec<String>,
    rules: Vec<CompiledAclRule>,
}

#[derive(Debug, Clone)]
struct CompiledGroupAcl {
    #[allow(dead_code)]
    name: String,
    rules: Vec<CompiledAclRule>,
}

impl AclEngine {
    /// Create a new ACL engine from configuration
    pub fn new(config: AclConfig) -> Result<Self, String> {
        let compiled = Self::compile_config(&config)?;

        Ok(Self {
            config: Arc::new(RwLock::new(compiled)),
        })
    }

    /// Compile ACL configuration for efficient evaluation
    fn compile_config(config: &AclConfig) -> Result<CompiledAclConfig, String> {
        let mut users = std::collections::HashMap::new();
        let mut groups = std::collections::HashMap::new();

        // Compile user rules
        for user_acl in &config.users {
            let compiled_rules: Result<Vec<_>, _> = user_acl
                .rules
                .iter()
                .map(CompiledAclRule::compile)
                .collect();

            users.insert(
                user_acl.username.clone(),
                CompiledUserAcl {
                    username: user_acl.username.clone(),
                    groups: user_acl.groups.clone(),
                    rules: compiled_rules?,
                },
            );
        }

        // Compile group rules
        for group_acl in &config.groups {
            let compiled_rules: Result<Vec<_>, _> = group_acl
                .rules
                .iter()
                .map(CompiledAclRule::compile)
                .collect();

            groups.insert(
                group_acl.name.clone(),
                CompiledGroupAcl {
                    name: group_acl.name.clone(),
                    rules: compiled_rules?,
                },
            );
        }

        Ok(CompiledAclConfig {
            global: config.global.clone(),
            users,
            groups,
        })
    }

    /// Evaluate ACL for a connection attempt (legacy method using static groups from config)
    /// Returns (Decision, matched_rule_description)
    pub async fn evaluate(
        &self,
        user: &str,
        dest: &Address,
        port: u16,
        protocol: &Protocol,
    ) -> (AclDecision, Option<String>) {
        let config = self.config.read().await;

        // Collect all rules for this user (user rules + group rules)
        let all_rules = self.collect_rules(&config, user);
        debug!(
            user = user,
            rule_count = all_rules.len(),
            "Collected ACL rules for evaluation"
        );

        if all_rules.is_empty() {
            debug!(
                user = user,
                dest = ?dest,
                port = port,
                "No ACL rules for user, applying default policy"
            );
            return (
                AclDecision::from(&config.global.default_policy),
                Some("Default policy".to_string()),
            );
        }

        // Evaluate rules in priority order (BLOCK rules first)
        for rule in &all_rules {
            if rule.matches(dest, port, protocol) {
                let decision = AclDecision::from(&rule.action);

                debug!(
                    user = user,
                    dest = ?dest,
                    port = port,
                    decision = ?decision,
                    rule = rule.description,
                    priority = rule.priority,
                    "ACL rule matched"
                );

                return (decision, Some(rule.description.clone()));
            }
        }

        // No rule matched - apply default policy
        let decision = AclDecision::from(&config.global.default_policy);

        debug!(
            user = user,
            dest = ?dest,
            port = port,
            decision = ?decision,
            "No ACL rule matched, applying default policy"
        );

        (decision, Some("Default policy".to_string()))
    }

    /// Evaluate ACL with dynamic groups from LDAP (via NSS/SSSD)
    ///
    /// This method:
    /// 1. Takes user's groups from LDAP/system (all groups, potentially thousands)
    /// 2. Filters ONLY groups defined in ACL config (case-insensitive matching)
    /// 3. Ignores groups not in ACL config (no need to define all LDAP groups)
    /// 4. Optionally adds per-user rules from [[users]] section
    /// 5. Evaluates rules in priority order (BLOCK first)
    ///
    /// Example:
    /// - User "alice" has LDAP groups: ["alice", "developers", "engineering", "hr", ...]
    /// - ACL config defines: [[groups]] name = "developers"
    /// - This method uses ONLY "developers" rules, ignores all other groups
    ///
    /// Returns (Decision, matched_rule_description)
    pub async fn evaluate_with_groups(
        &self,
        user: &str,
        user_groups: &[String],
        dest: &Address,
        port: u16,
        protocol: &Protocol,
    ) -> (AclDecision, Option<String>) {
        let config = self.config.read().await;

        // Collect rules from user's LDAP groups (only those defined in ACL config)
        let all_rules = self.collect_rules_from_groups(&config, user, user_groups);

        debug!(
            user = user,
            ldap_groups = ?user_groups,
            matched_groups = ?self.get_matched_groups(&config, user_groups),
            rule_count = all_rules.len(),
            "Collected ACL rules from LDAP groups for evaluation"
        );

        if all_rules.is_empty() {
            debug!(
                user = user,
                dest = ?dest,
                port = port,
                ldap_groups = ?user_groups,
                "No ACL rules matched for user groups, applying default policy"
            );
            return (
                AclDecision::from(&config.global.default_policy),
                Some("Default policy (no matching groups)".to_string()),
            );
        }

        // Evaluate rules in priority order (BLOCK rules first)
        for rule in &all_rules {
            if rule.matches(dest, port, protocol) {
                let decision = AclDecision::from(&rule.action);

                debug!(
                    user = user,
                    dest = ?dest,
                    port = port,
                    decision = ?decision,
                    rule = rule.description,
                    priority = rule.priority,
                    "ACL rule matched from LDAP groups"
                );

                return (decision, Some(rule.description.clone()));
            }
        }

        // No rule matched - apply default policy
        let decision = AclDecision::from(&config.global.default_policy);

        debug!(
            user = user,
            dest = ?dest,
            port = port,
            decision = ?decision,
            "No ACL rule matched, applying default policy"
        );

        (decision, Some("Default policy".to_string()))
    }

    /// Collect all rules for a user (user rules + group rules), sorted by priority
    fn collect_rules(&self, config: &CompiledAclConfig, user: &str) -> Vec<CompiledAclRule> {
        let mut all_rules = Vec::new();

        // Get user's rules
        if let Some(user_acl) = config.users.get(user) {
            all_rules.extend(user_acl.rules.clone());

            // Add rules from user's groups
            for group_name in &user_acl.groups {
                if let Some(group_acl) = config.groups.get(group_name) {
                    all_rules.extend(group_acl.rules.clone());
                }
            }
        }

        // Sort by priority
        // BLOCK rules have implicit higher priority (we sort by action first, then priority)
        all_rules.sort_by(|a, b| {
            match (&a.action, &b.action) {
                (Action::Block, Action::Allow) => std::cmp::Ordering::Less, // BLOCK first
                (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
                _ => b.priority.cmp(&a.priority), // Higher priority first
            }
        });

        debug!(
            user = user,
            rules = ?all_rules
                .iter()
                .map(|r| {
                    (
                        &r.action,
                        &r.description,
                        r.priority,
                        &r.destinations,
                        &r.ports,
                        &r.protocols,
                    )
                })
                .collect::<Vec<_>>(),
            "ACL rule order for user"
        );

        all_rules
    }

    /// Collect rules from LDAP groups (case-insensitive matching)
    ///
    /// This method:
    /// - Iterates through user's LDAP groups
    /// - For each LDAP group, checks if it exists in ACL config (case-insensitive)
    /// - If match found, adds that group's rules
    /// - Also adds per-user rules from [[users]] section if present
    ///
    /// Case-insensitive example:
    /// - LDAP group: "Developers"
    /// - ACL config: [[groups]] name = "developers"
    /// - Result: MATCH (case-insensitive)
    fn collect_rules_from_groups(
        &self,
        config: &CompiledAclConfig,
        user: &str,
        user_groups: &[String],
    ) -> Vec<CompiledAclRule> {
        let mut all_rules = Vec::new();

        // Add per-user rules first (highest priority)
        if let Some(user_acl) = config.users.get(user) {
            all_rules.extend(user_acl.rules.clone());
        }

        // Iterate through user's LDAP groups
        for ldap_group in user_groups {
            // Case-insensitive search for matching group in ACL config
            for (acl_group_name, group_acl) in &config.groups {
                if ldap_group.eq_ignore_ascii_case(acl_group_name) {
                    debug!(
                        ldap_group = ldap_group,
                        acl_group = acl_group_name,
                        rule_count = group_acl.rules.len(),
                        "Matched LDAP group to ACL group (case-insensitive)"
                    );
                    all_rules.extend(group_acl.rules.clone());
                    break; // Found match, move to next LDAP group
                }
            }
        }

        // Sort by priority: BLOCK rules first, then by priority value (higher first)
        all_rules.sort_by(|a, b| {
            match (&a.action, &b.action) {
                (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
                (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
                // Same action - sort by priority descending
                _ => b.priority.cmp(&a.priority),
            }
        });

        all_rules
    }

    /// Get list of LDAP groups that matched ACL groups (for debugging)
    fn get_matched_groups(
        &self,
        config: &CompiledAclConfig,
        user_groups: &[String],
    ) -> Vec<String> {
        let mut matched = Vec::new();

        for ldap_group in user_groups {
            for acl_group_name in config.groups.keys() {
                if ldap_group.eq_ignore_ascii_case(acl_group_name) {
                    matched.push(ldap_group.clone());
                    break;
                }
            }
        }

        matched
    }

    /// Hot reload ACL configuration
    pub async fn reload(&self, new_config: AclConfig) -> Result<(), String> {
        // Validate config
        new_config.validate()?;

        // Compile new config
        let compiled = Self::compile_config(&new_config)?;

        // Atomic swap
        let mut config = self.config.write().await;
        *config = compiled;

        info!("ACL configuration reloaded successfully");

        Ok(())
    }

    /// Get current config (for inspection)
    pub async fn get_user_count(&self) -> usize {
        let config = self.config.read().await;
        config.users.len()
    }

    /// Get current config (for inspection)
    pub async fn get_group_count(&self) -> usize {
        let config = self.config.read().await;
        config.groups.len()
    }
}

impl AclConfig {
    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        // Check for duplicate users
        let mut seen_users = std::collections::HashSet::new();
        for user in &self.users {
            if !seen_users.insert(&user.username) {
                return Err(format!("Duplicate user: {}", user.username));
            }
        }

        // Check for duplicate groups
        let mut seen_groups = std::collections::HashSet::new();
        for group in &self.groups {
            if !seen_groups.insert(&group.name) {
                return Err(format!("Duplicate group: {}", group.name));
            }
        }

        // Check that user groups exist
        for user in &self.users {
            for group_name in &user.groups {
                if !self.groups.iter().any(|g| &g.name == group_name) {
                    return Err(format!(
                        "User '{}' references non-existent group '{}'",
                        user.username, group_name
                    ));
                }
            }
        }

        // Validate that rules have at least one matcher
        for user in &self.users {
            for rule in &user.rules {
                if rule.destinations.is_empty() && rule.ports.is_empty() {
                    warn!(
                        "User '{}' has rule '{}' with no matchers (will match all)",
                        user.username, rule.description
                    );
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::types::{AclRule, GroupAcl, UserAcl};

    fn create_test_config() -> AclConfig {
        AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec!["developers".to_string()],
                rules: vec![
                    AclRule {
                        action: Action::Allow,
                        description: "Allow HTTPS".to_string(),
                        destinations: vec!["0.0.0.0/0".to_string()],
                        ports: vec!["443".to_string()],
                        protocols: vec![Protocol::Tcp],
                        priority: 100,
                    },
                    AclRule {
                        action: Action::Block,
                        description: "Block admin panel".to_string(),
                        destinations: vec!["admin.example.com".to_string()],
                        ports: vec!["*".to_string()],
                        protocols: vec![Protocol::Both],
                        priority: 1000,
                    },
                ],
            }],
            groups: vec![GroupAcl {
                name: "developers".to_string(),
                rules: vec![AclRule {
                    action: Action::Allow,
                    description: "Dev servers".to_string(),
                    destinations: vec!["*.dev.example.com".to_string()],
                    ports: vec!["*".to_string()],
                    protocols: vec![Protocol::Both],
                    priority: 50,
                }],
            }],
        }
    }

    #[tokio::test]
    async fn test_block_priority() {
        let engine = AclEngine::new(create_test_config()).unwrap();

        // BLOCK rule should win even though ALLOW also matches
        let (decision, rule) = engine
            .evaluate(
                "alice",
                &Address::Domain("admin.example.com".to_string()),
                443,
                &Protocol::Tcp,
            )
            .await;

        assert_eq!(decision, AclDecision::Block);
        assert_eq!(rule.unwrap(), "Block admin panel");
    }

    #[tokio::test]
    async fn test_allow_rule() {
        let engine = AclEngine::new(create_test_config()).unwrap();

        // Should match ALLOW rule for HTTPS
        let (decision, rule) = engine
            .evaluate(
                "alice",
                &Address::IPv4([93, 184, 216, 34]),
                443,
                &Protocol::Tcp,
            )
            .await;

        assert_eq!(decision, AclDecision::Allow);
        assert_eq!(rule.unwrap(), "Allow HTTPS");
    }

    #[tokio::test]
    async fn test_group_inheritance() {
        let engine = AclEngine::new(create_test_config()).unwrap();

        // Should match group rule
        let (decision, rule) = engine
            .evaluate(
                "alice",
                &Address::Domain("api.dev.example.com".to_string()),
                8080,
                &Protocol::Tcp,
            )
            .await;

        assert_eq!(decision, AclDecision::Allow);
        assert_eq!(rule.unwrap(), "Dev servers");
    }

    #[tokio::test]
    async fn test_default_policy() {
        let engine = AclEngine::new(create_test_config()).unwrap();

        // No rule matches - should use default BLOCK
        let (decision, rule) = engine
            .evaluate(
                "alice",
                &Address::IPv4([93, 184, 216, 34]),
                80,
                &Protocol::Tcp,
            )
            .await;

        assert_eq!(decision, AclDecision::Block);
        assert_eq!(rule.unwrap(), "Default policy");
    }

    #[tokio::test]
    async fn test_unknown_user_default_policy() {
        let engine = AclEngine::new(create_test_config()).unwrap();

        // Unknown user - should use default policy
        let (decision, _) = engine
            .evaluate(
                "bob",
                &Address::IPv4([93, 184, 216, 34]),
                443,
                &Protocol::Tcp,
            )
            .await;

        assert_eq!(decision, AclDecision::Block);
    }

    #[tokio::test]
    async fn test_acl_evaluation_performance_under_5ms() {
        let engine = AclEngine::new(create_test_config()).unwrap();
        let iterations = 100;
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            let _ = engine
                .evaluate(
                    "alice",
                    &Address::Domain("api.dev.example.com".to_string()),
                    443,
                    &Protocol::Tcp,
                )
                .await;
        }

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0 / iterations as f64;
        assert!(
            elapsed_ms <= 5.0,
            "Expected <=5ms per evaluation, observed {:.3}ms",
            elapsed_ms
        );
    }

    #[test]
    fn test_config_validation() {
        let mut config = create_test_config();

        // Valid config
        assert!(config.validate().is_ok());

        // Duplicate user
        config.users.push(config.users[0].clone());
        assert!(config.validate().is_err());

        // Reset
        config = create_test_config();

        // Non-existent group
        config.users[0].groups.push("non-existent".to_string());
        assert!(config.validate().is_err());
    }
}
