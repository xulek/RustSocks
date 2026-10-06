/// Benchmark: ACL / Dynamic Policy Engine evaluation
///
/// Measures the per-connection decision path (`evaluate_policy_with_context`) with
/// legacy rules only, with dynamic policies carrying conditions, and the admission
/// reservation tracker used for connection/quota limits.
use chrono::{TimeZone, Utc};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rustsocks::acl::types::{AclRule, UserAcl};
use rustsocks::acl::{
    AccessPolicy, AclConfig, AclEngine, Action, PolicyAdmissionLimits, PolicyConditions,
    PolicyEvaluationContext, PolicyMode, PolicySchedule, PolicyUsageSnapshot, PolicyUsageTracker,
    Protocol,
};
use rustsocks::protocol::Address;
use std::sync::Arc;
use tokio::runtime::Runtime;

const RULE_COUNTS: [usize; 3] = [10, 50, 200];

fn legacy_rule(i: usize, action: Action) -> AclRule {
    AclRule {
        action,
        description: format!("rule-{i}"),
        destinations: vec![format!("host{i}.example.com")],
        ports: vec!["443".to_string()],
        protocols: vec![Protocol::Tcp],
        priority: 1000 - i as u32,
    }
}

fn policy(i: usize, with_conditions: bool) -> AccessPolicy {
    let conditions = if with_conditions {
        PolicyConditions {
            source_ips: vec!["10.0.0.0/8".to_string()],
            auth_methods: vec!["userpass".to_string()],
            schedule: Some(PolicySchedule {
                days: vec![],
                start: "00:00".to_string(),
                end: "23:59".to_string(),
                utc_offset_minutes: 0,
            }),
            ..Default::default()
        }
    } else {
        PolicyConditions::default()
    };
    AccessPolicy {
        id: format!("policy-{i}"),
        enabled: true,
        mode: PolicyMode::Enforce,
        description: format!("policy-{i}"),
        users: vec!["alice".to_string()],
        groups: vec![],
        action: Action::Allow,
        destinations: vec![format!("host{i}.example.com")],
        ports: vec!["443".to_string()],
        protocols: vec![Protocol::Tcp],
        priority: 1000 - i as u32,
        enforce_conditions: with_conditions,
        conditions,
        owner: None,
        ticket: None,
        tags: vec![],
    }
}

fn config(rules: usize, policies: usize, with_conditions: bool) -> AclConfig {
    let mut config = AclConfig {
        users: vec![UserAcl {
            username: "alice".to_string(),
            groups: vec![],
            rules: (0..rules).map(|i| legacy_rule(i, Action::Allow)).collect(),
        }],
        policies: (0..policies).map(|i| policy(i, with_conditions)).collect(),
        ..Default::default()
    };
    config.global.default_policy = Action::Block;
    config
}

fn bench_evaluation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 9, 8, 10, 0, 0).unwrap();
    let source_ip = "10.1.2.3".parse().unwrap();
    let mut group = c.benchmark_group("policy_evaluation");

    for count in RULE_COUNTS {
        // (label, rules, policies, conditions)
        let scenarios = [
            ("legacy_only", count, 0, false),
            ("policies_no_conditions", 0, count, false),
            ("policies_with_conditions", 0, count, true),
        ];
        for (label, rules, policies, conditions) in scenarios {
            let engine = AclEngine::new(config(rules, policies, conditions)).unwrap();
            // Destination matches the lowest-priority entry: worst-case walk.
            let destination = Address::Domain(format!("host{}.example.com", count - 1));
            let usage = PolicyUsageSnapshot::default();

            group.bench_with_input(BenchmarkId::new(label, count), &count, |b, _| {
                b.iter(|| {
                    rt.block_on(
                        engine.evaluate_policy_with_context(PolicyEvaluationContext {
                            user: "alice",
                            groups: &[],
                            source_ip,
                            auth_method: "userpass",
                            destination: &destination,
                            port: 443,
                            protocol: &Protocol::Tcp,
                            now,
                            usage: usage.clone(),
                        }),
                    )
                });
            });
        }
    }

    group.finish();
}

fn bench_admission(c: &mut Criterion) {
    let tracker = Arc::new(PolicyUsageTracker::default());
    let limits = PolicyAdmissionLimits {
        max_active_connections: Some(u32::MAX),
        max_connections_per_minute: Some(u32::MAX),
        daily_transfer_limit_bytes: Some(u64::MAX),
        monthly_transfer_limit_bytes: Some(u64::MAX),
    };

    c.bench_function("policy_admission_reserve_release", |b| {
        b.iter(|| {
            let guard = tracker.reserve("alice", &limits, Utc::now()).unwrap();
            black_box(&guard);
            drop(guard);
        });
    });

    c.bench_function("policy_usage_snapshot", |b| {
        b.iter(|| black_box(tracker.snapshot("alice", Utc::now())));
    });

    c.bench_function("policy_usage_record_transfer", |b| {
        b.iter(|| tracker.record_transfer("alice", 4096, Utc::now()));
    });
}

criterion_group!(benches, bench_evaluation, bench_admission);
criterion_main!(benches);
