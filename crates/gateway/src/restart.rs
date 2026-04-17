use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::time::sleep;
use tracing;

use crate::base::PlatformAdapter;

/// Tracks reconnection state for a single platform.
#[derive(Debug)]
struct PlatformReconnectState {
    /// Whether auto-reconnect is enabled for this platform.
    enabled: bool,
    /// Current backoff in seconds.
    backoff_secs: u64,
    /// Maximum backoff in seconds.
    max_backoff_secs: u64,
    /// Number of consecutive reconnection failures.
    failure_count: u32,
    /// Last reconnection attempt time.
    last_attempt: Option<Instant>,
}

impl PlatformReconnectState {
    fn new() -> Self {
        Self {
            enabled: true,
            backoff_secs: 5,
            max_backoff_secs: 300,
            failure_count: 0,
            last_attempt: None,
        }
    }

    /// Calculate delay before next reconnection attempt.
    fn delay(&self) -> Duration {
        Duration::from_secs(self.backoff_secs)
    }

    /// Record a failed reconnection, increase backoff.
    fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_attempt = Some(Instant::now());
        // Exponential backoff with ceiling
        self.backoff_secs = (self.backoff_secs * 2).min(self.max_backoff_secs);
    }

    /// Record a successful reconnection, reset backoff.
    fn record_success(&mut self) {
        self.failure_count = 0;
        self.backoff_secs = 5;
        self.last_attempt = Some(Instant::now());
    }
}

/// Manages automatic reconnection for platform adapters.
///
/// When a platform disconnects unexpectedly, the restart manager
/// will attempt to reconnect with exponential backoff.
///
/// Features:
/// - Exponential backoff (5s → 10s → 20s → ... → 5min max)
/// - Per-platform enable/disable
/// - Failure counting
/// - Max retry limit before giving up
pub struct GatewayRestartManager {
    /// Per-platform reconnection state.
    platforms: Mutex<std::collections::HashMap<String, PlatformReconnectState>>,
    /// Maximum number of reconnection attempts before giving up.
    max_retries: u32,
    /// Health check interval.
    health_check_interval: Duration,
}

impl GatewayRestartManager {
    pub fn new() -> Self {
        Self {
            platforms: Mutex::new(std::collections::HashMap::new()),
            max_retries: 20,
            health_check_interval: Duration::from_secs(60),
        }
    }

    /// Set maximum reconnection attempts.
    pub fn with_max_retries(mut self, max: u32) -> Self {
        self.max_retries = max;
        self
    }

    /// Set health check interval.
    pub fn with_health_check_interval(mut self, interval: Duration) -> Self {
        self.health_check_interval = interval;
        self
    }

    /// Register a platform for auto-reconnect monitoring.
    pub fn register(&self, name: &str) {
        self.platforms
            .lock()
            .insert(name.to_string(), PlatformReconnectState::new());
    }

    /// Disable auto-reconnect for a platform.
    pub fn disable_reconnect(&self, name: &str) {
        if let Some(state) = self.platforms.lock().get_mut(name) {
            state.enabled = false;
        }
    }

    /// Enable auto-reconnect for a platform.
    pub fn enable_reconnect(&self, name: &str) {
        if let Some(state) = self.platforms.lock().get_mut(name) {
            state.enabled = true;
        }
    }

    /// Handle a platform disconnection: attempt reconnection with backoff.
    pub async fn handle_disconnect(&self, name: &str, adapter: &dyn PlatformAdapter) {
        let mut platforms = self.platforms.lock();
        let state = match platforms.get_mut(name) {
            Some(s) => s,
            None => {
                tracing::warn!(platform = name, "Platform disconnected but not registered for restart");
                return;
            }
        };

        if !state.enabled {
            tracing::info!(platform = name, "Auto-reconnect disabled for platform, skipping");
            return;
        }

        drop(platforms);

        for attempt in 1..=self.max_retries {
            let delay = {
                let platforms = self.platforms.lock();
                let state = platforms.get(name).unwrap();
                state.delay()
            };

            tracing::info!(
                platform = name,
                attempt = attempt,
                delay_secs = delay.as_secs(),
                "Attempting to reconnect platform"
            );

            sleep(delay).await;

            match adapter.connect().await {
                Ok(()) => {
                    tracing::info!(platform = name, attempt = attempt, "Platform reconnected successfully");
                    let mut platforms = self.platforms.lock();
                    if let Some(state) = platforms.get_mut(name) {
                        state.record_success();
                    }
                    return;
                }
                Err(e) => {
                    tracing::warn!(
                        platform = name,
                        attempt = attempt,
                        error = %e,
                        "Platform reconnection failed"
                    );
                    let mut platforms = self.platforms.lock();
                    if let Some(state) = platforms.get_mut(name) {
                        state.record_failure();
                    }
                }
            }
        }

        tracing::error!(
            platform = name,
            max_retries = self.max_retries,
            "Platform reconnection gave up after max retries"
        );
    }

