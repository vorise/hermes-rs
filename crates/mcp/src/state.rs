use std::sync::Arc;

use anyhow::Result;
use h_tools::Tool;
use tokio::sync::Mutex;

use crate::client::McpClient;
use crate::config::McpConfig;
use crate::tool_wrapper::McpToolRegistry;

/// Shared state for MCP server connections and tool discovery.
///
/// This struct is wrapped in `Arc` and shared between the command
/// handlers (for connect/disconnect) and the dispatch layer (for
/// wiring MCP tools into the tool registry).
pub struct McpState {
    /// Connected MCP client.
    pub client: Arc<Mutex<McpClient>>,
    /// Registry of MCP-discovered tools.
    pub tool_registry: Arc<Mutex<McpToolRegistry>>,
}

impl McpState {
    /// Create a new McpState with no servers connected.
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(McpClient::new())),
            tool_registry: Arc::new(Mutex::new(McpToolRegistry::new())),
        }
    }

    /// Connect to an MCP server by name (loads config entry automatically).
    ///
    /// Returns a message describing the connection result.
    pub async fn connect_server(&self, name: &str) -> Result<String> {
        let config = McpConfig::from_default()?;
        let entry = config
            .get_server(name)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{name}' not found in config"))?;

        if !entry.enabled {
            anyhow::bail!("MCP server '{name}' is disabled. Enable it with `/mcp enable {name}`.");
        }

        let mut client = self.client.lock().await;
        client.connect(name, entry).await?;
        drop(client);

        // Register tools from the newly connected server
        let tools = self.register_server_tools(name).await;

        let tool_list = if tools.is_empty() {
            "No tools discovered.".to_string()
        } else {
            format!(
                "Discovered tools: {}",
                tools.join(", ")
            )
        };

        Ok(format!("Connected to MCP server '{name}'. {tool_list}"))
    }

    /// Disconnect from an MCP server by name.
    pub async fn disconnect_server(&self, name: &str) -> Result<String> {
        let mut client = self.client.lock().await;
        client.disconnect(name).await?;
        drop(client);

        // Remove tools from the registry
        {
            let mut registry = self.tool_registry.lock().await;
            registry.remove_server(name);
        }

        Ok(format!("Disconnected from MCP server '{name}'."))
    }

    /// Register tools from a connected MCP server into the tool registry.
    /// Returns the names of registered tools.
    pub async fn register_server_tools(&self, name: &str) -> Vec<String> {
        let client = Arc::clone(&self.client);
        let mut registry = self.tool_registry.lock().await;
        registry.register_server(client, name).await
    }

    /// Get all MCP tools as `Arc<dyn Tool>` for the query loop.
    pub async fn tools(&self) -> Vec<Arc<dyn Tool>> {
        let registry = self.tool_registry.lock().await;
        registry.tools()
    }

    /// Check if any servers are connected.
    pub async fn is_empty(&self) -> bool {
        let registry = self.tool_registry.lock().await;
        registry.is_empty()
    }

    /// Get connected server names.
    pub async fn connected_servers(&self) -> Vec<String> {
        let client = self.client.lock().await;
        client.server_names().into_iter().map(String::from).collect()
    }

    /// Get total tool count across all connected servers.
    pub async fn tool_count(&self) -> usize {
        let registry = self.tool_registry.lock().await;
        registry.len()
    }
}

impl Default for McpState {
    fn default() -> Self {
        Self::new()
    }
}
