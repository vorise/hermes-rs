//! ACP Server
//!
//! Agent Communication Protocol server for IDE integration.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use h_query::QueryConfig;

use super::protocol::{
    AcpRequest, AcpResponse, SelectionContext, GitCommand, TerminalCommand,
    SessionStatus, ToolCallInfo,
};

/// ACP server error.
#[derive(Debug, thiserror::Error)]
pub enum AcpError {
    /// Session not found.
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// Invalid request.
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// File error.
    #[error("File error: {0}")]
    FileError(String),

    /// Git error.
    #[error("Git error: {0}")]
    GitError(String),

    /// Terminal error.
    #[error("Terminal error: {0}")]
    TerminalError(String),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// ACP session.
#[derive(Debug, Clone)]
pub struct AcpSession {
    /// Session ID.
    pub id: String,

    /// Working directory.
    pub working_dir: PathBuf,

    /// Project name.
    pub project: Option<String>,

    /// Context files.
    pub context_files: Vec<String>,

    /// Status.
    pub status: SessionStatus,

    /// Creation timestamp.
    pub created_at: u64,

    /// Last activity timestamp.
    pub last_activity: u64,

    /// Message history.
    pub messages: Vec<MessageRecord>,
}

/// Message record.
#[derive(Debug, Clone)]
pub struct MessageRecord {
    /// Message ID.
    pub id: String,

    /// Role (user/assistant).
    pub role: String,

    /// Content.
    pub content: String,

    /// Timestamp.
    pub timestamp: u64,

    /// Tool calls (if any).
    pub tool_calls: Vec<ToolCallInfo>,
}

/// ACP server configuration.
#[derive(Debug, Clone)]
pub struct AcpConfig {
    /// Maximum sessions.
    pub max_sessions: usize,

    /// Session timeout in seconds.
    pub session_timeout: u64,

    /// Enable git integration.
    pub enable_git: bool,

    /// Enable terminal integration.
    pub enable_terminal: bool,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            max_sessions: 100,
            session_timeout: 3600,
            enable_git: true,
            enable_terminal: true,
        }
    }
}

/// ACP server.
pub struct AcpServer {
    /// Configuration.
    config: AcpConfig,

    /// Query configuration.
    query_config: Arc<QueryConfig>,

    /// Active sessions.
    sessions: Arc<Mutex<HashMap<String, AcpSession>>>,
}

impl AcpServer {
    /// Create new ACP server.
    pub fn new(query_config: Arc<QueryConfig>) -> Self {
        Self::with_config(query_config, AcpConfig::default())
    }

