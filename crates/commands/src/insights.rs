use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /insights — Show usage analytics and insights.
pub struct InsightsCommand;

#[async_trait]
impl SlashCommand for InsightsCommand {
    fn name(&self) -> &str {
        "insights"
    }

    fn description(&self) -> &str {
        "Show usage analytics and insights"
    }

    fn category(&self) -> &str {
        "general"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        // Parse --days N argument
        let days = parse_days_arg(args).unwrap_or(7);

        let c = &ctx.cost;
        let msg = format!(
            "Usage Insights (last {days} days):\n  Sessions:    (not yet tracked)\n  Total tokens: {total}\n  Total cost:   ${cost:.4}\n  API calls:    {calls}\n  Model:        {provider}/{model}\n\n(Full analytics require querying the session database)",
            total = c.total_tokens(),
            cost = c.estimated_cost_usd,
            calls = c.api_call_count,
            provider = ctx.model.provider,
            model = ctx.model.model,
        );
        Ok(CommandResult::Message(msg))
    }
}

fn parse_days_arg(args: &str) -> Option<usize> {
    // Look for --days N
    let parts: Vec<&str> = args.split_whitespace().collect();
    for i in 0..parts.len() {
        if parts[i] == "--days" || parts[i] == "-d" {
            if let Some(next) = parts.get(i + 1) {
                return next.parse().ok();
            }
        }
    }
    None
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
            h_core::CostTracker {
                input_tokens: 5000,
                output_tokens: 3000,
                estimated_cost_usd: 0.1234,
                api_call_count: 5,
                ..h_core::CostTracker::default()
            },
            Some(85),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_insights_default_days() {
        let result = InsightsCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("last 7 days"));
                assert!(msg.contains("8000")); // total tokens
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_insights_custom_days() {
        let result = InsightsCommand
            .execute("--days 30", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("last 30 days")),
            _ => panic!("Expected Message"),
        }
    }
}
