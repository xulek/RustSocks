//! Redis-backed policy usage store shared between RustSocks instances.
//!
//! Layout (all keys carry a `{user}` hash tag so the scripts also work on Redis Cluster):
//!
//! * `<prefix>:{user}:active` – sorted set of live connections, score = lease expiry (ms).
//!   Every live connection is renewed by a heartbeat; a crashed instance's entries expire on
//!   their own, so connections never leak.
//! * `<prefix>:{user}:rate` – sorted set of recent admissions, score = admission time (ms).
//! * `<prefix>:{user}:d:YYYYMMDD` / `:m:YYYYMM` – transfer counters (UTC) with a TTL.
//!
//! Admission is one Lua script, so the check-and-reserve is atomic across instances. Redis
//! `TIME` is used for lease and rate arithmetic, so instance clock skew does not matter.
//!
//! Transfer bytes are batched and flushed every second. Quota checks therefore lag by up to
//! about a second across instances, which is acceptable for daily/monthly quotas.

use super::metrics::AclMetrics;
use super::policy::{
    PolicyAdmissionGuard, PolicyAdmissionLimits, PolicyUsageSnapshot, PolicyUsageTracker,
    ReleaseHandle,
};
use crate::config::PolicyStateSettings;
use crate::utils::error::{Result, RustSocksError};
use chrono::{DateTime, Datelike, Utc};
use dashmap::DashMap;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use redis::Script;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tracing::{debug, warn};

const RATE_WINDOW_MS: u64 = 60_000;
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const DAY_KEY_TTL_SECS: i64 = 2 * 24 * 3600;
const MONTH_KEY_TTL_SECS: i64 = 33 * 24 * 3600;

/// Atomically check every limit and, when all pass, record the connection.
/// ARGV: lease_ms, member, window_ms, max_active, max_rate, day_limit, month_limit (-1 = none).
const RESERVE_SCRIPT: &str = r#"
if redis.replicate_commands then redis.replicate_commands() end
local t = redis.call('TIME')
local now = tonumber(t[1]) * 1000 + math.floor(tonumber(t[2]) / 1000)
local lease = tonumber(ARGV[1])
local member = ARGV[2]
local window = tonumber(ARGV[3])
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', now)
redis.call('ZREMRANGEBYSCORE', KEYS[2], '-inf', now - window)
local max_active = tonumber(ARGV[4])
if max_active >= 0 and redis.call('ZCARD', KEYS[1]) >= max_active then
  return {0, 'active_connections'}
end
local max_rate = tonumber(ARGV[5])
if max_rate >= 0 and redis.call('ZCARD', KEYS[2]) >= max_rate then
  return {0, 'connection_rate'}
end
local day_limit = tonumber(ARGV[6])
if day_limit >= 0 and (tonumber(redis.call('GET', KEYS[3])) or 0) >= day_limit then
  return {0, 'daily_quota'}
end
local month_limit = tonumber(ARGV[7])
if month_limit >= 0 and (tonumber(redis.call('GET', KEYS[4])) or 0) >= month_limit then
  return {0, 'monthly_quota'}
end
redis.call('ZADD', KEYS[1], now + lease, member)
redis.call('ZADD', KEYS[2], now, member)
redis.call('PEXPIRE', KEYS[1], lease * 2)
redis.call('PEXPIRE', KEYS[2], window * 2)
return {1, ''}
"#;

/// Read the four counters. ARGV: window_ms. Returns strings (counters may exceed 2^53).
const SNAPSHOT_SCRIPT: &str = r#"
if redis.replicate_commands then redis.replicate_commands() end
local t = redis.call('TIME')
local now = tonumber(t[1]) * 1000 + math.floor(tonumber(t[2]) / 1000)
local window = tonumber(ARGV[1])
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', now)
redis.call('ZREMRANGEBYSCORE', KEYS[2], '-inf', now - window)
return {
  tostring(redis.call('ZCARD', KEYS[1])),
  tostring(redis.call('ZCARD', KEYS[2])),
  redis.call('GET', KEYS[3]) or '0',
  redis.call('GET', KEYS[4]) or '0'
}
"#;

