use super::matcher::CompiledAclRule;
use super::metrics::AclMetrics;
use super::policy::{
    CompiledAccessPolicy, PolicyAdmissionLimits, PolicyEvaluationContext, PolicyEvaluationOutcome,
    PolicyTraceEntry,
};
use super::types::{AclConfig, AclDecision, Action, GlobalAclConfig, PolicyMode, Protocol};
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
    policies: Vec<Arc<CompiledAccessPolicy>>,
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

#[derive(Debug, Clone)]
enum EvaluationCandidate {
    Legacy(Arc<CompiledAclRule>),
    Policy(Arc<CompiledAccessPolicy>),
}

impl EvaluationCandidate {
    fn priority(&self) -> u32 {
        match self {
            Self::Legacy(r) => r.priority,
            Self::Policy(p) => p.policy.priority,
        }
    }
    fn action(&self) -> &Action {
        match self {
            Self::Legacy(r) => &r.action,
            Self::Policy(p) => &p.policy.action,
        }
    }
}

fn compare_candidates(a: &EvaluationCandidate, b: &EvaluationCandidate) -> std::cmp::Ordering {
    b.priority()
        .cmp(&a.priority())
        .then_with(|| match (a.action(), b.action()) {
            (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
            (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
}

fn compare_rules(a: &Arc<CompiledAclRule>, b: &Arc<CompiledAclRule>) -> std::cmp::Ordering {
    b.priority
        .cmp(&a.priority)
        .then_with(|| match (&a.action, &b.action) {
            (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
            (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
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

            // Higher priority wins. BLOCK wins only when priorities are equal.
            compiled_rules.sort_by(compare_rules);

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

            // Higher priority wins. BLOCK wins only when priorities are equal.
            compiled_rules.sort_by(compare_rules);

            let compiled_group = CompiledGroupAcl {
                name: group_acl.name.clone(),
                rules: compiled_rules,
            };

            // Insert into both maps - regular and lowercase index
            groups.insert(group_acl.name.clone(), compiled_group.clone());
            groups_by_lowercase.insert(group_acl.name.to_lowercase(), compiled_group);
        }

        let policies = config
            .policies
            .iter()
            .filter(|policy| policy.enabled)
            .map(|policy| CompiledAccessPolicy::compile(policy).map(Arc::new))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CompiledAclConfig {
            global: config.global.clone(),
            users,
            groups,
            groups_by_lowercase,
            policies,
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

        // Evaluate rules by descending priority; BLOCK wins only on equal priority
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
    /// 5. Evaluates rules by descending priority (BLOCK wins only on equal priority)
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

        // Evaluate rules by descending priority; BLOCK wins only on equal priority
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

    /// Evaluate legacy ACL rules and dynamic policies in one shared priority space.
    pub async fn evaluate_policy_with_context(
        &self,
        ctx: PolicyEvaluationContext<'_>,
    ) -> PolicyEvaluationOutcome {
        let started = std::time::Instant::now();
        let outcome = self.evaluate_policy_uninstrumented(ctx).await;
        AclMetrics::observe_evaluation(started.elapsed().as_secs_f64());
        outcome
    }

    async fn evaluate_policy_uninstrumented(
        &self,
        ctx: PolicyEvaluationContext<'_>,
    ) -> PolicyEvaluationOutcome {
        let (mut candidates, default_policy, effective_groups) = {
            let config = self.config.read().await;
            let effective_groups = self.effective_groups(&config, ctx.user, ctx.groups);
            let mut candidates: Vec<EvaluationCandidate> = self
                .collect_rules_from_groups(&config, ctx.user, ctx.groups)
                .into_iter()
                .map(EvaluationCandidate::Legacy)
                .collect();
            candidates.extend(
                config
                    .policies
                    .iter()
                    .filter(|policy| policy.subject_matches(ctx.user, &effective_groups))
                    .cloned()
                    .map(EvaluationCandidate::Policy),
            );
            (
                candidates,
                config.global.default_policy.clone(),
                effective_groups,
            )
        };

        candidates.sort_unstable_by(compare_candidates);
        let mut trace = Vec::new();
        let policy_ctx = PolicyEvaluationContext {
            groups: &effective_groups,
            ..ctx
        };

        for candidate in candidates {
            match candidate {
                EvaluationCandidate::Legacy(rule) => {
                    let matched =
                        rule.matches(policy_ctx.destination, policy_ctx.port, policy_ctx.protocol);
                    trace.push(PolicyTraceEntry {
                        source: "legacy_acl".into(),
                        id: None,
                        description: rule.description.clone(),
                        priority: rule.priority,
                        action: rule.action.clone(),
                        mode: None,
                        target_matched: matched,
                        conditions_matched: None,
                        effective: matched,
                        reason: if matched {
                            "legacy ACL rule matched".into()
                        } else {
                            "destination, port, or protocol did not match".into()
                        },
                    });
                    if matched {
                        return PolicyEvaluationOutcome {
                            decision: AclDecision::from(&rule.action),
                            matched_rule: Some(rule.description.clone()),
                            matched_policy_id: None,
                            admission_limits: PolicyAdmissionLimits::default(),
                            trace,
                        };
                    }
                }
                EvaluationCandidate::Policy(policy) => {
                    let target_matched = policy.target_matches(
                        policy_ctx.destination,
                        policy_ctx.port,
                        policy_ctx.protocol,
                    );
                    if !target_matched {
                        trace.push(PolicyTraceEntry {
                            source: "policy".into(),
                            id: Some(policy.policy.id.clone()),
                            description: policy.policy.description.clone(),
                            priority: policy.policy.priority,
                            action: policy.policy.action.clone(),
                            mode: Some(policy.policy.mode.clone()),
                            target_matched: false,
                            conditions_matched: None,
                            effective: false,
                            reason: "destination, port, or protocol did not match".into(),
                        });
                        continue;
                    }
                    let (conditions_matched, condition_reason) =
                        policy.conditions_match(&policy_ctx);
                    if policy.policy.mode == PolicyMode::Monitor {
                        trace.push(PolicyTraceEntry {
                            source: "policy".into(),
                            id: Some(policy.policy.id.clone()),
                            description: policy.policy.description.clone(),
                            priority: policy.policy.priority,
                            action: policy.policy.action.clone(),
                            mode: Some(policy.policy.mode.clone()),
                            target_matched: true,
                            conditions_matched: Some(conditions_matched),
                            effective: false,
                            reason: format!("monitor mode: {}", condition_reason),
                        });
                        continue;
                    }
                    if !conditions_matched {
                        let gate_block = policy.policy.enforce_conditions;
                        trace.push(PolicyTraceEntry {
                            source: "policy".into(),
                            id: Some(policy.policy.id.clone()),
                            description: policy.policy.description.clone(),
                            priority: policy.policy.priority,
                            action: policy.policy.action.clone(),
                            mode: Some(policy.policy.mode.clone()),
                            target_matched: true,
                            conditions_matched: Some(false),
                            effective: gate_block,
                            reason: if gate_block {
                                format!("gate condition failed: {}", condition_reason)
                            } else {
                                condition_reason
                            },
                        });
                        if gate_block {
                            return PolicyEvaluationOutcome {
                                decision: AclDecision::Block,
                                matched_rule: Some(format!(
                                    "Policy '{}' gate: {}",
                                    policy.policy.id, policy.policy.description
                                )),
                                matched_policy_id: Some(policy.policy.id.clone()),
                                admission_limits: PolicyAdmissionLimits::default(),
                                trace,
                            };
                        }
                        continue;
                    }
                    trace.push(PolicyTraceEntry {
                        source: "policy".into(),
                        id: Some(policy.policy.id.clone()),
                        description: policy.policy.description.clone(),
                        priority: policy.policy.priority,
                        action: policy.policy.action.clone(),
                        mode: Some(policy.policy.mode.clone()),
                        target_matched: true,
                        conditions_matched: Some(true),
                        effective: true,
                        reason: condition_reason,
                    });
                    let admission_limits = if policy.policy.action == Action::Allow {
                        policy.admission_limits()
                    } else {
                        PolicyAdmissionLimits::default()
                    };
                    return PolicyEvaluationOutcome {
                        decision: AclDecision::from(&policy.policy.action),
                        matched_rule: Some(format!(
                            "Policy '{}': {}",
                            policy.policy.id, policy.policy.description
                        )),
                        matched_policy_id: Some(policy.policy.id.clone()),
                        admission_limits,
                        trace,
                    };
                }
            }
        }
        PolicyEvaluationOutcome {
            decision: AclDecision::from(&default_policy),
            matched_rule: Some("Default policy".into()),
            matched_policy_id: None,
            admission_limits: PolicyAdmissionLimits::default(),
            trace,
        }
    }

    pub async fn is_explicitly_blocked_with_policy_context(
        &self,
        ctx: PolicyEvaluationContext<'_>,
    ) -> Option<String> {
        let outcome = self.evaluate_policy_with_context(ctx).await;
        if outcome.decision == AclDecision::Block
            && outcome.trace.last().is_some_and(|entry| entry.effective)
        {
            AclMetrics::record_post_dns_block();
            outcome.matched_rule
        } else {
            None
        }
    }

    /// Re-evaluate explicit rules against an already-resolved destination.
    /// This is used after DNS resolution so domain allow rules cannot bypass
    /// higher-priority CIDR/IP denies while preserving normal rule precedence.
    pub async fn is_explicitly_blocked_with_groups(
        &self,
        user: &str,
        user_groups: &[String],
        dest: &Address,
        port: u16,
        protocol: &Protocol,
    ) -> Option<String> {
        let all_rules = {
            let config = self.config.read().await;
            self.collect_rules_from_groups(&config, user, user_groups)
        };

        for rule in &all_rules {
            if !rule.matches(dest, port, protocol) {
                continue;
            }

            // The resolved-IP recheck preserves normal ACL precedence: a higher
            // priority ALLOW may override a lower priority BLOCK. If no IP rule
            // matches, the original hostname decision remains authoritative.
            return match rule.action {
                Action::Block => Some(rule.description.clone()),
                Action::Allow => None,
            };
        }
        None
    }

    fn effective_groups(
        &self,
        config: &CompiledAclConfig,
        user: &str,
        dynamic_groups: &[String],
    ) -> Vec<String> {
        let mut groups = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if let Some(user_acl) = config.users.get(user) {
            for group in &user_acl.groups {
                let key = group.to_ascii_lowercase();
                if seen.insert(key) {
                    groups.push(group.clone());
                }
            }
        }
        for group in dynamic_groups {
            let key = group.to_ascii_lowercase();
            if seen.insert(key) {
                groups.push(group.clone());
            }
        }
        groups
    }

    /// Collect all rules for a user (user rules + group rules)
    /// Rules are merged from user/group sources and globally re-sorted here
    /// Returns Vec<Arc<CompiledAclRule>> - cloning Arc is cheap (atomic counter increment)
    fn collect_rules(&self, config: &CompiledAclConfig, user: &str) -> Vec<Arc<CompiledAclRule>> {
        // Pre-allocate capacity to avoid reallocations
        let estimated_capacity = if let Some(user_acl) = config.users.get(user) {
            // User rules + estimated group rules (assume ~5 rules per group on average)
            user_acl.rules.len() + user_acl.groups.len() * 5
        } else {
            0
        };
        let mut all_rules = Vec::with_capacity(estimated_capacity);

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

        // Re-sort combined user/group rules using the documented global order.
        all_rules.sort_unstable_by(compare_rules);

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
        // Pre-allocate capacity to avoid reallocations
        // Estimate: user rules + (LDAP groups that might match) * avg rules per group
        // Conservative estimate: assume 20% of LDAP groups match ACL config
        let user_rules_count = config.users.get(user).map_or(0, |u| u.rules.len());
        let estimated_group_rules = (user_groups.len() / 5 + 1) * 5; // ~20% match rate * 5 rules avg
        let mut all_rules = Vec::with_capacity(user_rules_count + estimated_group_rules);

        let mut matched_groups = std::collections::HashSet::new();

        // Add per-user rules and statically configured groups from [[users]].
        // Static memberships remain useful even when NSS/SSSD is unavailable.
        if let Some(user_acl) = config.users.get(user) {
            all_rules.extend(user_acl.rules.clone());
            for configured_group in &user_acl.groups {
                let lowercase_group = configured_group.to_lowercase();
                if matched_groups.insert(lowercase_group.clone()) {
                    if let Some(group_acl) = config.groups_by_lowercase.get(&lowercase_group) {
                        all_rules.extend(group_acl.rules.clone());
                    }
                }
            }
        }

        // Merge dynamic NSS/SSSD memberships case-insensitively and deduplicate
        // groups already supplied by the static user configuration.
        for dynamic_group in user_groups {
            let lowercase_group = dynamic_group.to_lowercase();
            if !matched_groups.insert(lowercase_group.clone()) {
                continue;
            }
            if let Some(group_acl) = config.groups_by_lowercase.get(&lowercase_group) {
                all_rules.extend(group_acl.rules.clone());
            }
        }

        all_rules.sort_unstable_by(compare_rules);

        all_rules
    }

    /// Get list of LDAP groups that matched ACL groups (for debugging)
    #[allow(dead_code)]
    fn get_matched_groups(
        &self,
        config: &CompiledAclConfig,
        user_groups: &[String],
    ) -> Vec<String> {
        // Pre-allocate with conservative estimate (assume ~20% match rate)
        let mut matched = Vec::with_capacity(user_groups.len() / 5 + 1);

        // Use O(1) lowercase lookup instead of nested loop
        for ldap_group in user_groups {
            let lowercase_group = ldap_group.to_lowercase();
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

    pub async fn get_policy_count(&self) -> usize {
        let config = self.config.read().await;
        config.policies.len()
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

        let mut seen_policy_ids = std::collections::HashSet::new();
        for policy in &self.policies {
            let key = policy.id.to_ascii_lowercase();
            if !seen_policy_ids.insert(key) {
                return Err(format!("Duplicate policy id: {}", policy.id));
            }
            super::policy::validate_policy(policy)?;
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
            policies: vec![],
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
