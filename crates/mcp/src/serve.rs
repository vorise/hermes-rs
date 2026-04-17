use std::io::{BufRead, Write, BufReader};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};

use h_tools::{Tool, ToolContext, create_all_tools};

/// MCP server that exposes Hermes tools via the Model Context Protocol.
///
/// Runs over stdio (JSON-RPC) so it can be used by Claude Desktop, Cursor,
/// and other MCP clients as a tool server.
///
/// ```text
/// MCP Client → stdin → McpServe → stdout → MCP Client
///                         │
///                    Hermes Tools
/// ```
pub struct McpServe {
    /// Registered tool implementations.
    tools: Vec<Arc<dyn Tool>>,
    /// Server name advertised during initialization.
    server_name: String,
    /// Server version.
    server_version: String,
    /// Max result size in characters (0 = unlimited).
    max_result_size: usize,
}

impl McpServe {
    pub fn new() -> Self {
        Self {
            tools: create_all_tools().into_iter().map(|t| t as Arc<dyn Tool>).collect(),
            server_name: "hermes-agent".to_string(),
            server_version: "0.1.0".to_string(),
            max_result_size: 100_000,
        }
    }

    /// Set the server name.
    pub fn with_server_name(mut self, name: &str) -> Self {
        self.server_name = name.to_string();
        self
    }

    /// Set the server version.
    pub fn with_server_version(mut self, version: &str) -> Self {
        self.server_version = version.to_string();
        self
    }

    /// Set the max result size in characters.
    pub fn with_max_result_size(mut self, size: usize) -> Self {
        self.max_result_size = size;
        self
    }

    /// Replace the default tool set with a custom one.
    pub fn with_tools(mut self, tools: Vec<Arc<dyn Tool>>) -> Self {
        self.tools = tools;
        self
    }

