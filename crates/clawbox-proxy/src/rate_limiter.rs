//! Per-tool rate limiting using a token bucket algorithm.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Token bucket rate limiter — allows burst up to capacity, refills over time.
#[non_exhaustive]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    default_capacity: u32,
    default_refill_rate: f64, // tokens per second
}

struct TokenBucket {
    tokens: f64,
    capacity: u32,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: u32, refill_rate: f64) -> Self {
        Self {
            tokens: capacity as f64,
            capacity,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity as f64);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl RateLimiter {
    /// Create a new rate limiter.
    /// `capacity` = max burst size, `refill_rate` = tokens per second.
    pub fn new(capacity: u32, refill_rate: f64) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            default_capacity: capacity,
            default_refill_rate: refill_rate,
        }
    }

    /// Try to consume one token for the given key (tool name or container ID).
    /// Returns true if allowed, false if rate limited.
    pub fn check(&self, key: &str) -> bool {
        // If another thread panicked while holding the lock, the mutex is "poisoned".
        // We recover by accepting the potentially-inconsistent inner data rather than
        // propagating the panic — rate limiting is best-effort and a stale bucket is
        // preferable to crashing all callers.
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let bucket = buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(self.default_capacity, self.default_refill_rate));
        bucket.try_consume()
    }

    /// Reset the limiter for a key (e.g., when a container is killed).
    pub fn reset(&self, key: &str) {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        buckets.remove(key);
    }

    /// Remove a single key's bucket. Returns `true` if the key existed.
    pub fn remove(&self, key: &str) -> bool {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        buckets.remove(key).is_some()
    }

    /// Remove all entries whose last activity is older than `max_age`.
    ///
    /// Should be called periodically (e.g. every 5 minutes) to prevent
    /// unbounded growth of the bucket map.
    pub fn cleanup_stale(&self, max_age: Duration) {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let cutoff = Instant::now() - max_age;
        buckets.retain(|_, bucket| bucket.last_refill > cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_burst_capacity() {
        let limiter = RateLimiter::new(3, 1.0);
        assert!(limiter.check("tool-a"));
        assert!(limiter.check("tool-a"));
        assert!(limiter.check("tool-a"));
        // Burst exhausted
        assert!(!limiter.check("tool-a"));
    }

    #[test]
    fn test_refill_after_wait() {
        let limiter = RateLimiter::new(1, 10.0); // 10 tokens/sec
        assert!(limiter.check("tool-b"));
        assert!(!limiter.check("tool-b"));
        // Wait for refill
        thread::sleep(Duration::from_millis(150));
        assert!(limiter.check("tool-b"));
    }

    #[test]
    fn test_independent_keys() {
        let limiter = RateLimiter::new(1, 1.0);
        assert!(limiter.check("a"));
        assert!(!limiter.check("a"));
        // Different key should have its own bucket
        assert!(limiter.check("b"));
    }

    #[test]
    fn test_reset() {
        let limiter = RateLimiter::new(1, 0.0); // no refill
        assert!(limiter.check("x"));
        assert!(!limiter.check("x"));
        limiter.reset("x");
        assert!(limiter.check("x"));
    }

    #[test]
    fn test_remove() {
        let limiter = RateLimiter::new(1, 0.0);
        assert!(limiter.check("y"));
        assert!(limiter.remove("y"));
        assert!(!limiter.remove("nonexistent"));
        // After remove, key gets a fresh bucket
        assert!(limiter.check("y"));
    }

    #[test]
    fn test_cleanup_stale() {
        let limiter = RateLimiter::new(1, 0.0);
        assert!(limiter.check("old"));
        thread::sleep(Duration::from_millis(50));
        assert!(limiter.check("new"));
        // Cleanup entries older than 30ms — "old" should be removed, "new" kept
        limiter.cleanup_stale(Duration::from_millis(30));
        // "old" was cleaned up, so it gets a fresh bucket
        assert!(limiter.check("old"));
    }
}
