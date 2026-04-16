use thiserror::Error;

/// Errors specific to MCP operations.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("MCP server '{0}' not found")]
    ServerNotFound(String),

    #[error("MCP server '{0}' already connected")]
    ServerAlreadyConnected(String),

    #[error("transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("tool '{name}' not found on server '{server}'")]
    ToolNotFound { server: String, name: String },

    #[error("resource '{uri}' not found on server '{server}'")]
    ResourceNotFound { server: String, uri: String },

    #[error("prompt '{name}' not found on server '{server}'")]
    PromptNotFound { server: String, name: String },

    #[error("server '{0}' does not support {1}")]
    CapabilityNotSupported(String, String),

    #[error("MCP tool name '{0}' shadows a built-in tool")]
    ShadowPrevention(String),

    #[error("server '{0}' disconnected unexpectedly: {1}")]
    ServerDisconnected(String, String),

    #[error("request timed out after {timeout_ms}ms")]
    RequestTimeout { timeout_ms: u64 },

    #[error("JSON-RPC error {code}: {message}")]
    JsonRpcError { code: i64, message: String },

    #[error("invalid server response: {0}")]
    InvalidResponse(String),
}

/// Transport-level errors.
#[derive(Debug, Error)]
pub enum TransportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("process error: {0}")]
    Process(String),

    #[error("HTTP error: {status}")]
    Http { status: u16, message: String },

    #[error("failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("channel closed")]
    ChannelClosed,

    #[error("transport not connected")]
    NotConnected,

    #[error("SSE error: {0}")]
    Sse(String),
}
