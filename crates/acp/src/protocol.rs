use serde::{Deserialize, Serialize};
use serde_json::Value;

/// ACP method names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    // Session management
    Initialize,
    Shutdown,
    // Message exchange
    SendMessage,
    CancelRequest,
    // File context
    SetFileContext,
    GetFileContext,
    // Selection-based operations
    SetSelection,
    // Git integration
    GitStatus,
    GitDiff,
    GitLog,
    // Terminal integration
    TerminalExec,
    TerminalRead,
    // Notifications (server -> client)
    Notification,
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Method::Initialize => "initialize",
            Method::Shutdown => "shutdown",
            Method::SendMessage => "sendMessage",
            Method::CancelRequest => "cancelRequest",
            Method::SetFileContext => "setFileContext",
            Method::GetFileContext => "getFileContext",
            Method::SetSelection => "setSelection",
            Method::GitStatus => "gitStatus",
            Method::GitDiff => "gitDiff",
            Method::GitLog => "gitLog",
            Method::TerminalExec => "terminalExec",
            Method::TerminalRead => "terminalRead",
            Method::Notification => "notification",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for Method {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "initialize" => Ok(Method::Initialize),
            "shutdown" => Ok(Method::Shutdown),
            "sendMessage" => Ok(Method::SendMessage),
            "cancelRequest" => Ok(Method::CancelRequest),
            "setFileContext" => Ok(Method::SetFileContext),
            "getFileContext" => Ok(Method::GetFileContext),
            "setSelection" => Ok(Method::SetSelection),
            "gitStatus" => Ok(Method::GitStatus),
            "gitDiff" => Ok(Method::GitDiff),
            "gitLog" => Ok(Method::GitLog),
            "terminalExec" => Ok(Method::TerminalExec),
            "terminalRead" => Ok(Method::TerminalRead),
            "notification" => Ok(Method::Notification),
            _ => Err(format!("unknown method: {s}")),
        }
    }
}

/// ACP error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCode(pub i64);

impl ErrorCode {
    pub const PARSE_ERROR: Self = Self(-32700);
    pub const INVALID_REQUEST: Self = Self(-32600);
    pub const METHOD_NOT_FOUND: Self = Self(-32601);
    pub const INVALID_PARAMS: Self = Self(-32602);
    pub const INTERNAL_ERROR: Self = Self(-32603);
}

/// ACP error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl std::fmt::Display for AcpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ACP error {} (code {}): {}", self.message, self.code, self.data.as_ref().map(|v| format!("{v}")).as_deref().unwrap_or(""))
    }
}

/// JSON-RPC style request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

impl AcpRequest {
    pub fn new(method: impl Into<String>, id: impl Into<Option<serde_json::Value>>, params: impl Into<Option<serde_json::Value>>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            method: method.into(),
            params: params.into(),
        }
    }

    pub fn parse_method(&self) -> Result<Method, String> {
        self.method.parse()
    }
}

/// JSON-RPC style response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AcpError>,
}

impl AcpResponse {
    pub fn success(id: Option<Value>, result: impl Into<Option<Value>>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: result.into(),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, err: AcpError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(err),
        }
    }

    pub fn notification(_method: impl Into<String>, _params: impl Into<Option<Value>>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            result: None,
            error: None,
        }
    }
}

/// Initialize request parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// IDE name (e.g. "vscode", "zed", "jetbrains").
    pub client_name: String,
    /// IDE version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    /// Current workspace root path.
    pub workspace_path: String,
}

/// Initialize response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    pub server_name: String,
    pub server_version: String,
    pub capabilities: ServerCapabilities,
    pub session_id: String,
}

/// Server capabilities advertised to the IDE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub file_context: bool,
    pub selection_operations: bool,
    pub git_integration: bool,
    pub terminal_integration: bool,
    pub streaming: bool,
}

impl Default for ServerCapabilities {
    fn default() -> Self {
        Self {
            file_context: true,
            selection_operations: true,
            git_integration: true,
            terminal_integration: true,
            streaming: true,
        }
    }
}

/// SendMessage request parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageParams {
    /// User message text.
    pub message: String,
    /// Session ID (optional, server assigns if absent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// CancelRequest parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelRequestParams {
    /// Request ID to cancel.
    pub request_id: Value,
}

/// SetFileContext request parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetFileContextParams {
    /// List of files with context.
    pub files: Vec<FileContextEntry>,
}

/// File context entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContextEntry {
    /// Absolute file path.
    pub path: String,
    /// Optional line range for focused context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<LineRange>,
}

/// Line range (1-based).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

/// Selection parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSelectionParams {
    /// Selected file path.
    pub file_path: String,
    /// Selected text.
    pub text: String,
    /// Selection start line (1-based).
    pub start_line: usize,
    /// Selection end line (1-based).
    pub end_line: usize,
}

/// GitStatus response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatusResult {
    pub branch: String,
    pub modified: Vec<String>,
    pub added: Vec<String>,
    pub deleted: Vec<String>,
    pub untracked: Vec<String>,
}

/// GitDiff response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiffResult {
    pub diff: String,
}

/// GitLog response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLogEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLogResult {
    pub entries: Vec<GitLogEntry>,
}

/// TerminalExec parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalExecParams {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

/// TerminalExec response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_display() {
        assert_eq!(Method::Initialize.to_string(), "initialize");
        assert_eq!(Method::SendMessage.to_string(), "sendMessage");
        assert_eq!(Method::GitStatus.to_string(), "gitStatus");
    }

    #[test]
    fn test_method_from_str() {
        assert_eq!("initialize".parse::<Method>().unwrap(), Method::Initialize);
        assert_eq!("sendMessage".parse::<Method>().unwrap(), Method::SendMessage);
        assert!("unknown".parse::<Method>().is_err());
    }

    #[test]
    fn test_request_parse_method() {
        let req = AcpRequest::new("initialize", Some(serde_json::json!(1)), serde_json::json!({
            "clientName": "test",
            "workspacePath": "/tmp"
        }));
        assert_eq!(req.parse_method().unwrap(), Method::Initialize);
    }

    #[test]
    fn test_response_success() {
        let resp = AcpResponse::success(Some(serde_json::json!(1)), serde_json::json!({"ok": true}));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_response_error() {
        let err = AcpError {
            code: ErrorCode::METHOD_NOT_FOUND.0,
            message: "method not found".to_string(),
            data: None,
        };
        let resp = AcpResponse::error(Some(serde_json::json!(1)), err);
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_server_capabilities_default() {
        let caps = ServerCapabilities::default();
        assert!(caps.file_context);
        assert!(caps.selection_operations);
        assert!(caps.git_integration);
        assert!(caps.terminal_integration);
        assert!(caps.streaming);
    }
}