    /// Run the MCP server loop, reading from stdin and writing to stdout.
    /// Blocks until EOF or shutdown.
    pub fn run(&self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let state = Arc::new(Mutex::new(ServerState {
            initialized: false,
        }));

        let mut reader = BufReader::new(stdin.lock());
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(error = %e, "MCP Serve read error");
                    break;
                }
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let response = self.handle_message(trimmed, &state);
            if let Some(resp) = response {
                let mut out = stdout.lock();
                writeln!(out, "{resp}").context("Failed to write MCP response")?;
                out.flush().context("Failed to flush MCP output")?;
            }
        }

        tracing::info!("MCP Serve stopped");
        Ok(())
    }

    /// Async version of run.
    pub async fn run_async(&self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let state = Arc::new(Mutex::new(ServerState {
            initialized: false,
        }));

        let mut reader = TokioBufReader::new(stdin);
        let mut out = stdout;
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(error = %e, "MCP Serve read error");
                    break;
                }
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let response = self.handle_message_async(trimmed, &state).await;
            if let Some(resp) = response {
                out.write_all(resp.as_bytes()).await
                    .context("Failed to write MCP response")?;
                out.write_all(b"\n").await
                    .context("Failed to write MCP newline")?;
                out.flush().await
                    .context("Failed to flush MCP output")?;
            }
        }

        tracing::info!("MCP Serve stopped");
        Ok(())
    }

    /// Handle a single JSON-RPC message (sync version).
    fn handle_message(&self, raw: &str, state: &Mutex<ServerState>) -> Option<String> {
        let msg: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "MCP Serve invalid JSON");
                return None;
            }
        };

        // Notifications have no id, no response needed
        if msg.get("method").is_some() && msg.get("id").is_none() {
            self.handle_notification(&msg, state);
            return None;
        }

        // Requests have id, need response
        if let Some(id) = msg.get("id") {
            let result = self.handle_request(&msg, state);
            let id_str = id.to_string();
            match result {
                Ok(value) => Some(format_json_rpc_response(&id_str, value)),
                Err(e) => Some(format_json_rpc_error(id_str, -32603, e.to_string())),
            }
        } else {
            None
        }
    }

    /// Handle a single JSON-RPC message (async version).
    async fn handle_message_async(&self, raw: &str, state: &Mutex<ServerState>) -> Option<String> {
        let msg: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "MCP Serve invalid JSON");
                return None;
            }
        };

        if msg.get("method").is_some() && msg.get("id").is_none() {
            self.handle_notification(&msg, state);
            return None;
        }

        if let Some(id) = msg.get("id").cloned() {
            let result = self.handle_request_async(&msg, state).await;
            let id_str = id.to_string();
            match result {
                Ok(value) => Some(format_json_rpc_response(&id_str, value)),
                Err(e) => Some(format_json_rpc_error(id_str, -32603, e.to_string())),
            }
        } else {
            None
        }
    }

    /// Handle a notification (no response).
    fn handle_notification(&self, msg: &Value, state: &Mutex<ServerState>) {
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        match method {
            "notifications/initialized" => {
                state.lock().initialized = true;
                tracing::info!("MCP Serve initialized");
            }
            _ => {
                tracing::debug!(method = %method, "MCP Serve unknown notification");
            }
        }
    }

    /// Handle a request (returns a response value or error).
    fn handle_request(&self, msg: &Value, state: &Mutex<ServerState>) -> Result<Value> {
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => self.handle_initialize(&params),
            "tools/list" => {
                self.check_initialized(state)?;
                self.handle_tools_list(&params)
            }
            "tools/call" => {
                self.check_initialized(state)?;
                // Sync tool execution — blocking but acceptable for stdio MCP
                self.handle_tools_call_sync(&params)
            }
            "resources/list" => {
                self.check_initialized(state)?;
                Ok(serde_json::json!({ "resources": [] }))
            }
            "prompts/list" => {
                self.check_initialized(state)?;
                Ok(serde_json::json!({ "prompts": [] }))
            }
            "ping" => Ok(Value::Null),
            _ => Err(anyhow!("Method not found: {method}")),
        }
    }

    /// Handle a request (async version).
    async fn handle_request_async(&self, msg: &Value, state: &Mutex<ServerState>) -> Result<Value> {
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => self.handle_initialize(&params),
            "tools/list" => {
                self.check_initialized(state)?;
                self.handle_tools_list(&params)
            }
            "tools/call" => {
                self.check_initialized(state)?;
                self.handle_tools_call(&params).await
            }
            "resources/list" => {
                self.check_initialized(state)?;
                Ok(serde_json::json!({ "resources": [] }))
            }
            "prompts/list" => {
                self.check_initialized(state)?;
                Ok(serde_json::json!({ "prompts": [] }))
            }
            "ping" => Ok(Value::Null),
            _ => Err(anyhow!("Method not found: {method}")),
        }
    }

    /// Handle the initialize handshake.
    fn handle_initialize(&self, params: &Value) -> Result<Value> {
        let protocol_version = params.get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("2024-11-05");

        Ok(serde_json::json!({
            "protocolVersion": protocol_version,
            "capabilities": {
                "tools": {
                    "listChanged": true,
                },
            },
            "serverInfo": {
                "name": self.server_name,
                "version": self.server_version,
            },
        }))
    }

    /// List all available tools.
    fn handle_tools_list(&self, _params: &Value) -> Result<Value> {
        let tools: Vec<Value> = self.tools.iter().map(|t| {
            serde_json::json!({
                "name": t.name(),
                "description": t.description(),
                "inputSchema": t.schema(),
            })
        }).collect();

        Ok(serde_json::json!({ "tools": tools }))
    }

    /// Call a tool (sync version, for stdio run).
    fn handle_tools_call_sync(&self, params: &Value) -> Result<Value> {
        let name = params.get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing 'name' in tools/call"))?;

        let arguments = params.get("arguments").cloned().unwrap_or(Value::Object(Default::default()));

        let tool = self.tools.iter().find(|t| t.name() == name)
            .ok_or_else(|| anyhow!("Unknown tool: {name}"))?;

        let ctx = ToolContext::default();

        // Block on async tool execution
        let rt = tokio::runtime::Handle::try_current()
            .ok();

        let result = if let Some(handle) = rt {
            // Already in a tokio runtime
            let fut = tool.execute(arguments, &ctx);
            tokio::task::block_in_place(|| handle.block_on(fut))
        } else {
            // No runtime, create one
            let rt = tokio::runtime::Runtime::new()
                .context("Failed to create tokio runtime for tool execution")?;
            let fut = tool.execute(arguments, &ctx);
            rt.block_on(fut)
        }?;

        let mut content = result.content;
        if self.max_result_size > 0 && content.len() > self.max_result_size {
            content.truncate(self.max_result_size);
        }

        Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": content,
            }],
            "isError": result.is_error,
        }))
    }

    /// Call a tool (async version).
    async fn handle_tools_call(&self, params: &Value) -> Result<Value> {
        let name = params.get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing 'name' in tools/call"))?;

        let arguments = params.get("arguments").cloned().unwrap_or(Value::Object(Default::default()));

        let tool = self.tools.iter().find(|t| t.name() == name)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown tool: {name}"))?;

        let ctx = ToolContext::default();
        let result = tool.execute(arguments, &ctx).await?;

        let mut content = result.content;
        if self.max_result_size > 0 && content.len() > self.max_result_size {
            content.truncate(self.max_result_size);
        }

        Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": content,
            }],
            "isError": result.is_error,
        }))
    }

    /// Check if the server has been initialized.
    fn check_initialized(&self, state: &Mutex<ServerState>) -> Result<()> {
        let s = state.lock();
        if !s.initialized {
            Err(anyhow!("Server not initialized. Send 'initialize' first."))
        } else {
            Ok(())
        }
    }

    /// Get tool count (for testing).
    #[cfg(test)]
    fn tool_count(&self) -> usize {
        self.tools.len()
    }
}

impl Default for McpServe {
    fn default() -> Self {
        Self::new()
    }
}

