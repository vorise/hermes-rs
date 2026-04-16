use serde::{Deserialize, Serialize};

use h_core::ToolDefinition;

/// MCP capability flags reported during server initialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpCapability {
    pub tools_supported: bool,
    pub resources_supported: bool,
    pub prompts_supported: bool,
    pub logging_supported: bool,
}

impl McpCapability {
    pub fn from_server_info(info: &serde_json::Value) -> Self {
        let mut caps = McpCapability::default();
        if let Some(capabilities) = info.get("capabilities") {
            caps.tools_supported = capabilities.get("tools").is_some();
            caps.resources_supported = capabilities.get("resources").is_some();
            caps.prompts_supported = capabilities.get("prompts").is_some();
            caps.logging_supported = capabilities.get("logging").is_some();
        }
        caps
    }
}

/// A tool discovered on an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server_name: String,
}

impl McpTool {
    /// Convert to the core tool definition used by the query loop.
    pub fn to_definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            self.name.clone(),
            self.description.clone(),
            self.input_schema.clone(),
        )
    }
}

/// A resource discovered on an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
    pub server_name: String,
}

/// Content within a resource reading result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceContent {
    Text { text: String, mime_type: Option<String> },
    Blob { data: String, mime_type: String },
}

/// A prompt discovered on an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Vec<PromptArgument>,
    pub server_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

/// Result of calling an MCP tool.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: Vec<ResourceContent>,
    pub is_error: bool,
}

impl ToolResult {
    /// Concatenate all text content into a single string.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                ResourceContent::Text { text, .. } => Some(text.clone()),
                ResourceContent::Blob { data, .. } => Some(data.clone()),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Truncate content to a maximum character count.
    pub fn truncate(&mut self, max_chars: usize) {
        let mut remaining = max_chars;
        for content in &mut self.content {
            match content {
                ResourceContent::Text { text, .. } | ResourceContent::Blob { data: text, .. } => {
                    if text.len() > remaining {
                        text.truncate(remaining);
                        remaining = 0;
                    } else {
                        remaining -= text.len();
                    }
                }
            }
            if remaining == 0 {
                break;
            }
        }
    }
}
