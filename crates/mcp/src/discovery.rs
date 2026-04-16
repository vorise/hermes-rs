//! MCP Discovery
//!
//! Dynamic tool discovery and registration for MCP servers.

use std::sync::Arc;
use anyhow::{Result, Context};
use tracing::info;
use async_trait::async_trait;
use serde_json::Value;

use h_core::{ToolDefinition, ToolResult};
use h_tools::{Tool, ToolContext};

use crate::client::McpClient;

/// Get tool definitions from MCP server for registration.
///
/// Returns tool definitions that can be registered elsewhere.
pub async fn get_tool_definitions(
    client: Arc<McpClient>,
    server_name: &str,
) -> Result<Vec<ToolDefinition>> {
    let tools = client.list_server_tools(server_name).await
        .context("Failed to list tools from MCP server")?;

    info!("Found {} tools from MCP server {}", tools.len(), server_name);
    Ok(tools)
}

/// Dynamic tool wrapper implementation.
pub struct McpDynamicToolWrapper {
    definition: ToolDefinition,
    client: Arc<McpClient>,
    server_name: String,
}

impl McpDynamicToolWrapper {
    /// Create new wrapper.
    pub fn new(
        definition: ToolDefinition,
        client: Arc<McpClient>,
        server_name: String,
    ) -> Self {
        Self {
            definition,
            client,
            server_name,
        }
    }
}

#[async_trait]
impl Tool for McpDynamicToolWrapper {
    fn name(&self) -> &str {
        &self.definition.function.name
    }

    fn toolset(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        &self.definition.function.description
    }

    fn schema(&self) -> Value {
        self.definition.function.parameters.clone()
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult> {
        // Call the MCP tool via the client
        let tool_name = &self.definition.function.name;
        let result = self.client.call_tool(&self.server_name, tool_name, args).await?;

        Ok(result)
    }
}

/// Create tool wrappers from definitions.
pub fn create_tool_wrappers(
    definitions: Vec<ToolDefinition>,
    client: Arc<McpClient>,
    server_name: &str,
) -> Vec<McpDynamicToolWrapper> {
    definitions
        .into_iter()
        .map(|def| McpDynamicToolWrapper::new(def, client.clone(), server_name.to_string()))
        .collect()
}

/// Discovery manager for MCP tools.
///
/// Manages the lifecycle of dynamically discovered MCP tools.
pub struct McpDiscovery {
    /// MCP client.
    client: Arc<McpClient>,
}

impl McpDiscovery {
    /// Create new discovery manager.
    pub fn new(client: Arc<McpClient>) -> Self {
        Self { client }
    }

    /// Get tool definitions from all connected servers.
    pub async fn get_all_definitions(&self) -> Result<Vec<(String, Vec<ToolDefinition>)>> {
        let servers = self.client.server_names().await;

        let mut result = Vec::new();
        for server in servers {
            let tools = get_tool_definitions(self.client.clone(), &server).await?;
            result.push((server, tools));
        }

        Ok(result)
    }

    /// Get tool definitions from a specific server.
    pub async fn get_server_definitions(&self, server_name: &str) -> Result<Vec<ToolDefinition>> {
        get_tool_definitions(self.client.clone(), server_name).await
    }
}

/// Generate prefixed name for MCP tool to avoid conflicts.
pub fn prefix_tool_name(server_name: &str, tool_name: &str) -> String {
    format!("mcp_{}_{}", server_name, tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_tool_name() {
        let prefixed = prefix_tool_name("filesystem", "read_file");
        assert_eq!(prefixed, "mcp_filesystem_read_file");
    }

    #[test]
    fn test_mcp_dynamic_tool_wrapper_new() {
        let client = Arc::new(McpClient::new());
        let definition = ToolDefinition::new("test", "Test tool", serde_json::json!({}));

        let wrapper = McpDynamicToolWrapper::new(definition, client, "test_server".to_string());
        assert_eq!(wrapper.name(), "test");
        assert_eq!(wrapper.server_name, "test_server");
    }

    #[tokio::test]
    async fn test_mcp_discovery_new() {
        let client = Arc::new(McpClient::new());

        let discovery = McpDiscovery::new(client);
        assert!(discovery.client.server_names().await.is_empty());
    }

    #[tokio::test]
    async fn test_get_tool_definitions_no_server() {
        let client = Arc::new(McpClient::new());

        // No servers connected, should return None from list_server_tools
        let tools = client.list_server_tools("test_server").await;
        assert!(tools.is_none());  // Server not connected
    }

    #[test]
    fn test_create_tool_wrappers() {
        let client = Arc::new(McpClient::new());
        let definitions = vec![
            ToolDefinition::new("tool1", "Tool 1", serde_json::json!({})),
            ToolDefinition::new("tool2", "Tool 2", serde_json::json!({})),
        ];

        let wrappers = create_tool_wrappers(definitions, client, "test_server");
        assert_eq!(wrappers.len(), 2);
    }
}