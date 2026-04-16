use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, ConfigChange, SlashCommand};

/// /model — Switch the current LLM provider and model.
pub struct ModelCommand;

#[async_trait]
impl SlashCommand for ModelCommand {
    fn name(&self) -> &str {
        "model"
    }

    fn description(&self) -> &str {
        "Switch the current LLM provider and model"
    }

    fn category(&self) -> &str {
        "model"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        if args.is_empty() {
            // Show current model
            return Ok(CommandResult::Message(format!(
                "Current model: {}/{}",
                ctx.model.provider, ctx.model.model
            )));
        }

        // Parse "provider/model" or just "provider"
        let parts: Vec<&str> = args.splitn(2, '/').collect();
        let (provider, model) = match parts.as_slice() {
            [p, m] => (p.to_string(), m.to_string()),
            [p] => {
                // Provider-only: use default model for that provider
                let default = default_model_for_provider(p);
                (p.to_string(), default.to_string())
            }
            _ => {
                return Ok(CommandResult::Message(
                    "Usage: /model <provider/model>".to_string(),
                ));
            }
        };

        Ok(CommandResult::ConfigChange(ConfigChange::Model {
            provider,
            model,
        }))
    }
}

/// Return a default model ID for a known provider shortcut.
fn default_model_for_provider(provider: &str) -> &str {
    match provider {
        "anthropic" => "claude-sonnet-4-6",
        "openai" => "gpt-4o",
        "openrouter" => "anthropic/claude-sonnet-4-6",
        "nous" => "nous-hermes-2-mixtral",
        "ollama" => "llama3",
        "xai" => "grok-2",
        "minimax" => "minimax-m1",
        "huggingface" => "mistral-7b-instruct",
        "mistral" => "mistral-large-latest",
        _ => "default",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    fn make_ctx() -> CommandContext {
        CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            vec![],
            h_core::ModelRef::new(
                h_core::ProviderId::new("anthropic"),
                h_core::ModelId::new("claude-sonnet-4-6"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_model_no_args_shows_current() {
        let result = ModelCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("claude-sonnet-4-6"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_model_switch() {
        let result = ModelCommand
            .execute("openai/gpt-4o", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::Model { provider, model }) => {
                assert_eq!(provider, "openai");
                assert_eq!(model, "gpt-4o");
            }
            _ => panic!("Expected ConfigChange::Model"),
        }
    }

    #[tokio::test]
    async fn test_model_provider_only() {
        let result = ModelCommand.execute("nous", &make_ctx()).await.unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::Model { provider, model }) => {
                assert_eq!(provider, "nous");
                assert_eq!(model, "nous-hermes-2-mixtral");
            }
            _ => panic!("Expected ConfigChange::Model"),
        }
    }
}
