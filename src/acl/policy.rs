use super::matcher::{CompiledAclRule, PreparedDestination};
use super::metrics::AclMetrics;
use super::types::{
    AccessPolicy, AclDecision, Action, PolicyConditions, PolicyMode, PolicySchedule, Protocol,
};
use crate::protocol::Address;
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Timelike, Utc};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyUsageSnapshot {
    pub active_connections: u32,
    pub connections_last_minute: u32,
    pub bytes_today: u64,
    pub bytes_this_month: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyAdmissionLimits {
    pub max_active_connections: Option<u32>,
    pub max_connections_per_minute: Option<u32>,
    pub daily_transfer_limit_bytes: Option<u64>,
    pub monthly_transfer_limit_bytes: Option<u64>,
}

impl PolicyAdmissionLimits {
    pub fn is_empty(&self) -> bool {
        self.max_active_connections.is_none()
            && self.max_connections_per_minute.is_none()
            && self.daily_transfer_limit_bytes.is_none()
            && self.monthly_transfer_limit_bytes.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyTraceEntry {
    pub source: String,
    pub id: Option<String>,
    pub description: String,
    pub priority: u32,
    pub action: Action,
    pub mode: Option<PolicyMode>,
    pub target_matched: bool,
    pub conditions_matched: Option<bool>,
    pub effective: bool,
    pub reason: String,
}

/// Which part of the engine produced a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionSource {
    /// A legacy `[[users]]` / `[[groups]]` rule matched.
    LegacyAcl,
    /// A dynamic policy decided (including a failed gate condition).
    Policy,
    /// Nothing matched; `global.default_policy` applied.
    Default,
}

#[derive(Debug, Clone)]
pub struct PolicyEvaluationOutcome {
    pub decision: AclDecision,
    pub matched_rule: Option<String>,
    pub matched_policy_id: Option<String>,
    pub admission_limits: PolicyAdmissionLimits,
    /// What decided. `Default` means no explicit rule or policy applied.
    pub source: DecisionSource,
    /// Monitor-mode policies whose target and conditions matched (would have applied).
    pub monitor_matches: u32,
    /// Ordered explanation of every candidate considered. Only populated by
    /// [`AclEngine::evaluate_policy_with_context`](super::engine::AclEngine::evaluate_policy_with_context);
    /// the traffic path leaves it empty because building it costs several allocations
    /// per candidate.
    pub trace: Vec<PolicyTraceEntry>,
}

#[derive(Debug, Clone)]
pub struct PolicyEvaluationContext<'a> {
    pub user: &'a str,
    pub groups: &'a [String],
    pub source_ip: IpAddr,
    pub auth_method: &'a str,
    pub destination: &'a Address,
    pub port: u16,
    pub protocol: &'a Protocol,
    pub now: DateTime<Utc>,
    pub usage: PolicyUsageSnapshot,
}

#[derive(Debug, Clone)]
pub struct CompiledAccessPolicy {
    pub policy: AccessPolicy,
    target: CompiledAclRule,
    users_lower: Vec<String>,
    groups_lower: Vec<String>,
    source_networks: Vec<IpNet>,
    auth_methods_lower: Vec<String>,
    schedule: Option<CompiledSchedule>,
}

/// A schedule parsed once at compile time: weekday bitmask (bit 1 = Monday .. bit 7 = Sunday,
/// `0` = every day) and start/end in minutes since midnight.
#[derive(Debug, Clone)]
struct CompiledSchedule {
    days_mask: u8,
    start: u32,
    end: u32,
    utc_offset_minutes: i64,
}

impl CompiledSchedule {
    fn compile(schedule: &PolicySchedule) -> Option<Self> {
        let mut days_mask = 0u8;
        for day in &schedule.days {
            days_mask |= 1 << weekday_number(day)?;
        }
        Some(Self {
            days_mask,
            start: parse_hhmm(&schedule.start)?,
            end: parse_hhmm(&schedule.end)?,
            utc_offset_minutes: i64::from(schedule.utc_offset_minutes),
        })
    }

    fn matches(&self, now: DateTime<Utc>) -> bool {
        let local = now + ChronoDuration::minutes(self.utc_offset_minutes);
        if self.days_mask != 0 && self.days_mask & (1 << local.weekday().number_from_monday()) == 0
        {
            return false;
        }
        let current = local.hour() * 60 + local.minute();
        if self.start < self.end {
            current >= self.start && current < self.end
        } else {
            current >= self.start || current < self.end
        }
    }
}

impl CompiledAccessPolicy {
    pub fn compile(policy: &AccessPolicy) -> Result<Self, String> {
        let target = CompiledAclRule::compile(&super::types::AclRule {
            action: policy.action.clone(),
            description: policy.description.clone(),
            destinations: policy.destinations.clone(),
            ports: policy.ports.clone(),
            protocols: policy.protocols.clone(),
            priority: policy.priority,
        })?;

        let mut source_networks = Vec::new();
        for value in &policy.conditions.source_ips {
            if let Ok(net) = value.parse::<IpNet>() {
                source_networks.push(net);
                continue;
            }
            let ip = value.parse::<IpAddr>().map_err(|_| {
                format!("Policy '{}': invalid source IP/CIDR '{}'", policy.id, value)
            })?;
            let suffix = if ip.is_ipv4() { 32 } else { 128 };
            source_networks.push(format!("{}/{}", ip, suffix).parse::<IpNet>().map_err(|e| {
                format!(
                    "Policy '{}': invalid source IP '{}': {}",
                    policy.id, value, e
                )
            })?);
        }

        let schedule = match &policy.conditions.schedule {
            Some(schedule) => {
                validate_schedule(&policy.id, schedule)?;
                CompiledSchedule::compile(schedule)
            }
            None => None,
        };

        Ok(Self {
            users_lower: policy
                .users
                .iter()
                .map(|v| v.to_ascii_lowercase())
                .collect(),
            groups_lower: policy
                .groups
                .iter()
                .map(|v| v.to_ascii_lowercase())
                .collect(),
            auth_methods_lower: policy
                .conditions
                .auth_methods
                .iter()
                .map(|v| v.to_ascii_lowercase())
                .collect(),
            policy: policy.clone(),
            target,
            source_networks,
            schedule,
        })
    }

    pub fn subject_matches(&self, user: &str, groups: &[String]) -> bool {
        let user_lower = user.to_ascii_lowercase();
        let groups_lower: Vec<String> = groups.iter().map(|g| g.to_ascii_lowercase()).collect();
        self.subject_matches_lowered(&user_lower, &groups_lower)
    }

    /// Whether the policy names any group (so group membership must be resolved to evaluate it).
    pub fn has_group_subjects(&self) -> bool {
        !self.groups_lower.is_empty()
    }

    /// Subject match for an already lowercased user and group list (the hot path).
    pub fn subject_matches_lowered(&self, user_lower: &str, groups_lower: &[String]) -> bool {
        if self.users_lower.is_empty() && self.groups_lower.is_empty() {
            return true;
        }
        if self
            .users_lower
            .iter()
            .any(|candidate| candidate == user_lower)
        {
            return true;
        }
        groups_lower
            .iter()
            .any(|group| self.groups_lower.iter().any(|candidate| candidate == group))
    }

    pub fn target_matches(&self, dest: &Address, port: u16, protocol: &Protocol) -> bool {
        self.target.matches(dest, port, protocol)
    }

    /// Target match for a destination analysed once per request (the hot path).
    pub fn target_matches_prepared(
        &self,
        dest: &PreparedDestination<'_>,
        port: u16,
        protocol: &Protocol,
    ) -> bool {
        self.target.matches_prepared(dest, port, protocol)
    }

    /// Evaluate the dynamic conditions and explain the result.
    pub fn conditions_match(&self, ctx: &PolicyEvaluationContext<'_>) -> (bool, String) {
        self.check_conditions(ctx, true)
    }

    /// Evaluate the dynamic conditions without building an explanation (the hot path).
    pub fn conditions_hold(&self, ctx: &PolicyEvaluationContext<'_>) -> bool {
        self.check_conditions(ctx, false).0
    }

    /// Shared implementation. The reason string is only built when `want_reason` is set, so
    /// the traffic path does not allocate.
    fn check_conditions(
        &self,
        ctx: &PolicyEvaluationContext<'_>,
        want_reason: bool,
    ) -> (bool, String) {
        let conditions = &self.policy.conditions;
        let fail = |reason: &dyn Fn() -> String| {
            (false, if want_reason { reason() } else { String::new() })
        };

        if !self.source_networks.is_empty()
            && !self
                .source_networks
                .iter()
                .any(|net| net.contains(&ctx.source_ip))
        {
            return fail(&|| {
                format!(
                    "source IP {} is outside allowed source networks",
                    ctx.source_ip
                )
            });
        }

        if !self.auth_methods_lower.is_empty()
            && !self
                .auth_methods_lower
                .iter()
                .any(|method| method.eq_ignore_ascii_case(ctx.auth_method))
        {
            return fail(&|| format!("authentication method '{}' is not allowed", ctx.auth_method));
        }

        if let Some(not_before) = conditions.not_before {
            if ctx.now < not_before {
                return fail(&|| format!("policy is not active before {}", not_before));
            }
        }

        if let Some(expires_at) = conditions.expires_at {
            if ctx.now >= expires_at {
                return fail(&|| format!("policy expired at {}", expires_at));
            }
        }

        if let Some(schedule) = &self.schedule {
            if !schedule.matches(ctx.now) {
                return fail(&|| "current time is outside policy schedule".to_string());
            }
        }

        if let Some(limit) = conditions.max_active_connections {
            if ctx.usage.active_connections >= limit {
                return fail(&|| {
                    format!(
                        "active connection limit reached ({}/{})",
                        ctx.usage.active_connections, limit
                    )
                });
            }
        }

        if let Some(limit) = conditions.max_connections_per_minute {
            if ctx.usage.connections_last_minute >= limit {
                return fail(&|| {
                    format!(
                        "connection rate limit reached ({}/{}/min)",
                        ctx.usage.connections_last_minute, limit
                    )
                });
            }
        }

        if let Some(limit) = conditions.daily_transfer_limit_bytes {
            if ctx.usage.bytes_today >= limit {
                return fail(&|| {
                    format!(
                        "daily transfer quota reached ({}/{})",
                        ctx.usage.bytes_today, limit
                    )
                });
            }
        }

        if let Some(limit) = conditions.monthly_transfer_limit_bytes {
            if ctx.usage.bytes_this_month >= limit {
                return fail(&|| {
                    format!(
                        "monthly transfer quota reached ({}/{})",
                        ctx.usage.bytes_this_month, limit
                    )
                });
            }
        }

        (
            true,
            if want_reason {
                "all dynamic conditions matched".to_string()
            } else {
                String::new()
            },
        )
    }

    pub fn admission_limits(&self) -> PolicyAdmissionLimits {
        PolicyAdmissionLimits {
            max_active_connections: self.policy.conditions.max_active_connections,
            max_connections_per_minute: self.policy.conditions.max_connections_per_minute,
            daily_transfer_limit_bytes: self.policy.conditions.daily_transfer_limit_bytes,
            monthly_transfer_limit_bytes: self.policy.conditions.monthly_transfer_limit_bytes,
        }
    }
}

fn validate_schedule(policy_id: &str, schedule: &PolicySchedule) -> Result<(), String> {
    let start = parse_hhmm(&schedule.start).ok_or_else(|| {
        format!(
            "Policy '{}': invalid schedule start '{}'",
            policy_id, schedule.start
        )
    })?;
    let end = parse_hhmm(&schedule.end).ok_or_else(|| {
        format!(
            "Policy '{}': invalid schedule end '{}'",
            policy_id, schedule.end
        )
    })?;
    if start == end {
        return Err(format!(
            "Policy '{}': schedule start and end cannot be identical",
            policy_id
        ));
    }
    if !(-24 * 60..=24 * 60).contains(&schedule.utc_offset_minutes) {
        return Err(format!(
            "Policy '{}': utc_offset_minutes must be between -1440 and 1440",
            policy_id
        ));
    }
    for day in &schedule.days {
        if weekday_number(day).is_none() {
            return Err(format!(
                "Policy '{}': invalid schedule day '{}'",
                policy_id, day
            ));
        }
    }
    Ok(())
}

fn parse_hhmm(value: &str) -> Option<u32> {
    let mut parts = value.split(':');
    let hour = parts.next()?.parse::<u32>().ok()?;
    let minute = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    Some(hour * 60 + minute)
}

fn weekday_number(value: &str) -> Option<u32> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mon" | "monday" => Some(1),
        "tue" | "tues" | "tuesday" => Some(2),
        "wed" | "wednesday" => Some(3),
        "thu" | "thur" | "thurs" | "thursday" => Some(4),
        "fri" | "friday" => Some(5),
        "sat" | "saturday" => Some(6),
        "sun" | "sunday" => Some(7),
        _ => None,
    }
}

