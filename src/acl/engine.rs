use super::matcher::CompiledAclRule;
use super::types::{AclConfig, AclDecision, Action, GlobalAclConfig, Protocol};
use crate::protocol::Address;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

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
    // Lowercase index for O(1) case-insensitive group lookup (critical optimization for LDAP)
    groups_by_lowercase: std::collections::HashMap<String, CompiledGroupAcl>,
}

#[derive(Debug, Clone)]
struct CompiledUserAcl {
    #[allow(dead_code)]
    username: String,
    groups: Vec<String>,
    // Use Arc to make cloning cheap (just atomic counter increment)
    rules: Vec<Arc<CompiledAclRule>>,
}

#[derive(Debug, Clone)]
struct CompiledGroupAcl {
    #[allow(dead_code)]
    name: String,
    // Use Arc to make cloning cheap (just atomic counter increment)
    rules: Vec<Arc<CompiledAclRule>>,
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
        let mut groups_by_lowercase = std::collections::HashMap::new();

        // Compile user rules (wrap in Arc for cheap cloning)
        for user_acl in &config.users {
            let mut compiled_rules: Vec<_> = user_acl
                .rules
                .iter()
                .map(|r| CompiledAclRule::compile(r).map(Arc::new))
                .collect::<Result<Vec<_>, _>>()?;

            // Pre-sort rules during compilation (optimization: avoid per-evaluation sorting)
            // BLOCK rules first, then by priority descending
            compiled_rules.sort_by(|a, b| {
                match (&a.action, &b.action) {
                    (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
                    (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
                    _ => b.priority.cmp(&a.priority),
                }
            });

            users.insert(
                user_acl.username.clone(),
                CompiledUserAcl {
                    username: user_acl.username.clone(),
                    groups: user_acl.groups.clone(),
                    rules: compiled_rules,
                },
            );
        }

        // Compile group rules (wrap in Arc for cheap cloning)
        for group_acl in &config.groups {
            let mut compiled_rules: Vec<_> = group_acl
                .rules
                .iter()
                .map(|r| CompiledAclRule::compile(r).map(Arc::new))
                .collect::<Result<Vec<_>, _>>()?;

            // Pre-sort rules during compilation (optimization: avoid per-evaluation sorting)
            // BLOCK rules first, then by priority descending
            compiled_rules.sort_by(|a, b| {
                match (&a.action, &b.action) {
                    (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
                    (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
                    _ => b.priority.cmp(&a.priority),
                }
            });

            let compiled_group = CompiledGroupAcl {
                name: group_acl.name.clone(),
                rules: compiled_rules,
            };

            // Insert into both maps - regular and lowercase index
            groups.insert(group_acl.name.clone(), compiled_group.clone());
            groups_by_lowercase.insert(
                group_acl.name.to_ascii_lowercase(),
                compiled_group,
            );
        }

        Ok(CompiledAclConfig {
            global: config.global.clone(),
            users,
            groups,
            groups_by_lowercase,
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
        // OPTIMIZATION: Minimize RwLock hold time - clone only what we need and release lock immediately
        let (all_rules, default_policy) = {
            let config = self.config.read().await;
            let rules = self.collect_rules(&config, user);
            let policy = config.global.default_policy.clone();
            (rules, policy)
            // Lock released here
        };

        if all_rules.is_empty() {
            return (
                AclDecision::from(&default_policy),
                Some("Default policy".to_string()),
            );
        }

        // Evaluate rules in priority order (BLOCK rules first)
        for rule in &all_rules {
            if rule.matches(dest, port, protocol) {
                return (
                    AclDecision::from(&rule.action),
                    Some(rule.description.clone()),
                );
            }
        }

        // No rule matched - apply default policy
        (
            AclDecision::from(&default_policy),
            Some("Default policy".to_string()),
        )
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
        // OPTIMIZATION: Minimize RwLock hold time - clone only what we need and release lock immediately
        let (all_rules, default_policy) = {
            let config = self.config.read().await;
            let rules = self.collect_rules_from_groups(&config, user, user_groups);
            let policy = config.global.default_policy.clone();
            (rules, policy)
            // Lock released here
        };

        if all_rules.is_empty() {
            return (
                AclDecision::from(&default_policy),
                Some("Default policy (no matching groups)".to_string()),
            );
        }

        // Evaluate rules in priority order (BLOCK rules first)
        for rule in &all_rules {
            if rule.matches(dest, port, protocol) {
                return (
                    AclDecision::from(&rule.action),
                    Some(rule.description.clone()),
                );
            }
        }

        // No rule matched - apply default policy
        (
            AclDecision::from(&default_policy),
            Some("Default policy".to_string()),
        )
    }

    /// Collect all rules for a user (user rules + group rules)
    /// Rules are pre-sorted during compilation, so no sorting needed here
    /// Returns Vec<Arc<CompiledAclRule>> - cloning Arc is cheap (atomic counter increment)
    fn collect_rules(&self, config: &CompiledAclConfig, user: &str) -> Vec<Arc<CompiledAclRule>> {
        let mut all_rules = Vec::new();

        // Get user's rules (already sorted during compilation)
        if let Some(user_acl) = config.users.get(user) {
            // Cheap clone - just Arc increment, no deep copy
            all_rules.extend(user_acl.rules.clone());

            // Add rules from user's groups (each group's rules are already sorted)
            for group_name in &user_acl.groups {
                if let Some(group_acl) = config.groups.get(group_name) {
                    // Cheap clone - just Arc increment, no deep copy
                    all_rules.extend(group_acl.rules.clone());
                }
            }
        }

        // OPTIMIZATION: Re-sort combined rules to maintain global priority order
        // This is needed because we're mixing user rules + group rules
        // Pre-sorted data sorts faster (O(n) for already sorted data with adaptive sort)
        all_rules.sort_unstable_by(|a, b| {
            match (&a.action, &b.action) {
                (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
                (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
                _ => b.priority.cmp(&a.priority),
            }
        });

        all_rules
    }

    /// Collect rules from LDAP groups (case-insensitive matching)
    ///
    /// This method:
    /// - Iterates through user's LDAP groups
    /// - For each LDAP group, checks if it exists in ACL config (case-insensitive)
    /// - If match found, adds that group's rules (already pre-sorted)
    /// - Also adds per-user rules from [[users]] section if present
    ///
    /// Case-insensitive example:
    /// - LDAP group: "Developers"
    /// - ACL config: [[groups]] name = "developers"
    /// - Result: MATCH (case-insensitive)
    ///
    /// Performance: O(n) where n = number of user's LDAP groups
    /// Uses precomputed lowercase HashMap for O(1) lookups instead of O(m) linear scan
    fn collect_rules_from_groups(
        &self,
        config: &CompiledAclConfig,
        user: &str,
        user_groups: &[String],
    ) -> Vec<Arc<CompiledAclRule>> {
        let mut all_rules = Vec::new();

        // Add per-user rules first (already sorted during compilation)
        if let Some(user_acl) = config.users.get(user) {
            // Cheap clone - just Arc increment, no deep copy
            all_rules.extend(user_acl.rules.clone());
        }

        // OPTIMIZATION: Iterate through user's LDAP groups with O(1) lookup instead of O(n*m) nested loop
        for ldap_group in user_groups {
            let lowercase_group = ldap_group.to_ascii_lowercase();
            if let Some(group_acl) = config.groups_by_lowercase.get(&lowercase_group) {
                // Cheap clone - just Arc increment, no deep copy
                // Rules are already sorted during compilation
                all_rules.extend(group_acl.rules.clone());
            }
        }

        // OPTIMIZATION: Re-sort combined rules to maintain global priority order
        // This is needed because we're mixing user rules + multiple group rules
        // Pre-sorting during compilation helps here (partially sorted data sorts faster)
        // Use sort_unstable_by for better performance (no stable sort needed for ACL rules)
        all_rules.sort_unstable_by(|a, b| {
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
    #[allow(dead_code)]
    fn get_matched_groups(
        &self,
        config: &CompiledAclConfig,
        user_groups: &[String],
    ) -> Vec<String> {
        let mut matched = Vec::new();

        // Use O(1) lowercase lookup instead of nested loop
        for ldap_group in user_groups {
            let lowercase_group = ldap_group.to_ascii_lowercase();
            if config.groups_by_lowercase.contains_key(&lowercase_group) {
                matched.push(ldap_group.clone());
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
