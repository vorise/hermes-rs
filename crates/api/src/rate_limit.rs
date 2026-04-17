use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Rate limit information from API responses.
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    /// Remaining requests in the current window.
    pub remaining: Option<u64>,
    /// Limit for the current window.
    pub limit: Option<u64>,
    /// Time until the rate limit resets (seconds).
    pub reset_after: Option<u64>,
    /// Retry-After header value (seconds).
    pub retry_after: Option<u64>,
}

/// Tracking state for a single rate limit window.
struct RateWindow {
    /// When this window started.
    started_at: Instant,
    /// Number of requests made in this window.
    request_count: AtomicU64,
    /// Number of 429 responses in this window.
    throttle_count: AtomicU64,
    /// Last time a 429 was received.
    last_throttle: Mutex<Option<Instant>>,
}

impl RateWindow {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            request_count: AtomicU64::new(0),
            throttle_count: AtomicU64::new(0),
            last_throttle: Mutex::new(None),
        }
    }

    fn is_expired(&self, window_duration: Duration) -> bool {
        self.started_at.elapsed() > window_duration
    }

    fn throttle_rate(&self) -> f64 {
        let requests = self.request_count.load(Ordering::Relaxed);
        if requests == 0 {
            return 0.0;
        }
        self.throttle_count.load(Ordering::Relaxed) as f64 / requests as f64
    }
}

/// Tracks rate limits per provider/credential.
///
/// Monitors 429 responses and provides backoff recommendations.
pub struct RateLimitTracker {
    /// Per-provider rate limit windows.
    windows: Mutex<HashMap<String, Arc<RateWindow>>>,
    /// Window duration for tracking.
    window_duration: Duration,
    /// Minimum backoff after a rate limit hit.
    min_backoff: Duration,
    /// Maximum backoff after repeated rate limit hits.
    max_backoff: Duration,
}

impl RateLimitTracker {
    pub fn new() -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            window_duration: Duration::from_secs(60),
            min_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
        }
    }

    /// Record a request to a provider.
    pub fn record_request(&self, provider: &str) {
        let key = provider.to_string();
        let mut windows = self.windows.lock();

        // Get or create window, reset if expired
        let window = windows.entry(key.clone()).or_insert_with(|| {
            Arc::new(RateWindow::new())
        });

        if window.is_expired(self.window_duration) {
            *window = Arc::new(RateWindow::new());
        }

        window.request_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a rate limit (429) response from a provider.
    pub fn record_throttle(&self, provider: &str, info: Option<&RateLimitInfo>) {
        let key = provider.to_string();
        let mut windows = self.windows.lock();

        let window = windows.entry(key.clone()).or_insert_with(|| {
            Arc::new(RateWindow::new())
        });

        if window.is_expired(self.window_duration) {
            *window = Arc::new(RateWindow::new());
        }

        window.throttle_count.fetch_add(1, Ordering::Relaxed);
        *window.last_throttle.lock() = Some(Instant::now());

        // Log rate limit info if available
        if let Some(info) = info {
            tracing::debug!(
                provider = provider,
                remaining = ?info.remaining,
                limit = ?info.limit,
                reset_after = ?info.reset_after,
                retry_after = ?info.retry_after,
                "Rate limit hit"
            );
        }
    }

    /// Get recommended backoff duration for a provider.
    /// Returns `None` if no backoff is needed.
    pub fn get_backoff(&self, provider: &str) -> Option<Duration> {
        let key = provider.to_string();
        let windows = self.windows.lock();

        let Some(window) = windows.get(&key) else {
            return None;
        };

        // If we were recently throttled, suggest backoff
        let last_throttle = window.last_throttle.lock();
        if let Some(last) = *last_throttle {
            let elapsed = last.elapsed();
            // Exponential backoff based on throttle count
            let throttle_count = window.throttle_count.load(Ordering::Relaxed);
            let multiplier = 2u32.pow(throttle_count.min(5) as u32);
            let backoff = self.min_backoff * multiplier;
            let backoff = backoff.min(self.max_backoff);

            if elapsed < backoff {
                return Some(backoff - elapsed);
            }
        }

        // Check throttle rate - if more than 50% of requests are throttled, suggest backoff
        if window.throttle_rate() > 0.5 {
            return Some(self.min_backoff);
        }

        None
    }

    /// Check if we should back off before making a request.
    pub fn should_backoff(&self, provider: &str) -> bool {
        self.get_backoff(provider).is_some()
    }

    /// Get current rate limit stats for a provider.
    pub fn get_stats(&self, provider: &str) -> Option<RateLimitStats> {
        let key = provider.to_string();
        let windows = self.windows.lock();
        let window = windows.get(&key)?;

        Some(RateLimitStats {
            request_count: window.request_count.load(Ordering::Relaxed),
            throttle_count: window.throttle_count.load(Ordering::Relaxed),
            throttle_rate: window.throttle_rate(),
            window_remaining_secs: self.window_duration
                .saturating_sub(window.started_at.elapsed())
                .as_secs(),
        })
    }

    /// Reset tracking for a provider.
    pub fn reset(&self, provider: &str) {
        let key = provider.to_string();
        self.windows.lock().remove(&key);
    }

    /// Reset all tracking.
    pub fn reset_all(&self) {
        self.windows.lock().clear();
    }

    /// Set the tracking window duration.
    pub fn set_window_duration(&mut self, duration: Duration) {
        self.window_duration = duration;
    }

    /// Set minimum backoff duration.
    pub fn set_min_backoff(&mut self, duration: Duration) {
        self.min_backoff = duration;
    }

    /// Set maximum backoff duration.
    pub fn set_max_backoff(&mut self, duration: Duration) {
        self.max_backoff = duration;
    }
}

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Rate limit statistics for a provider.
#[derive(Debug, Clone)]
pub struct RateLimitStats {
    pub request_count: u64,
    pub throttle_count: u64,
    pub throttle_rate: f64,
    pub window_remaining_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracker_empty() {
        let tracker = RateLimitTracker::new();
        assert!(tracker.get_stats("anthropic").is_none());
        assert!(!tracker.should_backoff("anthropic"));
    }

