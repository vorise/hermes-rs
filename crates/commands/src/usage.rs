use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /usage — Show current session token usage and cost.
pub struct UsageCommand;

#[async_trait]
impl SlashCommand for UsageCommand {
    fn name(&self) -> &str {
        "usage"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["cost", "tokens"]
    }

    fn description(&self) -> &str {
        "Show current session token usage and estimated cost"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let c = &ctx.cost;
        let mut lines = vec![
            "Token Usage:".to_string(),
            format!("  Input tokens:       {}", c.input_tokens),
            format!("  Output tokens:      {}", c.output_tokens),
            format!("  Cache read tokens:  {}", c.cache_read_tokens),
            format!("  Cache write tokens: {}", c.cache_write_tokens),
            format!("  Reasoning tokens:   {}", c.reasoning_tokens),
            format!("  Total tokens:       {}", c.total_tokens()),
            format!("  API calls:          {}", c.api_call_count),
            format!("  Estimated cost:     ${:.4}", c.estimated_cost_usd),
        ];
        if let Some(budget) = ctx.budget_remaining {
            lines.push(format!("  Iterations left:    {budget}"));
        }
        Ok(CommandResult::Message(lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_usage_command() {
        let cost = h_core::CostTracker {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_write_tokens: 100,
            reasoning_tokens: 300,
            estimated_cost_usd: 0.0425,
            api_call_count: 3,
        };
        let ctx = CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            vec![],
            h_core::ModelRef::new(
                h_core::ProviderId::new("test"),
                h_core::ModelId::new("test"),
            ),
            cost,
            Some(87),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        );
        let result = UsageCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("1000"));
                assert!(msg.contains("500"));
                assert!(msg.contains("0.0425"));
                assert!(msg.contains("87"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