#[derive(Debug)]
struct UserPolicyUsage {
    active_connections: u32,
    connection_times: VecDeque<DateTime<Utc>>,
    day_key: (i32, u32),
    month_key: (i32, u32),
    bytes_today: u64,
    bytes_this_month: u64,
    last_seen: DateTime<Utc>,
}

impl UserPolicyUsage {
    fn new(now: DateTime<Utc>) -> Self {
        Self {
            active_connections: 0,
            connection_times: VecDeque::new(),
            day_key: (now.year(), now.ordinal()),
            month_key: (now.year(), now.month()),
            bytes_today: 0,
            bytes_this_month: 0,
            last_seen: now,
        }
    }

    fn normalize(&mut self, now: DateTime<Utc>) {
        let day_key = (now.year(), now.ordinal());
        if self.day_key != day_key {
            self.day_key = day_key;
            self.bytes_today = 0;
        }
        let month_key = (now.year(), now.month());
        if self.month_key != month_key {
            self.month_key = month_key;
            self.bytes_this_month = 0;
        }
        let cutoff = now - ChronoDuration::seconds(60);
        while self
            .connection_times
            .front()
            .is_some_and(|timestamp| *timestamp < cutoff)
        {
            self.connection_times.pop_front();
        }
        self.last_seen = now;
    }

