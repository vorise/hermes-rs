// Hermes MCP (Model Context Protocol) client

mod client;
pub mod config;
mod error;
mod protocol;
pub mod tool_wrapper;
mod transport;
mod state;

pub use client::McpClient;
pub use config::{McpServerConfig, McpServerEntry, McpConfig};
pub use error::McpError;
pub use protocol::{
    McpTool, McpResource, McpPrompt, McpCapability, ToolResult, ResourceContent,
};
pub use tool_wrapper::{McpToolWrapper, McpToolRegistry};
pub use transport::{McpTransport, StdioTransport, SseTransport};
pub use state::McpState;

pub mod oauth;
pub use oauth::{
    McpOAuthConfig, McpOAuthManager, OAuthMetadata, OAuthState, OAuthToken,
};

pub mod serve;
pub use serve::McpServe;
