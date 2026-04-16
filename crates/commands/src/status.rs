use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /status — Show current session and platform status.
pub struct StatusCommand;

#[async_trait]
impl SlashCommand for StatusCommand {
    fn name(&self) -> &str {
        "status"
    }

    fn description(&self) -> &str {
        "Show current session and platform status"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let mut lines = vec![
            "Session Status:".to_string(),
            format!("  Session ID: {}", ctx.session_id),
            format!("  Model:      {}/{}", ctx.model.provider, ctx.model.model),
            format!("  Messages:   {}", ctx.messages.len()),
        ];
        if let Some(budget) = ctx.budget_remaining {
            lines.push(format!("  Budget:     {budget} iterations remaining"));
        }
        lines.push(format!("  Cost:       ${:.4}", ctx.cost.estimated_cost_usd));

        let status = if ctx.is_processing {
            "Processing..."
        } else {
            "Idle"
        };
        lines.push(format!("  Status:     {status}"));

        if let Some(enabled) = &ctx.hermes_config.enabled_toolsets {
            lines.push(format!("  Tools:      {} enabled", enabled.len()));
        }
        if let Some(skills) = &ctx.hermes_config.enabled_skills {
            lines.push(format!("  Skills:     {} enabled", skills.len()));
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
    async fn test_status_command() {
        let ctx = CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "sess-123".to_string(),
            vec![
                h_core::Message::user("hello"),
                h_core::Message::assistant("hi"),
            ],
            h_core::ModelRef::new(
                h_core::ProviderId::new("anthropic"),
                h_core::ModelId::new("claude-sonnet-4-6"),
            ),
            h_core::CostTracker {
                estimated_cost_usd: 0.0123,
                ..h_core::CostTracker::default()
            },
            Some(88),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        );
        let result = StatusCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("sess-123"));
                assert!(msg.contains("claude-sonnet-4-6"));
                assert!(msg.contains("88"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
