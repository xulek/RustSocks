use dashmap::DashMap;
use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};

/// Aggregate ACL statistics for observability and future metrics export.
#[derive(Debug)]
pub struct AclStats {
    total_allowed: AtomicU64,
    total_blocked: AtomicU64,
    per_user: DashMap<String, UserAclStats>,
}

#[derive(Debug, Default, Clone, Copy)]
struct UserAclStats {
    allowed: u64,
    blocked: u64,
}

impl AclStats {
    /// Create a fresh statistics tracker.
    pub fn new() -> Self {
        Self {
            total_allowed: AtomicU64::new(0),
            total_blocked: AtomicU64::new(0),
            per_user: DashMap::new(),
        }
    }

    /// Record an allowed decision.
    pub fn record_allow<'a>(&self, user: impl Into<Cow<'a, str>>) {
        let user = user.into();
        self.total_allowed.fetch_add(1, Ordering::Relaxed);
        self.per_user
            .entry(user.into_owned())
            .and_modify(|stats| stats.allowed += 1)
            .or_insert_with(|| UserAclStats {
                allowed: 1,
                blocked: 0,
            });
    }

    /// Record a blocked decision along with the user.
    pub fn record_block<'a>(&self, user: impl Into<Cow<'a, str>>) {
        let user = user.into();
        self.total_blocked.fetch_add(1, Ordering::Relaxed);
        self.per_user
            .entry(user.into_owned())
            .and_modify(|stats| stats.blocked += 1)
            .or_insert_with(|| UserAclStats {
                allowed: 0,
                blocked: 1,
            });
    }

    /// Snapshot overall counters (allowed, blocked).
    pub fn snapshot(&self) -> AclStatsSnapshot {
        AclStatsSnapshot {
            allowed: self.total_allowed.load(Ordering::Relaxed),
            blocked: self.total_blocked.load(Ordering::Relaxed),
        }
    }

    /// Get per-user counters if available.
    pub fn user_snapshot(&self, user: &str) -> Option<AclStatsSnapshot> {
        self.per_user.get(user).map(|stats| AclStatsSnapshot {
            allowed: stats.allowed,
            blocked: stats.blocked,
        })
    }
}

impl Default for AclStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable view of counters.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AclStatsSnapshot {
    pub allowed: u64,
    pub blocked: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_snapshot_counters() {
        let stats = AclStats::new();

        stats.record_allow("alice");
        stats.record_allow("alice");
        stats.record_block("alice");
        stats.record_block("bob");

        let totals = stats.snapshot();
        assert_eq!(totals.allowed, 2);
        assert_eq!(totals.blocked, 2);

        let alice = stats.user_snapshot("alice").unwrap();
        assert_eq!(alice.allowed, 2);
        assert_eq!(alice.blocked, 1);

        let bob = stats.user_snapshot("bob").unwrap();
        assert_eq!(bob.allowed, 0);
        assert_eq!(bob.blocked, 1);

        assert!(stats.user_snapshot("charlie").is_none());
    }
}