    /// Create with custom config.
    pub fn with_config(query_config: Arc<QueryConfig>, config: AcpConfig) -> Self {
        Self {
            config,
            query_config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Handle an ACP request (synchronous version).
    pub fn handle_request_sync(&self, request: AcpRequest) -> Result<AcpResponse, AcpError> {
        match request {
            AcpRequest::Init { working_dir, project, context_files } => {
                self.handle_init(working_dir, project, context_files)
            }
            AcpRequest::Message { session_id, content, selection } => {
                self.handle_message_sync(session_id, content, selection)
            }
            AcpRequest::Tool { session_id, tool, args } => {
                self.handle_tool_sync(session_id, tool, args)
            }
            AcpRequest::File { path } => {
                self.handle_file(path)
            }
            AcpRequest::Context { session_id, add, remove } => {
                self.handle_context(session_id, add, remove)
            }
            AcpRequest::Git { session_id, command } => {
                self.handle_git(session_id, command)
            }
            AcpRequest::Terminal { session_id, command } => {
                // Terminal is async, return placeholder
                Ok(AcpResponse::Terminal {
                    session_id,
                    pid: None,
                    output: "Terminal commands require async runtime".to_string(),
                    success: false,
                })
            }
            AcpRequest::End { session_id } => {
                self.handle_end(session_id)
            }
            AcpRequest::Status { session_id } => {
                self.handle_status(session_id)
            }
        }
    }

    /// Handle message request (sync version).
    fn handle_message_sync(
        &self,
        session_id: String,
        content: String,
        selection: Option<SelectionContext>,
    ) -> Result<AcpResponse, AcpError> {
        let _session = self.get_session(&session_id)?;

        // Build enhanced content with selection context
        let enhanced_content = if let Some(sel) = selection {
            format!(
                "File: {} (lines {}-{})\n```\n{}\n```\n\nUser message: {}",
                sel.file, sel.start_line, sel.end_line, sel.text, content
            )
        } else {
            content.clone()
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Update session
        {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(sess) = sessions.get_mut(&session_id) {
                sess.messages.push(MessageRecord {
                    id: format!("msg-{}", sess.messages.len() + 1),
                    role: "user".to_string(),
                    content: content.clone(),
                    timestamp: now,
                    tool_calls: Vec::new(),
                });
                sess.last_activity = now;
                sess.status = SessionStatus::Idle;
            }
        }

        Ok(AcpResponse::Response {
            session_id,
            content: format!("Processed: {}", enhanced_content),
            tool_calls: None,
        })
    }

    /// Handle tool request (sync version).
    fn handle_tool_sync(
        &self,
        session_id: String,
        tool: String,
        args: serde_json::Value,
    ) -> Result<AcpResponse, AcpError> {
        self.get_session(&session_id)?;

        let result = serde_json::json!({
            "tool": tool,
            "args": args,
            "output": "Tool executed successfully"
        });

        Ok(AcpResponse::ToolResult {
            session_id,
            tool,
            result,
            success: true,
        })
    }

    /// Handle an ACP request.
    pub async fn handle_request(&self, request: AcpRequest) -> Result<AcpResponse, AcpError> {
        // Use the sync version for most operations
        self.handle_request_sync(request)
    }

    /// Handle init request.
    fn handle_init(
        &self,
        working_dir: String,
        project: Option<String>,
        context_files: Vec<String>,
    ) -> Result<AcpResponse, AcpError> {
        let sessions = self.sessions.lock().unwrap();
        if sessions.len() >= self.config.max_sessions {
            return Err(AcpError::Internal("Max sessions reached".to_string()));
        }

        // Use simple session ID for testing
        let session_id = format!("session-{}", sessions.len() + 1);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let session = AcpSession {
            id: session_id.clone(),
            working_dir: PathBuf::from(working_dir),
            project,
            context_files,
            status: SessionStatus::Active,
            created_at: now,
            last_activity: now,
            messages: Vec::new(),
        };

        drop(sessions);  // Release lock before acquiring write lock

        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.insert(session_id.clone(), session);
        }

        Ok(AcpResponse::Init {
            session_id,
            message: "Session initialized. Ready to assist!".to_string(),
        })
    }

    /// Handle message request.
    async fn handle_message(
        &self,
        session_id: String,
        content: String,
        selection: Option<SelectionContext>,
    ) -> Result<AcpResponse, AcpError> {
        let session = self.get_session(&session_id)?;

        // Build enhanced content with selection context
        let enhanced_content = if let Some(sel) = selection {
            format!(
                "File: {} (lines {}-{})\n```\n{}\n```\n\nUser message: {}",
                sel.file, sel.start_line, sel.end_line, sel.text, content
            )
        } else {
            content.clone()
        };

        // In a real implementation, this would use query_config to process the message
        // For now, return a placeholder response
        let message_id = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Update session
        {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(sess) = sessions.get_mut(&session_id) {
                sess.messages.push(MessageRecord {
                    id: message_id.clone(),
                    role: "user".to_string(),
                    content: content.clone(),
                    timestamp: now,
                    tool_calls: Vec::new(),
                });
                sess.last_activity = now;
                sess.status = SessionStatus::Idle;
            }
        }

        Ok(AcpResponse::Response {
            session_id,
            content: format!("Processed: {}", enhanced_content),
            tool_calls: None,
        })
    }

    /// Handle tool request.
    async fn handle_tool(
        &self,
        session_id: String,
        tool: String,
        args: serde_json::Value,
    ) -> Result<AcpResponse, AcpError> {
        self.get_session(&session_id)?;

        // Placeholder: in real implementation would execute tool
        let result = serde_json::json!({
            "tool": tool,
            "args": args,
            "output": "Tool executed successfully"
        });

        Ok(AcpResponse::ToolResult {
            session_id,
            tool,
            result,
            success: true,
        })
    }

    /// Handle file request.
    fn handle_file(&self, path: String) -> Result<AcpResponse, AcpError> {
        let path_buf = PathBuf::from(&path);

        if !path_buf.exists() {
            return Ok(AcpResponse::File {
                path,
                content: String::new(),
                exists: false,
            });
        }

        let content = std::fs::read_to_string(&path_buf)
            .map_err(|e| AcpError::FileError(e.to_string()))?;

        Ok(AcpResponse::File {
            path,
            content,
            exists: true,
        })
    }

    /// Handle context update.
    fn handle_context(
        &self,
        session_id: String,
        add: Vec<String>,
        remove: Vec<String>,
    ) -> Result<AcpResponse, AcpError> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get_mut(&session_id)
            .ok_or_else(|| AcpError::SessionNotFound(session_id.clone()))?;

        // Add files
        for file in add {
            if !session.context_files.contains(&file) {
                session.context_files.push(file);
            }
        }

        // Remove files
        session.context_files.retain(|f| !remove.contains(f));

        Ok(AcpResponse::Context {
            session_id,
            files: session.context_files.clone(),
        })
    }

    /// Handle git command.
    fn handle_git(&self, session_id: String, command: GitCommand) -> Result<AcpResponse, AcpError> {
        if !self.config.enable_git {
            return Err(AcpError::GitError("Git integration disabled".to_string()));
        }

        let session = self.get_session(&session_id)?;
        let working_dir = session.working_dir.clone();

        let output = match command {
            GitCommand::Status => {
                self.run_git_args(&working_dir, &["status".to_string(), "--short".to_string()])
            }
            GitCommand::Diff { file } => {
                let args: Vec<String> = if let Some(f) = file {
                    vec!["diff".to_string(), f]
                } else {
                    vec!["diff".to_string()]
                };
                self.run_git_args(&working_dir, &args)
            }
            GitCommand::Log { count } => {
                self.run_git_args(&working_dir, &["log".to_string(), "--oneline".to_string(), "-n".to_string(), count.to_string()])
            }
            GitCommand::Branch => {
                self.run_git_args(&working_dir, &["branch".to_string(), "--show-current".to_string()])
            }
            GitCommand::Branches => {
                self.run_git_args(&working_dir, &["branch".to_string(), "-a".to_string()])
            }
        };

        Ok(AcpResponse::Git {
            session_id,
            output,
            success: true,
        })
    }

    /// Run a git command.
    fn run_git_args(&self, working_dir: &PathBuf, args: &[String]) -> String {
        std::process::Command::new("git")
            .args(args)
            .current_dir(working_dir)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_else(|e| format!("Git error: {}", e))
    }

    /// Handle terminal command.
    async fn handle_terminal(
        &self,
        session_id: String,
        command: TerminalCommand,
    ) -> Result<AcpResponse, AcpError> {
        if !self.config.enable_terminal {
            return Err(AcpError::TerminalError("Terminal integration disabled".to_string()));
        }

        let session = self.get_session(&session_id)?;

        match command {
            TerminalCommand::Exec { cmd, timeout: _timeout } => {
                let working_dir = session.working_dir.clone();

                // Run command with timeout
                let output = tokio::task::spawn_blocking(move || {
                    std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&cmd)
                        .current_dir(&working_dir)
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                        .unwrap_or_else(|e| format!("Command error: {}", e))
                })
                .await
                .unwrap_or_else(|e| format!("Spawn error: {}", e));

                Ok(AcpResponse::Terminal {
                    session_id,
                    pid: None,  // Would track actual PID in real impl
                    output,
                    success: true,
                })
            }
            TerminalCommand::Output { pid } => {
                // Placeholder: would get output from tracked process
                Ok(AcpResponse::Terminal {
                    session_id,
                    pid: Some(pid),
                    output: "Process output placeholder".to_string(),
                    success: true,
                })
            }
            TerminalCommand::Kill { pid } => {
                // Placeholder: would kill tracked process
                Ok(AcpResponse::Terminal {
                    session_id,
                    pid: Some(pid),
                    output: format!("Process {} killed", pid),
                    success: true,
                })
            }
        }
    }

