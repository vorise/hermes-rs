use serde::{Deserialize, Serialize};

/// Task type for the auxiliary client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuxiliaryTask {
    /// Compress/summarize a conversation segment.
    Compression,
    /// Generate a session title.
    TitleGeneration,
    /// Analyze an image (vision).
    Vision,
    /// Summarize a full session.
    SessionSummary,
    /// Consolidate memories.
    MemoryConsolidation,
    /// Generic text processing.
    TextProcessing,
}

impl std::fmt::Display for AuxiliaryTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuxiliaryTask::Compression => write!(f, "compression"),
            AuxiliaryTask::TitleGeneration => write!(f, "title_generation"),
            AuxiliaryTask::Vision => write!(f, "vision"),
            AuxiliaryTask::SessionSummary => write!(f, "session_summary"),
            AuxiliaryTask::MemoryConsolidation => write!(f, "memory_consolidation"),
            AuxiliaryTask::TextProcessing => write!(f, "text_processing"),
        }
    }
}

/// Configuration for the auxiliary LLM client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuxiliaryConfig {
    /// Model to use (defaults to a fast/cheap model).
    pub model: String,
    /// Provider (e.g., "anthropic", "openai").
    pub provider: String,
    /// Base URL override.
    pub base_url: String,
    /// API key override.
    pub api_key: String,
    /// Max tokens for auxiliary requests.
    pub max_tokens: Option<u32>,
    /// Temperature for auxiliary requests.
    pub temperature: Option<f64>,
}

impl Default for AuxiliaryConfig {
    fn default() -> Self {
        Self {
            model: "claude-haiku-4-20250414".to_string(),
            provider: "anthropic".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            max_tokens: Some(4096),
            temperature: Some(0.0),
        }
    }
}

impl AuxiliaryConfig {
    /// Resolve API key from config or environment.
    pub fn resolve_api_key(&self) -> Option<String> {
        if !self.api_key.is_empty() {
            return Some(self.api_key.clone());
        }
        // Fallback to provider's env var
        let env_var = match self.provider.to_lowercase().as_str() {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" | "openrouter" => "OPENAI_API_KEY",
            "nous" => "NOUS_API_KEY",
            _ => "API_KEY",
        };
        std::env::var(env_var).ok()
    }

    /// Resolve base URL from config or provider defaults.
    pub fn resolve_base_url(&self) -> String {
        if !self.base_url.is_empty() {
            return self.base_url.clone();
        }
        match self.provider.to_lowercase().as_str() {
            "anthropic" => "https://api.anthropic.com",
            "openai" => "https://api.openai.com/v1",
            "openrouter" => "https://openrouter.ai/api/v1",
            "nous" => "https://api.nousresearch.com/v1",
            _ => "https://api.openai.com/v1",
        }
        .to_string()
    }
}

/// System prompt templates for auxiliary tasks.
mod prompts {
    pub const COMPRESSION: &str =
        "You are a conversation summarizer. Summarize the following conversation segment \
         concisely, preserving key decisions, tool results, and context. Output only the summary.";

    pub const TITLE: &str =
        "You are a title generator. Generate a short, descriptive title (under 50 characters) \
         for the following conversation. Output only the title.";

    pub const VISION: &str =
        "You are a vision assistant. Analyze the following image and describe what you see \
         in detail.";

    pub const SESSION_SUMMARY: &str =
        "You are a session summarizer. Provide a comprehensive summary of the following \
         session, including: goals, key decisions, tools used, outcomes, and any remaining \
         action items.";

    pub const MEMORY_CONSOLIDATION: &str =
        "You are a memory curator. Review the following conversation and extract key facts, \
         preferences, and insights that should be stored as long-term memory.";
}

/// Build a system prompt for the given auxiliary task.
pub fn build_auxiliary_system_prompt(task: AuxiliaryTask) -> String {
    match task {
        AuxiliaryTask::Compression => prompts::COMPRESSION.to_string(),
        AuxiliaryTask::TitleGeneration => prompts::TITLE.to_string(),
        AuxiliaryTask::Vision => prompts::VISION.to_string(),
        AuxiliaryTask::SessionSummary => prompts::SESSION_SUMMARY.to_string(),
        AuxiliaryTask::MemoryConsolidation => prompts::MEMORY_CONSOLIDATION.to_string(),
        AuxiliaryTask::TextProcessing => String::from("You are a helpful text processing assistant."),
    }
}

/// Format a conversation segment for compression.
pub fn format_compression_input(messages: &str) -> String {
    format!("Conversation segment to summarize:\n\n{messages}")
}

/// Format a session for summarization.
pub fn format_session_summary_input(title: &str, messages: &str) -> String {
    format!("Session: {title}\n\nConversation:\n{messages}")
}

/// Format a vision request with image URL.
pub fn format_vision_input(description: &str, image_url: &str) -> String {
    format!("{description}\n\nImage: {image_url}")
}

/// Format a memory consolidation request.
pub fn format_memory_consolidation_input(conversation: &str) -> String {
    format!("Extract memories from this conversation:\n\n{conversation}")
}