/// Extend the lease of live connections only (`XX`: never resurrect a released one).
/// ARGV: lease_ms, member...
const RENEW_SCRIPT: &str = r#"
if redis.replicate_commands then redis.replicate_commands() end
local t = redis.call('TIME')
local now = tonumber(t[1]) * 1000 + math.floor(tonumber(t[2]) / 1000)
local lease = tonumber(ARGV[1])
for i = 2, #ARGV do
  redis.call('ZADD', KEYS[1], 'XX', now + lease, ARGV[i])
end
redis.call('PEXPIRE', KEYS[1], lease * 2)
return #ARGV - 1
"#;

pub struct RedisUsageStore {
    conn: ConnectionManager,
    prefix: String,
    instance_id: String,
    lease_ms: u64,
    fail_closed: bool,
    op_timeout: Duration,
    /// Fallback when Redis is unavailable, and a mirror of this instance's own transfers.
    local: Arc<PolicyUsageTracker>,
    next_reservation: AtomicU64,
    /// Live reservations of this instance (id -> user key), renewed by the heartbeat.
    live: DashMap<u64, String>,
    /// Transfer bytes not yet flushed to Redis (user key -> bytes).
    pending_bytes: DashMap<String, u64>,
    reserve_script: Script,
    snapshot_script: Script,
    renew_script: Script,
    runtime: tokio::runtime::Handle,
}

impl std::fmt::Debug for RedisUsageStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisUsageStore")
            .field("prefix", &self.prefix)
            .field("instance_id", &self.instance_id)
            .finish()
    }
}

/// Escape characters that would break the `{hash tag}` or key structure.
fn escape_user(user: &str) -> String {
    let mut out = String::with_capacity(user.len());
    for c in user.to_ascii_lowercase().chars() {
        match c {
            '%' => out.push_str("%25"),
            '{' => out.push_str("%7B"),
            '}' => out.push_str("%7D"),
            ':' => out.push_str("%3A"),
            c => out.push(c),
        }
    }
    out
}

fn limit_arg(limit: Option<u64>) -> i128 {
    limit.map(i128::from).unwrap_or(-1)
}

fn denial_message(reason: &str) -> String {
    match reason {
        "active_connections" => "active connection limit reached",
        "connection_rate" => "connection rate limit reached",
        "daily_quota" => "daily transfer quota reached",
        "monthly_quota" => "monthly transfer quota reached",
        _ => "policy admission limit reached",
    }
    .to_string()
}

fn static_reason(reason: &str) -> &'static str {
    match reason {
        "active_connections" => "active_connections",
        "connection_rate" => "connection_rate",
        "daily_quota" => "daily_quota",
        "monthly_quota" => "monthly_quota",
        _ => "other",
    }
}

impl RedisUsageStore {
    pub async fn connect(settings: &PolicyStateSettings) -> Result<Arc<Self>> {
        let url = settings.redis_url.as_deref().unwrap_or_default();
        let client = redis::Client::open(url)
            .map_err(|e| RustSocksError::Config(format!("invalid policy_state.redis_url: {e}")))?;
        let op_timeout = Duration::from_millis(settings.operation_timeout_ms);
        let config = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(op_timeout.max(Duration::from_secs(2))))
            .set_response_timeout(Some(op_timeout));
        let conn = ConnectionManager::new_with_config(client, config)
            .await
            .map_err(|e| {
                RustSocksError::Config(format!("cannot connect to policy_state.redis_url: {e}"))
            })?;

