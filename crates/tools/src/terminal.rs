use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::approval::is_destructive_command;
use crate::process_registry::ProcessRegistry;
use crate::tool::{Tool, ToolContext, ToolResult};

/// Maximum output size returned to the LLM (100KB).
const MAX_OUTPUT_SIZE: usize = 100 * 1024;

/// Timeout for command execution in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Terminal execution tool.
///
/// Executes shell commands and returns stdout/stderr output.
pub struct TerminalTool {
    process_registry: Arc<Mutex<ProcessRegistry>>,
}

impl TerminalTool {
    pub fn new() -> Self {
        Self {
            process_registry: Arc::new(Mutex::new(ProcessRegistry::new())),
        }
    }

    pub fn registry(&self) -> Arc<Mutex<ProcessRegistry>> {
        self.process_registry.clone()
    }
}

impl Default for TerminalTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TerminalTool {
    fn name(&self) -> &str {
        "terminal"
    }

    fn toolset(&self) -> &str {
        "terminal"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the terminal. Returns stdout and stderr output. \
        For long-running commands, use 'background: true' to run asynchronously."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Maximum execution time in seconds (default: 60)"
                },
                "background": {
                    "type": "boolean",
                    "description": "Run the command in the background (default: false)"
                }
            },
            "required": ["command"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let command = args.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: command"))?;

        let timeout = args.get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        let background = args.get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if is_destructive_command(command) {
            tracing::warn!("Destructive command detected: {command}");
        }

        if background {
            // Spawn background process directly
            match tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&ctx.working_dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => {
                    let pid = child.id().unwrap_or(0);
                    Ok(ToolResult::ok(format!(
                        "Started background process: {command} (pid: {pid})"
                    )))
                }
                Err(e) => Ok(ToolResult::err(format!("Failed to start background process: {e}"))),
            }
        } else {
            self.execute_with_timeout(command, timeout, ctx).await
        }
    }
}

impl TerminalTool {
    async fn execute_with_timeout(&self, command: &str, timeout_secs: u64, ctx: &ToolContext) -> Result<ToolResult> {
        let timeout = std::time::Duration::from_secs(timeout_secs);

        let output = tokio::time::timeout(
            timeout,
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&ctx.working_dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output(),
        ).await;

        match output {
            Ok(Ok(result)) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let stderr = String::from_utf8_lossy(&result.stderr);
                let exit_code = result.status.code().unwrap_or(-1);

                let mut combined = String::new();
                if exit_code != 0 {
                    combined.push_str(&format!("Exit code: {exit_code}\n"));
                }

                if !stdout.trim().is_empty() {
                    combined.push_str(&stdout);
                }

                if !stderr.trim().is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&stderr);
                }

                if combined.is_empty() {
                    combined = "(no output)".to_string();
                }

                Ok(ToolResult::ok(truncate(&combined, MAX_OUTPUT_SIZE)))
            }
            Ok(Err(e)) => Ok(ToolResult::err(format!("Failed to execute command: {e}"))),
            Err(_) => Ok(ToolResult::err(format!(
                "Command timed out after {timeout_secs} seconds"
            ))),
        }
    }
}

fn truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let truncated = &s[..max_bytes.min(s.len())];
        let boundary = truncated
            .char_indices()
            .last()
            .map(|(i, _)| i + 1)
            .unwrap_or(0);
        format!("{}... [truncated, {} bytes total]", &truncated[..boundary], s.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            task_id: "test".to_string(),
            config: std::sync::Arc::new(h_core::HermesConfig::default()),
            working_dir: std::env::temp_dir(),
            clarify: None,
        }
    }

    #[tokio::test]
    async fn test_terminal_echo() {
        let tool = TerminalTool::new();
        let ctx = test_ctx();

        let result = tool.execute(
            json!({"command": "echo hello"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("hello"));
    }

    #[tokio::test]
    async fn test_terminal_pwd() {
        let tool = TerminalTool::new();
        let ctx = test_ctx();

        let result = tool.execute(
            json!({"command": "pwd"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("tmp"));
    }

    #[tokio::test]
    async fn test_terminal_missing_command() {
        let tool = TerminalTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("command"));
    }

    #[tokio::test]
    async fn test_terminal_timeout() {
        let tool = TerminalTool::new();
        let ctx = test_ctx();

        let result = tool.execute(
            json!({"command": "sleep 10", "timeout": 1}),
            &ctx,
        ).await.unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("timed out"));
    }
}
