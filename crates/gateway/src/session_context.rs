use std::collections::HashMap;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Tracks per-session metadata within the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    /// Platform this session belongs to (e.g., "telegram", "discord").
    pub platform: String,
    /// Platform-specific user ID.
    pub user_id: String,
    /// Platform-specific chat/channel ID.
    pub chat_id: String,
    /// Current turn number (incremented per user message).
    pub current_turn: u64,
    /// Timestamp of last activity (UTC epoch seconds).
    pub last_activity: u64,
    /// Whether the session is currently being processed.
    pub is_active: bool,
}

impl SessionContext {
    pub fn new(platform: &str, user_id: &str, chat_id: &str) -> Self {
        Self {
            platform: platform.to_string(),
            user_id: user_id.to_string(),
            chat_id: chat_id.to_string(),
            current_turn: 0,
            last_activity: now_secs(),
            is_active: false,
        }
    }

    /// Increment the turn counter and update last activity.
    pub fn record_turn(&mut self) {
        self.current_turn += 1;
        self.last_activity = now_secs();
    }

    /// Mark the session as currently being processed.
    pub fn mark_active(&mut self, active: bool) {
        self.is_active = active;
        if active {
            self.last_activity = now_secs();
        }
    }
}

/// Gateway-wide session context tracker.
///
/// Tracks per-session metadata: platform user ID, channel, current turn,
/// last activity timestamp, and active processing state.
pub struct SessionContextTracker {
    contexts: Mutex<HashMap<String, SessionContext>>,
}

impl SessionContextTracker {
    pub fn new() -> Self {
        Self {
            contexts: Mutex::new(HashMap::new()),
        }
    }

    /// Get or create a session context for a platform-user pair.
    pub fn get_or_create(&self, platform: &str, user_id: &str, chat_id: &str) -> SessionContext {
        let key = session_key(platform, user_id);
        let mut contexts = self.contexts.lock();
        contexts
            .entry(key)
            .or_insert_with(|| SessionContext::new(platform, user_id, chat_id))
            .clone()
    }

    /// Record a user turn for a platform-user pair.
    pub fn record_turn(&self, platform: &str, user_id: &str) {
        let key = session_key(platform, user_id);
        if let Some(ctx) = self.contexts.lock().get_mut(&key) {
            ctx.record_turn();
        }
    }

    /// Mark a session as active/inactive (being processed by the agent).
    pub fn set_active(&self, platform: &str, user_id: &str, active: bool) {
        let key = session_key(platform, user_id);
        if let Some(ctx) = self.contexts.lock().get_mut(&key) {
            ctx.mark_active(active);
        }
    }

    /// Check if a session is currently being processed.
    pub fn is_active(&self, platform: &str, user_id: &str) -> bool {
        let key = session_key(platform, user_id);
        self.contexts
            .lock()
            .get(&key)
            .map(|c| c.is_active)
            .unwrap_or(false)
    }

    /// Get the current turn count for a platform-user pair.
    pub fn current_turn(&self, platform: &str, user_id: &str) -> u64 {
        let key = session_key(platform, user_id);
        self.contexts
            .lock()
            .get(&key)
            .map(|c| c.current_turn)
            .unwrap_or(0)
    }

    /// Get a session context, if it exists.
    pub fn get(&self, platform: &str, user_id: &str) -> Option<SessionContext> {
        let key = session_key(platform, user_id);
        self.contexts.lock().get(&key).cloned()
    }

    /// Remove a session context.
    pub fn remove(&self, platform: &str, user_id: &str) -> bool {
        let key = session_key(platform, user_id);
        self.contexts.lock().remove(&key).is_some()
    }

    /// List all active session contexts.
    pub fn active_sessions(&self) -> Vec<SessionContext> {
        self.contexts
            .lock()
            .values()
            .filter(|c| c.is_active)
            .cloned()
            .collect()
    }

    /// List all session contexts for a platform.
    pub fn sessions_for(&self, platform: &str) -> Vec<SessionContext> {
        self.contexts
            .lock()
            .values()
            .filter(|c| c.platform == platform)
            .cloned()
            .collect()
    }

