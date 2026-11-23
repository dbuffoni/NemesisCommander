//! A token bucket that limits throughput in bytes per second.

use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use nmc_ops::Throttle;
use tokio::time::Instant;

/// Limits throughput to `rate` bytes per second, allowing bursts of up to
/// one second's worth of data.
///
/// The bucket can be shared (`Arc<TokenBucket>`) by many copies to enforce
/// a global limit, and the rate can be changed while transfers run.
pub struct TokenBucket {
    state: Mutex<State>,
}

struct State {
    rate: u64,
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    /// A bucket for `bytes_per_second` (0 means unlimited).
    pub fn new(bytes_per_second: u64) -> Self {
        TokenBucket {
            state: Mutex::new(State {
                rate: bytes_per_second,
                tokens: bytes_per_second as f64,
                last: Instant::now(),
            }),
        }
    }

    /// Changes the rate (0 = unlimited).
    pub fn set_rate(&self, bytes_per_second: u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.rate = bytes_per_second;
        state.tokens = state.tokens.min(bytes_per_second as f64);
    }

    /// The current rate in bytes per second (0 = unlimited).
    pub fn rate(&self) -> u64 {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).rate
    }

    /// Takes `bytes` tokens and returns how long the caller must wait before
    /// sending them. Tokens may go negative: that debt is what makes callers
    /// wait, so large chunks are handled without starving anyone.
    fn reserve(&self, bytes: usize) -> Duration {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.rate == 0 {
            return Duration::ZERO;
        }
        let now = Instant::now();
        let elapsed = now.duration_since(state.last).as_secs_f64();
        state.last = now;
        let capacity = state.rate as f64;
        state.tokens = (state.tokens + elapsed * capacity).min(capacity);
        state.tokens -= bytes as f64;
        if state.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-state.tokens / capacity)
        }
    }
}

#[async_trait]
impl Throttle for TokenBucket {
    async fn acquire(&self, bytes: usize) {
        let wait = self.reserve(bytes);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    fn limited(&self) -> bool {
        self.rate() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn limits_average_rate() {
        let bucket = TokenBucket::new(1000);
        let start = Instant::now();
        // 1000 bytes of burst are free, the next 3000 take ~3 seconds.
        for _ in 0..40 {
            bucket.acquire(100).await;
        }
        let elapsed = start.elapsed().as_secs_f64();
        assert!((2.9..3.2).contains(&elapsed), "elapsed {elapsed}");
    }

    #[tokio::test(start_paused = true)]
    async fn zero_means_unlimited() {
        let bucket = TokenBucket::new(0);
        let start = Instant::now();
        bucket.acquire(10_000_000).await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn rate_can_change() {
        let bucket = TokenBucket::new(100);
        bucket.set_rate(0);
        let start = Instant::now();
        bucket.acquire(1_000_000).await;
        assert_eq!(start.elapsed(), Duration::ZERO);
        assert_eq!(bucket.rate(), 0);
    }
}
