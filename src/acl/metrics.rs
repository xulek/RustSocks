//! Prometheus metrics for ACL/policy decisions, admission limits and authentication failures.
//!
//! No per-user labels are used on purpose: usernames can be attacker controlled
//! (e.g. unauthenticated SOCKS4), which would allow unbounded label cardinality.

use super::policy::PolicyEvaluationOutcome;

#[cfg(feature = "metrics")]
mod enabled {
    use super::*;

    /// Decision source label values.
    pub(crate) const USAGE_STORE_OPS: [&str; 5] =
        ["reserve", "snapshot", "release", "heartbeat", "flush"];
    pub(crate) const SOURCES: [&str; 4] = ["legacy_acl", "policy", "default", "post_dns"];
    /// Admission limit reasons recorded by the usage tracker.
    const ADMISSION_REASONS: [&str; 4] = [
        "active_connections",
        "connection_rate",
        "daily_quota",
        "monthly_quota",
    ];
    /// Authentication methods reported by `AuthManager::socks_method_name`.
    const AUTH_METHODS: [&str; 5] = ["none", "userpass", "pam.address", "pam.username", "gssapi"];

    /// Classify which part of the engine produced a decision.
    pub(crate) fn decision_source(outcome: &PolicyEvaluationOutcome) -> &'static str {
        if outcome.matched_policy_id.is_some() {
            "policy"
        } else if outcome
            .trace
            .last()
            .is_some_and(|entry| entry.effective && entry.source == "legacy_acl")
        {
            "legacy_acl"
        } else {
            "default"
        }
    }

    /// Number of monitor-mode policies that matched the target and conditions
    /// (i.e. would have applied had they been enforced).
    pub(crate) fn monitor_matches(outcome: &PolicyEvaluationOutcome) -> usize {
        outcome
            .trace
            .iter()
            .filter(|entry| {
                entry.mode == Some(crate::acl::types::PolicyMode::Monitor)
                    && entry.target_matched
                    && entry.conditions_matched == Some(true)
            })
            .count()
    }

    use lazy_static::lazy_static;
    use prometheus::{
        register_histogram, register_int_counter, register_int_counter_vec, Histogram,
        HistogramOpts, IntCounter, IntCounterVec,
    };

    lazy_static! {
        pub static ref ACL_DECISIONS: IntCounterVec = register_int_counter_vec!(
            "rustsocks_acl_decisions_total",
            "ACL/policy decisions by outcome and deciding source",
            &["decision", "source"]
        )
        .expect("register rustsocks_acl_decisions_total counter vec");
        pub static ref POLICY_MONITOR_MATCHES: IntCounter = register_int_counter!(
            "rustsocks_policy_monitor_matches_total",
            "Monitor-mode policies that would have applied to a request"
        )
        .expect("register rustsocks_policy_monitor_matches_total counter");
        pub static ref POLICY_ADMISSION_DENIED: IntCounterVec = register_int_counter_vec!(
            "rustsocks_policy_admission_denied_total",
            "Sessions denied by policy admission limits and quotas",
            &["reason"]
        )
        .expect("register rustsocks_policy_admission_denied_total counter vec");
        pub static ref AUTH_FAILURES: IntCounterVec = register_int_counter_vec!(
            "rustsocks_socks_auth_failures_total",
            "Failed SOCKS client authentication attempts by method",
            &["method"]
        )
        .expect("register rustsocks_socks_auth_failures_total counter vec");
        pub static ref USAGE_STORE_ERRORS: IntCounterVec = register_int_counter_vec!(
            "rustsocks_policy_usage_store_errors_total",
            "Failed operations against the shared policy usage store, by operation",
            &["op"]
        )
        .expect("register rustsocks_policy_usage_store_errors_total counter vec");
        pub static ref POLICY_EVALUATION: Histogram = register_histogram!(HistogramOpts::new(
            "rustsocks_policy_evaluation_seconds",
            "Time spent evaluating ACL rules and dynamic policies for one request"
        )
        .buckets(vec![
            0.000_005, 0.000_01, 0.000_025, 0.000_05, 0.000_1, 0.000_25, 0.000_5, 0.001, 0.005,
            0.01
        ]))
        .expect("register rustsocks_policy_evaluation_seconds histogram");
    }

    #[derive(Debug, Clone, Copy)]
    pub struct AclMetrics;

    impl AclMetrics {
        /// Record the primary decision taken for a request.
        #[inline]
        pub fn record_outcome(outcome: &PolicyEvaluationOutcome) {
            let decision = match outcome.decision {
                crate::acl::AclDecision::Allow => "allow",
                crate::acl::AclDecision::Block => "block",
            };
            ACL_DECISIONS
                .with_label_values(&[decision, decision_source(outcome)])
                .inc();
            let monitored = monitor_matches(outcome);
            if monitored > 0 {
                POLICY_MONITOR_MATCHES.inc_by(monitored as u64);
            }
        }

        /// Record a block decided after DNS resolution (resolved IP hit an explicit block).
        #[inline]
        pub fn record_post_dns_block() {
            ACL_DECISIONS
                .with_label_values(&["block", "post_dns"])
                .inc();
        }

        #[inline]
        pub fn observe_evaluation(duration_secs: f64) {
            POLICY_EVALUATION.observe(duration_secs);
        }

        #[inline]
        pub fn record_admission_denied(reason: &'static str) {
            POLICY_ADMISSION_DENIED.with_label_values(&[reason]).inc();
        }

        #[inline]
        pub fn record_usage_store_error(op: &'static str) {
            USAGE_STORE_ERRORS.with_label_values(&[op]).inc();
        }

        #[inline]
        pub fn record_auth_failure(method: &str) {
            // Only known method names become label values.
            let method = AUTH_METHODS
                .iter()
                .copied()
                .find(|known| *known == method)
                .unwrap_or("unknown");
            AUTH_FAILURES.with_label_values(&[method]).inc();
        }
    }

    /// Register every collector and instantiate the known label series so `/metrics`
    /// exposes a stable schema (zero values) before the first request.
    pub fn init() {
        lazy_static::initialize(&ACL_DECISIONS);
        lazy_static::initialize(&POLICY_MONITOR_MATCHES);
        lazy_static::initialize(&POLICY_ADMISSION_DENIED);
        lazy_static::initialize(&AUTH_FAILURES);
        lazy_static::initialize(&POLICY_EVALUATION);
        lazy_static::initialize(&USAGE_STORE_ERRORS);
        for op in USAGE_STORE_OPS {
            USAGE_STORE_ERRORS.with_label_values(&[op]).inc_by(0);
        }

        for source in SOURCES {
            for decision in ["allow", "block"] {
                // `post_dns` can only block; skip the impossible combination.
                if source == "post_dns" && decision == "allow" {
                    continue;
                }
                ACL_DECISIONS
                    .with_label_values(&[decision, source])
                    .inc_by(0);
            }
        }
        for reason in ADMISSION_REASONS {
            POLICY_ADMISSION_DENIED
                .with_label_values(&[reason])
                .inc_by(0);
        }
        for method in AUTH_METHODS {
            AUTH_FAILURES.with_label_values(&[method]).inc_by(0);
        }
    }
}