/// Result from an auxiliary LLM call.
#[derive(Debug, Clone)]
pub struct AuxiliaryResult {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub task: AuxiliaryTask,
}

impl AuxiliaryResult {
    /// Total tokens used.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// Resolve the best auxiliary model for a given task based on available providers.
pub fn resolve_auxiliary_model(task: AuxiliaryTask, config: &AuxiliaryConfig) -> String {
    if !config.model.is_empty() {
        return config.model.clone();
    }

    // Task-specific model selection
    match task {
        AuxiliaryTask::Vision => {
            // Vision needs a capable model
            "claude-sonnet-4-6-20250514".to_string()
        }
        AuxiliaryTask::Compression | AuxiliaryTask::TitleGeneration => {
            // Fast/cheap models for simple tasks
            "claude-haiku-4-20250414".to_string()
        }
        AuxiliaryTask::SessionSummary | AuxiliaryTask::MemoryConsolidation => {
            // Moderate capability
            "claude-sonnet-4-6-20250514".to_string()
        }
        AuxiliaryTask::TextProcessing => {
            "claude-haiku-4-20250414".to_string()
        }
    }
}

/// Estimate whether a model supports vision.
pub fn model_supports_vision(model_id: &str) -> bool {
    let m = model_id.to_lowercase();
    if m.contains("haiku") {
        return false;
    }
    m.contains("claude") || m.contains("gpt-4") || m.contains("gemini")
}

/// Auxiliary LLM client for compression, summarization, title generation, etc.
///
/// Makes direct HTTP calls to LLM APIs using a fast/cheap model.
pub struct AuxiliaryClient {
    client: reqwest::Client,
    config: AuxiliaryConfig,
}

impl AuxiliaryClient {
    /// Create a new auxiliary client with default config.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            config: AuxiliaryConfig::default(),
        }
    }

    /// Create a new auxiliary client with a specific config.
    pub fn with_config(config: AuxiliaryConfig) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            config,
        }
    }

    /// Execute an auxiliary task and return the result.
    pub async fn execute(&self, task: AuxiliaryTask, user_message: &str) -> anyhow::Result<AuxiliaryResult> {
        let system_prompt = build_auxiliary_system_prompt(task);
        let model = resolve_auxiliary_model(task, &self.config);

        let api_key = self.config.resolve_api_key()
            .ok_or_else(|| anyhow::anyhow!("No API key available for auxiliary task: {}. Set {} or configure auxiliary API key", task, self.config.provider.to_uppercase() + "_API_KEY"))?;

        let base_url = self.config.resolve_base_url();

        let max_tokens = self.config.max_tokens.unwrap_or(4096);
        let temperature = self.config.temperature.unwrap_or(0.0);

        match self.config.provider.to_lowercase().as_str() {
            "anthropic" => {
                self.call_anthropic(&api_key, &base_url, &system_prompt, user_message, &model, max_tokens, temperature, task).await
            }
            "openai" | "openrouter" | "nous" | "mistral" | "moonshot" | "kimi" | "minimax" | "huggingface" | "ollama" => {
                self.call_openai_compat(&api_key, &base_url, &system_prompt, user_message, &model, max_tokens, temperature, task).await
            }
            _ => {
                anyhow::bail!("Unsupported auxiliary provider: {}", self.config.provider)
            }
        }
    }

    async fn call_anthropic(
        &self,
        api_key: &str,
        base_url: &str,
        system_prompt: &str,
        user_message: &str,
        model: &str,
        max_tokens: u32,
        temperature: f64,
        task: AuxiliaryTask,
    ) -> anyhow::Result<AuxiliaryResult> {
        let resp = self.client
            .post(format!("{base_url}/v1/messages"))
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "temperature": temperature,
                "system": system_prompt,
                "messages": [
                    {"role": "user", "content": user_message}
                ]
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error ({status}): {body}");
        }

        let body: serde_json::Value = resp.json().await?;
        let text = body["content"][0]["text"].as_str().unwrap_or("").to_string();
        let input_tokens = body["usage"]["input_tokens"].as_u64().unwrap_or(0);
        let output_tokens = body["usage"]["output_tokens"].as_u64().unwrap_or(0);

        Ok(AuxiliaryResult { text, input_tokens, output_tokens, task })
    }

    async fn call_openai_compat(
        &self,
        api_key: &str,
        base_url: &str,
        system_prompt: &str,
        user_message: &str,
        model: &str,
        max_tokens: u32,
        temperature: f64,
        task: AuxiliaryTask,
    ) -> anyhow::Result<AuxiliaryResult> {
        let resp = self.client
            .post(format!("{base_url}/chat/completions"))
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "temperature": temperature,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_message}
                ]
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI-compatible API error ({status}): {body}");
        }

        let body: serde_json::Value = resp.json().await?;
        let text = body["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();
        let usage = &body["usage"];
        let input_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
        let output_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);

        Ok(AuxiliaryResult { text, input_tokens, output_tokens, task })
    }
}

