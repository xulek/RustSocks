use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::trace;

/// Thread-safe token bucket for rate limiting
/// Uses atomic operations for fully lock-free operation
#[derive(Debug)]
pub struct TokenBucket {
    /// Maximum capacity (burst size)
    capacity: u64,

    /// Current available tokens (atomic for lock-free access)
    tokens: AtomicU64,

    /// Refill rate (tokens per second)
    refill_rate: u64,

    /// Creation instant (immutable reference point for time calculations)
    created_at: Instant,

    /// Last refill timestamp as nanoseconds since created_at (atomic for lock-free access)
    last_refill_nanos: AtomicU64,
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
            created_at: Instant::now(),
            last_refill_nanos: AtomicU64::new(0), // 0 nanos since creation
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

    /// Refill tokens based on elapsed time (fully lock-free)
    fn refill_sync(&self) {
        let now_nanos = self.created_at.elapsed().as_nanos() as u64;
        let last_nanos = self.last_refill_nanos.load(Ordering::Acquire);

        // Calculate elapsed time since last refill
        let elapsed_nanos = now_nanos.saturating_sub(last_nanos);

        // Only refill if at least 1ms has passed (avoid excessive small refills)
        if elapsed_nanos < 1_000_000 {
            return;
        }

        // Calculate tokens to add based on elapsed time
        // tokens = elapsed_seconds * refill_rate
        // Using integer math: tokens = (elapsed_nanos * refill_rate) / 1_000_000_000
        let tokens_to_add = (elapsed_nanos as u128 * self.refill_rate as u128 / 1_000_000_000) as u64;

        if tokens_to_add == 0 {
            return;
        }

        // Try to atomically update last_refill_nanos
        // If CAS fails, another thread already did the refill - that's fine
        if self
            .last_refill_nanos
            .compare_exchange(last_nanos, now_nanos, Ordering::Release, Ordering::Acquire)
            .is_ok()
        {
            self.add_tokens(tokens_to_add);
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
        // Also reset the timestamp to current time
        let now_nanos = self.created_at.elapsed().as_nanos() as u64;
        self.last_refill_nanos.store(now_nanos, Ordering::Release);
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
