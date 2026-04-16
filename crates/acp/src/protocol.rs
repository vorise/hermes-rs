//! ACP Protocol
//!
//! Agent Communication Protocol for IDE integration.
//! JSON-based protocol for communication between IDE and Hermes agent.

use serde::{Deserialize, Serialize};

/// ACP request types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AcpRequest {
    /// Initialize a new session.
    #[serde(rename = "init")]
    Init {
        /// Working directory.
        working_dir: String,

        /// Project name (optional).
        project: Option<String>,

        /// Initial context files.
        context_files: Vec<String>,
    },

    /// Send a message to the agent.
    #[serde(rename = "message")]
    Message {
        /// Session ID.
        session_id: String,

        /// User message.
        content: String,

        /// Selected text context (optional).
        selection: Option<SelectionContext>,
    },

    /// Execute a tool.
    #[serde(rename = "tool")]
    Tool {
        /// Session ID.
        session_id: String,

        /// Tool name.
        tool: String,

        /// Tool arguments.
        args: serde_json::Value,
    },

    /// Get file content.
    #[serde(rename = "file")]
    File {
        /// File path.
        path: String,
    },

    /// Update file context.
    #[serde(rename = "context")]
    Context {
        /// Session ID.
        session_id: String,

        /// Files to add to context.
        add: Vec<String>,

        /// Files to remove from context.
        remove: Vec<String>,
    },

    /// Git operation.
    #[serde(rename = "git")]
    Git {
        /// Session ID.
        session_id: String,

        /// Git command.
        command: GitCommand,
    },

    /// Terminal operation.
    #[serde(rename = "terminal")]
    Terminal {
        /// Session ID.
        session_id: String,

        /// Terminal command.
        command: TerminalCommand,
    },

    /// End session.
    #[serde(rename = "end")]
    End {
        /// Session ID.
        session_id: String,
    },

    /// Get session status.
    #[serde(rename = "status")]
    Status {
        /// Session ID.
        session_id: String,
    },
}

/// Selection context from IDE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionContext {
    /// File path.
    pub file: String,

    /// Selected text.
    pub text: String,

    /// Start line.
    pub start_line: u32,

    /// End line.
    pub end_line: u32,

    /// Selection type.
    pub selection_type: SelectionType,
}

/// Selection type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionType {
    /// Full file.
    File,

    /// Code block.
    Block,

    /// Function/method.
    Function,

    /// Class.
    Class,

    /// Arbitrary selection.
    Selection,
}

/// Git command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum GitCommand {
    /// Get status.
    #[serde(rename = "status")]
    Status,

    /// Get diff.
    #[serde(rename = "diff")]
    Diff {
        /// File path (optional).
        file: Option<String>,
    },

    /// Get log.
    #[serde(rename = "log")]
    Log {
        /// Number of commits.
        count: u32,
    },

    /// Get current branch.
    #[serde(rename = "branch")]
    Branch,

    /// List branches.
    #[serde(rename = "branches")]
    Branches,
}

/// Terminal command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum TerminalCommand {
    /// Execute command.
    #[serde(rename = "exec")]
    Exec {
        /// Command to execute.
        cmd: String,

        /// Timeout in seconds.
        timeout: Option<u32>,
    },

    /// Get output.
    #[serde(rename = "output")]
    Output {
        /// Process ID.
        pid: u32,
    },

    /// Kill process.
    #[serde(rename = "kill")]
    Kill {
        /// Process ID.
        pid: u32,
    },
}