impl Default for AuxiliaryClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auxiliary_task_display() {
        assert_eq!(AuxiliaryTask::Compression.to_string(), "compression");
        assert_eq!(AuxiliaryTask::TitleGeneration.to_string(), "title_generation");
        assert_eq!(AuxiliaryTask::Vision.to_string(), "vision");
        assert_eq!(AuxiliaryTask::SessionSummary.to_string(), "session_summary");
    }

    #[test]
    fn test_auxiliary_config_default() {
        let config = AuxiliaryConfig::default();
        assert_eq!(config.model, "claude-haiku-4-20250414");
        assert_eq!(config.provider, "anthropic");
        assert_eq!(config.max_tokens, Some(4096));
        assert_eq!(config.temperature, Some(0.0));
    }

    #[test]
    fn test_auxiliary_config_resolve_api_key() {
        // With explicit key
        let config = AuxiliaryConfig {
            api_key: "sk-test".to_string(),
            ..Default::default()
        };
        assert_eq!(config.resolve_api_key(), Some("sk-test".to_string()));
    }

    #[test]
    fn test_auxiliary_config_resolve_base_url() {
        let config = AuxiliaryConfig {
            provider: "openai".to_string(),
            ..Default::default()
        };
        assert_eq!(config.resolve_base_url(), "https://api.openai.com/v1");

        let config2 = AuxiliaryConfig {
            base_url: "https://custom.api.com".to_string(),
            ..Default::default()
        };
        assert_eq!(config2.resolve_base_url(), "https://custom.api.com");
    }

    #[test]
    fn test_build_auxiliary_system_prompt() {
        let p = build_auxiliary_system_prompt(AuxiliaryTask::Compression);
        assert!(p.contains("summarizer"));
        assert!(p.contains("concisely"));

        let p2 = build_auxiliary_system_prompt(AuxiliaryTask::TitleGeneration);
        assert!(p2.contains("title"));
        assert!(p2.contains("50 characters"));
    }

    #[test]
    fn test_format_compression_input() {
        let result = format_compression_input("user: hello\nassistant: hi");
        assert!(result.contains("Conversation segment to summarize"));
        assert!(result.contains("user: hello"));
    }

    #[test]
    fn test_format_session_summary_input() {
        let result = format_session_summary_input("My Session", "user: test");
        assert!(result.contains("Session: My Session"));
        assert!(result.contains("user: test"));
    }

    #[test]
    fn test_format_vision_input() {
        let result = format_vision_input("Describe this", "https://example.com/img.png");
        assert!(result.contains("Describe this"));
        assert!(result.contains("https://example.com/img.png"));
    }

    #[test]
    fn test_auxiliary_result_total() {
        let result = AuxiliaryResult {
            text: "test".to_string(),
            input_tokens: 100,
            output_tokens: 50,
            task: AuxiliaryTask::Compression,
        };
        assert_eq!(result.total_tokens(), 150);
    }

    #[test]
    fn test_resolve_auxiliary_model_custom() {
        let config = AuxiliaryConfig {
            model: "custom-model".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_auxiliary_model(AuxiliaryTask::Vision, &config),
            "custom-model"
        );
    }

    #[test]
    fn test_resolve_auxiliary_model_defaults() {
        // Empty config to trigger task-specific defaults
        let config = AuxiliaryConfig {
            model: String::new(),
            ..Default::default()
        };
        let vision_model = resolve_auxiliary_model(AuxiliaryTask::Vision, &config);
        assert!(vision_model.contains("sonnet"));

        let compression_model = resolve_auxiliary_model(AuxiliaryTask::Compression, &config);
        assert!(compression_model.contains("haiku"));
    }

    #[test]
    fn test_model_supports_vision() {
        assert!(model_supports_vision("claude-sonnet-4-6"));
        assert!(!model_supports_vision("claude-haiku-4"));
        assert!(model_supports_vision("gpt-4o"));
        assert!(model_supports_vision("gpt-4o-mini"));
        assert!(model_supports_vision("gemini-pro"));
    }

    #[tokio::test]
    async fn test_auxiliary_client_no_api_key_fails() {
        // Clear any potential API key env vars
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
        unsafe { std::env::remove_var("OPENAI_API_KEY") };

        let client = AuxiliaryClient::new();
        let result = client.execute(AuxiliaryTask::TitleGeneration, "test").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("No API key"));
    }

    #[test]
    fn test_auxiliary_client_construction() {
        let client = AuxiliaryClient::new();
        // Verify default config is set
        assert_eq!(client.config.provider, "anthropic");

        let config = AuxiliaryConfig {
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            ..Default::default()
        };
        let client2 = AuxiliaryClient::with_config(config);
        assert_eq!(client2.config.api_key, "test-key");
        assert_eq!(client2.config.model, "test-model");
    }

    #[test]
    fn test_auxiliary_client_default_trait() {
        let client: AuxiliaryClient = Default::default();
        assert_eq!(client.config.provider, "anthropic");
    }
}
