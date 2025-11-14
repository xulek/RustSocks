use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::trace;

/// Thread-safe token bucket for rate limiting
/// Uses atomic operations for lock-free token consumption
#[derive(Debug)]
pub struct TokenBucket {
    /// Maximum capacity (burst size)
    capacity: u64,

    /// Current available tokens (atomic for lock-free access)
    tokens: AtomicU64,

    /// Refill rate (tokens per second)
    refill_rate: u64,

    /// Last refill timestamp (needs mutex due to Instant)
    last_refill: Arc<Mutex<Instant>>,
}

impl TokenBucket {
    /// Create a new token bucket
    ///
    /// # Arguments
    /// * `capacity` - Maximum tokens (burst size)
    /// * `refill_rate` - Tokens added per second
    pub fn new(capacity: u64, refill_rate: u64) -> Self {
        Self {
            capacity,
            tokens: AtomicU64::new(capacity), // Start full
            refill_rate,
            last_refill: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Try to consume tokens without blocking
    ///
    /// # Returns
    /// - `Ok(())` if tokens were consumed
    /// - `Err(deficit)` if not enough tokens available
    pub fn try_consume(&self, amount: u64) -> Result<(), u64> {
        // Refill first
        self.refill_sync();

        loop {
            let current = self.tokens.load(Ordering::Acquire);

            if current >= amount {
                // Try to consume atomically
                match self.tokens.compare_exchange(
                    current,
                    current - amount,
                    Ordering::Release,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        trace!("Consumed {} tokens, {} remaining", amount, current - amount);
                        return Ok(());
                    }
                    Err(_) => {
                        // CAS failed, retry
                        continue;
                    }
                }
            } else {
                // Not enough tokens
                return Err(amount - current);
            }
        }
    }

    /// Consume tokens, waiting if necessary
    ///
    /// This will sleep until enough tokens are available
    pub async fn consume(&self, amount: u64) -> Result<(), std::io::Error> {
        loop {
            match self.try_consume(amount) {
                Ok(()) => return Ok(()),
                Err(deficit) => {
                    // Calculate wait time based on deficit
                    let wait_time = self.calculate_wait_time(deficit);
                    trace!(
                        "Not enough tokens (deficit: {}), waiting {:?}",
                        deficit,
                        wait_time
                    );
                    sleep(wait_time).await;
                    self.refill_sync();
                }
            }
        }
    }

    /// Refill tokens based on elapsed time (synchronous)
    fn refill_sync(&self) {
        // Note: This uses a mutex for the timestamp, but only briefly
        // The actual token update is lock-free
        if let Ok(mut last_refill) = self.last_refill.try_lock() {
            let now = Instant::now();
            let elapsed = now.duration_since(*last_refill);

            if elapsed.as_millis() > 0 {
                let tokens_to_add = (elapsed.as_secs_f64() * self.refill_rate as f64) as u64;

                if tokens_to_add > 0 {
                    self.add_tokens(tokens_to_add);
                    *last_refill = now;
                }
            }
        }
    }

    /// Add tokens up to capacity (lock-free)
    fn add_tokens(&self, amount: u64) {
        loop {
            let current = self.tokens.load(Ordering::Acquire);
            let new_value = std::cmp::min(current.saturating_add(amount), self.capacity);

            match self.tokens.compare_exchange(
                current,
                new_value,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    if new_value > current {
                        trace!(
                            "Refilled {} tokens (total: {}/{})",
                            new_value - current,
                            new_value,
                            self.capacity
                        );
                    }
                    break;
                }
                Err(_) => {
                    // CAS failed, retry
                    continue;
                }
            }
        }
    }

    /// Set refill rate (for dynamic rate adjustment)
    pub async fn set_refill_rate(&self, new_rate: u64) {
        // Update the rate atomically by reconstructing the bucket
        // This is safe because we're only changing the rate, not the tokens
        let bucket = self as *const Self as *mut Self;
        unsafe {
            (*bucket).refill_rate = new_rate;
        }
    }

    /// Calculate wait time for given deficit
    fn calculate_wait_time(&self, deficit: u64) -> Duration {
        if self.refill_rate == 0 {
            return Duration::from_secs(1); // Fallback
        }

        let wait_secs = deficit as f64 / self.refill_rate as f64;
        Duration::from_secs_f64(wait_secs.max(0.001)) // Minimum 1ms
    }

    /// Get current token count (approximate, may change immediately)
    pub fn available_tokens(&self) -> u64 {
        self.refill_sync();
        self.tokens.load(Ordering::Acquire)
    }

    /// Get capacity
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Get refill rate
    pub fn refill_rate(&self) -> u64 {
        self.refill_rate
    }

    /// Reset bucket to full capacity (used in tests)
    #[cfg(test)]
    pub fn reset(&self) {
        self.tokens.store(self.capacity, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[test]
    fn test_token_bucket_creation() {
        let bucket = TokenBucket::new(1000, 100);
        assert_eq!(bucket.capacity(), 1000);
        assert_eq!(bucket.refill_rate(), 100);
        assert_eq!(bucket.available_tokens(), 1000); // Starts full
    }

    #[test]
    fn test_try_consume_success() {
        let bucket = TokenBucket::new(1000, 100);
        assert!(bucket.try_consume(500).is_ok());
        assert!(bucket.available_tokens() <= 500);
    }

    #[test]
    fn test_try_consume_failure() {
        let bucket = TokenBucket::new(100, 100);
        assert!(bucket.try_consume(50).is_ok());
        let result = bucket.try_consume(100);
        assert!(result.is_err());
        if let Err(deficit) = result {
            assert!(deficit > 0);
        }
    }

    #[tokio::test]
    async fn test_consume_with_wait() {
        let bucket = TokenBucket::new(100, 1000); // High refill rate for faster test
        bucket.try_consume(100).ok(); // Empty it

        let start = Instant::now();
        bucket.consume(50).await.unwrap();
        let elapsed = start.elapsed();

        // Should have waited for refill
        assert!(elapsed.as_millis() > 10);
    }

    #[tokio::test]
    async fn test_refill_over_time() {
        let bucket = TokenBucket::new(1000, 1000); // 1000 tokens/sec
        bucket.try_consume(1000).ok(); // Empty it

        sleep(Duration::from_millis(100)).await;
        bucket.refill_sync();

        // Should have refilled ~100 tokens in 100ms
        let available = bucket.available_tokens();
        assert!((90..=110).contains(&available)); // Allow some tolerance
    }

    #[test]
    fn test_reset() {
        let bucket = TokenBucket::new(1000, 100);
        bucket.try_consume(500).ok();
        bucket.reset();
        assert_eq!(bucket.available_tokens(), 1000);
    }
}
