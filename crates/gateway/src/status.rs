use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Status of a single platform connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformStatus {
    /// Platform name.
    pub name: String,
    /// Whether the platform is currently connected.
    pub connected: bool,
    /// Number of messages sent since startup.
    pub messages_sent: u64,
    /// Number of messages received since startup.
    pub messages_received: u64,
    /// Number of errors since startup.
    pub errors: u64,
    /// Last error message, if any.
    pub last_error: Option<String>,
    /// Time of last activity (UTC epoch seconds).
    pub last_activity: Option<u64>,
}

impl PlatformStatus {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            connected: false,
            messages_sent: 0,
            messages_received: 0,
            errors: 0,
            last_error: None,
            last_activity: None,
        }
    }

    pub fn record_send(&mut self) {
        self.messages_sent += 1;
        self.last_activity = Some(now_secs());
    }

    pub fn record_receive(&mut self) {
        self.messages_received += 1;
        self.last_activity = Some(now_secs());
    }

    pub fn record_error(&mut self, error: &str) {
        self.errors += 1;
        self.last_error = Some(error.to_string());
        self.last_activity = Some(now_secs());
    }
}

/// Gateway-wide status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStatusInfo {
    /// Whether the gateway is running.
    pub running: bool,
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Number of connected platforms.
    pub connected_platforms: usize,
    /// Total platforms configured.
    pub total_platforms: usize,
    /// Number of active sessions.
    pub active_sessions: usize,
    /// Total messages processed since startup.
    pub total_messages: u64,
    /// Total errors since startup.
    pub total_errors: u64,
    /// Platform-specific statuses.
    pub platforms: Vec<PlatformStatus>,
}

/// Gateway status tracker.
///
/// Tracks platform connection status, message counts, errors,
/// and gateway uptime.
pub struct StatusTracker {
    /// When the gateway was started.
    start_time: Mutex<Instant>,
    /// Per-platform status.
    platforms: Mutex<std::collections::HashMap<String, PlatformStatus>>,
    /// Whether the gateway is running.
    running: Mutex<bool>,
}

impl StatusTracker {
    pub fn new() -> Self {
        Self {
            start_time: Mutex::new(Instant::now()),
            platforms: Mutex::new(std::collections::HashMap::new()),
            running: Mutex::new(false),
        }
    }

    /// Mark the gateway as running.
    pub fn mark_running(&self, running: bool) {
        *self.running.lock() = running;
        if running {
            *self.start_time.lock() = Instant::now();
        }
    }

    /// Register a platform.
    pub fn register_platform(&self, name: &str) {
        self.platforms.lock().entry(name.to_string())
            .or_insert_with(|| PlatformStatus::new(name));
    }

    /// Mark a platform as connected.
    pub fn platform_connected(&self, name: &str) {
        if let Some(status) = self.platforms.lock().get_mut(name) {
            status.connected = true;
        }
    }

    /// Mark a platform as disconnected.
    pub fn platform_disconnected(&self, name: &str) {
        if let Some(status) = self.platforms.lock().get_mut(name) {
            status.connected = false;
        }
    }

    /// Record a message sent on a platform.
    pub fn record_send(&self, platform: &str) {
        if let Some(status) = self.platforms.lock().get_mut(platform) {
            status.record_send();
        }
    }

    /// Record a message received on a platform.
    pub fn record_receive(&self, platform: &str) {
        if let Some(status) = self.platforms.lock().get_mut(platform) {
            status.record_receive();
        }
    }

    /// Record an error on a platform.
    pub fn record_error(&self, platform: &str, error: &str) {
        if let Some(status) = self.platforms.lock().get_mut(platform) {
            status.record_error(error);
        }
    }

    /// Get the current gateway status.
    pub fn get_info(&self, active_sessions: usize) -> GatewayStatusInfo {
        let platforms = self.platforms.lock();
        let connected = platforms.values().filter(|p| p.connected).count();
        let total = platforms.len();
        let total_messages: u64 = platforms.values().map(|p| p.messages_received).sum();
        let total_errors: u64 = platforms.values().map(|p| p.errors).sum();

        GatewayStatusInfo {
            running: *self.running.lock(),
            uptime_secs: self.start_time.lock().elapsed().as_secs(),
            connected_platforms: connected,
            total_platforms: total,
            active_sessions,
            total_messages,
            total_errors,
            platforms: platforms.values().cloned().collect(),
        }
    }

    /// Get a specific platform's status.
    pub fn platform_status(&self, name: &str) -> Option<PlatformStatus> {
        self.platforms.lock().get(name).cloned()
    }
}

impl Default for StatusTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format the gateway status for display.
pub fn format_status(info: &GatewayStatusInfo) -> String {
    let mut lines = Vec::new();

    lines.push(format!("Gateway Status"));
    lines.push(format!("  Running: {}", if info.running { "Yes" } else { "No" }));
    lines.push(format!("  Uptime: {}s", info.uptime_secs));
    lines.push(format!("  Platforms: {}/{} connected", info.connected_platforms, info.total_platforms));
    lines.push(format!("  Active sessions: {}", info.active_sessions));
    lines.push(format!("  Messages processed: {}", info.total_messages));
    lines.push(format!("  Errors: {}", info.total_errors));

    if !info.platforms.is_empty() {
        lines.push(String::new());
        lines.push("Platforms:".to_string());
        for p in &info.platforms {
            let status = if p.connected { "connected" } else { "disconnected" };
            lines.push(format!("  {} ({}) - {} sent, {} recv, {} errors",
                p.name, status, p.messages_sent, p.messages_received, p.errors));
            if let Some(ref err) = p.last_error {
                lines.push(format!("    Last error: {err}"));
            }
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_status_new() {
        let status = PlatformStatus::new("telegram");
        assert!(!status.connected);
        assert_eq!(status.messages_sent, 0);
        assert_eq!(status.name, "telegram");
    }

    #[test]
    fn test_platform_status_record() {
        let mut status = PlatformStatus::new("discord");
        status.record_send();
        status.record_send();
        status.record_receive();
        status.record_error("API error");

        assert_eq!(status.messages_sent, 2);
        assert_eq!(status.messages_received, 1);
        assert_eq!(status.errors, 1);
        assert_eq!(status.last_error, Some("API error".to_string()));
    }

    #[test]
    fn test_status_tracker() {
        let tracker = StatusTracker::new();
        tracker.mark_running(true);
        tracker.register_platform("telegram");
        tracker.register_platform("discord");
        tracker.platform_connected("telegram");

        let info = tracker.get_info(3);
        assert!(info.running);
        assert_eq!(info.connected_platforms, 1);
        assert_eq!(info.total_platforms, 2);
        assert_eq!(info.active_sessions, 3);
    }

    #[test]
    fn test_platform_disconnected() {
        let tracker = StatusTracker::new();
        tracker.register_platform("telegram");
        tracker.platform_connected("telegram");
        tracker.platform_disconnected("telegram");

        let status = tracker.platform_status("telegram").unwrap();
        assert!(!status.connected);
    }

    #[test]
    fn test_format_status() {
        let tracker = StatusTracker::new();
        tracker.mark_running(true);
        tracker.register_platform("telegram");
        tracker.platform_connected("telegram");
        tracker.record_receive("telegram");

        let info = tracker.get_info(1);
        let formatted = format_status(&info);
        assert!(formatted.contains("Gateway Status"));
        assert!(formatted.contains("telegram"));
        assert!(formatted.contains("connected"));
    }
}
