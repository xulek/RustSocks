//! Policy usage state (active connections, connection rate, transfer quotas).
//!
//! [`PolicyUsage`] is the single entry point used by the session manager and the
//! request handlers. It dispatches to one of two backends:
//!
//! * **local**: the in-process [`PolicyUsageTracker`]. No dependencies, but counters are
//!   lost on restart and every instance enforces limits on its own.
//! * **redis** (feature `redis`): counters shared between instances so limits apply to
//!   the whole deployment. See [`super::usage_redis`].

use super::policy::{
    PolicyAdmissionGuard, PolicyAdmissionLimits, PolicyUsageSnapshot, PolicyUsageTracker,
};
use crate::config::PolicyStateSettings;
use crate::utils::error::Result;
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Clone)]
pub struct PolicyUsage {
    backend: Backend,
}

#[derive(Clone)]
enum Backend {
    Local(Arc<PolicyUsageTracker>),
    #[cfg(feature = "redis")]
    Redis(Arc<super::usage_redis::RedisUsageStore>),
}

impl Default for PolicyUsage {
    fn default() -> Self {
        Self::local()
    }
}

impl std::fmt::Debug for PolicyUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyUsage")
            .field("backend", &self.backend_name())
            .finish()
    }
}

impl PolicyUsage {
    /// In-process counters.
    pub fn local() -> Self {
        Self {
            backend: Backend::Local(Arc::new(PolicyUsageTracker::default())),
        }
    }

    /// Wrap an existing in-process tracker (used by tests and benchmarks).
    pub fn from_tracker(tracker: Arc<PolicyUsageTracker>) -> Self {
        Self {
            backend: Backend::Local(tracker),
        }
    }

    /// Build the backend selected by `[policy_state]`.
    ///
    /// Connecting to Redis happens here, so a misconfigured or unreachable Redis is
    /// reported at startup rather than on the first connection.
    pub async fn connect(settings: &PolicyStateSettings) -> Result<Self> {
        match settings.backend.as_str() {
            "memory" => Ok(Self::local()),
            #[cfg(feature = "redis")]
            "redis" => {
                let store = super::usage_redis::RedisUsageStore::connect(settings).await?;
                Ok(Self {
                    backend: Backend::Redis(store),
                })
            }
            other => Err(crate::utils::error::RustSocksError::Config(format!(
                "unsupported policy_state.backend \"{other}\" (is the `redis` feature enabled?)"
            ))),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        match &self.backend {
            Backend::Local(_) => "memory",
            #[cfg(feature = "redis")]
            Backend::Redis(_) => "redis",
        }
    }

    /// Current usage for a user. Never fails: a backend error degrades to local counters.
    pub async fn snapshot(&self, user: &str, now: DateTime<Utc>) -> PolicyUsageSnapshot {
        match &self.backend {
            Backend::Local(tracker) => tracker.snapshot(user, now),
            #[cfg(feature = "redis")]
            Backend::Redis(store) => store.snapshot(user, now).await,
        }
    }

    /// Atomically reserve capacity for one new connection.
    pub async fn reserve(
        &self,
        user: &str,
        limits: &PolicyAdmissionLimits,
        now: DateTime<Utc>,
    ) -> std::result::Result<PolicyAdmissionGuard, String> {
        match &self.backend {
            Backend::Local(tracker) => tracker.reserve(user, limits, now),
            #[cfg(feature = "redis")]
            Backend::Redis(store) => store.reserve(user, limits, now).await,
        }
    }

    /// Record proxied bytes. Never blocks: the shared backend batches updates.
    pub fn record_transfer(&self, user: &str, bytes: u64, now: DateTime<Utc>) {
        match &self.backend {
            Backend::Local(tracker) => tracker.record_transfer(user, bytes, now),
            #[cfg(feature = "redis")]
            Backend::Redis(store) => store.record_transfer(user, bytes, now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_backend_behaves_like_the_tracker() {
        let usage = PolicyUsage::local();
        let now = Utc::now();
        let limits = PolicyAdmissionLimits {
            max_active_connections: Some(1),
            ..Default::default()
        };

        assert_eq!(usage.backend_name(), "memory");
        let guard = usage.reserve("alice", &limits, now).await.unwrap();
        assert_eq!(usage.snapshot("alice", now).await.active_connections, 1);
        assert!(usage.reserve("alice", &limits, now).await.is_err());

        drop(guard);
        assert_eq!(usage.snapshot("alice", now).await.active_connections, 0);

        usage.record_transfer("alice", 700, now);
        assert_eq!(usage.snapshot("alice", now).await.bytes_today, 700);
    }

    #[tokio::test]
    async fn memory_backend_is_selected_by_default_settings() {
        let usage = PolicyUsage::connect(&PolicyStateSettings::default())
            .await
            .unwrap();
        assert_eq!(usage.backend_name(), "memory");
    }

    #[tokio::test]
    async fn unknown_backend_is_rejected() {
        let settings = PolicyStateSettings {
            backend: "etcd".to_string(),
            ..Default::default()
        };
        assert!(PolicyUsage::connect(&settings).await.is_err());
    }
}
