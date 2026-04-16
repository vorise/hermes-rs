use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};

use crate::error::TransportError;

/// MCP transport layer — how we communicate with an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    /// Spawn a subprocess and communicate via stdin/stdout (JSON-RPC).
    Stdio {
        command: String,
        args: Vec<String>,
        env: Option<Vec<(String, String)>>,
    },
    /// Connect to a remote MCP server via HTTP Server-Sent Events.
    Sse {
        url: String,
    },
}

/// JSON-RPC request.
#[derive(Debug, Clone, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

/// JSON-RPC response.
#[derive(Debug, Clone, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<u64>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

/// JSON-RPC notification (no id).
#[derive(Debug, Clone, Serialize)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

type PendingMap = std::collections::HashMap<u64, mpsc::Sender<Value>>;

/// Stdio-based MCP transport.
///
/// Spawns a subprocess and sends/receives JSON-RPC messages over stdin/stdout.
pub struct StdioTransport {
    command: String,
    args: Vec<String>,
    env: Option<Vec<(String, String)>>,
    /// Next request ID.
    next_id: u64,
    /// stdin writer.
    stdin: Option<tokio::process::ChildStdin>,
    /// Child process handle.
    child: Option<Arc<Mutex<tokio::process::Child>>>,
    /// Pending responses keyed by request ID.
    pending: Arc<Mutex<PendingMap>>,
}

impl StdioTransport {
    pub fn new(command: String, args: Vec<String>, env: Option<Vec<(String, String)>>) -> Self {
        Self {
            command,
            args,
            env,
            next_id: 1,
            stdin: None,
            child: None,
            pending: Arc::new(Mutex::new(PendingMap::new())),
        }
    }

    /// Start the subprocess and begin reading stdout.
    pub async fn connect(&mut self) -> Result<()> {
        let mut cmd = tokio::process::Command::new(&self.command);
        cmd.args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(ref env_vars) = self.env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server: {}", self.command))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            TransportError::Process("Failed to capture stdin of child".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            TransportError::Process("Failed to capture stdout of child".to_string())
        })?;

        self.stdin = Some(stdin);
        self.child = Some(Arc::new(Mutex::new(child)));

        // Wrap stdout in BufReader + Arc for shared reading
        let reader = Arc::new(Mutex::new(BufReader::new(stdout)));

        // Start a background task to parse and dispatch incoming messages
        let pending = self.pending.clone();
        tokio::spawn(Self::read_loop(reader, pending));

        tracing::info!(command = %self.command, "MCP stdio transport connected");
        Ok(())
    }

    /// Read loop: parses JSON-RPC messages from stdout and dispatches to pending futures.
    async fn read_loop(
        reader: Arc<Mutex<BufReader<tokio::process::ChildStdout>>>,
        pending: Arc<Mutex<PendingMap>>,
    ) {
        let mut line = String::new();
        loop {
            line.clear();
            let mut locked = reader.lock().await;
            match locked.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(error = %e, "MCP read error");
                    break;
                }
            }
            drop(locked);

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                if let Some(id) = response.id {
                    let mut map = pending.lock().await;
                    if let Some(tx) = map.remove(&id) {
                        let value = response.result.unwrap_or(Value::Null);
                        let _ = tx.send(value).await;
                    }
                }
            }
        }
    }

    /// Send a JSON-RPC request and wait for the response.
    pub async fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let json = serde_json::to_string(&request)
            .map_err(|e| TransportError::Json(e))?;

        let stdin = self.stdin.as_mut().ok_or(TransportError::NotConnected)?;
        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| TransportError::Io(e))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| TransportError::Io(e))?;

        // Set up response channel
        let (tx, mut rx) = mpsc::channel(1);
        {
            let mut map = self.pending.lock().await;
            map.insert(id, tx);
        }

        // Wait for response with timeout
        let timeout_ms = 30_000;
        match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            rx.recv(),
        )
        .await
        {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(TransportError::ChannelClosed.into()),
            Err(_) => Err(
                crate::error::McpError::RequestTimeout { timeout_ms }.into(),
            ),
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    pub async fn notify(&mut self, method: &str, params: Option<Value>) -> Result<()> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };

        let json = serde_json::to_string(&notification)
            .map_err(|e| TransportError::Json(e))?;

        let stdin = self.stdin.as_mut().ok_or(TransportError::NotConnected)?;
        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| TransportError::Io(e))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| TransportError::Io(e))?;

        Ok(())
    }

    /// Disconnect by killing the subprocess.
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(ref mut child) = self.child {
            let mut c = child.lock().await;
            let _ = c.kill().await;
            let _ = c.wait().await;
        }
        self.stdin.take();
        tracing::info!("MCP stdio transport disconnected");
        Ok(())
    }
}