    fn snapshot(&self) -> PolicyUsageSnapshot {
        PolicyUsageSnapshot {
            active_connections: self.active_connections,
            connections_last_minute: self.connection_times.len().min(u32::MAX as usize) as u32,
            bytes_today: self.bytes_today,
            bytes_this_month: self.bytes_this_month,
        }
    }
}

#[derive(Debug)]
pub struct PolicyUsageTracker {
    users: Mutex<HashMap<String, UserPolicyUsage>>,
    max_users: usize,
}

impl Default for PolicyUsageTracker {
    fn default() -> Self {
        Self::new(100_000)
    }
}

impl PolicyUsageTracker {
    pub fn new(max_users: usize) -> Self {
        Self {
            users: Mutex::new(HashMap::new()),
            max_users: max_users.max(1),
        }
    }

    fn key(user: &str) -> String {
        user.to_ascii_lowercase()
    }

    pub fn snapshot(&self, user: &str, now: DateTime<Utc>) -> PolicyUsageSnapshot {
        let mut users = self
            .users
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Read-only: never create entries, so lookups for arbitrary names
        // (e.g. the ACL test endpoint) cannot grow the tracker.
        match users.get_mut(&Self::key(user)) {
            Some(entry) => {
                entry.normalize(now);
                entry.snapshot()
            }
            None => PolicyUsageSnapshot::default(),
        }
    }

