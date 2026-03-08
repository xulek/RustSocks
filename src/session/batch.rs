use super::store::SessionStore;
use super::types::Session;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::{debug, error, info};

#[derive(Debug, Clone)]
pub struct BatchConfig {
    pub batch_size: usize,
    pub batch_interval: Duration,
}

impl BatchConfig {
    pub fn from_settings(batch_size: usize, batch_interval_ms: u64) -> Self {
        Self {
            batch_size,
            batch_interval: Duration::from_millis(batch_interval_ms),
        }
    }
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            batch_interval: Duration::from_secs(1),
        }
    }
}

#[derive(Debug)]
pub struct BatchWriter {
    store: Arc<SessionStore>,
    config: BatchConfig,
    queue: Mutex<VecDeque<Session>>,
    flush_notify: Notify,
    shutdown_notify: Notify,
    consecutive_failures: AtomicU64,
    last_flush_error: Mutex<Option<String>>,
}

impl BatchWriter {
    pub fn new(store: Arc<SessionStore>, config: BatchConfig) -> Arc<Self> {
        let capacity = config.batch_size;
        Arc::new(Self {
            queue: Mutex::new(VecDeque::with_capacity(capacity)),
            flush_notify: Notify::new(),
            shutdown_notify: Notify::new(),
            store,
            config,
            consecutive_failures: AtomicU64::new(0),
            last_flush_error: Mutex::new(None),
        })
    }

    pub async fn enqueue(&self, session: Session) {
        let mut queue = self.queue.lock().await;
        queue.push_back(session);

        if queue.len() >= self.config.batch_size {
            debug!(
                len = queue.len(),
                "Batch size threshold reached, triggering flush"
            );
            self.flush_notify.notify_one();
        }
    }

    pub async fn flush(&self) {
        let mut queue = self.queue.lock().await;

        if queue.is_empty() {
            return;
        }

        let batch: Vec<Session> = queue.drain(..).collect();
        drop(queue);

        let count = batch.len();
        debug!(count, "Flushing session batch to store");

        if let Err(e) = self.store.save_batch(batch.clone()).await {
            let consecutive_failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
            let mut queue = self.queue.lock().await;
            for session in batch.into_iter().rev() {
                queue.push_front(session);
            }
            let pending = queue.len();
            drop(queue);

            {
                let mut last_flush_error = self.last_flush_error.lock().await;
                *last_flush_error = Some(e.to_string());
            }

            error!(
                error = %e,
                consecutive_failures,
                pending,
                "Failed to persist session batch; batch requeued"
            );
        } else {
            self.consecutive_failures.store(0, Ordering::Relaxed);
            let mut last_flush_error = self.last_flush_error.lock().await;
            *last_flush_error = None;
            debug!(count, "Session batch persisted successfully");
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
                        writer.flush().await;
                        break;
                    }
                }
            }
        });

        info!(
            batch_size = self.config.batch_size,
            interval_ms = self.config.batch_interval.as_millis(),
            "Session batch writer started"
        );
    }

    pub async fn shutdown(&self) {
        self.shutdown_notify.notify_waiters();
        self.flush().await;

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
            },
        );
        writer.enqueue(sample_session()).await;
        writer.flush().await;

        assert_eq!(writer.queue_len().await, 1);
        assert_eq!(writer.consecutive_failures(), 1);
    }
}
