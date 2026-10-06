//! Tests for the Redis policy-usage backend against a real Redis server.
//!
//! They run only when `REDIS_URL` is set (for example `redis://127.0.0.1:6379/0`) and are
//! skipped otherwise, so the normal test run needs no Redis. Every test uses a unique key
//! prefix, so tests can run in parallel and against a shared Redis.
#![cfg(feature = "redis")]

use chrono::Utc;
use rustsocks::acl::usage_redis::RedisUsageStore;
use rustsocks::acl::PolicyAdmissionLimits;
use rustsocks::config::PolicyStateSettings;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn redis_url() -> Option<String> {
    std::env::var("REDIS_URL").ok().filter(|v| !v.is_empty())
}

macro_rules! require_redis {
    () => {
        match redis_url() {
            Some(url) => url,
            None => {
                eprintln!("REDIS_URL not set; skipping Redis backend test");
                return;
            }
        }
    };
}

fn settings(url: &str, prefix: &str, failure_mode: &str, lease_secs: u64) -> PolicyStateSettings {
    PolicyStateSettings {
        backend: "redis".to_string(),
        redis_url: Some(url.to_string()),
        key_prefix: prefix.to_string(),
        failure_mode: failure_mode.to_string(),
        lease_secs,
        operation_timeout_ms: 250,
    }
}

fn unique_prefix(test: &str) -> String {
    format!("rustsocks-test-{test}-{}", uuid::Uuid::new_v4().simple())
}

async fn eventually<F, Fut>(what: &str, timeout: Duration, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if check().await {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn active_limit(n: u32) -> PolicyAdmissionLimits {
    PolicyAdmissionLimits {
        max_active_connections: Some(n),
        ..Default::default()
    }
}

#[tokio::test]
async fn active_connection_limit_is_shared_between_instances() {
    let url = require_redis!();
    let prefix = unique_prefix("active");
    let a = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 60))
        .await
        .unwrap();
    let b = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 60))
        .await
        .unwrap();
    let limits = active_limit(3);
    let now = Utc::now();

    let g1 = a.reserve("alice", &limits, now).await.unwrap();
    let _g2 = a.reserve("alice", &limits, now).await.unwrap();
    let _g3 = b.reserve("alice", &limits, now).await.unwrap();

    // The limit applies to the whole deployment, whichever instance is asked.
    for store in [&a, &b] {
        let err = store.reserve("alice", &limits, now).await.unwrap_err();
        assert!(err.contains("active connection limit"), "{err}");
    }
    // Other users are unaffected.
    assert!(a.reserve("bob", &limits, now).await.is_ok());

    // Releasing on one instance frees a slot for the other.
    drop(g1);
    eventually("slot released", Duration::from_secs(2), || async {
        b.reserve("alice", &limits, Utc::now()).await.is_ok()
    })
    .await;
}

#[tokio::test]
async fn connection_rate_is_shared_and_not_refunded_on_release() {
    let url = require_redis!();
    let prefix = unique_prefix("rate");
    let a = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 60))
        .await
        .unwrap();
    let b = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 60))
        .await
        .unwrap();
    let limits = PolicyAdmissionLimits {
        max_connections_per_minute: Some(3),
        ..Default::default()
    };
    let now = Utc::now();

    drop(a.reserve("alice", &limits, now).await.unwrap());
    drop(b.reserve("alice", &limits, now).await.unwrap());
    drop(a.reserve("alice", &limits, now).await.unwrap());

    let err = b.reserve("alice", &limits, now).await.unwrap_err();
    assert!(err.contains("connection rate limit"), "{err}");
}

#[tokio::test]
async fn snapshot_aggregates_all_instances() {
    let url = require_redis!();
    let prefix = unique_prefix("snapshot");
    let a = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 60))
        .await
        .unwrap();
    let b = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 60))
        .await
        .unwrap();
    let none = PolicyAdmissionLimits::default();
    let now = Utc::now();

    let _g1 = a.reserve("Alice", &none, now).await.unwrap();
    let _g2 = a.reserve("alice", &none, now).await.unwrap();
    let _g3 = b.reserve("ALICE", &none, now).await.unwrap();

    // User names are case-insensitive, like the in-memory backend.
    let snap = a.snapshot("alice", now).await;
    assert_eq!(snap.active_connections, 3);
    assert_eq!(snap.connections_last_minute, 3);
}

#[tokio::test]
async fn transfer_quota_is_shared_after_flush() {
    let url = require_redis!();
    let prefix = unique_prefix("quota");
    let a = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 60))
        .await
        .unwrap();
    let b = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 60))
        .await
        .unwrap();
    let limits = PolicyAdmissionLimits {
        daily_transfer_limit_bytes: Some(500),
        monthly_transfer_limit_bytes: Some(10_000),
        ..Default::default()
    };
    let now = Utc::now();

    assert!(b.reserve("alice", &limits, now).await.is_ok());
    a.record_transfer("alice", 600, now);

    // The issuing instance sees its own bytes immediately (unflushed counter included).
    assert_eq!(a.snapshot("alice", now).await.bytes_today, 600);

    // The other instance sees them once the batch is flushed, and is then denied.
    eventually(
        "quota visible on other instance",
        Duration::from_secs(4),
        || async { b.snapshot("alice", Utc::now()).await.bytes_today >= 600 },
    )
    .await;
    let err = b.reserve("alice", &limits, Utc::now()).await.unwrap_err();
    assert!(err.contains("daily transfer quota"), "{err}");
    assert_eq!(b.snapshot("alice", Utc::now()).await.bytes_this_month, 600);
}