    /// Get the total number of tracked sessions.
    pub fn len(&self) -> usize {
        self.contexts.lock().len()
    }

    /// Clean up stale sessions (no activity for the given number of seconds).
    pub fn cleanup_stale(&self, max_age_secs: u64) -> usize {
        let now = now_secs();
        let mut contexts = self.contexts.lock();
        let before = contexts.len();
        contexts.retain(|_, c| now - c.last_activity < max_age_secs);
        before - contexts.len()
    }
}

impl Default for SessionContextTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn session_key(platform: &str, user_id: &str) -> String {
    format!("{platform}:{user_id}")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_context_new() {
        let ctx = SessionContext::new("telegram", "user1", "chat1");
        assert_eq!(ctx.platform, "telegram");
        assert_eq!(ctx.user_id, "user1");
        assert_eq!(ctx.chat_id, "chat1");
        assert_eq!(ctx.current_turn, 0);
        assert!(!ctx.is_active);
    }

    #[test]
    fn test_record_turn() {
        let mut ctx = SessionContext::new("discord", "user2", "ch2");
        ctx.record_turn();
        ctx.record_turn();
        assert_eq!(ctx.current_turn, 2);
    }

    #[test]
    fn test_tracker_get_or_create() {
        let tracker = SessionContextTracker::new();
        let ctx1 = tracker.get_or_create("telegram", "u1", "c1");
        assert_eq!(ctx1.current_turn, 0);

        let ctx2 = tracker.get_or_create("telegram", "u1", "c1");
        assert_eq!(ctx2.current_turn, 0); // Same session, not incremented
    }

    #[test]
    fn test_tracker_record_turn() {
        let tracker = SessionContextTracker::new();
        tracker.get_or_create("telegram", "u1", "c1");
        tracker.record_turn("telegram", "u1");
        tracker.record_turn("telegram", "u1");
        assert_eq!(tracker.current_turn("telegram", "u1"), 2);
    }

    #[test]
    fn test_tracker_active() {
        let tracker = SessionContextTracker::new();
        tracker.get_or_create("telegram", "u1", "c1");

        assert!(!tracker.is_active("telegram", "u1"));
        tracker.set_active("telegram", "u1", true);
        assert!(tracker.is_active("telegram", "u1"));
        tracker.set_active("telegram", "u1", false);
        assert!(!tracker.is_active("telegram", "u1"));
    }

    #[test]
    fn test_tracker_remove() {
        let tracker = SessionContextTracker::new();
        tracker.get_or_create("telegram", "u1", "c1");
        assert!(tracker.get("telegram", "u1").is_some());
        assert!(tracker.remove("telegram", "u1"));
        assert!(tracker.get("telegram", "u1").is_none());
    }

    #[test]
    fn test_tracker_active_sessions() {
        let tracker = SessionContextTracker::new();
        tracker.get_or_create("telegram", "u1", "c1");
        tracker.get_or_create("discord", "u2", "c2");
        tracker.set_active("telegram", "u1", true);

        let active = tracker.active_sessions();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].platform, "telegram");
    }

    #[test]
    fn test_tracker_sessions_for() {
        let tracker = SessionContextTracker::new();
        tracker.get_or_create("telegram", "u1", "c1");
        tracker.get_or_create("telegram", "u2", "c2");
        tracker.get_or_create("discord", "u3", "c3");

        let tg = tracker.sessions_for("telegram");
        assert_eq!(tg.len(), 2);
    }

    #[test]
    fn test_tracker_cleanup_stale() {
        let tracker = SessionContextTracker::new();
        tracker.get_or_create("telegram", "u1", "c1");
        tracker.get_or_create("discord", "u2", "c2");

        // Nothing is stale yet
        let cleaned = tracker.cleanup_stale(3600);
        assert_eq!(cleaned, 0);
        assert_eq!(tracker.len(), 2);
    }

    #[test]
    fn test_tracker_len() {
        let tracker = SessionContextTracker::new();
        tracker.get_or_create("telegram", "u1", "c1");
        tracker.get_or_create("discord", "u2", "c2");
        assert_eq!(tracker.len(), 2);
    }
}