/// Server state shared across the message handling loop.
struct ServerState {
    initialized: bool,
}

/// Format a JSON-RPC response.
fn format_json_rpc_response(id: &str, result: Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id.parse::<u64>().unwrap_or(0),
        "result": result,
    }).to_string()
}

/// Format a JSON-RPC error.
fn format_json_rpc_error(id: String, code: i64, message: String) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id.parse::<u64>().unwrap_or(0),
        "error": {
            "code": code,
            "message": message,
        },
    }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_server_has_tools() {
        let server = McpServe::new();
        assert!(server.tool_count() > 0);
    }

    #[test]
    fn test_server_name_version() {
        let server = McpServe::new()
            .with_server_name("custom-hermes")
            .with_server_version("1.0.0");
        assert_eq!(server.server_name, "custom-hermes");
        assert_eq!(server.server_version, "1.0.0");
    }

    #[test]
    fn test_max_result_size() {
        let server = McpServe::new().with_max_result_size(50_000);
        assert_eq!(server.max_result_size, 50_000);
    }

    #[test]
    fn test_initialize_response() {
        let server = McpServe::new();
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1.0" },
        });

        let result = server.handle_initialize(&params).unwrap();
        assert_eq!(result["serverInfo"]["name"], "hermes-agent");
        assert_eq!(result["serverInfo"]["version"], "0.1.0");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[test]
    fn test_tools_list() {
        let server = McpServe::new();
        let result = server.handle_tools_list(&Value::Null).unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
        // Check that each tool has the required fields
        for tool in tools {
            assert!(tool.get("name").is_some());
            assert!(tool.get("description").is_some());
            assert!(tool.get("inputSchema").is_some());
        }
    }

    #[test]
    fn test_tools_call_unknown_tool() {
        let server = McpServe::new();
        let params = serde_json::json!({
            "name": "nonexistent_tool",
            "arguments": {},
        });
        let result = server.handle_tools_call_sync(&params);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent_tool"));
    }

    #[test]
    fn test_not_initialized_check() {
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: false });
        let result = server.check_initialized(&state);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_message_initialize() {
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: false });

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1.0" },
            },
        });

        let response = server.handle_message(&msg.to_string(), &state);
        assert!(response.is_some());
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["serverInfo"]["name"], "hermes-agent");
    }

    #[test]
    fn test_handle_notification_initialized() {
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: false });

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        });

        // No response expected for notifications
        let response = server.handle_message(&msg.to_string(), &state);
        assert!(response.is_none());
        assert!(state.lock().initialized);
    }

    #[test]
    fn test_handle_message_unknown_method() {
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: true });

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "unknown/method",
        });

        let response = server.handle_message(&msg.to_string(), &state);
        assert!(response.is_some());
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert!(resp.get("error").is_some());
    }

    #[test]
    fn test_handle_message_invalid_json() {
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: true });

        let response = server.handle_message("not valid json {{{", &state);
        assert!(response.is_none());
    }

    #[test]
    fn test_handle_ping() {
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: true });

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping",
        });

        let response = server.handle_message(&msg.to_string(), &state);
        assert!(response.is_some());
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert!(resp["result"].is_null());
    }

    #[test]
    fn test_resources_list_empty() {
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: true });
        let result = server.handle_request(
            &serde_json::json!({ "method": "resources/list" }),
            &state,
        ).unwrap();
        assert_eq!(result["resources"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_prompts_list_empty() {
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: true });
        let result = server.handle_request(
            &serde_json::json!({ "method": "prompts/list" }),
            &state,
        ).unwrap();
        assert_eq!(result["prompts"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_custom_tools() {
        // Test with no tools
        let server = McpServe::new().with_tools(vec![]);
        assert_eq!(server.tool_count(), 0);
    }

    #[test]
    fn test_tools_call_read_file_sync() {
        // Test calling an actual tool — read a file that exists in the repo
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: true });

        let params = serde_json::json!({
            "name": "read_file",
            "arguments": { "path": "Cargo.toml" },
        });

        let result = server.handle_request(
            &serde_json::json!({ "method": "tools/call", "params": params }),
            &state,
        );

        // Should succeed and contain content
        assert!(result.is_ok());
        let value = result.unwrap();
        let content = value["content"][0]["text"].as_str().unwrap_or("");
        assert!(!content.is_empty());
    }

    #[tokio::test]
    async fn test_tools_call_read_file_async() {
        let server = McpServe::new();
        let state = Mutex::new(ServerState { initialized: true });

        let msg = serde_json::json!({
            "method": "tools/call",
            "params": {
                "name": "read_file",
                "arguments": { "path": "Cargo.toml" },
            },
        });

        let result = server.handle_request_async(&msg, &state).await;
        assert!(result.is_ok());
        let value = result.unwrap();
        let content = value["content"][0]["text"].as_str().unwrap_or("");
        assert!(!content.is_empty());
    }
}