    pub fn reserve(
        self: &Arc<Self>,
        user: &str,
        limits: &PolicyAdmissionLimits,
        now: DateTime<Utc>,
    ) -> Result<PolicyAdmissionGuard, String> {
        let key = Self::key(user);
        let mut users = self
            .users
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = users
            .entry(key.clone())
            .or_insert_with(|| UserPolicyUsage::new(now));
        entry.normalize(now);

        if let Some(limit) = limits.max_active_connections {
            if entry.active_connections >= limit {
                AclMetrics::record_admission_denied("active_connections");
                return Err(format!(
                    "active connection limit reached ({}/{})",
                    entry.active_connections, limit
                ));
            }
        }
        if let Some(limit) = limits.max_connections_per_minute {
            if entry.connection_times.len() >= limit as usize {
                AclMetrics::record_admission_denied("connection_rate");
                return Err(format!(
                    "connection rate limit reached ({}/{}/min)",
                    entry.connection_times.len(),
                    limit
                ));
            }
        }
        if let Some(limit) = limits.daily_transfer_limit_bytes {
            if entry.bytes_today >= limit {
                AclMetrics::record_admission_denied("daily_quota");
                return Err(format!(
                    "daily transfer quota reached ({}/{})",
                    entry.bytes_today, limit
                ));
            }
        }
        if let Some(limit) = limits.monthly_transfer_limit_bytes {
            if entry.bytes_this_month >= limit {
                AclMetrics::record_admission_denied("monthly_quota");
                return Err(format!(
                    "monthly transfer quota reached ({}/{})",
                    entry.bytes_this_month, limit
                ));
            }
        }

        entry.active_connections = entry.active_connections.saturating_add(1);
        entry.connection_times.push_back(now);
        drop(users);
        self.prune(now);

        Ok(PolicyAdmissionGuard::new(
            Arc::clone(self) as Arc<dyn ReleaseHandle>,
            key,
            0,
        ))
    }