#[cfg(not(feature = "metrics"))]
mod disabled {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    pub struct AclMetrics;

    impl AclMetrics {
        #[inline]
        pub fn record_outcome(_outcome: &PolicyEvaluationOutcome) {}

        #[inline]
        pub fn record_post_dns_block() {}

        #[inline]
        pub fn observe_evaluation(_duration_secs: f64) {}

        #[inline]
        pub fn record_admission_denied(_reason: &'static str) {}

        #[inline]
        pub fn record_usage_store_error(_op: &'static str) {}

        #[inline]
        pub fn record_auth_failure(_method: &str) {}
    }

    #[inline]
    pub fn init() {}
}

#[cfg(not(feature = "metrics"))]
pub use disabled::*;
#[cfg(feature = "metrics")]
pub use enabled::*;

#[cfg(all(test, feature = "metrics"))]
mod tests {
    use super::*;
    use crate::acl::types::{Action, PolicyMode};
    use crate::acl::{AclDecision, PolicyAdmissionLimits, PolicyTraceEntry};
    use prometheus::{Encoder, TextEncoder};

    fn outcome(
        decision: AclDecision,
        policy_id: Option<&str>,
        trace: Vec<PolicyTraceEntry>,
    ) -> PolicyEvaluationOutcome {
        PolicyEvaluationOutcome {
            decision,
            matched_rule: None,
            matched_policy_id: policy_id.map(str::to_string),
            admission_limits: PolicyAdmissionLimits::default(),
            trace,
        }
    }

