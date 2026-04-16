//! MCP Client
//!
//! Model Context Protocol client for connecting to MCP servers
//! and discovering/calling tools dynamically.

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{debug, info};
use tokio::sync::Mutex;

use h_core::{ToolDefinition, FunctionDef, ToolResult};
use h_tools::ToolRegistry;

use crate::transport::{McpTransport, McpConnection};

/// MCP Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server name/identifier.
    pub name: String,

    /// Transport type.
    pub transport: McpTransportType,

    /// Server capabilities.
    #[serde(default)]
    pub capabilities: McpCapabilities,
}

/// MCP Transport type configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransportType {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Sse {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

/// MCP Server capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpCapabilities {
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub resources: bool,
    #[serde(default)]
    pub prompts: bool,
    #[serde(default)]
    pub streaming: bool,
}

/// MCP Server state.
pub struct McpServer {
    /// Server name.
    pub name: String,

    /// Connection.
    pub connection: Option<Arc<Mutex<McpConnection>>>,

    /// Server capabilities (from initialize response).
    pub capabilities: McpCapabilities,

    /// Server info.
    pub server_info: Option<McpServerInfo>,

    /// Registered tools from this server.
    pub tools: Vec<ToolDefinition>,
}

/// MCP Server info from initialize response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub version: String,
}

/// MCP Protocol version.
pub const MCP_VERSION: &str = "2024-11-05";

/// MCP Client managing multiple server connections.
pub struct McpClient {
    /// Connected servers.
    servers: Arc<Mutex<HashMap<String, McpServer>>>,

    /// Tool registry for dynamic discovery (optional).
    #[allow(dead_code)]
    tool_registry: Option<Arc<ToolRegistry>>,
}

impl McpClient {
    /// Create new MCP client.
    pub fn new() -> Self {
        Self {
            servers: Arc::new(Mutex::new(HashMap::new())),
            tool_registry: None,
        }
    }

    /// Create with tool registry for dynamic discovery.
    pub fn with_registry(tool_registry: Arc<ToolRegistry>) -> Self {
        Self {
            servers: Arc::new(Mutex::new(HashMap::new())),
            tool_registry: Some(tool_registry),
        }
    }

    /// Connect to an MCP server.
    pub async fn connect(&self, config: &McpServerConfig) -> Result<()> {
        debug!("Connecting to MCP server: {}", config.name);

        // Create transport
        let transport = create_transport(&config.transport)?;
        let connection = McpConnection::new(transport);

        // Initialize connection
        let init_result = initialize_connection(&connection, &config.name).await?;

        // Fetch tools from server
        let tools = list_tools_from_server(&connection).await?;
        let tool_count = tools.len();

        // Create server entry
        let server = McpServer {
            name: config.name.clone(),
            connection: Some(Arc::new(Mutex::new(connection))),
            capabilities: init_result.capabilities,
            server_info: Some(init_result.server_info),
            tools,
        };

        // Store server
        let mut servers = self.servers.lock().await;
        servers.insert(config.name.clone(), server);

        info!("Connected to MCP server: {} with {} tools", config.name, tool_count);
        Ok(())
    }

    /// Disconnect from an MCP server.
    pub async fn disconnect(&self, name: &str) -> Result<()> {
        debug!("Disconnecting from MCP server: {}", name);

        let mut servers = self.servers.lock().await;

        if let Some(server) = servers.remove(name) {
            if let Some(conn) = server.connection {
                let conn = conn.lock().await;
                conn.close().await?;
            }
            info!("Disconnected from MCP server: {}", name);
        }

        Ok(())
    }

    /// List all tools from all connected servers.
    pub async fn list_tools(&self) -> Vec<ToolDefinition> {
        let servers = self.servers.lock().await;
        servers.values()
            .flat_map(|s| s.tools.clone())
            .collect()
    }