    pub fn record_transfer(&self, user: &str, bytes: u64, now: DateTime<Utc>) {
        if bytes == 0 {
            return;
        }
        let mut users = self
            .users
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = users
            .entry(Self::key(user))
            .or_insert_with(|| UserPolicyUsage::new(now));
        entry.normalize(now);
        entry.bytes_today = entry.bytes_today.saturating_add(bytes);
        entry.bytes_this_month = entry.bytes_this_month.saturating_add(bytes);
        drop(users);
        self.prune(now);
    }

    fn release(&self, user_key: &str) {
        let mut users = self
            .users
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = users.get_mut(user_key) {
            entry.active_connections = entry.active_connections.saturating_sub(1);
            entry.last_seen = Utc::now();
        }
    }

    fn prune(&self, now: DateTime<Utc>) {
        let mut users = self
            .users
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if users.len() <= self.max_users {
            return;
        }
        let stale_before = now - ChronoDuration::hours(1);
        users.retain(|_, usage| {
            usage.normalize(now);
            usage.active_connections > 0
                || !usage.connection_times.is_empty()
                || usage.bytes_today > 0
                || usage.bytes_this_month > 0
                || usage.last_seen >= stale_before
        });
    }
}

/// Releases the capacity held by an admission reservation.
///
/// Implemented by every usage backend so [`PolicyAdmissionGuard`] does not depend on
/// where the counters live (in memory or in a store shared by several instances).
/// `release_reservation` is called from `Drop` and therefore must not block.
pub trait ReleaseHandle: Send + Sync + std::fmt::Debug {
    fn release_reservation(&self, user_key: &str, reservation_id: u64);
}

impl ReleaseHandle for PolicyUsageTracker {
    fn release_reservation(&self, user_key: &str, _reservation_id: u64) {
        self.release(user_key);
    }
}

/// Holds one admitted connection against a user's policy limits until dropped.
#[derive(Debug)]
pub struct PolicyAdmissionGuard {
    releaser: Arc<dyn ReleaseHandle>,
    user_key: String,
    reservation_id: u64,
    released: bool,
}

impl PolicyAdmissionGuard {
    pub(crate) fn new(
        releaser: Arc<dyn ReleaseHandle>,
        user_key: String,
        reservation_id: u64,
    ) -> Self {
        Self {
            releaser,
            user_key,
            reservation_id,
            released: false,
        }
    }

    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if !self.released {
            self.releaser
                .release_reservation(&self.user_key, self.reservation_id);
            self.released = true;
        }
    }
}

impl Drop for PolicyAdmissionGuard {
    fn drop(&mut self) {
        self.release_inner();
    }
}