/// SSE-based MCP transport.
///
/// Connects to a remote MCP server via HTTP and receives messages via Server-Sent Events.
pub struct SseTransport {
    url: String,
    client: reqwest::Client,
    /// The endpoint to send JSON-RPC POST requests to.
    post_endpoint: Option<String>,
    next_id: u64,
    /// Pending responses keyed by request ID.
    pending: Arc<Mutex<PendingMap>>,
}

impl SseTransport {
    pub fn new(url: String) -> Self {
        Self {
            url,
            client: reqwest::Client::new(),
            post_endpoint: None,
            next_id: 1,
            pending: Arc::new(Mutex::new(PendingMap::new())),
        }
    }

    /// Connect to the SSE endpoint and discover the POST endpoint.
    pub async fn connect(&mut self) -> Result<()> {
        tracing::info!(url = %self.url, "MCP SSE transport connecting");

        // Start SSE listener in background
        let url = self.url.clone();
        let pending = self.pending.clone();

        tokio::spawn(Self::sse_loop(url, pending));

        // Use the base URL for POST requests
        self.post_endpoint = Some(self.url.clone());
        tracing::info!("MCP SSE transport connected");
        Ok(())
    }

    /// SSE read loop — reads events and dispatches to pending futures.
    async fn sse_loop(url: String, pending: Arc<Mutex<PendingMap>>) {
        let client = reqwest::Client::new();
        loop {
            let response = match client
                .get(&url)
                .header("Accept", "text/event-stream")
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(error = %e, "MCP SSE connection error");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(error = %e, "MCP SSE stream error");
                        break;
                    }
                };

                let text = String::from_utf8_lossy(&chunk);
                for line in text.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(data) {
                            if let Some(id) = response.id {
                                let mut map = pending.lock().await;
                                if let Some(tx) = map.remove(&id) {
                                    let value = response.result.unwrap_or(Value::Null);
                                    let _ = tx.send(value).await;
                                }
                            }
                        }
                    }
                }
            }

            // Reconnect on disconnect
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    /// Send a JSON-RPC request via HTTP POST.
    pub async fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let post_url = self
            .post_endpoint
            .as_deref()
            .unwrap_or(&self.url);

        // Set up response channel
        let (tx, mut rx) = mpsc::channel(1);
        {
            let mut map = self.pending.lock().await;
            map.insert(id, tx);
        }

        // Send via POST
        self.client
            .post(post_url)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("Failed to send MCP request to {post_url}"))?;

        // Wait for response via SSE with timeout
        let timeout_ms = 30_000;
        match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            rx.recv(),
        )
        .await
        {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(TransportError::ChannelClosed.into()),
            Err(_) => Err(
                crate::error::McpError::RequestTimeout { timeout_ms }.into(),
            ),
        }
    }

    /// SSE doesn't support notifications separately.
    pub async fn notify(&mut self, _method: &str, _params: Option<Value>) -> Result<()> {
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        tracing::info!("MCP SSE transport disconnected");
        Ok(())
    }
}
