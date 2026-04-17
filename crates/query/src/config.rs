use h_core::{Message, ModelRef, ToolDefinition};

use crate::budget::IterationBudget;

/// Configuration for a single query loop execution.
#[derive(Clone)]
pub struct QueryConfig {
    pub model: ModelRef,
    pub api_mode: h_api::provider::ApiMode,
    pub tools: Vec<ToolDefinition>,
    pub system_prompt: String,
    pub messages: Vec<Message>,
    pub max_iterations: u32,
    pub max_tokens: Option<u32>,
    pub reasoning_config: Option<ReasoningConfig>,
    pub request_overrides: Option<serde_json::Value>,
}

/// Reasoning/thinking configuration for the model.
#[derive(Debug, Clone)]
pub struct ReasoningConfig {
    pub enabled: bool,
    pub budget_tokens: Option<u32>,
    pub effort: Option<String>, // "low", "medium", "high"
}

/// Result of a query loop execution.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub final_text: String,
    pub messages: Vec<Message>,
    pub iterations_used: u32,
    pub tool_calls_made: u32,
    pub cost: h_core::CostTracker,
    pub stopped_reason: StopReason,
}

/// Why the query loop stopped.
#[derive(Debug, Clone)]
pub enum StopReason {
    Completed,
    BudgetExhausted,
    Interrupted,
    Error(String),
    MaxIterationsReached,
}

impl QueryConfig {
    pub fn new(model: ModelRef) -> Self {
        Self {
            model,
            api_mode: h_api::provider::ApiMode::ChatCompletions,
            tools: vec![],
            system_prompt: String::new(),
            messages: vec![],
            max_iterations: 90,
            max_tokens: None,
            reasoning_config: None,
            request_overrides: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = prompt;
        self
    }

    pub fn with_message(mut self, msg: Message) -> Self {
        self.messages.push(msg);
        self
    }

    pub fn with_max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = n;
        self
    }

    pub fn budget(&self) -> IterationBudget {
        IterationBudget::new(self.max_iterations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use h_core::{ModelId, ProviderId};

    #[test]
    fn test_query_config_builder() {
        let model = ModelRef::new(ProviderId::new("anthropic"), ModelId::new("claude-sonnet-4-6"));
        let config = QueryConfig::new(model.clone())
            .with_max_iterations(50)
            .with_system_prompt("You are helpful".to_string())
            .with_message(Message::user("Hello"));

        assert_eq!(config.max_iterations, 50);
        assert_eq!(config.system_prompt, "You are helpful");
        assert_eq!(config.messages.len(), 1);
        assert_eq!(config.model, model);
    }

    #[test]
    fn test_default_iterations() {
        let model = ModelRef::new(ProviderId::new("openai"), ModelId::new("gpt-4o"));
        let config = QueryConfig::new(model);
        assert_eq!(config.max_iterations, 90);
    }
}
