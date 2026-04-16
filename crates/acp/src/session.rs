use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use uuid::Uuid;

use crate::context::{ContextSnapshot, FileContext, Selection};

/// State of an ACP session.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// Session ID.
    pub id: String,
    /// Workspace path.
    pub workspace_path: String,
    /// IDE client name.
    pub client_name: String,
    /// Active file context.
    pub files: Vec<FileContext>,
    /// Current selection.
    pub selection: Option<Selection>,
    /// Message history.
    pub messages: Vec<SessionMessage>,
}

/// A message within an ACP session.
#[derive(Debug, Clone)]
pub struct SessionMessage {
    pub role: String, // "user" | "assistant" | "system" | "tool_result"
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl SessionMessage {
    pub fn user(content: String) -> Self {
        Self {
            role: "user".to_string(),
            content,
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn assistant(content: String) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            timestamp: chrono::Utc::now(),
        }
    }
}

impl SessionState {
    pub fn new(id: String, workspace_path: String, client_name: String) -> Self {
        Self {
            id,
            workspace_path,
            client_name,
            files: Vec::new(),
            selection: None,
            messages: Vec::new(),
        }
    }

    pub fn context_snapshot(&self) -> ContextSnapshot {
        ContextSnapshot {
            files: self.files.clone(),
            selection: self.selection.clone(),
            git_branch: None,
        }
    }
}

/// Manages multiple ACP sessions.
pub struct AcpSession {
    sessions: Arc<Mutex<HashMap<String, SessionState>>>,
}

impl AcpSession {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new session.
    pub fn create_session(&self, workspace_path: String, client_name: String) -> String {
        let id = Uuid::new_v4().to_string();
        let state = SessionState::new(id.clone(), workspace_path, client_name);
        self.sessions.lock().insert(id.clone(), state);
        id
    }

    /// Get a session by ID.
    pub fn get_session(&self, id: &str) -> Option<SessionState> {
        self.sessions.lock().get(id).cloned()
    }

    /// Get or create session from session_id (creates new if absent).
    pub fn get_or_create_session(
        &self,
        session_id: Option<String>,
        workspace_path: String,
        client_name: String,
    ) -> String {
        if let Some(id) = &session_id {
            if self.sessions.lock().contains_key(id) {
                return id.clone();
            }
        }
        self.create_session(workspace_path, client_name)
    }

    /// Close a session.
    pub fn close_session(&self, id: &str) -> bool {
        self.sessions.lock().remove(id).is_some()
    }

    /// Update file context for a session.
    pub fn set_file_context(&self, session_id: &str, files: Vec<FileContext>) {
        if let Some(state) = self.sessions.lock().get_mut(session_id) {
            state.files = files;
        }
    }

    /// Update selection for a session.
    pub fn set_selection(&self, session_id: &str, selection: Selection) {
        if let Some(state) = self.sessions.lock().get_mut(session_id) {
            state.selection = Some(selection);
        }
    }

    /// Add a message to session history.
    pub fn add_message(&self, session_id: &str, message: SessionMessage) {
        if let Some(state) = self.sessions.lock().get_mut(session_id) {
            state.messages.push(message);
        }
    }

    /// Get context snapshot for a session.
    pub fn get_context(&self, session_id: &str) -> Option<ContextSnapshot> {
        self.sessions.lock().get(session_id).map(|s| s.context_snapshot())
    }

    /// Count active sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.lock().len()
    }

    /// Get the current working directory for a session.
    pub fn workspace_path(&self, session_id: &str) -> Option<String> {
        self.sessions.lock().get(session_id).map(|s| s.workspace_path.clone())
    }
}

impl Default for AcpSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_message_roles() {
        let user_msg = SessionMessage::user("hello".to_string());
        assert_eq!(user_msg.role, "user");
        assert_eq!(user_msg.content, "hello");

        let assistant_msg = SessionMessage::assistant("hi there".to_string());
        assert_eq!(assistant_msg.role, "assistant");
    }

    #[test]
    fn test_create_and_get_session() {
        let session = AcpSession::new();
        let id = session.create_session("/workspace".to_string(), "vscode".to_string());
        assert!(!id.is_empty());

        let state = session.get_session(&id).unwrap();
        assert_eq!(state.workspace_path, "/workspace");
        assert_eq!(state.client_name, "vscode");
    }

    #[test]
    fn test_get_or_create_session() {
        let session = AcpSession::new();
        let id1 = session.get_or_create_session(
            None,
            "/workspace".to_string(),
            "vscode".to_string(),
        );
        // Reuse the same session
        let id2 = session.get_or_create_session(
            Some(id1.clone()),
            "/workspace".to_string(),
            "vscode".to_string(),
        );
        assert_eq!(id1, id2);

        // Missing session creates new
        let id3 = session.get_or_create_session(
            Some("nonexistent".to_string()),
            "/workspace2".to_string(),
            "zed".to_string(),
        );
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_close_session() {
        let session = AcpSession::new();
        let id = session.create_session("/workspace".to_string(), "vscode".to_string());
        assert!(session.close_session(&id));
        assert!(!session.close_session(&id)); // already closed
        assert!(session.get_session(&id).is_none());
    }

    #[test]
    fn test_set_file_context() {
        let session = AcpSession::new();
        let id = session.create_session("/workspace".to_string(), "vscode".to_string());
        session.set_file_context(&id, vec![FileContext {
            path: "/src/main.rs".to_string(),
            content: "fn main() {}".to_string(),
            is_open: true,
        }]);
        let ctx = session.get_context(&id).unwrap();
        assert_eq!(ctx.files.len(), 1);
        assert_eq!(ctx.files[0].path, "/src/main.rs");
    }

    #[test]
    fn test_set_selection() {
        let session = AcpSession::new();
        let id = session.create_session("/workspace".to_string(), "vscode".to_string());
        session.set_selection(&id, Selection {
            file_path: "/src/main.rs".to_string(),
            text: "fn main()".to_string(),
            start_line: 1,
            end_line: 1,
        });
        let ctx = session.get_context(&id).unwrap();
        assert!(ctx.selection.is_some());
        let sel = ctx.selection.unwrap();
        assert_eq!(sel.text, "fn main()");
    }

    #[test]
    fn test_add_message() {
        let session = AcpSession::new();
        let id = session.create_session("/workspace".to_string(), "vscode".to_string());
        session.add_message(&id, SessionMessage::user("hello".to_string()));
        session.add_message(&id, SessionMessage::assistant("hi".to_string()));
        let state = session.get_session(&id).unwrap();
        assert_eq!(state.messages.len(), 2);
    }

    #[test]
    fn test_session_count() {
        let session = AcpSession::new();
        assert_eq!(session.session_count(), 0);
        session.create_session("/w1".to_string(), "vscode".to_string());
        session.create_session("/w2".to_string(), "zed".to_string());
        assert_eq!(session.session_count(), 2);
    }
}