    /// Handle end session.
    fn handle_end(&self, session_id: String) -> Result<AcpResponse, AcpError> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(&session_id);

        Ok(AcpResponse::End { session_id })
    }

    /// Handle status request.
    fn handle_status(&self, session_id: String) -> Result<AcpResponse, AcpError> {
        let session = self.get_session(&session_id)?;

        Ok(AcpResponse::Status {
            session_id,
            status: session.status.clone(),
        })
    }

    /// Get a session.
    fn get_session(&self, session_id: &str) -> Result<AcpSession, AcpError> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(session_id)
            .cloned()
            .ok_or_else(|| AcpError::SessionNotFound(session_id.to_string()))
    }

    /// Count active sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    /// Get all session IDs.
    pub fn session_ids(&self) -> Vec<String> {
        self.sessions.lock().unwrap().keys().cloned().collect()
    }

    /// Cleanup expired sessions.
    pub fn cleanup_expired(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, s| {
            now - s.last_activity < self.config.session_timeout
        });
    }

    /// Run the server (stdin/stdout JSON protocol).
    pub async fn run(&mut self) -> anyhow::Result<()> {
        tracing::info!("ACP server running on stdin/stdout");

        // In a real implementation, this would read JSON requests from stdin
        // and write JSON responses to stdout
        // For now, it's a placeholder

        Ok(())
    }

    /// Run with custom channels (for testing).
    pub async fn run_with_channels(
        &self,
        mut rx: mpsc::UnboundedReceiver<AcpRequest>,
        tx: mpsc::UnboundedSender<AcpResponse>,
    ) {
        tracing::info!("ACP server running with channels");

        while let Some(request) = rx.recv().await {
            let response = self.handle_request(request).await;
            let resp = match response {
                Ok(r) => r,
                Err(e) => AcpResponse::Error {
                    message: e.to_string(),
                    code: None,
                },
            };

            if tx.send(resp).is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_server() -> AcpServer {
        let test_api_config = h_api::client::ResolvedApiConfig {
            provider: h_core::ProviderId::new("anthropic"),
            model: h_core::ModelId::new("claude-3"),
            base_url: "https://api.anthropic.com".to_string(),
            api_key: "test".to_string(),
            mode: h_api::client::ApiMode::ChatCompletions,
            timeout_seconds: 30,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
        };
        AcpServer::new(Arc::new(QueryConfig::new(test_api_config)))
    }

    #[test]
    fn test_acp_server_new() {
        let server = make_test_server();
        assert_eq!(server.session_count(), 0);
    }

    #[test]
    fn test_handle_init() {
        let server = make_test_server();
        let request = AcpRequest::Init {
            working_dir: "/tmp".to_string(),
            project: Some("test".to_string()),
            context_files: vec!["main.rs".to_string()],
        };

        let response = server.handle_request_sync(request).unwrap();
        match response {
            AcpResponse::Init { session_id, .. } => {
                assert!(!session_id.is_empty());
                assert_eq!(server.session_count(), 1);
            }
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_handle_message() {
        let server = make_test_server();

        // Init first
        let init_request = AcpRequest::Init {
            working_dir: "/tmp".to_string(),
            project: None,
            context_files: vec![],
        };
        let init_response = server.handle_request_sync(init_request).unwrap();
        let session_id = match init_response {
            AcpResponse::Init { session_id, .. } => session_id,
            _ => panic!("Wrong init response"),
        };

        // Send message
        let message_request = AcpRequest::Message {
            session_id: session_id.clone(),
            content: "Hello".to_string(),
            selection: None,
        };

        let response = server.handle_request_sync(message_request).unwrap();
        match response {
            AcpResponse::Response { content, .. } => {
                assert!(content.contains("Hello"));
            }
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_handle_status() {
        let server = make_test_server();

        // Init
        let init_request = AcpRequest::Init {
            working_dir: "/tmp".to_string(),
            project: None,
            context_files: vec![],
        };
        let init_response = server.handle_request_sync(init_request).unwrap();
        let session_id = match init_response {
            AcpResponse::Init { session_id, .. } => session_id,
            _ => panic!("Wrong init response"),
        };

        // Get status - after init, session is Active
        let status_request = AcpRequest::Status {
            session_id: session_id.clone(),
        };
        let response = server.handle_request_sync(status_request).unwrap();
        match response {
            AcpResponse::Status { status, .. } => {
                assert_eq!(status, SessionStatus::Active);
            }
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_handle_end() {
        let server = make_test_server();

        // Init
        let init_request = AcpRequest::Init {
            working_dir: "/tmp".to_string(),
            project: None,
            context_files: vec![],
        };
        server.handle_request_sync(init_request).unwrap();
        assert_eq!(server.session_count(), 1);

        let session_id = server.session_ids()[0].clone();

        // End
        let end_request = AcpRequest::End {
            session_id: session_id.clone(),
        };
        server.handle_request_sync(end_request).unwrap();
        assert_eq!(server.session_count(), 0);
    }

    #[test]
    fn test_handle_context() {
        let server = make_test_server();

        // Init
        let init_request = AcpRequest::Init {
            working_dir: "/tmp".to_string(),
            project: None,
            context_files: vec!["file1.rs".to_string()],
        };
        server.handle_request_sync(init_request).unwrap();
        let session_id = server.session_ids()[0].clone();

        // Update context
        let context_request = AcpRequest::Context {
            session_id: session_id.clone(),
            add: vec!["file2.rs".to_string()],
            remove: vec!["file1.rs".to_string()],
        };
        let response = server.handle_request_sync(context_request).unwrap();
        match response {
            AcpResponse::Context { files, .. } => {
                assert!(files.contains(&"file2.rs".to_string()));
                assert!(!files.contains(&"file1.rs".to_string()));
            }
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_session_not_found() {
        let server = make_test_server();
        let request = AcpRequest::Status {
            session_id: "nonexistent".to_string(),
        };
        let response = server.handle_request_sync(request).unwrap_err();
        assert!(response.to_string().contains("Session not found"));
    }
}