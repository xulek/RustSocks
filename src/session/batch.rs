use super::store::SessionStore;
use super::types::Session;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tokio::time::{interval, timeout, timeout_at, Duration, Instant, MissedTickBehavior};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct BatchConfig {
    pub batch_size: usize,
    pub batch_interval: Duration,
    pub queue_capacity: usize,
}

impl BatchConfig {
    pub fn from_settings(batch_size: usize, batch_interval_ms: u64, queue_capacity: usize) -> Self {
        Self {
            batch_size,
            batch_interval: Duration::from_millis(batch_interval_ms),
            queue_capacity: queue_capacity.max(batch_size).max(1),
        }
    }
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            batch_interval: Duration::from_secs(1),
            queue_capacity: 10_000,
        }
    }
}

const ENQUEUE_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_FLUSH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct BatchWriter {
    store: Arc<SessionStore>,
    config: BatchConfig,
    queue: Mutex<VecDeque<Session>>,
    flush_lock: Mutex<()>,
    flush_notify: Notify,
    space_notify: Notify,
    shutdown_notify: Notify,
    consecutive_failures: AtomicU64,
    last_flush_error: Mutex<Option<String>>,
    dropped_sessions: AtomicU64,
}

impl BatchWriter {
    pub fn new(store: Arc<SessionStore>, mut config: BatchConfig) -> Arc<Self> {
        config.batch_size = config.batch_size.max(1);
        if config.batch_interval.is_zero() {
            config.batch_interval = Duration::from_millis(1);
        }
        config.queue_capacity = config.queue_capacity.max(config.batch_size).max(1);
        let capacity = config.queue_capacity;
        Arc::new(Self {
            queue: Mutex::new(VecDeque::with_capacity(capacity)),
            flush_lock: Mutex::new(()),
            flush_notify: Notify::new(),
            space_notify: Notify::new(),
            shutdown_notify: Notify::new(),
            store,
            config,
            consecutive_failures: AtomicU64::new(0),
            last_flush_error: Mutex::new(None),
            dropped_sessions: AtomicU64::new(0),
        })
    }

    pub async fn enqueue(&self, session: Session) -> bool {
        let deadline = Instant::now() + ENQUEUE_WAIT_TIMEOUT;
        let mut session = Some(session);
        loop {
            // Register before checking the queue to avoid a lost wakeup.
            let notified = self.space_notify.notified();
            {
                let mut queue = self.queue.lock().await;
                if queue.len() < self.config.queue_capacity {
                    queue.push_back(session.take().expect("session queued once"));
                    if queue.len() >= self.config.batch_size {
                        self.flush_notify.notify_one();
                    }
                    return true;
                }
            }

            self.flush_notify.notify_one();
            if timeout_at(deadline, notified).await.is_err() {
                let dropped = self.dropped_sessions.fetch_add(1, Ordering::Relaxed) + 1;
                warn!(
                    queue_capacity = self.config.queue_capacity,
                    dropped,
                    "Session persistence queue remained full; dropping snapshot to preserve server liveness"
                );
                return false;
            }
        }
    }

    pub async fn flush(&self) {
        // Serialize writes so timer, threshold and shutdown flushes cannot overlap.
        let _flush_guard = self.flush_lock.lock().await;

        let batch: Vec<Session> = {
            let queue = self.queue.lock().await;
            if queue.is_empty() {
                return;
            }
            queue
                .iter()
                .take(self.config.batch_size.max(1))
                .cloned()
                .collect()
        };

        let count = batch.len();
        debug!(count, "Flushing session batch to store");

        if let Err(e) = self.store.save_batch(batch).await {
            let consecutive_failures =
                self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
            let pending = self.queue.lock().await.len();

            {
                let mut last_flush_error = self.last_flush_error.lock().await;
                *last_flush_error = Some(e.to_string());
            }

            error!(
                error = %e,
                consecutive_failures,
                pending,
                "Failed to persist session batch; bounded queue retained for retry"
            );
        } else {
            {
                let mut queue = self.queue.lock().await;
                for _ in 0..count {
                    let _ = queue.pop_front();
                }
            }
            self.space_notify.notify_waiters();
            self.consecutive_failures.store(0, Ordering::Relaxed);
            let mut last_flush_error = self.last_flush_error.lock().await;
            *last_flush_error = None;
            debug!(count, "Session batch persisted successfully");
        }
    }