    /// List tools from a specific server.
    pub async fn list_server_tools(&self, server_name: &str) -> Option<Vec<ToolDefinition>> {
        let servers = self.servers.lock().await;
        servers.get(server_name).map(|s| s.tools.clone())
    }

    /// Call a tool on an MCP server.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<ToolResult> {
        debug!("Calling MCP tool: {} on server {}", tool_name, server_name);

        let servers = self.servers.lock().await;

        let server = servers.get(server_name)
            .context("Server not connected")?;

        let conn = server.connection.as_ref()
            .context("Server has no connection")?;

        let conn = conn.lock().await;

        // Send tools/call request
        let request = McpRequest {
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": tool_name,
                "arguments": args,
            })),
        };

        let response = conn.send_request(request).await?;

        // Parse response
        if let Some(content) = response.result {
            let content_str = content.to_string();
            Ok(ToolResult::success(content_str))
        } else {
            // Check for error
            if let Some(error) = response.error {
                Ok(ToolResult::error(error.message))
            } else {
                Ok(ToolResult::success("Tool executed successfully"))
            }
        }
    }

    /// Get connected server names.
    pub async fn server_names(&self) -> Vec<String> {
        let servers = self.servers.lock().await;
        servers.keys().cloned().collect()
    }

    /// Check if a server is connected.
    pub async fn is_connected(&self, name: &str) -> bool {
        let servers = self.servers.lock().await;
        servers.contains_key(name)
    }

    /// Get server info.
    pub async fn get_server_info(&self, name: &str) -> Option<McpServerInfo> {
        let servers = self.servers.lock().await;
        servers.get(name).and_then(|s| s.server_info.clone())
    }

    /// List resources from a server.
    pub async fn list_resources(&self, server_name: &str) -> Result<Vec<McpResource>> {
        let servers = self.servers.lock().await;

        let server = servers.get(server_name)
            .context("Server not connected")?;

        if !server.capabilities.resources {
            bail!("Server does not support resources");
        }

        let conn = server.connection.as_ref()
            .context("Server has no connection")?;

        let conn = conn.lock().await;

        let request = McpRequest {
            method: "resources/list".to_string(),
            params: None,
        };

        let response = conn.send_request(request).await?;

        if let Some(result) = response.result {
            let resources: McpResourceListResult = serde_json::from_value(result)?;
            Ok(resources.resources)
        } else {
            Ok(Vec::new())
        }
    }

    /// Read a resource from a server.
    pub async fn read_resource(&self, server_name: &str, uri: &str) -> Result<String> {
        let servers = self.servers.lock().await;

        let server = servers.get(server_name)
            .context("Server not connected")?;

        let conn = server.connection.as_ref()
            .context("Server has no connection")?;

        let conn = conn.lock().await;

        let request = McpRequest {
            method: "resources/read".to_string(),
            params: Some(json!({ "uri": uri })),
        };

        let response = conn.send_request(request).await?;

        if let Some(result) = response.result {
            let read_result: McpResourceReadResult = serde_json::from_value(result)?;
            Ok(read_result.contents)
        } else {
            bail!("Resource not found or empty")
        }
    }

    /// List prompts from a server.
    pub async fn list_prompts(&self, server_name: &str) -> Result<Vec<McpPrompt>> {
        let servers = self.servers.lock().await;

        let server = servers.get(server_name)
            .context("Server not connected")?;

        if !server.capabilities.prompts {
            bail!("Server does not support prompts");
        }

        let conn = server.connection.as_ref()
            .context("Server has no connection")?;

        let conn = conn.lock().await;

        let request = McpRequest {
            method: "prompts/list".to_string(),
            params: None,
        };

        let response = conn.send_request(request).await?;

        if let Some(result) = response.result {
            let prompts: McpPromptListResult = serde_json::from_value(result)?;
            Ok(prompts.prompts)
        } else {
            Ok(Vec::new())
        }
    }

    /// Get a prompt from a server.
    pub async fn get_prompt(&self, server_name: &str, name: &str, args: Option<Value>) -> Result<String> {
        let servers = self.servers.lock().await;

        let server = servers.get(server_name)
            .context("Server not connected")?;

        let conn = server.connection.as_ref()
            .context("Server has no connection")?;

        let conn = conn.lock().await;

        let request = McpRequest {
            method: "prompts/get".to_string(),
            params: Some(json!({
                "name": name,
                "arguments": args,
            })),
        };

        let response = conn.send_request(request).await?;

        if let Some(result) = response.result {
            let prompt_result: McpPromptGetResult = serde_json::from_value(result)?;
            Ok(prompt_result.description)
        } else {
            bail!("Prompt not found")
        }
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Create transport from configuration.
fn create_transport(config: &McpTransportType) -> Result<McpTransport> {
    match config {
        McpTransportType::Stdio { command, args, env } => {
            Ok(McpTransport::stdio(command.clone(), args.clone(), env.clone()))
        }
        McpTransportType::Sse { url, headers } => {
            Ok(McpTransport::sse(url.clone(), headers.clone()))
        }
    }
}

/// Initialize connection with MCP handshake.
async fn initialize_connection(connection: &McpConnection, _name: &str) -> Result<McpInitializeResult> {
    debug!("Sending initialize request");

    let request = McpRequest {
        method: "initialize".to_string(),
        params: Some(json!({
            "protocolVersion": MCP_VERSION,
            "clientInfo": {
                "name": "hermes-mcp",
                "version": "1.0",
            },
            "capabilities": {
                "tools": true,
                "resources": true,
                "prompts": true,
            },
        })),
    };

    let response = connection.send_request(request).await?;

    if let Some(error) = &response.error {
        bail!("Initialize failed: {}", error.message);
    }

    let result = response.result.context("No initialize result")?;
    let init_result: McpInitializeResult = serde_json::from_value(result)?;

    // Send initialized notification
    connection.send_notification(McpNotification {
        method: "notifications/initialized".to_string(),
        params: None,
    }).await?;

    debug!("Connection initialized: {} v{}", init_result.server_info.name, init_result.server_info.version);
    Ok(init_result)
}

/// List tools from server.
async fn list_tools_from_server(connection: &McpConnection) -> Result<Vec<ToolDefinition>> {
    debug!("Requesting tools list");

    let request = McpRequest {
        method: "tools/list".to_string(),
        params: None,
    };

    let response = connection.send_request(request).await?;

    if let Some(result) = response.result {
        let tools_result: McpToolsListResult = serde_json::from_value(result)?;

        let tools: Vec<ToolDefinition> = tools_result.tools
            .into_iter()
            .map(|t| ToolDefinition {
                r#type: "function".to_string(),
                function: FunctionDef {
                    name: t.name,
                    description: t.description.unwrap_or_default(),
                    parameters: t.input_schema,
                },
            })
            .collect();

        debug!("Received {} tools", tools.len());
        Ok(tools)
    } else {
        Ok(Vec::new())
    }
}

// ============================================================================
// MCP Protocol Types
// ============================================================================

/// MCP Request.
#[derive(Debug, Clone, Serialize)]
pub struct McpRequest {
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// MCP Response.
#[derive(Debug, Clone, Deserialize)]
pub struct McpResponse {
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<McpError>,
}

/// MCP Error.
#[derive(Debug, Clone, Deserialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
}

/// MCP Notification.
#[derive(Debug, Clone, Serialize)]
pub struct McpNotification {
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// Initialize result.
#[derive(Debug, Clone, Deserialize)]
pub struct McpInitializeResult {
    #[serde(alias = "protocolVersion")]
    pub protocol_version: String,
    #[serde(alias = "serverInfo")]
    pub server_info: McpServerInfo,
    #[serde(default)]
    pub capabilities: McpCapabilities,
}

/// Tools list result.
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolsListResult {
    #[serde(default)]
    pub tools: Vec<McpTool>,
}

/// MCP Tool definition.
#[derive(Debug, Clone, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(alias = "inputSchema")]
    pub input_schema: Value,
}

/// MCP Resource.
#[derive(Debug, Clone, Deserialize)]
pub struct McpResource {
    pub uri: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Resources list result.
#[derive(Debug, Clone, Deserialize)]
pub struct McpResourceListResult {
    #[serde(default)]
    pub resources: Vec<McpResource>,
}

/// Resource read result.
#[derive(Debug, Clone, Deserialize)]
pub struct McpResourceReadResult {
    #[serde(default)]
    pub contents: String,
}

/// MCP Prompt.
#[derive(Debug, Clone, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
}

/// Prompt argument.
#[derive(Debug, Clone, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// Prompts list result.
#[derive(Debug, Clone, Deserialize)]
pub struct McpPromptListResult {
    #[serde(default)]
    pub prompts: Vec<McpPrompt>,
}

/// Prompt get result.
#[derive(Debug, Clone, Deserialize)]
pub struct McpPromptGetResult {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub messages: Vec<McpPromptMessage>,
}

/// Prompt message.
#[derive(Debug, Clone, Deserialize)]
pub struct McpPromptMessage {
    pub role: String,
    pub content: McpPromptContent,
}

/// Prompt content.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpPromptContent {
    Text { text: String },
    Image { data: String, #[serde(alias = "mimeType")] mime_type: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_client_new() {
        let client = McpClient::new();
        assert!(client.tool_registry.is_none());
    }

    #[test]
    fn test_mcp_server_config_stdio() {
        let config = McpServerConfig {
            name: "test".to_string(),
            transport: McpTransportType::Stdio {
                command: "test-server".to_string(),
                args: vec!["--port".to_string(), "8080".to_string()],
                env: HashMap::new(),
            },
            capabilities: McpCapabilities::default(),
        };

        assert_eq!(config.name, "test");
    }

    #[test]
    fn test_mcp_server_config_sse() {
        let config = McpServerConfig {
            name: "test".to_string(),
            transport: McpTransportType::Sse {
                url: "http://localhost:8080/sse".to_string(),
                headers: HashMap::new(),
            },
            capabilities: McpCapabilities::default(),
        };

        assert_eq!(config.name, "test");
    }

    #[test]
    fn test_mcp_request_serialize() {
        let request = McpRequest {
            method: "tools/list".to_string(),
            params: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("tools/list"));
    }

    #[test]
    fn test_mcp_response_deserialize() {
        let json = r#"{"result": {"tools": []}}"#;
        let response: McpResponse = serde_json::from_str(json).unwrap();
        assert!(response.result.is_some());
    }

    #[test]
    fn test_mcp_tool_definition() {
        let tool = McpTool {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            input_schema: json!({"type": "object"}),
        };

        assert_eq!(tool.name, "read_file");
    }

    #[test]
    fn test_mcp_capabilities_default() {
        let caps = McpCapabilities::default();
        assert!(!caps.tools);
        assert!(!caps.resources);
        assert!(!caps.prompts);
    }

    #[test]
    fn test_create_transport_stdio() {
        let config = McpTransportType::Stdio {
            command: "test".to_string(),
            args: vec!["arg1".to_string()],
            env: HashMap::new(),
        };

        let transport = create_transport(&config).unwrap();
        assert_eq!(transport.name(), "stdio");
    }

    #[test]
    fn test_create_transport_sse() {
        let config = McpTransportType::Sse {
            url: "http://test".to_string(),
            headers: HashMap::new(),
        };

        let transport = create_transport(&config).unwrap();
        assert_eq!(transport.name(), "sse");
    }

    #[test]
    fn test_mcp_initialize_result_deserialize() {
        let json = r#"{"protocolVersion": "2024-11-05", "serverInfo": {"name": "test", "version": "1.0"}}"#;
        let result: McpInitializeResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.protocol_version, "2024-11-05");
        assert_eq!(result.server_info.name, "test");
    }
}