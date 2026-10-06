pub mod crud;
pub mod engine;
pub mod loader;
pub mod matcher;
pub mod metrics;
pub mod persistence;
pub mod policy;
pub mod stats;
pub mod types;
pub mod usage;
#[cfg(feature = "redis")]
pub mod usage_redis;
pub mod watcher;

pub use crud::{RuleIdentifier, RuleSearchCriteria, RuleSearchResult};
pub use engine::AclEngine;
pub use loader::{create_example_acl_config, load_acl_config, load_acl_config_sync};
pub use metrics::AclMetrics;
pub use persistence::{load_config, save_config};
pub use policy::{
    DecisionSource, PolicyAdmissionGuard, PolicyAdmissionLimits, PolicyEvaluationContext,
    PolicyEvaluationOutcome, PolicyTraceEntry, PolicyUsageSnapshot, PolicyUsageTracker,
};
pub use stats::{AclStats, AclStatsSnapshot};
pub use types::{
    AccessPolicy, AclConfig, AclDecision, Action, PolicyConditions, PolicyMode, PolicySchedule,
    Protocol,
};
pub use usage::PolicyUsage;
pub use watcher::AclWatcher;

/// Ensure all ACL/policy Prometheus collectors are registered before a scrape.
#[inline]
pub fn ensure_metrics_registered() {
    metrics::init();
}