#[tokio::test]
async fn live_connections_survive_beyond_one_lease_while_instance_is_alive() {
    let url = require_redis!();
    let prefix = unique_prefix("heartbeat");
    // 1 s lease: without the heartbeat the entry would expire almost immediately.
    let a = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 1))
        .await
        .unwrap();
    let limits = active_limit(1);
    let _guard = a.reserve("alice", &limits, Utc::now()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(3200)).await;
    assert!(
        a.reserve("alice", &limits, Utc::now()).await.is_err(),
        "heartbeat must keep the live connection counted"
    );
}

#[tokio::test]
async fn crashed_instance_connections_expire_with_their_lease() {
    let url = require_redis!();
    let prefix = unique_prefix("crash");
    let survivor = RedisUsageStore::connect(&settings(&url, &prefix, "fail_closed", 60))
        .await
        .unwrap();
    let limits = active_limit(1);

    // The "crashing" instance lives on its own runtime. Shutting that runtime down kills
    // its heartbeat and Redis tasks without releasing anything, like a process crash.
    let crashed_settings = settings(&url, &prefix, "fail_closed", 1);
    let crashed_limits = limits.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = RedisUsageStore::connect(&crashed_settings).await.unwrap();
            let guard = store
                .reserve("alice", &crashed_limits, Utc::now())
                .await
                .unwrap();
            std::mem::forget(guard);
            std::mem::forget(store);
        });
        rt.shutdown_background();
    })
    .join()
    .unwrap();

    // Until the 1 s lease runs out the dead instance's connection still counts.
    assert!(survivor
        .reserve("alice", &limits, Utc::now())
        .await
        .is_err());

    eventually(
        "lease expiry frees the slot",
        Duration::from_secs(6),
        || async { survivor.reserve("alice", &limits, Utc::now()).await.is_ok() },
    )
    .await;
}

/// TCP proxy to Redis that can be switched to swallow all traffic, emulating a Redis
/// that accepts connections but stops answering.
struct StallableProxy {
    addr: std::net::SocketAddr,
    stalled: Arc<AtomicBool>,
}

impl StallableProxy {
    async fn start(upstream: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stalled = Arc::new(AtomicBool::new(false));
        let flag = stalled.clone();
        tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    break;
                };
                let Ok(server) = TcpStream::connect(&upstream).await else {
                    continue;
                };
                let (mut cr, mut cw) = client.into_split();
                let (mut sr, mut sw) = server.into_split();
                let f1 = flag.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = cr.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        if !f1.load(Ordering::SeqCst) && sw.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                });
                let f2 = flag.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = sr.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        if !f2.load(Ordering::SeqCst) && cw.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        Self { addr, stalled }
    }

    fn url(&self) -> String {
        format!("redis://{}/0", self.addr)
    }

    fn stall(&self) {
        self.stalled.store(true, Ordering::SeqCst);
    }
}

fn upstream_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap();
    let host_port = rest.split('/').next().unwrap();
    host_port.rsplit('@').next().unwrap().to_string()
}

#[tokio::test]
async fn fail_closed_denies_limited_policies_but_admits_unlimited_ones() {
    let url = require_redis!();
    let proxy = StallableProxy::start(upstream_of(&url)).await;
    let store = RedisUsageStore::connect(&settings(
        &proxy.url(),
        &unique_prefix("closed"),
        "fail_closed",
        60,
    ))
    .await
    .unwrap();
    proxy.stall();

    let started = Instant::now();
    let err = store
        .reserve("alice", &active_limit(5), Utc::now())
        .await
        .unwrap_err();
    assert!(err.contains("unavailable"), "{err}");
    // The connection path must not hang on a stalled Redis.
    assert!(started.elapsed() < Duration::from_secs(2));

    // Nothing to enforce: admit using local counters instead of refusing everyone.
    assert!(store
        .reserve("alice", &PolicyAdmissionLimits::default(), Utc::now())
        .await
        .is_ok());
}

#[tokio::test]
async fn fail_open_falls_back_to_local_counters() {
    let url = require_redis!();
    let proxy = StallableProxy::start(upstream_of(&url)).await;
    let store = RedisUsageStore::connect(&settings(
        &proxy.url(),
        &unique_prefix("open"),
        "fail_open",
        60,
    ))
    .await
    .unwrap();
    proxy.stall();

    let limits = active_limit(1);
    let _held = store.reserve("alice", &limits, Utc::now()).await.unwrap();
    // Local fallback still enforces the limit for this instance.
    assert!(store.reserve("alice", &limits, Utc::now()).await.is_err());

    // Snapshots degrade to local counters instead of failing.
    assert_eq!(
        store.snapshot("alice", Utc::now()).await.active_connections,
        1
    );

    let mut buffer = Vec::new();
    prometheus::Encoder::encode(
        &prometheus::TextEncoder::new(),
        &prometheus::gather(),
        &mut buffer,
    )
    .unwrap();
    let text = String::from_utf8(buffer).unwrap();
    let total: f64 = text
        .lines()
        .filter(|line| line.starts_with("rustsocks_policy_usage_store_errors_total{"))
        .filter_map(|line| line.rsplit(' ').next()?.parse::<f64>().ok())
        .sum();
    assert!(total >= 1.0, "store errors must be counted");
}
