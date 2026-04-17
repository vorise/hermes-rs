use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use h_core::{HermesConfig, ToolDefinition};
use serde_json::Value;

/// Callback for interactive user clarification.
/// Takes a question and optional list of choices, returns the user's answer.
pub type ClarifyCallback = Arc<dyn Fn(&str, &[&str]) -> String + Send + Sync>;

/// Context provided to tool execution.
#[derive(Clone)]
pub struct ToolContext {
    pub session_id: String,
    pub task_id: String,
    pub config: Arc<HermesConfig>,
    pub working_dir: PathBuf,
    /// Optional callback for interactive clarification requests.
    pub clarify: Option<ClarifyCallback>,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            task_id: String::new(),
            config: Arc::new(HermesConfig::default()),
            working_dir: std::env::current_dir().unwrap_or_default(),
            clarify: None,
        }
    }
}

/// Result of a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    pub persisted_path: Option<PathBuf>,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            persisted_path: None,
        }
    }

    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            persisted_path: None,
        }
    }
}

/// The core tool trait. Each tool implementation provides this.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn toolset(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;

    /// Whether this tool requires specific env vars to be available.
    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    /// Max result size in characters. None for unlimited.
    fn max_result_size_chars(&self) -> Option<usize> {
        None
    }

    /// Execute the tool with the given arguments.
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult>;

    /// Convert this tool to an OpenAI-compatible tool definition.
    fn to_definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            self.name().to_string(),
            self.description().to_string(),
            self.schema(),
        )
    }
}