    async fn flush_all(&self) {
        loop {
            let before = self.queue.lock().await.len();
            if before == 0 {
                return;
            }

            self.flush().await;
            let after = self.queue.lock().await.len();
            if after >= before {
                // No forward progress means the backing store is currently failing.
                // Keep the bounded queue intact for retry rather than spinning.
                return;
            }
        }
    }

    pub fn start(self: &Arc<Self>) {
        let interval_duration = self.config.batch_interval;
        let writer = Arc::clone(self);

        tokio::spawn(async move {
            let mut ticker = interval(interval_duration);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        writer.flush().await;
                    }
                    _ = writer.flush_notify.notified() => {
                        writer.flush().await;
                    }
                    _ = writer.shutdown_notify.notified() => {
                        writer.flush_all().await;
                        break;
                    }
                }
            }
        });

        info!(
            batch_size = self.config.batch_size,
            queue_capacity = self.config.queue_capacity,
            interval_ms = self.config.batch_interval.as_millis(),
            "Session batch writer started"
        );
    }

    pub async fn shutdown(&self) {
        self.shutdown_notify.notify_waiters();
        if timeout(SHUTDOWN_FLUSH_TIMEOUT, self.flush_all())
            .await
            .is_err()
        {
            error!(
                timeout_secs = SHUTDOWN_FLUSH_TIMEOUT.as_secs(),
                "Timed out while flushing the session persistence queue during shutdown"
            );
        }

        let pending = self.queue.lock().await.len();
        if pending > 0 {
            error!(
                pending,
                consecutive_failures = self.consecutive_failures.load(Ordering::Relaxed),
                "Batch writer shutdown completed with pending sessions still queued"
            );
        }
    }

    #[cfg(test)]
    async fn queue_len(&self) -> usize {
        self.queue.lock().await.len()
    }

    #[cfg(test)]
    fn consecutive_failures(&self) -> u64 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn dropped_sessions(&self) -> u64 {
        self.dropped_sessions.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{types::Protocol as SessionProtocol, ConnectionInfo, SessionStatus};
    use std::net::{IpAddr, Ipv4Addr};

    fn sample_session() -> Session {
        let connection = ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            source_port: 5000,
            dest_ip: "example.com".to_string(),
            dest_port: 443,
            protocol: SessionProtocol::Tcp,
        };

        let mut session = Session::new("alice".to_string(), connection, "allow", None);
        session.close(Some("finished".into()), SessionStatus::Closed);
        session
    }

    #[tokio::test]
    async fn flush_requeues_batch_when_store_write_fails() {
        let store = Arc::new(SessionStore::connect("sqlite::memory:").await.unwrap());
        store.close_for_test().await;

        let writer = BatchWriter::new(
            store,
            BatchConfig {
                batch_size: 1,
                batch_interval: Duration::from_secs(60),
                queue_capacity: 8,
            },
        );
        writer.enqueue(sample_session()).await;
        writer.flush().await;

        assert_eq!(writer.queue_len().await, 1);
        assert_eq!(writer.consecutive_failures(), 1);
    }
    #[tokio::test]
    async fn flush_all_drains_more_than_one_batch() {
        let store = Arc::new(SessionStore::connect("sqlite::memory:").await.unwrap());
        let writer = BatchWriter::new(
            store,
            BatchConfig {
                batch_size: 2,
                batch_interval: Duration::from_secs(60),
                queue_capacity: 8,
            },
        );

        for _ in 0..5 {
            assert!(writer.enqueue(sample_session()).await);
        }
        writer.flush_all().await;

        assert_eq!(writer.queue_len().await, 0);
        assert_eq!(writer.dropped_sessions(), 0);
    }
}