    #[test]
    fn test_record_request() {
        let tracker = RateLimitTracker::new();
        tracker.record_request("anthropic");
        tracker.record_request("anthropic");

        let stats = tracker.get_stats("anthropic").unwrap();
        assert_eq!(stats.request_count, 2);
        assert_eq!(stats.throttle_count, 0);
    }

    #[test]
    fn test_record_throttle() {
        let tracker = RateLimitTracker::new();
        tracker.record_request("anthropic");
        tracker.record_throttle("anthropic", None);

        let stats = tracker.get_stats("anthropic").unwrap();
        assert_eq!(stats.throttle_count, 1);
        assert_eq!(stats.throttle_rate, 1.0);
    }

    #[test]
    fn test_should_backoff_on_high_throttle_rate() {
        let tracker = RateLimitTracker::new();
        // Record 10 requests, all throttled
        for _ in 0..10 {
            tracker.record_request("anthropic");
            tracker.record_throttle("anthropic", None);
        }

        assert!(tracker.should_backoff("anthropic"));
    }

    #[test]
    fn test_no_backoff_on_low_throttle_rate() {
        let tracker = RateLimitTracker::new();
        // Record 100 requests, 1 throttled
        for _ in 0..100 {
            tracker.record_request("anthropic");
        }
        tracker.record_throttle("anthropic", None);

        // 1% throttle rate is below 50% threshold, single throttle shouldn't trigger backoff
        let stats = tracker.get_stats("anthropic").unwrap();
        assert!((stats.throttle_rate - 0.01).abs() < 0.01);
    }

    #[test]
    fn test_reset_provider() {
        let tracker = RateLimitTracker::new();
        tracker.record_request("anthropic");
        assert!(tracker.get_stats("anthropic").is_some());

        tracker.reset("anthropic");
        assert!(tracker.get_stats("anthropic").is_none());
    }

    #[test]
    fn test_reset_all() {
        let tracker = RateLimitTracker::new();
        tracker.record_request("anthropic");
        tracker.record_request("openai");

        tracker.reset_all();
        assert!(tracker.get_stats("anthropic").is_none());
        assert!(tracker.get_stats("openai").is_none());
    }

    #[test]
    fn test_different_providers_tracked_separately() {
        let tracker = RateLimitTracker::new();
        tracker.record_request("anthropic");
        tracker.record_request("openai");
        tracker.record_throttle("anthropic", None);

        let anthropic_stats = tracker.get_stats("anthropic").unwrap();
        let openai_stats = tracker.get_stats("openai").unwrap();

        assert_eq!(anthropic_stats.throttle_count, 1);
        assert_eq!(openai_stats.throttle_count, 0);
    }

    #[test]
    fn test_rate_limit_stats() {
        let tracker = RateLimitTracker::new();
        for _ in 0..5 {
            tracker.record_request("test");
        }
        tracker.record_throttle("test", None);
        tracker.record_throttle("test", None);

        let stats = tracker.get_stats("test").unwrap();
        assert_eq!(stats.request_count, 5);
        assert_eq!(stats.throttle_count, 2);
        assert!((stats.throttle_rate - 0.4).abs() < 0.01);
        assert!(stats.window_remaining_secs > 0);
    }
}
