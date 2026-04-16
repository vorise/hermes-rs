// Hermes MCP (Model Context Protocol) client

mod client;
pub mod config;
mod error;
mod protocol;
pub mod tool_wrapper;
mod transport;

pub use client::McpClient;
pub use config::{McpServerConfig, McpServerEntry, McpConfig};
pub use error::McpError;
pub use protocol::{
    McpTool, McpResource, McpPrompt, McpCapability, ToolResult, ResourceContent,
};
pub use tool_wrapper::{McpToolWrapper, McpToolRegistry};
pub use transport::{McpTransport, StdioTransport, SseTransport};
