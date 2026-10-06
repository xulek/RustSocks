use super::matcher::{CompiledAclRule, PreparedDestination};
use super::metrics::AclMetrics;
use super::policy::{
    CompiledAccessPolicy, DecisionSource, PolicyAdmissionLimits, PolicyEvaluationContext,
    PolicyEvaluationOutcome, PolicyTraceEntry,
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
    /// Enabled policies sorted by (priority descending, BLOCK before ALLOW).
    policies: Vec<Arc<CompiledAccessPolicy>>,
    /// Whether any enabled policy names a group, i.e. group membership must be resolved
    /// to evaluate policy subjects.
    has_group_policies: bool,
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

fn compare_rules(a: &Arc<CompiledAclRule>, b: &Arc<CompiledAclRule>) -> std::cmp::Ordering {
    b.priority
        .cmp(&a.priority)
        .then_with(|| match (&a.action, &b.action) {
            (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
            (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
}

/// A sorted list of candidates (legacy rules or policies) taking part in the merge.
struct Source<'a> {
    kind: SourceKind<'a>,
    index: usize,
}

enum SourceKind<'a> {
    Legacy(&'a [Arc<CompiledAclRule>]),
    Policies(&'a [Arc<CompiledAccessPolicy>]),
}

#[derive(Clone, Copy)]
enum Candidate<'a> {
    Legacy(&'a CompiledAclRule),
    Policy(&'a CompiledAccessPolicy),
}

impl<'a> Candidate<'a> {
    fn priority(&self) -> u32 {
        match self {
            Self::Legacy(rule) => rule.priority,
            Self::Policy(policy) => policy.policy.priority,
        }
    }

    fn is_block(&self) -> bool {
        match self {
            Self::Legacy(rule) => rule.action == Action::Block,
            Self::Policy(policy) => policy.policy.action == Action::Block,
        }
    }

    /// Strictly better: higher priority first; on equal priority BLOCK before ALLOW, then
    /// policy before legacy rule.
    fn precedes(&self, other: &Candidate<'_>) -> bool {
        match self.priority().cmp(&other.priority()) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => {
                if self.is_block() != other.is_block() {
                    self.is_block()
                } else {
                    // Same priority and action: a policy is evaluated before a legacy rule,
                    // so a failing gate condition cannot be bypassed by an equally-ranked
                    // legacy ALLOW. (Earlier sources win among the same kind.)
                    matches!(self, Candidate::Policy(_)) && matches!(other, Candidate::Legacy(_))
                }
            }
        }
    }
}

impl<'a> Source<'a> {
    fn legacy(rules: &'a [Arc<CompiledAclRule>]) -> Self {
        Self {
            kind: SourceKind::Legacy(rules),
            index: 0,
        }
    }

    fn policies(policies: &'a [Arc<CompiledAccessPolicy>]) -> Self {
        Self {
            kind: SourceKind::Policies(policies),
            index: 0,
        }
    }

    /// Next candidate of this source, skipping policies whose subject does not match.
    fn peek(&mut self, user_lower: &str, groups_lower: &[String]) -> Option<Candidate<'a>> {
        match self.kind {
            SourceKind::Legacy(rules) => rules.get(self.index).map(|rule| Candidate::Legacy(rule)),
            SourceKind::Policies(policies) => {
                while let Some(policy) = policies.get(self.index) {
                    if policy.subject_matches_lowered(user_lower, groups_lower) {
                        return Some(Candidate::Policy(policy));
                    }
                    self.index += 1;
                }
                None
            }
        }
    }

    fn advance(&mut self) {
        self.index += 1;
    }
}

/// Add a legacy rule list unless it is empty or already present (the same group reached via
/// differently-cased names, or both statically and dynamically).
fn push_legacy_source<'a>(sources: &mut Vec<Source<'a>>, rules: &'a [Arc<CompiledAclRule>]) {
    if rules.is_empty() {
        return;
    }
    let already = sources.iter().any(|source| match source.kind {
        SourceKind::Legacy(existing) => std::ptr::eq(existing.as_ptr(), rules.as_ptr()),
        SourceKind::Policies(_) => false,
    });
    if !already {
        sources.push(Source::legacy(rules));
    }
}

/// Lowercase `name` into a reused buffer (same result as `str::to_lowercase`).
fn lowercase_into(buffer: &mut String, name: &str) {
    buffer.clear();
    if name.is_ascii() {
        buffer.push_str(name);
        buffer.make_ascii_lowercase();
    } else {
        buffer.push_str(&name.to_lowercase());
    }
}

fn policy_trace(
    policy: &CompiledAccessPolicy,
    target_matched: bool,
    conditions_matched: Option<bool>,
    effective: bool,
    reason: String,
) -> PolicyTraceEntry {
    PolicyTraceEntry {
        source: "policy".into(),
        id: Some(policy.policy.id.clone()),
        description: policy.policy.description.clone(),
        priority: policy.policy.priority,
        action: policy.policy.action.clone(),
        mode: Some(policy.policy.mode.clone()),
        target_matched,
        conditions_matched,
        effective,
        reason,
    }
}

fn compare_policies(
    a: &Arc<CompiledAccessPolicy>,
    b: &Arc<CompiledAccessPolicy>,
) -> std::cmp::Ordering {
    b.policy.priority.cmp(&a.policy.priority).then_with(|| {
        match (&a.policy.action, &b.policy.action) {
            (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
            (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
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

        let mut policies = config
            .policies
            .iter()
            .filter(|policy| policy.enabled)
            .map(|policy| CompiledAccessPolicy::compile(policy).map(Arc::new))
            .collect::<Result<Vec<_>, _>>()?;
        // Stable sort keeps configuration order for equal (priority, action).
        policies.sort_by(compare_policies);
        let has_group_policies = policies.iter().any(|policy| policy.has_group_subjects());

        Ok(CompiledAclConfig {
            global: config.global.clone(),
            users,
            groups,
            groups_by_lowercase,
            policies,
            has_group_policies,
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

    /// Evaluate legacy ACL rules and dynamic policies in one shared priority space, with a
    /// full explanation trace. Used by the Explain/Simulator API.
    pub async fn evaluate_policy_with_context(
        &self,
        ctx: PolicyEvaluationContext<'_>,
    ) -> PolicyEvaluationOutcome {
        let started = std::time::Instant::now();
        let outcome = {
            let config = self.config.read().await;
            Self::evaluate_compiled::<true>(&config, &ctx)
        };
        AclMetrics::observe_evaluation(started.elapsed().as_secs_f64());
        outcome
    }

    /// Same decision as [`Self::evaluate_policy_with_context`] without building the
    /// explanation trace. This is the per-connection path.
    pub async fn evaluate_policy_for_traffic(
        &self,
        ctx: PolicyEvaluationContext<'_>,
    ) -> PolicyEvaluationOutcome {
        let started = std::time::Instant::now();
        let outcome = {
            let config = self.config.read().await;
            Self::evaluate_compiled::<false>(&config, &ctx)
        };
        AclMetrics::observe_evaluation(started.elapsed().as_secs_f64());
        outcome
    }

    /// Core evaluation. Legacy rules and policies are each already sorted by (priority
    /// descending, BLOCK before ALLOW), so candidates are produced by merging those sorted
    /// lists lazily instead of copying and re-sorting them for every request. The result is
    /// identical to sorting the union, and evaluation stops at the first match.
    ///
    /// `TRACE` selects whether the per-candidate explanation is built.
    fn evaluate_compiled<const TRACE: bool>(
        config: &CompiledAclConfig,
        ctx: &PolicyEvaluationContext<'_>,
    ) -> PolicyEvaluationOutcome {
        let dest = PreparedDestination::new(ctx.destination);

        // Subject keys for policies, lowercased once per request and only when needed.
        let user_lower = if config.policies.is_empty() {
            String::new()
        } else {
            ctx.user.to_ascii_lowercase()
        };
        let mut groups_lower: Vec<String> = Vec::new();
        if config.has_group_policies {
            if let Some(user_acl) = config.users.get(ctx.user) {
                groups_lower.extend(user_acl.groups.iter().map(|g| g.to_ascii_lowercase()));
            }
            groups_lower.extend(ctx.groups.iter().map(|g| g.to_ascii_lowercase()));
        }

        // Collect the sorted sources: user rules, rules of every matching group, policies.
        let mut sources: Vec<Source<'_>> = Vec::with_capacity(8);
        let mut key = String::new();
        if let Some(user_acl) = config.users.get(ctx.user) {
            push_legacy_source(&mut sources, &user_acl.rules);
            for configured in &user_acl.groups {
                lowercase_into(&mut key, configured);
                if let Some(group) = config.groups_by_lowercase.get(key.as_str()) {
                    push_legacy_source(&mut sources, &group.rules);
                }
            }
        }
        for dynamic in ctx.groups {
            lowercase_into(&mut key, dynamic);
            if let Some(group) = config.groups_by_lowercase.get(key.as_str()) {
                push_legacy_source(&mut sources, &group.rules);
            }
        }
        if !config.policies.is_empty() {
            sources.push(Source::policies(&config.policies));
        }

        let mut trace: Vec<PolicyTraceEntry> = Vec::new();
        let mut monitor_matches: u32 = 0;

        loop {
            // Pick the best head among the sources (ties keep the earlier source).
            let mut best: Option<(usize, Candidate<'_>)> = None;
            for (index, source) in sources.iter_mut().enumerate() {
                if let Some(candidate) = source.peek(&user_lower, &groups_lower) {
                    let better = match &best {
                        None => true,
                        Some((_, current)) => candidate.precedes(current),
                    };
                    if better {
                        best = Some((index, candidate));
                    }
                }
            }
            let Some((index, candidate)) = best else {
                break;
            };
            sources[index].advance();

            match candidate {
                Candidate::Legacy(rule) => {
                    let matched = rule.matches_prepared(&dest, ctx.port, ctx.protocol);
                    if TRACE {
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
                    }
                    if matched {
                        return PolicyEvaluationOutcome {
                            decision: AclDecision::from(&rule.action),
                            matched_rule: Some(rule.description.clone()),
                            matched_policy_id: None,
                            admission_limits: PolicyAdmissionLimits::default(),
                            source: DecisionSource::LegacyAcl,
                            monitor_matches,
                            trace,
                        };
                    }
                }
                Candidate::Policy(policy) => {
                    let target_matched =
                        policy.target_matches_prepared(&dest, ctx.port, ctx.protocol);
                    if !target_matched {
                        if TRACE {
                            trace.push(policy_trace(
                                policy,
                                false,
                                None,
                                false,
                                "destination, port, or protocol did not match".into(),
                            ));
                        }
                        continue;
                    }

                    let (conditions_matched, condition_reason) = if TRACE {
                        policy.conditions_match(ctx)
                    } else {
                        (policy.conditions_hold(ctx), String::new())
                    };

                    if policy.policy.mode == PolicyMode::Monitor {
                        if conditions_matched {
                            monitor_matches += 1;
                        }
                        if TRACE {
                            trace.push(policy_trace(
                                policy,
                                true,
                                Some(conditions_matched),
                                false,
                                format!("monitor mode: {}", condition_reason),
                            ));
                        }
                        continue;
                    }

                    if !conditions_matched {
                        let gate_block = policy.policy.enforce_conditions;
                        if TRACE {
                            trace.push(policy_trace(
                                policy,
                                true,
                                Some(false),
                                gate_block,
                                if gate_block {
                                    format!("gate condition failed: {}", condition_reason)
                                } else {
                                    condition_reason
                                },
                            ));
                        }
                        if gate_block {
                            return PolicyEvaluationOutcome {
                                decision: AclDecision::Block,
                                matched_rule: Some(format!(
                                    "Policy '{}' gate: {}",
                                    policy.policy.id, policy.policy.description
                                )),
                                matched_policy_id: Some(policy.policy.id.clone()),
                                admission_limits: PolicyAdmissionLimits::default(),
                                source: DecisionSource::Policy,
                                monitor_matches,
                                trace,
                            };
                        }
                        continue;
                    }

                    if TRACE {
                        trace.push(policy_trace(
                            policy,
                            true,
                            Some(true),
                            true,
                            condition_reason,
                        ));
                    }
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
                        source: DecisionSource::Policy,
                        monitor_matches,
                        trace,
                    };
                }
            }
        }

        PolicyEvaluationOutcome {
            decision: AclDecision::from(&config.global.default_policy),
            matched_rule: Some("Default policy".into()),
            matched_policy_id: None,
            admission_limits: PolicyAdmissionLimits::default(),
            source: DecisionSource::Default,
            monitor_matches,
            trace,
        }
    }

    /// The decision for an already-resolved destination, used after DNS resolution and for
    /// BIND peers: a block only counts when an explicit rule or policy produced it.
    pub async fn is_explicitly_blocked_with_policy_context(
        &self,
        ctx: PolicyEvaluationContext<'_>,
    ) -> Option<String> {
        let outcome = self.evaluate_policy_for_traffic(ctx).await;
        if outcome.decision == AclDecision::Block && outcome.source != DecisionSource::Default {
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

    #[cfg(test)]
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

/// The previous evaluation algorithm (collect, clone and sort every request, build the full
/// trace). Kept as an oracle: the optimized path must agree with it on every input.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct ReferenceOutcome {
    pub decision: AclDecision,
    pub matched_rule: Option<String>,
    pub matched_policy_id: Option<String>,
    pub admission_limits: PolicyAdmissionLimits,
    pub trace: Vec<PolicyTraceEntry>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
enum EvaluationCandidate {
    Legacy(Arc<CompiledAclRule>),
    Policy(Arc<CompiledAccessPolicy>),
}

#[cfg(test)]
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

#[cfg(test)]
fn compare_candidates(a: &EvaluationCandidate, b: &EvaluationCandidate) -> std::cmp::Ordering {
    b.priority()
        .cmp(&a.priority())
        .then_with(|| match (a.action(), b.action()) {
            (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
            (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
}

#[cfg(test)]
impl AclEngine {
    pub(crate) async fn evaluate_policy_reference(
        &self,
        ctx: PolicyEvaluationContext<'_>,
    ) -> ReferenceOutcome {
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
                        return ReferenceOutcome {
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
                            return ReferenceOutcome {
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
                    return ReferenceOutcome {
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
        ReferenceOutcome {
            decision: AclDecision::from(&default_policy),
            matched_rule: Some("Default policy".into()),
            matched_policy_id: None,
            admission_limits: PolicyAdmissionLimits::default(),
            trace,
        }
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

/// Differential tests: the optimized evaluation must agree with the previous algorithm
/// (`evaluate_policy_reference`) on every input.
#[cfg(test)]
mod differential {
    use super::*;
    use crate::acl::types::{
        AccessPolicy, AclRule, GroupAcl, PolicyConditions, PolicySchedule, UserAcl,
    };
    use crate::acl::PolicyUsageSnapshot;
    use chrono::{TimeZone, Utc};
    use proptest::prelude::*;

    const DESTINATIONS: [&str; 9] = [
        "*",
        "example.com",
        "*.example.com",
        "api.example.com",
        "10.0.0.0/8",
        "192.168.1.5",
        "2001:db8::/32",
        "evil.org",
        "*.evil.org",
    ];
    const PORTS: [&str; 4] = ["443", "80-90", "*", "22,25"];
    const GROUPS: [&str; 3] = ["dev", "ops", "qa"];

    fn arb_action() -> impl Strategy<Value = Action> {
        prop_oneof![Just(Action::Allow), Just(Action::Block)]
    }

    fn arb_protocols() -> impl Strategy<Value = Vec<Protocol>> {
        prop_oneof![
            Just(vec![Protocol::Tcp]),
            Just(vec![Protocol::Udp]),
            Just(vec![Protocol::Both]),
        ]
    }

    fn arb_destinations() -> impl Strategy<Value = Vec<String>> {
        prop::sample::subsequence(DESTINATIONS.to_vec(), 1..3)
            .prop_map(|v| v.into_iter().map(String::from).collect())
    }

    fn arb_ports() -> impl Strategy<Value = Vec<String>> {
        prop::sample::subsequence(PORTS.to_vec(), 1..3)
            .prop_map(|v| v.into_iter().map(String::from).collect())
    }

    fn arb_rule() -> impl Strategy<Value = AclRule> {
        (
            arb_action(),
            arb_destinations(),
            arb_ports(),
            arb_protocols(),
        )
            .prop_map(|(action, destinations, ports, protocols)| AclRule {
                action,
                description: String::new(),
                destinations,
                ports,
                protocols,
                priority: 0,
            })
    }

    fn arb_conditions(allow: bool) -> impl Strategy<Value = PolicyConditions> {
        let source_ips = prop::option::of(
            prop::sample::subsequence(vec!["10.0.0.0/8", "192.168.0.0/16", "8.8.8.8"], 1..3)
                .prop_map(|v| v.into_iter().map(String::from).collect::<Vec<_>>()),
        );
        let auth_methods = prop::option::of(
            prop::sample::subsequence(vec!["userpass", "gssapi", "none"], 1..3)
                .prop_map(|v| v.into_iter().map(String::from).collect::<Vec<_>>()),
        );
        let schedule = prop::option::of((
            prop::sample::subsequence(vec!["mon", "tue", "wed", "thu", "fri", "sat", "sun"], 0..4),
            prop::sample::select(vec!["00:00", "08:00", "22:00"]),
            prop::sample::select(vec!["06:00", "17:00", "23:30"]),
            prop::sample::select(vec![0i32, 120, -300]),
        ))
        .prop_map(|opt| {
            opt.map(|(days, start, end, offset)| PolicySchedule {
                days: days.into_iter().map(String::from).collect(),
                start: start.to_string(),
                end: end.to_string(),
                utc_offset_minutes: offset,
            })
        });
        let window = (
            prop::option::of(Just(Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap())),
            prop::option::of(Just(Utc.with_ymd_and_hms(2026, 9, 20, 0, 0, 0).unwrap())),
        );
        let limits = (
            prop::option::of(1u32..4),
            prop::option::of(1u32..4),
            prop::option::of(1u64..800),
            prop::option::of(1u64..800),
        );
        (source_ips, auth_methods, schedule, window, limits).prop_map(
            move |(source_ips, auth_methods, schedule, (not_before, expires_at), limits)| {
                PolicyConditions {
                    source_ips: source_ips.unwrap_or_default(),
                    auth_methods: auth_methods.unwrap_or_default(),
                    schedule,
                    not_before,
                    expires_at,
                    max_active_connections: if allow { limits.0 } else { None },
                    max_connections_per_minute: if allow { limits.1 } else { None },
                    daily_transfer_limit_bytes: if allow { limits.2 } else { None },
                    monthly_transfer_limit_bytes: if allow { limits.3 } else { None },
                }
            },
        )
    }

    fn arb_policy() -> impl Strategy<Value = AccessPolicy> {
        arb_action().prop_flat_map(|action| {
            let allow = action == Action::Allow;
            (
                Just(action),
                prop_oneof![Just(PolicyMode::Enforce), Just(PolicyMode::Monitor)],
                prop::sample::subsequence(vec!["alice", "Bob"], 0..2),
                prop::sample::subsequence(GROUPS.to_vec(), 0..2),
                arb_destinations(),
                arb_ports(),
                arb_protocols(),
                any::<bool>(),
                arb_conditions(allow),
            )
                .prop_map(
                    |(
                        action,
                        mode,
                        users,
                        groups,
                        destinations,
                        ports,
                        protocols,
                        gate,
                        conditions,
                    )| {
                        AccessPolicy {
                            id: String::new(),
                            enabled: true,
                            enforce_conditions: gate && action == Action::Allow,
                            mode,
                            description: String::new(),
                            users: users.into_iter().map(String::from).collect(),
                            groups: groups.into_iter().map(String::from).collect(),
                            action,
                            destinations,
                            ports,
                            protocols,
                            priority: 0,
                            conditions,
                            owner: None,
                            ticket: None,
                            tags: vec![],
                        }
                    },
                )
        })
    }

    fn arb_config() -> impl Strategy<Value = AclConfig> {
        (
            arb_action(),
            prop::collection::vec(arb_rule(), 0..5),
            prop::sample::subsequence(GROUPS.to_vec(), 0..3),
            prop::collection::vec(prop::collection::vec(arb_rule(), 0..5), 3),
            prop::collection::vec(arb_policy(), 0..7),
            Just((0u32..64).collect::<Vec<_>>()).prop_shuffle(),
        )
            .prop_map(
                |(default, user_rules, user_groups, group_rules, policies, priorities)| {
                    // Unique priorities: ties are an explicit, separately tested rule.
                    let mut next = priorities.into_iter();
                    let mut take = || next.next().unwrap() * 10;

                    let mut user_rules = user_rules;
                    user_rules.iter_mut().for_each(|r| r.priority = take());

                    let groups: Vec<GroupAcl> = GROUPS
                        .iter()
                        .zip(group_rules)
                        .map(|(name, mut rules)| {
                            rules.iter_mut().for_each(|r| r.priority = take());
                            GroupAcl {
                                name: name.to_string(),
                                rules,
                            }
                        })
                        .collect();

                    let mut policies = policies;
                    for (i, policy) in policies.iter_mut().enumerate() {
                        policy.id = format!("p{i}");
                        policy.description = format!("policy {i}");
                        policy.priority = take();
                    }

                    let mut config = AclConfig {
                        users: vec![UserAcl {
                            username: "alice".to_string(),
                            groups: user_groups.into_iter().map(String::from).collect(),
                            rules: user_rules,
                        }],
                        groups,
                        policies,
                        ..Default::default()
                    };
                    config.global.default_policy = default;
                    // Descriptions identify which rule matched.
                    for (i, rule) in config.users[0].rules.iter_mut().enumerate() {
                        rule.description = format!("user rule {i}");
                    }
                    for group in &mut config.groups {
                        for (i, rule) in group.rules.iter_mut().enumerate() {
                            rule.description = format!("{} rule {i}", group.name);
                        }
                    }
                    config
                },
            )
    }

    #[derive(Debug, Clone)]
    struct Request {
        user: &'static str,
        groups: Vec<String>,
        source_ip: std::net::IpAddr,
        auth_method: &'static str,
        destination: Address,
        port: u16,
        protocol: Protocol,
        now: chrono::DateTime<Utc>,
        usage: PolicyUsageSnapshot,
    }

    fn arb_request() -> impl Strategy<Value = Request> {
        let destination = prop_oneof![
            Just(Address::Domain("example.com".into())),
            Just(Address::Domain("API.Example.com".into())),
            Just(Address::Domain("x.example.com".into())),
            Just(Address::Domain("192.168.1.5".into())),
            Just(Address::Domain("evil.org".into())),
            Just(Address::Domain("a.evil.org".into())),
            Just(Address::IPv4([10, 1, 2, 3])),
            Just(Address::IPv4([192, 168, 1, 5])),
            Just(Address::IPv4([8, 8, 8, 8])),
            Just(Address::IPv6([
                0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1
            ])),
        ];
        let now = prop::sample::select(vec![
            Utc.with_ymd_and_hms(2026, 9, 8, 10, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 9, 12, 23, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 9, 14, 3, 30, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 8, 30, 12, 0, 0).unwrap(),
        ]);
        (
            prop::sample::select(vec!["alice", "Bob", "carol"]),
            prop::sample::subsequence(vec!["dev", "DEV", "Ops", "qa", "other"], 0..4),
            prop::sample::select(vec!["10.1.1.1", "192.168.9.9", "8.8.8.8"]),
            prop::sample::select(vec!["none", "userpass", "gssapi"]),
            destination,
            prop::sample::select(vec![443u16, 80, 85, 22, 25]),
            prop_oneof![Just(Protocol::Tcp), Just(Protocol::Udp)],
            now,
            (0u32..5, 0u32..5, 0u64..1000, 0u64..1000),
        )
            .prop_map(
                |(user, groups, ip, auth, destination, port, protocol, now, usage)| Request {
                    user,
                    groups: groups.into_iter().map(String::from).collect(),
                    source_ip: ip.parse().unwrap(),
                    auth_method: auth,
                    destination,
                    port,
                    protocol,
                    now,
                    usage: PolicyUsageSnapshot {
                        active_connections: usage.0,
                        connections_last_minute: usage.1,
                        bytes_today: usage.2,
                        bytes_this_month: usage.3,
                    },
                },
            )
    }

    fn context(request: &Request) -> PolicyEvaluationContext<'_> {
        PolicyEvaluationContext {
            user: request.user,
            groups: &request.groups,
            source_ip: request.source_ip,
            auth_method: request.auth_method,
            destination: &request.destination,
            port: request.port,
            protocol: &request.protocol,
            now: request.now,
            usage: request.usage.clone(),
        }
    }

    fn reference_source(reference: &ReferenceOutcome) -> DecisionSource {
        if reference.matched_policy_id.is_some() {
            DecisionSource::Policy
        } else if reference
            .trace
            .last()
            .is_some_and(|entry| entry.effective && entry.source == "legacy_acl")
        {
            DecisionSource::LegacyAcl
        } else {
            DecisionSource::Default
        }
    }

    fn reference_monitor_matches(reference: &ReferenceOutcome) -> u32 {
        reference
            .trace
            .iter()
            .filter(|entry| {
                entry.mode == Some(PolicyMode::Monitor)
                    && entry.target_matched
                    && entry.conditions_matched == Some(true)
            })
            .count() as u32
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]

        #[test]
        fn optimized_evaluation_matches_the_reference(
            config in arb_config(),
            requests in prop::collection::vec(arb_request(), 1..12),
        ) {
            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let engine = AclEngine::new(config).unwrap();

            for request in &requests {
                let reference = rt.block_on(engine.evaluate_policy_reference(context(request)));
                let explained = rt.block_on(engine.evaluate_policy_with_context(context(request)));
                let fast = rt.block_on(engine.evaluate_policy_for_traffic(context(request)));

                // The explained path must be identical to the reference, trace included.
                prop_assert_eq!(&explained.decision, &reference.decision);
                prop_assert_eq!(&explained.matched_rule, &reference.matched_rule);
                prop_assert_eq!(&explained.matched_policy_id, &reference.matched_policy_id);
                prop_assert_eq!(&explained.admission_limits, &reference.admission_limits);
                prop_assert_eq!(format!("{:?}", explained.trace), format!("{:?}", reference.trace));
                prop_assert_eq!(explained.source, reference_source(&reference));
                prop_assert_eq!(explained.monitor_matches, reference_monitor_matches(&reference));

                // The traffic path must reach the same decision without building a trace.
                prop_assert_eq!(&fast.decision, &reference.decision);
                prop_assert_eq!(&fast.matched_rule, &reference.matched_rule);
                prop_assert_eq!(&fast.matched_policy_id, &reference.matched_policy_id);
                prop_assert_eq!(&fast.admission_limits, &reference.admission_limits);
                prop_assert_eq!(fast.source, explained.source);
                prop_assert_eq!(fast.monitor_matches, explained.monitor_matches);
                prop_assert!(fast.trace.is_empty());

                // Post-DNS check agrees with the explicit-block definition.
                let explicit = rt.block_on(engine.is_explicitly_blocked_with_policy_context(context(request)));
                let expected = (reference.decision == AclDecision::Block
                    && reference.trace.last().is_some_and(|entry| entry.effective))
                    .then(|| reference.matched_rule.clone())
                    .flatten();
                prop_assert_eq!(explicit, expected);
            }
        }
    }

    fn rule(action: Action, priority: u32, description: &str) -> AclRule {
        AclRule {
            action,
            description: description.to_string(),
            destinations: vec!["*".to_string()],
            ports: vec!["*".to_string()],
            protocols: vec![Protocol::Both],
            priority,
        }
    }

    fn gate_policy(priority: u32) -> AccessPolicy {
        AccessPolicy {
            id: "gate".to_string(),
            enabled: true,
            mode: PolicyMode::Enforce,
            description: "gate".to_string(),
            users: vec![],
            groups: vec![],
            action: Action::Allow,
            destinations: vec!["*".to_string()],
            ports: vec!["*".to_string()],
            protocols: vec![Protocol::Both],
            priority,
            enforce_conditions: true,
            conditions: PolicyConditions {
                auth_methods: vec!["gssapi".to_string()],
                ..Default::default()
            },
            owner: None,
            ticket: None,
            tags: vec![],
        }
    }

    #[tokio::test]
    async fn equal_priority_and_action_prefers_the_policy_so_a_gate_cannot_be_bypassed() {
        let config = AclConfig {
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec![],
                rules: vec![rule(Action::Allow, 100, "legacy allow")],
            }],
            policies: vec![gate_policy(100)],
            ..Default::default()
        };
        let engine = AclEngine::new(config).unwrap();
        let destination = Address::Domain("example.com".into());
        let ctx = |auth: &'static str| PolicyEvaluationContext {
            user: "alice",
            groups: &[],
            source_ip: "10.0.0.1".parse().unwrap(),
            auth_method: auth,
            destination: &destination,
            port: 443,
            protocol: &Protocol::Tcp,
            now: Utc::now(),
            usage: PolicyUsageSnapshot::default(),
        };

        // Gate condition (gssapi) fails: the equally-ranked legacy ALLOW must not win.
        let denied = engine.evaluate_policy_for_traffic(ctx("userpass")).await;
        assert_eq!(denied.decision, AclDecision::Block);
        assert_eq!(denied.source, DecisionSource::Policy);

        let allowed = engine.evaluate_policy_for_traffic(ctx("gssapi")).await;
        assert_eq!(allowed.decision, AclDecision::Allow);
        assert_eq!(allowed.matched_policy_id.as_deref(), Some("gate"));
    }

    #[tokio::test]
    async fn equal_priority_block_still_beats_allow_across_sources() {
        let config = AclConfig {
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec![],
                rules: vec![rule(Action::Block, 100, "legacy block")],
            }],
            policies: vec![{
                let mut p = gate_policy(100);
                p.enforce_conditions = false;
                p.conditions = PolicyConditions::default();
                p
            }],
            ..Default::default()
        };
        let engine = AclEngine::new(config).unwrap();
        let destination = Address::Domain("example.com".into());
        let outcome = engine
            .evaluate_policy_for_traffic(PolicyEvaluationContext {
                user: "alice",
                groups: &[],
                source_ip: "10.0.0.1".parse().unwrap(),
                auth_method: "none",
                destination: &destination,
                port: 443,
                protocol: &Protocol::Tcp,
                now: Utc::now(),
                usage: PolicyUsageSnapshot::default(),
            })
            .await;
        assert_eq!(outcome.decision, AclDecision::Block);
        assert_eq!(outcome.matched_rule.as_deref(), Some("legacy block"));
    }
}
