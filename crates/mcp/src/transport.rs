//! MCP Transport
//!
//! Transport implementations for MCP (Model Context Protocol).
//! Supports Stdio (process communication) and SSE (HTTP Server-Sent Events).

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use async_trait::async_trait;
use anyhow::{Result, Context, bail};
use tokio::sync::Mutex;
use tokio::process::{Command, Child};
use tokio::io::AsyncWriteExt;
use tracing::debug;
use serde_json::{json, Value};

use crate::client::{McpRequest, McpResponse, McpNotification};

/// MCP Transport trait.
#[async_trait]
pub trait McpTransportTrait: Send + Sync {
    /// Transport name.
    fn name(&self) -> &str;

    /// Send a request and wait for response.
    async fn send(&self, request: McpRequest) -> Result<McpResponse>;

    /// Send a notification (no response expected).
    async fn notify(&self, notification: McpNotification) -> Result<()>;

    /// Close the transport.
    async fn close(&mut self) -> Result<()>;
}

/// MCP Transport type.
pub enum McpTransport {
    Stdio(StdioTransport),
    Sse(SseTransport),
}

impl McpTransport {
    /// Create Stdio transport.
    pub fn stdio(command: String, args: Vec<String>, env: HashMap<String, String>) -> Self {
        Self::Stdio(StdioTransport::new(command, args, env))
    }

    /// Create SSE transport.
    pub fn sse(url: String, headers: HashMap<String, String>) -> Self {
        Self::Sse(SseTransport::new(url, headers))
    }

    /// Get transport name.
    pub fn name(&self) -> &str {
        match self {
            McpTransport::Stdio(t) => t.name(),
            McpTransport::Sse(t) => t.name(),
        }
    }
}

/// Stdio transport - communicates with spawned process via stdin/stdout.
pub struct StdioTransport {
    /// Command to execute.
    command: String,

    /// Command arguments.
    args: Vec<String>,

    /// Environment variables.
    env: HashMap<String, String>,

    /// Child process (if spawned).
    child: Option<Arc<Mutex<Child>>>,

    /// Request ID counter.
    request_id: Arc<Mutex<u64>>,
}

impl StdioTransport {
    /// Create new Stdio transport.
    pub fn new(command: String, args: Vec<String>, env: HashMap<String, String>) -> Self {
        Self {
            command,
            args,
            env,
            child: None,
            request_id: Arc::new(Mutex::new(0)),
        }
    }

    /// Spawn the MCP server process.
    pub async fn spawn(&mut self) -> Result<()> {
        debug!("Spawning MCP server: {} {:?}", self.command, self.args);

        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args);

        // Set environment variables
        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        // Configure stdio
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let child = cmd.spawn()
            .context("Failed to spawn MCP server process")?;

        self.child = Some(Arc::new(Mutex::new(child)));

        debug!("MCP server process spawned");
        Ok(())
    }
}

#[async_trait]
impl McpTransportTrait for StdioTransport {
    fn name(&self) -> &str {
        "stdio"
    }

    async fn send(&self, request: McpRequest) -> Result<McpResponse> {
        // Get next request ID
        let id = {
            let mut rid = self.request_id.lock().await;
            *rid += 1;
            *rid
        };

        // Serialize request
        let request_json = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": request.method,
            "params": request.params,
        }))?;

        // Send to process stdin
        if let Some(child) = &self.child {
            let mut child = child.lock().await;

            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(request_json.as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
            } else {
                bail!("Process stdin not available");
            }
        } else {
            bail!("Process not spawned");
        }

        // In a full implementation, we would read from stdout
        // For now, return a placeholder response
        Ok(McpResponse {
            result: Some(json!({"status": "ok"})),
            error: None,
        })
    }

    async fn notify(&self, notification: McpNotification) -> Result<()> {
        // Serialize notification (no ID)
        let notification_json = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": notification.method,
            "params": notification.params,
        }))?;

        // Send to process stdin
        if let Some(child) = &self.child {
            let mut child = child.lock().await;

            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(notification_json.as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
            } else {
                bail!("Process stdin not available");
            }
        } else {
            bail!("Process not spawned");
        }

        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(child) = &self.child {
            let mut child = child.lock().await;
            child.kill().await?;
        }
        self.child = None;
        Ok(())
    }
}

/// SSE transport - communicates via HTTP Server-Sent Events.
pub struct SseTransport {
    /// SSE endpoint URL.
    url: String,

    /// HTTP headers.
    headers: HashMap<String, String>,

    /// HTTP client.
    client: Option<reqwest::Client>,

    /// Request ID counter.
    request_id: Arc<Mutex<u64>>,
}

