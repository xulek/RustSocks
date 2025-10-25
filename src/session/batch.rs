use super::store::SessionStore;
use super::types::Session;
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
    queue: Mutex<Vec<Session>>,
    flush_notify: Notify,
    shutdown_notify: Notify,
}

impl BatchWriter {
    pub fn new(store: Arc<SessionStore>, config: BatchConfig) -> Arc<Self> {
        let capacity = config.batch_size;
        Arc::new(Self {
            queue: Mutex::new(Vec::with_capacity(capacity)),
            flush_notify: Notify::new(),
            shutdown_notify: Notify::new(),
            store,
            config,
        })
    }

    pub async fn enqueue(&self, session: Session) {
        let mut queue = self.queue.lock().await;
        queue.push(session);

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

        let mut batch = Vec::with_capacity(queue.len());
        std::mem::swap(&mut *queue, &mut batch);
        drop(queue);

        let count = batch.len();
        debug!(count, "Flushing session batch to store");

        if let Err(e) = self.store.save_batch(batch).await {
            error!(error = %e, "Failed to persist session batch");
        } else {
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
    }
}