    fn entry(
        source: &str,
        mode: Option<PolicyMode>,
        conditions: Option<bool>,
        effective: bool,
    ) -> PolicyTraceEntry {
        PolicyTraceEntry {
            source: source.to_string(),
            id: None,
            description: String::new(),
            priority: 0,
            action: Action::Allow,
            mode,
            target_matched: true,
            conditions_matched: conditions,
            effective,
            reason: String::new(),
        }
    }

    #[test]
    fn classifies_decision_source() {
        assert_eq!(
            decision_source(&outcome(AclDecision::Allow, Some("p1"), vec![])),
            "policy"
        );
        assert_eq!(
            decision_source(&outcome(
                AclDecision::Block,
                None,
                vec![entry("legacy_acl", None, None, true)]
            )),
            "legacy_acl"
        );
        assert_eq!(
            decision_source(&outcome(AclDecision::Block, None, vec![])),
            "default"
        );
        // A non-effective legacy trace entry means the default policy decided.
        assert_eq!(
            decision_source(&outcome(
                AclDecision::Block,
                None,
                vec![entry("legacy_acl", None, None, false)]
            )),
            "default"
        );
    }

    #[test]
    fn counts_only_matching_monitor_policies() {
        let trace = vec![
            entry("policy", Some(PolicyMode::Monitor), Some(true), false),
            entry("policy", Some(PolicyMode::Monitor), Some(false), false),
            entry("policy", Some(PolicyMode::Enforce), Some(true), true),
        ];
        assert_eq!(
            monitor_matches(&outcome(AclDecision::Allow, None, trace)),
            1
        );
    }

    #[test]
    fn init_exports_stable_zero_series() {
        init();
        let mut buffer = Vec::new();
        TextEncoder::new()
            .encode(&prometheus::gather(), &mut buffer)
            .unwrap();
        let output = String::from_utf8(buffer).unwrap();

        for needle in [
            "rustsocks_acl_decisions_total{decision=\"block\",source=\"default\"}",
            "rustsocks_acl_decisions_total{decision=\"allow\",source=\"policy\"}",
            "rustsocks_policy_monitor_matches_total",
            "rustsocks_policy_admission_denied_total{reason=\"daily_quota\"}",
            "rustsocks_socks_auth_failures_total{method=\"userpass\"}",
            "rustsocks_policy_evaluation_seconds",
            "rustsocks_policy_usage_store_errors_total{op=\"reserve\"}",
        ] {
            assert!(output.contains(needle), "missing series: {needle}");
        }
    }

    #[test]
    fn recording_increments_counters_and_bounds_auth_labels() {
        let before = AUTH_FAILURES.with_label_values(&["unknown"]).get();
        AclMetrics::record_auth_failure("some-attacker-controlled-value");
        assert_eq!(
            AUTH_FAILURES.with_label_values(&["unknown"]).get(),
            before + 1
        );

        let allow_before = ACL_DECISIONS.with_label_values(&["allow", "policy"]).get();
        AclMetrics::record_outcome(&outcome(AclDecision::Allow, Some("p"), vec![]));
        assert_eq!(
            ACL_DECISIONS.with_label_values(&["allow", "policy"]).get(),
            allow_before + 1
        );
    }
}