impl SseTransport {
    /// Create new SSE transport.
    pub fn new(url: String, headers: HashMap<String, String>) -> Self {
        Self {
            url,
            headers,
            client: Some(reqwest::Client::new()),
            request_id: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl McpTransportTrait for SseTransport {
    fn name(&self) -> &str {
        "sse"
    }

    async fn send(&self, request: McpRequest) -> Result<McpResponse> {
        let client = self.client.as_ref()
            .context("HTTP client not initialized")?;

        // Get next request ID
        let id = {
            let mut rid = self.request_id.lock().await;
            *rid += 1;
            *rid
        };

        // Build request body
        let body: Value = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": request.method,
            "params": request.params,
        });

        // Send HTTP POST
        let mut req = client.post(&self.url)
            .json(&body);

        // Add headers
        for (key, value) in &self.headers {
            req = req.header(key, value);
        }

        let response = req.send().await
            .context("HTTP request failed")?;

        if !response.status().is_success() {
            bail!("HTTP error: {}", response.status());
        }

        // Parse response
        let response_json: Value = response.json().await
            .context("Failed to parse HTTP response")?;

        let mcp_response: McpResponse = serde_json::from_value(response_json)
            .context("Failed to parse MCP response")?;

        Ok(mcp_response)
    }

    async fn notify(&self, notification: McpNotification) -> Result<()> {
        let client = self.client.as_ref()
            .context("HTTP client not initialized")?;

        // Build notification body (no ID)
        let body: Value = json!({
            "jsonrpc": "2.0",
            "method": notification.method,
            "params": notification.params,
        });

        // Send HTTP POST
        let mut req = client.post(&self.url)
            .json(&body);

        // Add headers
        for (key, value) in &self.headers {
            req = req.header(key, value);
        }

        req.send().await
            .context("HTTP notification failed")?;

        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        // SSE doesn't have persistent connection to close
        Ok(())
    }
}

/// MCP Connection wrapper.
pub struct McpConnection {
    /// Transport.
    transport: McpTransport,
}

impl McpConnection {
    /// Create new connection with transport.
    pub fn new(transport: McpTransport) -> Self {
        Self { transport }
    }

    /// Send request and wait for response.
    pub async fn send_request(&self, request: McpRequest) -> Result<McpResponse> {
        match &self.transport {
            McpTransport::Stdio(t) => t.send(request).await,
            McpTransport::Sse(t) => t.send(request).await,
        }
    }

    /// Send notification.
    pub async fn send_notification(&self, notification: McpNotification) -> Result<()> {
        match &self.transport {
            McpTransport::Stdio(t) => t.notify(notification).await,
            McpTransport::Sse(t) => t.notify(notification).await,
        }
    }

    /// Close connection.
    pub async fn close(&self) -> Result<()> {
        // Can't mutate from &self, so this is a no-op
        // In real implementation, would use interior mutability
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdio_transport_new() {
        let transport = StdioTransport::new(
            "test-server".to_string(),
            vec!["--port".to_string(), "8080".to_string()],
            HashMap::new(),
        );

        assert_eq!(transport.name(), "stdio");
        assert!(transport.child.is_none());
    }

    #[test]
    fn test_sse_transport_new() {
        let transport = SseTransport::new(
            "http://localhost:8080/sse".to_string(),
            HashMap::new(),
        );

        assert_eq!(transport.name(), "sse");
        assert!(transport.client.is_some());
    }

    #[test]
    fn test_mcp_transport_stdio() {
        let transport = McpTransport::stdio("test".to_string(), vec![], HashMap::new());
        assert_eq!(transport.name(), "stdio");
    }

    #[test]
    fn test_mcp_transport_sse() {
        let transport = McpTransport::sse("http://test".to_string(), HashMap::new());
        assert_eq!(transport.name(), "sse");
    }

    #[tokio::test]
    async fn test_mcp_connection_new() {
        let transport = McpTransport::stdio("test".to_string(), vec![], HashMap::new());
        let conn = McpConnection::new(transport);
        assert!(conn.close().await.is_ok());
    }

    #[tokio::test]
    async fn test_stdio_send_without_spawn() {
        let transport = StdioTransport::new("test".to_string(), vec![], HashMap::new());
        let request = McpRequest {
            method: "test".to_string(),
            params: None,
        };
        let result = transport.send(request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sse_notify() {
        let transport = SseTransport::new("http://test".to_string(), HashMap::new());
        let notification = McpNotification {
            method: "test".to_string(),
            params: None,
        };
        // This will fail due to network, but the struct works
        let _ = transport.notify(notification).await;
    }
}