        let store = Arc::new(Self {
            conn,
            prefix: settings.key_prefix.clone(),
            instance_id: uuid::Uuid::new_v4().simple().to_string(),
            lease_ms: settings.lease_secs.saturating_mul(1000),
            fail_closed: settings.failure_mode == "fail_closed",
            op_timeout,
            local: Arc::new(PolicyUsageTracker::default()),
            next_reservation: AtomicU64::new(1),
            live: DashMap::new(),
            pending_bytes: DashMap::new(),
            reserve_script: Script::new(RESERVE_SCRIPT),
            snapshot_script: Script::new(SNAPSHOT_SCRIPT),
            renew_script: Script::new(RENEW_SCRIPT),
            runtime: tokio::runtime::Handle::current(),
        });
        store.spawn_background_tasks();
        Ok(store)
    }

    fn base(&self, user_key: &str) -> String {
        format!("{}:{{{}}}", self.prefix, user_key)
    }

    fn keys(&self, user_key: &str, now: DateTime<Utc>) -> [String; 4] {
        let base = self.base(user_key);
        [
            format!("{base}:active"),
            format!("{base}:rate"),
            format!(
                "{base}:d:{:04}{:02}{:02}",
                now.year(),
                now.month(),
                now.day()
            ),
            format!("{base}:m:{:04}{:02}", now.year(), now.month()),
        ]
    }

    fn spawn_background_tasks(self: &Arc<Self>) {
        // Heartbeat: keep this instance's live connections from expiring.
        let weak: Weak<Self> = Arc::downgrade(self);
        let renew_every = Duration::from_millis((self.lease_ms / 3).max(500));
        self.runtime.spawn(async move {
            let mut ticker = tokio::time::interval(renew_every);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let Some(store) = weak.upgrade() else { break };
                store.renew_leases().await;
            }
        });

        // Flusher: push batched transfer bytes.
        let weak: Weak<Self> = Arc::downgrade(self);
        self.runtime.spawn(async move {
            let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let Some(store) = weak.upgrade() else { break };
                store.flush_transfers().await;
            }
        });
    }

    async fn renew_leases(&self) {
        let mut by_user: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for entry in self.live.iter() {
            by_user
                .entry(entry.value().clone())
                .or_default()
                .push(self.member(*entry.key()));
        }
        let now = Utc::now();
        for (user_key, members) in by_user {
            let key = self.keys(&user_key, now)[0].clone();
            let mut invocation = self.renew_script.key(key);
            invocation.arg(self.lease_ms);
            for member in &members {
                invocation.arg(member);
            }
            let mut conn = self.conn.clone();
            let result: std::result::Result<_, String> = match tokio::time::timeout(
                self.op_timeout,
                invocation.invoke_async::<i64>(&mut conn),
            )
            .await
            {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Err("timeout".to_string()),
            };
            if let Err(error) = result {
                AclMetrics::record_usage_store_error("heartbeat");
                warn!(%error, "Failed to renew policy connection leases");
            }
        }
    }

    async fn flush_transfers(&self) {
        let drained: Vec<(String, u64)> = self
            .pending_bytes
            .iter()
            .map(|entry| (entry.key().clone(), *entry.value()))
            .collect();
        if drained.is_empty() {
            return;
        }
        let now = Utc::now();
        let mut pipe = redis::pipe();
        let mut taken = Vec::with_capacity(drained.len());
        for (user_key, _) in drained {
            // Take exactly what we are about to send; concurrent additions stay pending.
            let Some((_, bytes)) = self.pending_bytes.remove(&user_key) else {
                continue;
            };
            let keys = self.keys(&user_key, now);
            pipe.cmd("INCRBY")
                .arg(&keys[2])
                .arg(bytes)
                .ignore()
                .cmd("EXPIRE")
                .arg(&keys[2])
                .arg(DAY_KEY_TTL_SECS)
                .ignore()
                .cmd("INCRBY")
                .arg(&keys[3])
                .arg(bytes)
                .ignore()
                .cmd("EXPIRE")
                .arg(&keys[3])
                .arg(MONTH_KEY_TTL_SECS)
                .ignore();
            taken.push((user_key, bytes));
        }
        if taken.is_empty() {
            return;
        }
        let mut conn = self.conn.clone();
        let outcome =
            tokio::time::timeout(self.op_timeout, pipe.query_async::<()>(&mut conn)).await;
        if !matches!(outcome, Ok(Ok(()))) {
            AclMetrics::record_usage_store_error("flush");
            warn!("Failed to flush policy transfer counters; keeping them for retry");
            for (user_key, bytes) in taken {
                *self.pending_bytes.entry(user_key).or_insert(0) += bytes;
            }
        }
    }

    fn member(&self, reservation_id: u64) -> String {
        format!("{}:{}", self.instance_id, reservation_id)
    }

    pub async fn snapshot(&self, user: &str, now: DateTime<Utc>) -> PolicyUsageSnapshot {
        let user_key = escape_user(user);
        let keys = self.keys(&user_key, now);
        let mut invocation = self.snapshot_script.key(&keys[0]);
        invocation
            .key(&keys[1])
            .key(&keys[2])
            .key(&keys[3])
            .arg(RATE_WINDOW_MS);
        let mut conn = self.conn.clone();
        match tokio::time::timeout(
            self.op_timeout,
            invocation.invoke_async::<Vec<String>>(&mut conn),
        )
        .await
        {
            Ok(Ok(values)) if values.len() == 4 => {
                let parse = |s: &str| s.parse::<u64>().unwrap_or(0);
                let unflushed = self
                    .pending_bytes
                    .get(&user_key)
                    .map(|bytes| *bytes)
                    .unwrap_or(0);
                PolicyUsageSnapshot {
                    active_connections: parse(&values[0]).min(u32::MAX as u64) as u32,
                    connections_last_minute: parse(&values[1]).min(u32::MAX as u64) as u32,
                    bytes_today: parse(&values[2]).saturating_add(unflushed),
                    bytes_this_month: parse(&values[3]).saturating_add(unflushed),
                }
            }
            _ => {
                AclMetrics::record_usage_store_error("snapshot");
                debug!("Policy usage snapshot fell back to local counters");
                self.local.snapshot(user, now)
            }
        }
    }

    pub async fn reserve(
        self: &Arc<Self>,
        user: &str,
        limits: &PolicyAdmissionLimits,
        now: DateTime<Utc>,
    ) -> std::result::Result<PolicyAdmissionGuard, String> {
        let user_key = escape_user(user);
        let id = self.next_reservation.fetch_add(1, Ordering::Relaxed);
        let member = self.member(id);
        let keys = self.keys(&user_key, now);

        let mut invocation = self.reserve_script.key(&keys[0]);
        invocation
            .key(&keys[1])
            .key(&keys[2])
            .key(&keys[3])
            .arg(self.lease_ms)
            .arg(&member)
            .arg(RATE_WINDOW_MS)
            .arg(limit_arg(limits.max_active_connections.map(u64::from)).to_string())
            .arg(limit_arg(limits.max_connections_per_minute.map(u64::from)).to_string())
            .arg(limit_arg(limits.daily_transfer_limit_bytes).to_string())
            .arg(limit_arg(limits.monthly_transfer_limit_bytes).to_string());

        let mut conn = self.conn.clone();
        let outcome = tokio::time::timeout(
            self.op_timeout,
            invocation.invoke_async::<(i64, String)>(&mut conn),
        )
        .await;

        match outcome {
            Ok(Ok((1, _))) => {
                self.live.insert(id, user_key.clone());
                Ok(PolicyAdmissionGuard::new(
                    Arc::clone(self) as Arc<dyn ReleaseHandle>,
                    user_key,
                    id,
                ))
            }
            Ok(Ok((_, reason))) => {
                AclMetrics::record_admission_denied(static_reason(&reason));
                Err(denial_message(&reason))
            }
            Ok(Err(_)) | Err(_) => {
                AclMetrics::record_usage_store_error("reserve");
                if limits.is_empty() || !self.fail_closed {
                    warn!(
                        user = %user,
                        "Policy usage store unavailable; admitting with local counters"
                    );
                    self.local.reserve(user, limits, now)
                } else {
                    Err("policy usage store unavailable (fail_closed)".to_string())
                }
            }
        }
    }

    pub fn record_transfer(&self, user: &str, bytes: u64, now: DateTime<Utc>) {
        if bytes == 0 {
            return;
        }
        self.local.record_transfer(user, bytes, now);
        let mut entry = self.pending_bytes.entry(escape_user(user)).or_insert(0);
        *entry = entry.saturating_add(bytes);
    }
}

impl ReleaseHandle for RedisUsageStore {
    fn release_reservation(&self, user_key: &str, reservation_id: u64) {
        self.live.remove(&reservation_id);
        let key = self.keys(user_key, Utc::now())[0].clone();
        let member = self.member(reservation_id);
        let mut conn = self.conn.clone();
        let op_timeout = self.op_timeout;
        // Runs from Drop, possibly outside the runtime: use the stored handle and never block.
        self.runtime.spawn(async move {
            let result = tokio::time::timeout(
                op_timeout,
                redis::cmd("ZREM")
                    .arg(&key)
                    .arg(&member)
                    .query_async::<i64>(&mut conn),
            )
            .await;
            if !matches!(result, Ok(Ok(_))) {
                // The lease expires on its own; just account for the failure.
                AclMetrics::record_usage_store_error("release");
            }
        });
    }
}
