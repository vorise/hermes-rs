use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use h_tools::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::client::McpClient;

/// Wraps an MCP-discovered tool to implement the h-tools `Tool` trait.
///
/// Each MCP server tool becomes a separate wrapper instance that forwards
/// execute() calls to the shared McpClient.
pub struct McpToolWrapper {
    mcp_client: Arc<Mutex<McpClient>>,
    server_name: String,
    tool_name: String,
    description: String,
    input_schema: Value,
}

impl McpToolWrapper {
    pub fn new(
        mcp_client: Arc<Mutex<McpClient>>,
        server_name: String,
        tool_name: String,
        description: String,
        input_schema: Value,
    ) -> Self {
        Self {
            mcp_client,
            server_name,
            tool_name,
            description,
            input_schema,
        }
    }

    /// Create wrappers for all tools from a connected MCP server.
    pub async fn create_all(
        mcp_client: Arc<Mutex<McpClient>>,
        server_name: &str,
    ) -> Vec<Self> {
        let client = mcp_client.lock().await;
        let tools = client.list_tools_for(server_name);
        drop(client);

        tools.into_iter().map(|tool| Self {
            mcp_client: Arc::clone(&mcp_client),
            server_name: server_name.to_string(),
            tool_name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
        }).collect()
    }
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn toolset(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext)
        -> Result<ToolResult>
    {
        let mut client = self.mcp_client.lock().await;
        match client.call_tool(&self.server_name, &self.tool_name, args).await {
            Ok(result) => {
                let content = result.content.iter().map(|c| {
                    match c {
                        crate::ResourceContent::Text { text, .. } => text.clone(),
                        crate::ResourceContent::Blob { data, mime_type } => {
                            format!("[{mime_type} image, base64: {data}]")
                        }
                    }
                }).collect::<Vec<_>>().join("\n");

                let truncated = if content.len() > 50_000 {
                    format!("{}... [truncated]", &content[..50_000])
                } else {
                    content
                };

                if result.is_error {
                    Ok(ToolResult::err(format!("[MCP {}/{} error] {}", self.server_name, self.tool_name, truncated)))
                } else {
                    Ok(ToolResult::ok(truncated))
                }
            }
            Err(e) => {
                Ok(ToolResult::err(format!("MCP tool execution failed: {e}")))
            }
        }
    }
}

/// Registry for MCP tools that can be dynamically added/removed from the tool system.
pub struct McpToolRegistry {
    tools: std::collections::HashMap<String, Arc<dyn Tool>>,
}

impl McpToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: std::collections::HashMap::new(),
        }
    }

    /// Register all tools from an MCP server. Returns the names of registered tools.
    pub async fn register_server(
        &mut self,
        mcp_client: Arc<Mutex<McpClient>>,
        server_name: &str,
    ) -> Vec<String> {
        // Remove existing tools from this server
        let prefix = format!("{server_name}/");
        let existing: Vec<_> = self.tools.keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for key in existing {
            self.tools.remove(&key);
        }

        // Add new tools
        let wrappers = McpToolWrapper::create_all(mcp_client, server_name).await;
        let names: Vec<String> = wrappers.iter().map(|w| w.tool_name.clone()).collect();
        for wrapper in wrappers {
            let key = format!("{}/{}", server_name, wrapper.tool_name);
            self.tools.insert(key, Arc::new(wrapper));
        }
        names
    }

    /// Remove all tools from an MCP server.
    pub fn remove_server(&mut self, server_name: &str) {
        let prefix = format!("{server_name}/");
        let keys: Vec<_> = self.tools.keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for key in keys {
            self.tools.remove(&key);
        }
    }

    /// Get all registered MCP tools.
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.values().cloned().collect()
    }

    /// Get tool count.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_registry_new_is_empty() {
        let registry = McpToolRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_mcp_tool_registry_tools_empty() {
        let registry = McpToolRegistry::new();
        let tools = registry.tools();
        assert!(tools.is_empty());
    }
}
