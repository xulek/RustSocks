//! Per-IP throttling of failed authentication attempts.
//!
//! Shared by every credential-based SOCKS backend (built-in username/password and
//! PAM username) so brute-force protection behaves identically across backends.
//! The table is bounded: an attacker cycling through source addresses cannot grow
//! memory without limit.

use dashmap::DashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct AttemptState {
    failures: u32,
    first_failure: Instant,
}

pub(crate) struct AuthThrottle {
    attempts: DashMap<IpAddr, AttemptState>,
    max_failures: u32,
    lockout: Duration,
    max_entries: usize,
}

impl AuthThrottle {
    pub(crate) fn new(max_failures: u32, lockout: Duration, max_entries: usize) -> Self {
        Self {
            attempts: DashMap::new(),
            max_failures: max_failures.max(1),
            lockout,
            max_entries: max_entries.max(1),
        }
    }

    /// True while the address has exhausted its failure budget inside the lockout window.
    pub(crate) fn is_limited(&self, client_ip: IpAddr) -> bool {
        self.is_limited_at(client_ip, Instant::now())
    }

    fn is_limited_at(&self, client_ip: IpAddr, now: Instant) -> bool {
        if let Some(entry) = self.attempts.get(&client_ip) {
            if now.duration_since(entry.first_failure) < self.lockout {
                return entry.failures >= self.max_failures;
            }
        }
        self.attempts.remove(&client_ip);
        false
    }

    pub(crate) fn record_failure(&self, client_ip: IpAddr) {
        self.record_failure_at(client_ip, Instant::now());
    }

    fn record_failure_at(&self, client_ip: IpAddr, now: Instant) {
        if !self.attempts.contains_key(&client_ip) && self.attempts.len() >= self.max_entries {
            self.purge_expired(now);
            if self.attempts.len() >= self.max_entries {
                // Table is full of live lockouts; do not grow it further.
                return;
            }
        }

        self.attempts
            .entry(client_ip)
            .and_modify(|state| {
                if now.duration_since(state.first_failure) >= self.lockout {
                    *state = AttemptState {
                        failures: 1,
                        first_failure: now,
                    };
                } else {
                    state.failures = state.failures.saturating_add(1);
                }
            })
            .or_insert(AttemptState {
                failures: 1,
                first_failure: now,
            });
    }

    pub(crate) fn clear(&self, client_ip: IpAddr) {
        self.attempts.remove(&client_ip);
    }

    fn purge_expired(&self, now: Instant) {
        self.attempts
            .retain(|_, state| now.duration_since(state.first_failure) < self.lockout);
    }

    #[cfg(test)]
    fn tracked(&self) -> usize {
        self.attempts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn locks_out_after_max_failures_and_isolates_addresses() {
        let throttle = AuthThrottle::new(3, Duration::from_secs(60), 100);
        for _ in 0..2 {
            throttle.record_failure(ip(1));
        }
        assert!(!throttle.is_limited(ip(1)));
        throttle.record_failure(ip(1));
        assert!(throttle.is_limited(ip(1)));
        assert!(!throttle.is_limited(ip(2)));
    }

    #[test]
    fn success_clears_failures() {
        let throttle = AuthThrottle::new(2, Duration::from_secs(60), 100);
        throttle.record_failure(ip(1));
        throttle.record_failure(ip(1));
        assert!(throttle.is_limited(ip(1)));
        throttle.clear(ip(1));
        assert!(!throttle.is_limited(ip(1)));
    }

    #[test]
    fn lockout_expires() {
        let throttle = AuthThrottle::new(1, Duration::from_secs(60), 100);
        let start = Instant::now();
        throttle.record_failure_at(ip(1), start);
        assert!(throttle.is_limited_at(ip(1), start + Duration::from_secs(59)));
        assert!(!throttle.is_limited_at(ip(1), start + Duration::from_secs(61)));
        assert_eq!(throttle.tracked(), 0);
    }

    #[test]
    fn table_is_bounded_and_purges_expired_entries() {
        let throttle = AuthThrottle::new(1, Duration::from_secs(60), 2);
        let start = Instant::now();
        throttle.record_failure_at(ip(1), start);
        throttle.record_failure_at(ip(2), start);
        // Full of live entries: a third address is not tracked.
        throttle.record_failure_at(ip(3), start + Duration::from_secs(1));
        assert_eq!(throttle.tracked(), 2);
        assert!(!throttle.is_limited_at(ip(3), start + Duration::from_secs(1)));
        // Once the old entries expire, new ones are admitted again.
        throttle.record_failure_at(ip(3), start + Duration::from_secs(120));
        assert_eq!(throttle.tracked(), 1);
        assert!(throttle.is_limited_at(ip(3), start + Duration::from_secs(121)));
    }
}