    /// Get the health check interval.
    pub fn health_check_interval(&self) -> Duration {
        self.health_check_interval
    }

    /// Get failure count for a platform.
    pub fn failure_count(&self, name: &str) -> u32 {
        self.platforms
            .lock()
            .get(name)
            .map(|s| s.failure_count)
            .unwrap_or(0)
    }

    /// Get all platforms being monitored.
    pub fn monitored_platforms(&self) -> Vec<String> {
        self.platforms.lock().keys().cloned().collect()
    }

    /// Check if a platform is eligible for auto-reconnect.
    pub fn is_reconnect_enabled(&self, name: &str) -> bool {
        self.platforms
            .lock()
            .get(name)
            .map(|s| s.enabled)
            .unwrap_or(false)
    }
}

impl Default for GatewayRestartManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_manager_is_empty() {
        let manager = GatewayRestartManager::new();
        assert!(manager.monitored_platforms().is_empty());
    }

    #[test]
    fn test_register_platform() {
        let manager = GatewayRestartManager::new();
        manager.register("telegram");
        manager.register("discord");

        let platforms = manager.monitored_platforms();
        assert_eq!(platforms.len(), 2);
        assert!(platforms.contains(&"telegram".to_string()));
        assert!(platforms.contains(&"discord".to_string()));
    }

    #[test]
    fn test_disable_reconnect() {
        let manager = GatewayRestartManager::new();
        manager.register("telegram");

        assert!(manager.is_reconnect_enabled("telegram"));
        manager.disable_reconnect("telegram");
        assert!(!manager.is_reconnect_enabled("telegram"));
    }

    #[test]
    fn test_enable_reconnect() {
        let manager = GatewayRestartManager::new();
        manager.register("telegram");
        manager.disable_reconnect("telegram");
        manager.enable_reconnect("telegram");
        assert!(manager.is_reconnect_enabled("telegram"));
    }

    #[test]
    fn test_failure_count_nonexistent() {
        let manager = GatewayRestartManager::new();
        assert_eq!(manager.failure_count("nonexistent"), 0);
    }

    #[test]
    fn test_reconnect_disabled_for_unregistered() {
        let manager = GatewayRestartManager::new();
        assert!(!manager.is_reconnect_enabled("unknown"));
    }

    #[test]
    fn test_max_retries_custom() {
        let manager = GatewayRestartManager::new().with_max_retries(5);
        // Can't easily test the actual reconnect loop, but verify the config is set
        // by checking the builder pattern works
        assert_eq!(manager.max_retries, 5);
    }

    #[test]
    fn test_health_check_interval_custom() {
        let manager = GatewayRestartManager::new()
            .with_health_check_interval(Duration::from_secs(30));
        assert_eq!(manager.health_check_interval(), Duration::from_secs(30));
    }

    #[test]
    fn test_default_manager() {
        let manager = GatewayRestartManager::default();
        assert_eq!(manager.health_check_interval(), Duration::from_secs(60));
    }

    #[test]
    fn test_reconnect_state_exponential_backoff() {
        let mut state = PlatformReconnectState::new();
        assert_eq!(state.backoff_secs, 5);

        state.record_failure();
        assert_eq!(state.backoff_secs, 10);

        state.record_failure();
        assert_eq!(state.backoff_secs, 20);

        state.record_failure();
        assert_eq!(state.backoff_secs, 40);

        // After success, reset
        state.record_success();
        assert_eq!(state.backoff_secs, 5);
        assert_eq!(state.failure_count, 0);
    }

    #[test]
    fn test_reconnect_state_max_backoff() {
        let mut state = PlatformReconnectState {
            max_backoff_secs: 60,
            ..PlatformReconnectState::new()
        };

        for _ in 0..10 {
            state.record_failure();
        }

        // Should cap at max_backoff_secs
        assert_eq!(state.backoff_secs, 60);
    }
}