/// ACP response types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AcpResponse {
    /// Session initialized.
    #[serde(rename = "init")]
    Init {
        /// Session ID.
        session_id: String,

        /// Welcome message.
        message: String,
    },

    /// Agent response.
    #[serde(rename = "response")]
    Response {
        /// Session ID.
        session_id: String,

        /// Response content.
        content: String,

        /// Tool calls made (optional).
        tool_calls: Option<Vec<ToolCallInfo>>,
    },

    /// Tool result.
    #[serde(rename = "tool_result")]
    ToolResult {
        /// Session ID.
        session_id: String,

        /// Tool name.
        tool: String,

        /// Result.
        result: serde_json::Value,

        /// Success status.
        success: bool,
    },

    /// File content.
    #[serde(rename = "file")]
    File {
        /// File path.
        path: String,

        /// File content.
        content: String,

        /// File exists.
        exists: bool,
    },

    /// Context update result.
    #[serde(rename = "context")]
    Context {
        /// Session ID.
        session_id: String,

        /// Current context files.
        files: Vec<String>,
    },

    /// Git result.
    #[serde(rename = "git")]
    Git {
        /// Session ID.
        session_id: String,

        /// Git output.
        output: String,

        /// Success status.
        success: bool,
    },

    /// Terminal result.
    #[serde(rename = "terminal")]
    Terminal {
        /// Session ID.
        session_id: String,

        /// Process ID (for exec).
        pid: Option<u32>,

        /// Output.
        output: String,

        /// Success status.
        success: bool,
    },

    /// Session ended.
    #[serde(rename = "end")]
    End {
        /// Session ID.
        session_id: String,
    },

    /// Session status.
    #[serde(rename = "status")]
    Status {
        /// Session ID.
        session_id: String,

        /// Status.
        status: SessionStatus,
    },

    /// Error response.
    #[serde(rename = "error")]
    Error {
        /// Error message.
        message: String,

        /// Error code.
        code: Option<String>,
    },

    /// Streaming delta.
    #[serde(rename = "delta")]
    Delta {
        /// Session ID.
        session_id: String,

        /// Text delta.
        delta: String,

        /// Message ID.
        message_id: String,
    },
}

/// Tool call information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    /// Tool name.
    pub name: String,

    /// Arguments preview.
    pub args_preview: String,

    /// Result preview.
    pub result_preview: String,
}

/// Session status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Session is active.
    Active,

    /// Session is processing.
    Processing,

    /// Session is idle.
    Idle,

    /// Session has ended.
    Ended,
}

/// Parse an ACP request from JSON.
pub fn parse_request(json: &str) -> Result<AcpRequest, serde_json::Error> {
    serde_json::from_str(json)
}

/// Serialize an ACP response to JSON.
pub fn serialize_response(response: &AcpResponse) -> Result<String, serde_json::Error> {
    serde_json::to_string(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_init_request() {
        let json = r#"{"type":"init","working_dir":"/home/user/project","context_files":[]}"#;
        let req = parse_request(json).unwrap();
        match req {
            AcpRequest::Init { working_dir, .. } => {
                assert_eq!(working_dir, "/home/user/project");
            }
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_parse_message_request() {
        let json = r#"{"type":"message","session_id":"s1","content":"hello"}"#;
        let req = parse_request(json).unwrap();
        match req {
            AcpRequest::Message { session_id, content, .. } => {
                assert_eq!(session_id, "s1");
                assert_eq!(content, "hello");
            }
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_serialize_init_response() {
        let resp = AcpResponse::Init {
            session_id: "s1".to_string(),
            message: "Welcome".to_string(),
        };
        let json = serialize_response(&resp).unwrap();
        assert!(json.contains("init"));
        assert!(json.contains("s1"));
    }

    #[test]
    fn test_serialize_error_response() {
        let resp = AcpResponse::Error {
            message: "Not found".to_string(),
            code: Some("404".to_string()),
        };
        let json = serialize_response(&resp).unwrap();
        assert!(json.contains("error"));
        assert!(json.contains("404"));
    }

    #[test]
    fn test_selection_context() {
        let sel = SelectionContext {
            file: "main.rs".to_string(),
            text: "fn main()".to_string(),
            start_line: 1,
            end_line: 5,
            selection_type: SelectionType::Function,
        };
        let json = serde_json::to_string(&sel).unwrap();
        assert!(json.contains("function"));
    }

    #[test]
    fn test_git_command() {
        let cmd = GitCommand::Status;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("status"));
    }

    #[test]
    fn test_terminal_command() {
        let cmd = TerminalCommand::Exec {
            cmd: "ls".to_string(),
            timeout: Some(10),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("exec"));
        assert!(json.contains("ls"));
    }
}