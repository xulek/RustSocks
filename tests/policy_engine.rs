use chrono::{TimeZone, Utc};
use rustsocks::acl::{
    AccessPolicy, AclConfig, AclDecision, AclEngine, Action, PolicyConditions,
    PolicyEvaluationContext, PolicyMode, PolicySchedule, PolicyUsageSnapshot, PolicyUsageTracker,
    Protocol,
};
use rustsocks::protocol::Address;
use std::sync::Arc;

fn policy(id: &str, action: Action, priority: u32) -> AccessPolicy {
    AccessPolicy {
        id: id.to_string(),
        enabled: true,
        mode: PolicyMode::Enforce,
        description: id.to_string(),
        users: vec!["alice".to_string()],
        groups: vec![],
        action,
        destinations: vec!["example.com".to_string()],
        ports: vec!["443".to_string()],
        protocols: vec![Protocol::Tcp],
        priority,
        enforce_conditions: false,
        conditions: PolicyConditions::default(),
        owner: None,
        ticket: None,
        tags: vec![],
    }
}

async fn evaluate(
    engine: &AclEngine,
    usage: PolicyUsageSnapshot,
) -> rustsocks::acl::PolicyEvaluationOutcome {
    let destination = Address::Domain("example.com".to_string());
    engine
        .evaluate_policy_with_context(PolicyEvaluationContext {
            user: "alice",
            groups: &[],
            source_ip: "10.1.2.3".parse().unwrap(),
            auth_method: "userpass",
            destination: &destination,
            port: 443,
            protocol: &Protocol::Tcp,
            now: Utc.with_ymd_and_hms(2026, 9, 8, 10, 0, 0).unwrap(),
            usage,
        })
        .await
}

#[tokio::test]
async fn higher_priority_policy_wins_in_shared_priority_space() {
    let mut low_block = policy("low-block", Action::Block, 100);
    low_block.users.clear();
    let high_allow = policy("high-allow", Action::Allow, 200);
    let config = AclConfig {
        policies: vec![low_block, high_allow],
        ..Default::default()
    };
    let engine = AclEngine::new(config).unwrap();
    let outcome = evaluate(&engine, PolicyUsageSnapshot::default()).await;
    assert_eq!(outcome.decision, AclDecision::Allow);
    assert_eq!(outcome.matched_policy_id.as_deref(), Some("high-allow"));
}

#[tokio::test]
async fn monitor_policy_does_not_change_effective_decision() {
    let mut monitor = policy("observe-block", Action::Block, 5000);
    monitor.mode = PolicyMode::Monitor;
    let allow = policy("actual-allow", Action::Allow, 1000);
    let config = AclConfig {
        policies: vec![monitor, allow],
        ..Default::default()
    };
    let engine = AclEngine::new(config).unwrap();
    let outcome = evaluate(&engine, PolicyUsageSnapshot::default()).await;
    assert_eq!(outcome.decision, AclDecision::Allow);
    assert!(outcome
        .trace
        .iter()
        .any(|entry| entry.id.as_deref() == Some("observe-block") && !entry.effective));
}

#[tokio::test]
async fn gate_policy_blocks_when_dynamic_condition_fails() {
    let mut gated = policy("business-hours", Action::Allow, 2000);
    gated.enforce_conditions = true;
    gated.conditions.schedule = Some(PolicySchedule {
        days: vec!["mon".to_string()],
        start: "09:00".to_string(),
        end: "17:00".to_string(),
        utc_offset_minutes: 0,
    });
    let config = AclConfig {
        policies: vec![gated],
        ..Default::default()
    };
    let engine = AclEngine::new(config).unwrap();
    let destination = Address::Domain("example.com".to_string());
    let outcome = engine
        .evaluate_policy_with_context(PolicyEvaluationContext {
            user: "alice",
            groups: &[],
            source_ip: "10.1.2.3".parse().unwrap(),
            auth_method: "userpass",
            destination: &destination,
            port: 443,
            protocol: &Protocol::Tcp,
            now: Utc.with_ymd_and_hms(2026, 9, 8, 10, 0, 0).unwrap(), // Tuesday
            usage: PolicyUsageSnapshot::default(),
        })
        .await;
    assert_eq!(outcome.decision, AclDecision::Block);
    assert_eq!(outcome.matched_policy_id.as_deref(), Some("business-hours"));
}

