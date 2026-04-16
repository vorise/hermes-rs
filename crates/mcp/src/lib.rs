//! Hermes MCP Crate
//!
//! Model Context Protocol client implementation.

pub mod client;
pub mod transport;
pub mod discovery;

pub use client::{McpClient, McpServerConfig, McpTransportType, McpServerInfo, McpCapabilities};
pub use transport::{McpTransport, McpConnection, McpTransportTrait};
pub use discovery::{McpDiscovery, McpDynamicToolWrapper, get_tool_definitions, create_tool_wrappers, prefix_tool_name};