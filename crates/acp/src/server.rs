use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Notify;
use tracing::info;

use crate::context::{FileContext, Selection};
use crate::protocol::{
    AcpError, AcpRequest, AcpResponse, ErrorCode, InitializeParams, InitializeResult,
    Method, ServerCapabilities, SetFileContextParams, SetSelectionParams,
    SendMessageParams, GitStatusResult, GitDiffResult, GitLogEntry, GitLogResult,
    TerminalExecParams, TerminalExecResult,
};
use crate::session::{AcpSession, SessionMessage};

const SERVER_NAME: &str = "hermes-acp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// ACP server for IDE integration.
pub struct AcpServer {
    session: Arc<AcpSession>,
    interrupt: Arc<Notify>,
    workspace_path: String,
    git_integration: bool,
}

impl AcpServer {
    pub fn new(workspace_path: String) -> Self {
        Self {
            session: Arc::new(AcpSession::new()),
            interrupt: Arc::new(Notify::new()),
            workspace_path,
            git_integration: true,
        }
    }

    /// Get interrupt signal for cancellation.
    pub fn interrupt_notify(&self) -> Arc<Notify> {
        self.interrupt.clone()
    }

    /// Run the ACP server over stdio (JSON-RPC via stdin/stdout).
    pub async fn run_stdio(&self) -> Result<()> {
        info!("ACP server starting on stdio");

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let stdout = tokio::io::stdout();
        let writer = Arc::new(tokio::sync::Mutex::new(stdout));

        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                info!("ACP server: stdin closed, shutting down");
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let request: AcpRequest = match serde_json::from_str(trimmed) {
                Ok(req) => req,
                Err(e) => {
                    let resp = AcpResponse::error(
                        None,
                        AcpError {
                            code: ErrorCode::PARSE_ERROR.0,
                            message: format!("Parse error: {e}"),
                            data: None,
                        },
                    );
                    send_response(&writer, &resp).await?;
                    continue;
                }
            };

            let response = self.handle_request(request).await;
            if response.is_some() {
                send_response(&writer, &response.unwrap()).await?;
            }
        }