#[tokio::test]
async fn quota_limit_is_visible_in_explain_trace() {
    let mut quota = policy("daily-quota", Action::Allow, 1000);
    quota.enforce_conditions = true;
    quota.conditions.daily_transfer_limit_bytes = Some(1_000);
    let config = AclConfig {
        policies: vec![quota],
        ..Default::default()
    };
    let engine = AclEngine::new(config).unwrap();
    let outcome = evaluate(
        &engine,
        PolicyUsageSnapshot {
            bytes_today: 1_000,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(outcome.decision, AclDecision::Block);
    assert!(outcome
        .trace
        .iter()
        .any(|entry| entry.reason.contains("daily transfer quota")));
}

#[test]
fn admission_reservation_is_atomic_for_active_connection_limit() {
    let tracker = Arc::new(PolicyUsageTracker::new(100));
    let limits = rustsocks::acl::PolicyAdmissionLimits {
        max_active_connections: Some(1),
        ..Default::default()
    };
    let first = tracker.reserve("alice", &limits, Utc::now()).unwrap();
    assert!(tracker.reserve("alice", &limits, Utc::now()).is_err());
    drop(first);
    assert!(tracker.reserve("alice", &limits, Utc::now()).is_ok());
}

async fn decide(
    engine: &AclEngine,
    user: &str,
    groups: &[&str],
) -> rustsocks::acl::PolicyEvaluationOutcome {
    let destination = Address::Domain("example.com".to_string());
    let groups: Vec<String> = groups.iter().map(|g| g.to_string()).collect();
    engine
        .evaluate_policy_for_traffic(PolicyEvaluationContext {
            user,
            groups: &groups,
            source_ip: "10.1.2.3".parse().unwrap(),
            auth_method: "userpass",
            destination: &destination,
            port: 443,
            protocol: &Protocol::Tcp,
            now: Utc.with_ymd_and_hms(2026, 9, 8, 10, 0, 0).unwrap(),
            usage: PolicyUsageSnapshot::default(),
        })
        .await
}

fn scoped_allow(users: &[&str], groups: &[&str]) -> AccessPolicy {
    let mut p = policy("scoped", Action::Allow, 100);
    p.users = users.iter().map(|u| u.to_string()).collect();
    p.groups = groups.iter().map(|g| g.to_string()).collect();
    p
}

fn blocking_default(policies: Vec<AccessPolicy>) -> AclEngine {
    let mut config = AclConfig {
        policies,
        ..Default::default()
    };
    config.global.default_policy = Action::Block;
    AclEngine::new(config).unwrap()
}

#[tokio::test]
async fn user_scoped_policy_applies_only_to_that_user_case_insensitively() {
    let engine = blocking_default(vec![scoped_allow(&["Alice"], &[])]);

    let alice = decide(&engine, "alice", &[]).await;
    assert_eq!(alice.decision, AclDecision::Allow);
    assert_eq!(alice.matched_policy_id.as_deref(), Some("scoped"));
    assert_eq!(
        decide(&engine, "ALICE", &[]).await.decision,
        AclDecision::Allow
    );

    // Everyone else falls through to the default policy.
    let bob = decide(&engine, "bob", &[]).await;
    assert_eq!(bob.decision, AclDecision::Block);
    assert!(bob.matched_policy_id.is_none());
}

#[tokio::test]
async fn group_scoped_policy_uses_dynamic_groups_case_insensitively() {
    let engine = blocking_default(vec![scoped_allow(&[], &["Developers"])]);

    for groups in [&["developers"][..], &["DEVELOPERS", "other"][..]] {
        let outcome = decide(&engine, "carol", groups).await;
        assert_eq!(outcome.decision, AclDecision::Allow, "groups: {groups:?}");
        assert_eq!(outcome.matched_policy_id.as_deref(), Some("scoped"));
    }

    // Not a member of the group: the policy does not apply.
    for groups in [&[][..], &["ops", "qa"][..]] {
        let outcome = decide(&engine, "carol", groups).await;
        assert_eq!(outcome.decision, AclDecision::Block, "groups: {groups:?}");
        assert!(outcome.matched_policy_id.is_none());
    }
}

#[tokio::test]
async fn group_scoped_policy_also_uses_static_user_group_membership() {
    let mut config = AclConfig {
        users: vec![rustsocks::acl::types::UserAcl {
            username: "dave".to_string(),
            groups: vec!["ops".to_string()],
            rules: vec![],
        }],
        groups: vec![rustsocks::acl::types::GroupAcl {
            name: "ops".to_string(),
            rules: vec![],
        }],
        policies: vec![scoped_allow(&[], &["ops"])],
        ..Default::default()
    };
    config.global.default_policy = Action::Block;
    let engine = AclEngine::new(config).unwrap();

    // dave is in "ops" only through the static [[users]] entry.
    assert_eq!(
        decide(&engine, "dave", &[]).await.decision,
        AclDecision::Allow
    );
    assert_eq!(
        decide(&engine, "erin", &[]).await.decision,
        AclDecision::Block
    );
}

#[tokio::test]
async fn global_policy_without_subject_applies_to_everyone() {
    let engine = blocking_default(vec![scoped_allow(&[], &[])]);
    assert_eq!(
        decide(&engine, "anyone", &[]).await.decision,
        AclDecision::Allow
    );
}
