use std::sync::Arc;

use anyhow::Result;
use h_core::SessionDB;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Metadata tracked for an active gateway session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewaySession {
    /// Session ID from the session database.
    pub session_id: String,
    /// Platform identifier (e.g., "telegram").
    pub platform: String,
    /// User identifier on the platform.
    pub user_id: String,
    /// Chat/channel identifier on the platform.
    pub chat_id: String,
    /// Whether the session is currently active (processing a query).
    pub active: bool,
}

/// Stores and manages sessions for all connected platform users.
///
/// Each platform-user pair maps to a unique session in the SessionDB.
pub struct GatewaySessionStore {
    db: Arc<SessionDB>,
    /// Map of "platform:user_id" → GatewaySession.
    sessions: Mutex<std::collections::HashMap<String, GatewaySession>>,
}

impl GatewaySessionStore {
    /// Create a new session store.
    pub fn new(db: Arc<SessionDB>) -> Self {
        Self {
            db,
            sessions: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Get or create a session for a platform-user pair.
    ///
    /// If a session already exists in memory, return it.
    /// Otherwise, create a new session in the DB and track it.
    pub async fn get_or_create(
        &self,
        platform: &str,
        user_id: &str,
        chat_id: &str,
    ) -> Result<GatewaySession> {
        let key = session_key(platform, user_id);

        // Check in-memory cache
        {
            let sessions = self.sessions.lock();
            if let Some(session) = sessions.get(&key) {
                return Ok(session.clone());
            }
        }

        // Create a new session in the DB
        let session = h_core::session::Session::new("gateway");
        let session_id = session.id.clone();
        self.db.create_session(&session)?;

        let gateway_session = GatewaySession {
            session_id: session_id.clone(),
            platform: platform.to_string(),
            user_id: user_id.to_string(),
            chat_id: chat_id.to_string(),
            active: false,
        };

        self.sessions.lock().insert(key.clone(), gateway_session.clone());

        tracing::info!(
            platform, user_id, session_id,
            "Created new gateway session"
        );

        Ok(gateway_session)
    }

    /// Get an existing session by platform and user ID.
    pub fn get(&self, platform: &str, user_id: &str) -> Option<GatewaySession> {
        let key = session_key(platform, user_id);
        self.sessions.lock().get(&key).cloned()
    }

    /// Mark a session as active (processing).
    pub fn set_active(&self, platform: &str, user_id: &str, active: bool) {
        let key = session_key(platform, user_id);
        if let Some(session) = self.sessions.lock().get_mut(&key) {
            session.active = active;
        }
    }

    /// Check if a session is currently processing.
    pub fn is_active(&self, platform: &str, user_id: &str) -> bool {
        let key = session_key(platform, user_id);
        self.sessions.lock()
            .get(&key)
            .map(|s| s.active)
            .unwrap_or(false)
    }

    /// Remove a session from the store.
    pub fn remove(&self, platform: &str, user_id: &str) -> Option<GatewaySession> {
        let key = session_key(platform, user_id);
        self.sessions.lock().remove(&key)
    }

    /// Get the number of active sessions.
    pub fn active_count(&self) -> usize {
        self.sessions.lock().values().filter(|s| s.active).count()
    }

    /// Get the total number of tracked sessions.
    pub fn len(&self) -> usize {
        self.sessions.lock().len()
    }

    /// Get session DB reference.
    pub fn db(&self) -> &Arc<SessionDB> {
        &self.db
    }
}

/// Create a unique key for a platform-user pair.
fn session_key(platform: &str, user_id: &str) -> String {
    format!("{platform}:{user_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gateway_session_store() {
        let db = Arc::new(SessionDB::new_in_memory().unwrap());
        let store = GatewaySessionStore::new(db);

        assert_eq!(store.len(), 0);
        assert!(!store.is_active("telegram", "user1"));

        let session = store.get_or_create("telegram", "user1", "chat1").await.unwrap();
        assert_eq!(session.platform, "telegram");
        assert_eq!(session.user_id, "user1");
        assert!(!session.active);
        assert_eq!(store.len(), 1);

        // Second call returns the cached session
        let session2 = store.get_or_create("telegram", "user1", "chat1").await.unwrap();
        assert_eq!(session.session_id, session2.session_id);

        // Test active flag
        store.set_active("telegram", "user1", true);
        assert!(store.is_active("telegram", "user1"));
        assert_eq!(store.active_count(), 1);

        store.set_active("telegram", "user1", false);
        assert!(!store.is_active("telegram", "user1"));

        // Test get for nonexistent
        assert!(store.get("telegram", "nonexistent").is_none());
    }

    #[test]
    fn test_session_key() {
        assert_eq!(session_key("telegram", "123"), "telegram:123");
    }

    #[tokio::test]
    async fn test_remove_session() {
        let db = Arc::new(SessionDB::new_in_memory().unwrap());
        let store = GatewaySessionStore::new(db);

        store.get_or_create("discord", "user1", "chat1").await.unwrap();
        assert_eq!(store.len(), 1);

        let removed = store.remove("discord", "user1");
        assert!(removed.is_some());
        assert_eq!(store.len(), 0);
    }
}
