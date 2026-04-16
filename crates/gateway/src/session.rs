//! Gateway Session Store
//!
//! Manages session state per platform-user combination.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use h_core::HermesConfig;

/// Gateway session for a specific platform-user pair.
#[derive(Debug, Clone)]
pub struct GatewaySession {
    /// Session ID (from core session).
    pub session_id: String,

    /// Platform identifier.
    pub platform: String,

    /// Chat/user identifier.
    pub chat_id: String,

    /// Last message ID from platform.
    pub last_message_id: Option<String>,

    /// Is this a paired session (group → DM)?
    pub is_paired: bool,

    /// Pairing code (if applicable).
    pub pair_code: Option<String>,

    /// Session metadata.
    pub metadata: HashMap<String, String>,
}

impl GatewaySession {
    /// Create new gateway session.
    pub fn new(
        session_id: impl Into<String>,
        platform: impl Into<String>,
        chat_id: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            platform: platform.into(),
            chat_id: chat_id.into(),
            last_message_id: None,
            is_paired: false,
            pair_code: None,
            metadata: HashMap::new(),
        }
    }

    /// Get the source key for this session.
    pub fn source_key(&self) -> String {
        format!("{}:{}", self.platform, self.chat_id)
    }

    /// Update last message ID.
    pub fn update_last_message(&mut self, message_id: String) {
        self.last_message_id = Some(message_id);
    }

    /// Set pairing info.
    pub fn set_paired(&mut self, pair_code: String) {
        self.is_paired = true;
        self.pair_code = Some(pair_code);
    }

    /// Add metadata.
    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }
}

/// Gateway session store.
///
/// Manages mapping from platform-user combinations to Hermes sessions.
pub struct GatewaySessionStore {
    /// Core session database.
    #[allow(dead_code)]
    db: Arc<h_core::SessionDB>,

    /// Gateway sessions by source key.
    sessions: Arc<RwLock<HashMap<String, GatewaySession>>>,

    /// Hermes config.
    #[allow(dead_code)]
    config: Arc<HermesConfig>,
}

impl GatewaySessionStore {
    /// Create new session store.
    pub fn new(db: Arc<h_core::SessionDB>, config: Arc<HermesConfig>) -> Self {
        Self {
            db,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Get or create session for a platform-user.
    pub fn get_or_create(&self, platform: &str, chat_id: &str) -> GatewaySession {
        let key = format!("{}:{}", platform, chat_id);

        {
            let sessions = self.sessions.read();
            if let Some(session) = sessions.get(&key) {
                return session.clone();
            }
        }

        // Create new session
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = GatewaySession::new(session_id, platform, chat_id);

        {
            let mut sessions = self.sessions.write();
            sessions.insert(key.clone(), session.clone());
        }

        session
    }

    /// Get existing session.
    pub fn get(&self, platform: &str, chat_id: &str) -> Option<GatewaySession> {
        let key = format!("{}:{}", platform, chat_id);
        let sessions = self.sessions.read();
        sessions.get(&key).cloned()
    }

    /// Update session.
    pub fn update(&self, session: GatewaySession) {
        let key = session.source_key();
        let mut sessions = self.sessions.write();
        sessions.insert(key, session);
    }

    /// Remove session.
    pub fn remove(&self, platform: &str, chat_id: &str) -> Option<GatewaySession> {
        let key = format!("{}:{}", platform, chat_id);
        let mut sessions = self.sessions.write();
        sessions.remove(&key)
    }

    /// Get all sessions.
    pub fn all_sessions(&self) -> Vec<GatewaySession> {
        let sessions = self.sessions.read();
        sessions.values().cloned().collect()
    }

    /// Get sessions for a platform.
    pub fn platform_sessions(&self, platform: &str) -> Vec<GatewaySession> {
        let sessions = self.sessions.read();
        sessions.values()
            .filter(|s| s.platform == platform)
            .cloned()
            .collect()
    }

    /// Count sessions.
    pub fn count(&self) -> usize {
        let sessions = self.sessions.read();
        sessions.len()
    }

    /// Find session by pair code.
    pub fn find_by_pair_code(&self, pair_code: &str) -> Option<GatewaySession> {
        let sessions = self.sessions.read();
        sessions.values()
            .find(|s| s.pair_code.as_deref() == Some(pair_code))
            .cloned()
    }

    /// Clear all sessions.
    pub fn clear(&self) {
        let mut sessions = self.sessions.write();
        sessions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_session_new() {
        let session = GatewaySession::new("session-1", "telegram", "chat-1");
        assert_eq!(session.session_id, "session-1");
        assert_eq!(session.platform, "telegram");
        assert_eq!(session.chat_id, "chat-1");
        assert!(session.last_message_id.is_none());
    }

    #[test]
    fn test_gateway_session_source_key() {
        let session = GatewaySession::new("session-1", "telegram", "chat-1");
        assert_eq!(session.source_key(), "telegram:chat-1");
    }

    #[test]
    fn test_gateway_session_update_last_message() {
        let mut session = GatewaySession::new("session-1", "telegram", "chat-1");
        session.update_last_message("msg-123".to_string());
        assert_eq!(session.last_message_id, Some("msg-123".to_string()));
    }

    #[test]
    fn test_gateway_session_set_paired() {
        let mut session = GatewaySession::new("session-1", "telegram", "chat-1");
        session.set_paired("ABC123".to_string());
        assert!(session.is_paired);
        assert_eq!(session.pair_code, Some("ABC123".to_string()));
    }

    #[test]
    fn test_gateway_session_store_new() {
        let db = Arc::new(h_core::SessionDB::open_in_memory().unwrap());
        let config = Arc::new(HermesConfig::default());
        let store = GatewaySessionStore::new(db, config);
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn test_gateway_session_store_get_or_create() {
        let db = Arc::new(h_core::SessionDB::open_in_memory().unwrap());
        let config = Arc::new(HermesConfig::default());
        let store = GatewaySessionStore::new(db, config);

        let session1 = store.get_or_create("telegram", "chat-1");
        assert_eq!(session1.platform, "telegram");

        // Getting again should return same session
        let session2 = store.get_or_create("telegram", "chat-1");
        assert_eq!(session1.session_id, session2.session_id);
    }

    #[test]
    fn test_gateway_session_store_remove() {
        let db = Arc::new(h_core::SessionDB::open_in_memory().unwrap());
        let config = Arc::new(HermesConfig::default());
        let store = GatewaySessionStore::new(db, config);

        store.get_or_create("telegram", "chat-1");
        let removed = store.remove("telegram", "chat-1");
        assert!(removed.is_some());
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn test_gateway_session_store_platform_sessions() {
        let db = Arc::new(h_core::SessionDB::open_in_memory().unwrap());
        let config = Arc::new(HermesConfig::default());
        let store = GatewaySessionStore::new(db, config);

        store.get_or_create("telegram", "chat-1");
        store.get_or_create("telegram", "chat-2");
        store.get_or_create("discord", "chat-3");

        let telegram_sessions = store.platform_sessions("telegram");
        assert_eq!(telegram_sessions.len(), 2);
    }
}