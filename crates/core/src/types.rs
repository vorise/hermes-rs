use serde::{Deserialize, Serialize};

// ── Provider / Model ──────────────────────────────────────────────

/// Provider identifier (e.g., "anthropic", "openrouter", "openai").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn new<S: Into<String>>(s: S) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Model identifier (e.g., "claude-sonnet-4-6").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelId(pub String);

impl ModelId {
    pub fn new<S: Into<String>>(s: S) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ModelId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ModelId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Combined provider + model reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: ProviderId,
    pub model: ModelId,
}

impl ModelRef {
    pub fn new(provider: ProviderId, model: ModelId) -> Self {
        Self { provider, model }
    }
    pub fn display(&self) -> String {
        format!("{}/{}", self.provider.0, self.model.0)
    }
}

impl std::fmt::Display for ModelRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.provider.0, self.model.0)
    }
}

// ── Messages ──────────────────────────────────────────────────────

/// Message role in the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::System => write!(f, "system"),
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
            Role::Tool => write!(f, "tool"),
        }
    }
}

/// A single content part within a multi-part message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Message content — either plain text or multi-part.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Multi(Vec<ContentPart>),
}

impl Content {
    pub fn text(s: impl Into<String>) -> Self {
        Content::Text(s.into())
    }
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Content::Text(t) => Some(t),
            Content::Multi(parts) => parts.iter().find_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            }),
        }
    }
}

/// A tool call request from the assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(flatten)]
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON string of arguments.
    pub arguments: String,
}

impl ToolCallFunction {
    pub fn parse_args(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_str(&self.arguments)
    }
}

/// Tool definition in OpenAI-compatible format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub type_: String,
    pub function: FunctionDef,
}

impl ToolDefinition {
    pub fn function(name: String, description: String, parameters: serde_json::Value) -> Self {
        Self {
            type_: "function".to_string(),
            function: FunctionDef {
                name,
                description,
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    /// JSON Schema for the parameters.
    pub parameters: serde_json::Value,
}

/// A single message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(Content::Text(content.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(Content::Text(content.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(Content::Text(content.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
        }
    }

    pub fn assistant_tool_calls(
        tool_calls: Vec<ToolCall>,
        content: Option<String>,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: content.map(Content::Text),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
            reasoning: None,
        }
    }

    pub fn tool_result(tool_call_id: String, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(Content::Text(content.into())),
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
            name: None,
            reasoning: None,
        }
    }
}

/// Cost tracking for a session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostTracker {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub estimated_cost_usd: f64,
    pub api_call_count: u64,
}

impl CostTracker {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_read_tokens
            + self.cache_write_tokens
            + self.reasoning_tokens
    }

    pub fn add(&mut self, other: &CostTracker) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.estimated_cost_usd += other.estimated_cost_usd;
        self.api_call_count += other.api_call_count;
    }
}