        Ok(())
    }

    /// Handle a single ACP request and return a response.
    async fn handle_request(&self, req: AcpRequest) -> Option<AcpResponse> {
        let id = req.id.clone();

        let method = match req.parse_method() {
            Ok(m) => m,
            Err(_) => {
                return Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::METHOD_NOT_FOUND.0,
                        message: format!("Method not found: {}", req.method),
                        data: None,
                    },
                ));
            }
        };

        let params = req.params.unwrap_or(Value::Null);

        match method {
            Method::Initialize => self.handle_initialize(id, params).await,
            Method::Shutdown => {
                info!("ACP server shutting down");
                Some(AcpResponse::success(id, json!({})))
            }
            Method::SendMessage => self.handle_send_message(id, params).await,
            Method::CancelRequest => self.handle_cancel(id, params).await,
            Method::SetFileContext => self.handle_set_file_context(id, params).await,
            Method::GetFileContext => self.handle_get_file_context(id, params).await,
            Method::SetSelection => self.handle_set_selection(id, params).await,
            Method::GitStatus => self.handle_git_status(id, params).await,
            Method::GitDiff => self.handle_git_diff(id, params).await,
            Method::GitLog => self.handle_git_log(id, params).await,
            Method::TerminalExec => self.handle_terminal_exec(id, params).await,
            Method::TerminalRead => {
                Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::METHOD_NOT_FOUND.0,
                        message: "terminalRead not yet implemented".to_string(),
                        data: None,
                    },
                ))
            }
            Method::Notification => None, // notifications don't get responses
        }
    }

    async fn handle_initialize(&self, id: Option<Value>, params: Value) -> Option<AcpResponse> {
        let init: Result<InitializeParams, _> = serde_json::from_value(params);
        let init = match init {
            Ok(p) => p,
            Err(e) => {
                return Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::INVALID_PARAMS.0,
                        message: format!("Invalid initialize params: {e}"),
                        data: None,
                    },
                ));
            }
        };

        let session_id = self.session.create_session(
            init.workspace_path.clone(),
            init.client_name.clone(),
        );

        let result = InitializeResult {
            server_name: SERVER_NAME.to_string(),
            server_version: SERVER_VERSION.to_string(),
            capabilities: ServerCapabilities::default(),
            session_id,
        };

        Some(AcpResponse::success(id, json!(result)))
    }

    async fn handle_send_message(&self, id: Option<Value>, params: Value) -> Option<AcpResponse> {
        let msg_params: Result<SendMessageParams, _> = serde_json::from_value(params);
        let msg_params = match msg_params {
            Ok(p) => p,
            Err(e) => {
                return Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::INVALID_PARAMS.0,
                        message: format!("Invalid sendMessage params: {e}"),
                        data: None,
                    },
                ));
            }
        };

        let session_id = self.session.get_or_create_session(
            msg_params.session_id.clone(),
            self.workspace_path.clone(),
            "acp-client".to_string(),
        );

        // Add user message
        self.session.add_message(&session_id, SessionMessage::user(msg_params.message.clone()));

        // Get context for prompt injection
        let context = self.session.get_context(&session_id);
        let _prompt_with_context = if let Some(ctx) = context {
            let ctx_text = ctx.format_prompt_context();
            if !ctx_text.is_empty() {
                format!("{}\n\n{}", ctx_text, msg_params.message)
            } else {
                msg_params.message.clone()
            }
        } else {
            msg_params.message.clone()
        };

        // Echo response (placeholder - actual LLM call would go here)
        let response_text = format!("Received: {}", msg_params.message);
        self.session.add_message(&session_id, SessionMessage::assistant(response_text.clone()));

        Some(AcpResponse::success(id, json!({
            "session_id": session_id,
            "response": response_text,
        })))
    }

    async fn handle_cancel(&self, id: Option<Value>, _params: Value) -> Option<AcpResponse> {
        self.interrupt.notify_one();
        Some(AcpResponse::success(id, json!({ "cancelled": true })))
    }

    async fn handle_set_file_context(&self, id: Option<Value>, params: Value) -> Option<AcpResponse> {
        let ctx_params: Result<SetFileContextParams, _> = serde_json::from_value(params);
        let ctx_params = match ctx_params {
            Ok(p) => p,
            Err(e) => {
                return Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::INVALID_PARAMS.0,
                        message: format!("Invalid setFileContext params: {e}"),
                        data: None,
                    },
                ));
            }
        };

        // Use the most recent session or create one
        let session_id = self.session.get_or_create_session(
            None,
            self.workspace_path.clone(),
            "acp-client".to_string(),
        );

        // Convert FileContextEntry to FileContext
        let files: Vec<FileContext> = ctx_params.files.into_iter().map(|entry| {
            FileContext {
                path: entry.path,
                content: format!("[content for file with range: {:?}]", entry.range),
                is_open: true,
            }
        }).collect();

        self.session.set_file_context(&session_id, files);

        Some(AcpResponse::success(id, json!({ "session_id": session_id })))
    }

    async fn handle_get_file_context(&self, id: Option<Value>, params: Value) -> Option<AcpResponse> {
        // Extract session_id from params if present
        let session_id = params.get("sessionId").and_then(|v| v.as_str()).map(|s| s.to_string());
        let session_id = match session_id {
            Some(id) => id,
            None => {
                return Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::INVALID_PARAMS.0,
                        message: "sessionId required".to_string(),
                        data: None,
                    },
                ));
            }
        };

        match self.session.get_context(&session_id) {
            Some(ctx) => Some(AcpResponse::success(id, json!(ctx))),
            None => Some(AcpResponse::error(
                id,
                AcpError {
                    code: ErrorCode::INVALID_PARAMS.0,
                    message: "Session not found".to_string(),
                    data: None,
                },
            )),
        }
    }

    async fn handle_set_selection(&self, id: Option<Value>, params: Value) -> Option<AcpResponse> {
        let sel_params: Result<SetSelectionParams, _> = serde_json::from_value(params);
        let sel_params = match sel_params {
            Ok(p) => p,
            Err(e) => {
                return Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::INVALID_PARAMS.0,
                        message: format!("Invalid setSelection params: {e}"),
                        data: None,
                    },
                ));
            }
        };

        let session_id = self.session.get_or_create_session(
            None,
            self.workspace_path.clone(),
            "acp-client".to_string(),
        );

        let selection = Selection {
            file_path: sel_params.file_path,
            text: sel_params.text,
            start_line: sel_params.start_line,
            end_line: sel_params.end_line,
        };

        self.session.set_selection(&session_id, selection);

        Some(AcpResponse::success(id, json!({ "session_id": session_id })))
    }

    async fn handle_git_status(&self, id: Option<Value>, _params: Value) -> Option<AcpResponse> {
        if !self.git_integration {
            return Some(AcpResponse::error(
                id,
                AcpError {
                    code: ErrorCode::INTERNAL_ERROR.0,
                    message: "Git integration disabled".to_string(),
                    data: None,
                },
            ));
        }

        // Run git status command
        let result = match self.run_git_command(&["status", "--porcelain"]).await {
            Ok(output) => {
                let mut modified = Vec::new();
                let mut added = Vec::new();
                let mut deleted = Vec::new();
                let mut untracked = Vec::new();

                for line in output.lines() {
                    if line.len() < 4 {
                        continue;
                    }
                    let status = &line[..2];
                    let path = line[3..].to_string();
                    match status {
                        " M" | "M " | "MM" => modified.push(path),
                        "A " | "AM" => added.push(path),
                        " D" | "MD" | "DD" => deleted.push(path),
                        "??" => untracked.push(path),
                        _ => {}
                    }
                }

                // Get current branch
                let branch = self.run_git_command(&["branch", "--show-current"]).await
                    .unwrap_or_else(|_| "HEAD".to_string())
                    .trim()
                    .to_string();

                GitStatusResult {
                    branch,
                    modified,
                    added,
                    deleted,
                    untracked,
                }
            }
            Err(e) => {
                return Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::INTERNAL_ERROR.0,
                        message: format!("Git command failed: {e}"),
                        data: None,
                    },
                ));
            }
        };

        Some(AcpResponse::success(id, json!(result)))
    }

    async fn handle_git_diff(&self, id: Option<Value>, params: Value) -> Option<AcpResponse> {
        let file = params.get("file").and_then(|v| v.as_str());
        let args = match file {
            Some(f) => vec!["diff", f],
            None => vec!["diff", "--staged"],
        };

        match self.run_git_command(&args).await {
            Ok(diff) => Some(AcpResponse::success(id, json!(GitDiffResult { diff }))),
            Err(e) => Some(AcpResponse::error(
                id,
                AcpError {
                    code: ErrorCode::INTERNAL_ERROR.0,
                    message: format!("Git diff failed: {e}"),
                    data: None,
                },
            )),
        }
    }

    async fn handle_git_log(&self, id: Option<Value>, params: Value) -> Option<AcpResponse> {
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20);
        let git_format = r#"%H%n%an%n%ad%n%s"#;
        let limit_str = format!("-{limit}");
        let pretty_str = format!("--pretty=format:{git_format}");
        let args = vec!["log", limit_str.as_str(), "--date=iso", pretty_str.as_str()];

        match self.run_git_command(&args).await {
            Ok(output) => {
                let mut entries = Vec::new();
                let mut lines = output.lines();
                while let Some(hash) = lines.next() {
                    let author = lines.next().unwrap_or("");
                    let date = lines.next().unwrap_or("");
                    let message = lines.next().unwrap_or("");
                    if hash.is_empty() {
                        continue;
                    }
                    entries.push(GitLogEntry {
                        hash: hash.to_string(),
                        author: author.to_string(),
                        date: date.to_string(),
                        message: message.to_string(),
                    });
                }
                Some(AcpResponse::success(id, json!(GitLogResult { entries })))
            }
            Err(e) => Some(AcpResponse::error(
                id,
                AcpError {
                    code: ErrorCode::INTERNAL_ERROR.0,
                    message: format!("Git log failed: {e}"),
                    data: None,
                },
            )),
        }
    }

    async fn handle_terminal_exec(&self, id: Option<Value>, params: Value) -> Option<AcpResponse> {
        let term_params: Result<TerminalExecParams, _> = serde_json::from_value(params);
        let term_params = match term_params {
            Ok(p) => p,
            Err(e) => {
                return Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::INVALID_PARAMS.0,
                        message: format!("Invalid terminalExec params: {e}"),
                        data: None,
                    },
                ));
            }
        };

        let working_dir = term_params.working_dir.as_deref().unwrap_or(&self.workspace_path);

        // Parse command into args (simple space splitting, doesn't handle quotes)
        let parts: Vec<&str> = term_params.command.split_whitespace().collect();
        if parts.is_empty() {
            return Some(AcpResponse::error(
                id,
                AcpError {
                    code: ErrorCode::INVALID_PARAMS.0,
                    message: "Empty command".to_string(),
                    data: None,
                },
            ));
        }

        let output = match tokio::process::Command::new(parts[0])
            .args(&parts[1..])
            .current_dir(working_dir)
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                return Some(AcpResponse::error(
                    id,
                    AcpError {
                        code: ErrorCode::INTERNAL_ERROR.0,
                        message: format!("Command execution failed: {e}"),
                        data: None,
                    },
                ));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        Some(AcpResponse::success(id, json!(TerminalExecResult {
            exit_code,
            stdout,
            stderr,
        })))
    }

    async fn run_git_command(&self, args: &[&str]) -> Result<String> {
        let output = tokio::process::Command::new("git")
            .args(args)
            .current_dir(&self.workspace_path)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("git command failed: {}", stderr.trim()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

async fn send_response(
    writer: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    response: &AcpResponse,
) -> Result<()> {
    let json = serde_json::to_string(response)?;
    let mut w = writer.lock().await;
    w.write_all(json.as_bytes()).await?;
    w.write_all(b"\n").await?;
    w.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acp_server_new() {
        let server = AcpServer::new("/workspace".to_string());
        assert_eq!(server.workspace_path, "/workspace");
        assert_eq!(server.session.session_count(), 0);
    }

    #[tokio::test]
    async fn test_handle_initialize() {
        let server = AcpServer::new("/workspace".to_string());
        let params = json!({
            "clientName": "vscode",
            "clientVersion": "1.0",
            "workspacePath": "/workspace"
        });
        let resp = server.handle_initialize(Some(json!(1)), params).await.unwrap();
        assert!(resp.error.is_none());
        let result: InitializeResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(result.server_name, "hermes-acp");
        assert!(!result.session_id.is_empty());
    }

    #[tokio::test]
    async fn test_handle_send_message() {
        let server = AcpServer::new("/workspace".to_string());
        let params = json!({
            "message": "hello world"
        });
        let resp = server.handle_send_message(Some(json!(1)), params).await.unwrap();
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert!(result.get("response").is_some());
    }

    #[tokio::test]
    async fn test_handle_invalid_method_params() {
        let server = AcpServer::new("/workspace".to_string());
        let resp = server.handle_send_message(Some(json!(1)), json!({})).await.unwrap();
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_acp_request_method_parsing() {
        let req = AcpRequest::new("sendMessage", Some(json!(1)), json!({"message": "test"}));
        assert_eq!(req.parse_method().unwrap(), Method::SendMessage);

        let req2 = AcpRequest::new("unknownMethod", Some(json!(2)), None);
        assert!(req2.parse_method().is_err());
    }
}