pub fn validate_policy(policy: &AccessPolicy) -> Result<(), String> {
    if policy.id.trim().is_empty() {
        return Err("Policy id cannot be empty".to_string());
    }
    if policy.destinations.is_empty() {
        return Err(format!(
            "Policy '{}': destinations cannot be empty",
            policy.id
        ));
    }
    if policy.ports.is_empty() {
        return Err(format!("Policy '{}': ports cannot be empty", policy.id));
    }
    if policy.protocols.is_empty() {
        return Err(format!("Policy '{}': protocols cannot be empty", policy.id));
    }
    if policy.enforce_conditions && policy.action != Action::Allow {
        return Err(format!(
            "Policy '{}': enforce_conditions is supported only for allow policies",
            policy.id
        ));
    }
    if policy.action == Action::Block && has_admission_limits(&policy.conditions) {
        return Err(format!(
            "Policy '{}': admission/quota limits are only meaningful on allow policies",
            policy.id
        ));
    }
    if let (Some(start), Some(end)) = (policy.conditions.not_before, policy.conditions.expires_at) {
        if end <= start {
            return Err(format!(
                "Policy '{}': expires_at must be later than not_before",
                policy.id
            ));
        }
    }
    if policy.conditions.max_active_connections == Some(0)
        || policy.conditions.max_connections_per_minute == Some(0)
        || policy.conditions.daily_transfer_limit_bytes == Some(0)
        || policy.conditions.monthly_transfer_limit_bytes == Some(0)
    {
        return Err(format!(
            "Policy '{}': limits must be greater than zero",
            policy.id
        ));
    }
    CompiledAccessPolicy::compile(policy).map(|_| ())
}

fn has_admission_limits(conditions: &PolicyConditions) -> bool {
    conditions.max_active_connections.is_some()
        || conditions.max_connections_per_minute.is_some()
        || conditions.daily_transfer_limit_bytes.is_some()
        || conditions.monthly_transfer_limit_bytes.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker_len(tracker: &PolicyUsageTracker) -> usize {
        tracker.users.lock().unwrap().len()
    }

    #[test]
    fn snapshot_of_unknown_user_does_not_create_entry() {
        let tracker = PolicyUsageTracker::default();
        let now = Utc::now();

        assert_eq!(
            tracker.snapshot("nobody", now),
            PolicyUsageSnapshot::default()
        );
        assert_eq!(
            tracker.snapshot("someone-else", now),
            PolicyUsageSnapshot::default()
        );
        assert_eq!(tracker_len(&tracker), 0);
    }

    #[test]
    fn snapshot_reflects_reservation_and_release() {
        let tracker = Arc::new(PolicyUsageTracker::default());
        let now = Utc::now();
        let limits = PolicyAdmissionLimits::default();

        let guard = tracker.reserve("Alice", &limits, now).unwrap();
        let snap = tracker.snapshot("alice", now);
        assert_eq!(snap.active_connections, 1);
        assert_eq!(snap.connections_last_minute, 1);

        drop(guard);
        let snap = tracker.snapshot("ALICE", now);
        assert_eq!(snap.active_connections, 0);
        assert_eq!(snap.connections_last_minute, 1);
        assert_eq!(tracker_len(&tracker), 1);
    }

    #[test]
    fn snapshot_reports_recorded_transfer_and_resets_after_day_change() {
        let tracker = PolicyUsageTracker::default();
        let now = Utc::now();

        tracker.record_transfer("bob", 500, now);
        let snap = tracker.snapshot("bob", now);
        assert_eq!(snap.bytes_today, 500);
        assert_eq!(snap.bytes_this_month, 500);

        let next_day = now + ChronoDuration::days(1);
        let snap = tracker.snapshot("bob", next_day);
        assert_eq!(snap.bytes_today, 0);
    }

    #[test]
    fn reserve_enforces_active_connection_limit() {
        let tracker = Arc::new(PolicyUsageTracker::default());
        let now = Utc::now();
        let limits = PolicyAdmissionLimits {
            max_active_connections: Some(1),
            ..Default::default()
        };

        let _guard = tracker.reserve("carol", &limits, now).unwrap();
        assert!(tracker.reserve("carol", &limits, now).is_err());
    }
}
