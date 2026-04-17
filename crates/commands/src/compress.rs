use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /compress — Manually trigger context compression.
pub struct CompressCommand;

#[async_trait]
impl SlashCommand for CompressCommand {
    fn name(&self) -> &str {
        "compress"
    }

    fn description(&self) -> &str {
        "Manually trigger context compression"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let msg_count = ctx.messages.len();
        let user_msgs = ctx.messages.iter().filter(|m| m.role == h_core::Role::User).count();
        let assistant_msgs = ctx.messages.iter().filter(|m| m.role == h_core::Role::Assistant).count();
        let tool_msgs = ctx.messages.iter().filter(|m| m.role == h_core::Role::Tool).count();

        // Estimate token usage
        let mut total_chars = 0;
        let mut max_msg_chars = 0;
        for msg in &ctx.messages {
            if let Some(ref content) = msg.content {
                if let Some(text) = content.as_text() {
                    let chars = text.len();
                    total_chars += chars;
                    if chars > max_msg_chars {
                        max_msg_chars = chars;
                    }
                }
            }
        }
        // Rough estimate: ~4 chars per token for English text
        let estimated_tokens = total_chars / 4;

        // Model context limits (approximate)
        let context_limit = if ctx.model.model.as_str().contains("opus")
            || ctx.model.model.as_str().contains("4o")
            || ctx.model.model.as_str().contains("sonnet")
        {
            200_000 // Claude 3.5/4, GPT-4o
        } else {
            128_000 // Conservative default
        };
        let context_usage_pct = if context_limit > 0 {
            (estimated_tokens as f64 / context_limit as f64) * 100.0
        } else {
            0.0
        };

        let compression_recommended = context_usage_pct > 70.0 || estimated_tokens > 50_000;

        let mut lines = vec![
            "Context Analysis:".to_string(),
            String::new(),
            format!("  Messages:       {msg_count} ({user_msgs} user, {assistant_msgs} assistant, {tool_msgs} tool)"),
            format!("  Estimated tokens: ~{estimated_tokens}"),
            format!("  Context limit:    {context_limit}"),
            format!("  Context usage:    {context_usage_pct:.1}%"),
            format!("  Largest message:  {max_msg_chars} chars"),
            String::new(),
        ];

        if compression_recommended {
            lines.push(format!("  [!] Compression recommended. Use an auxiliary model with fewer tokens to generate a summary."));
        } else {
            lines.push("  Context size is within comfortable limits. No compression needed.".to_string());
        }

        lines.push(String::new());
        lines.push("  (LLM-based context compression requires wiring up an auxiliary model call.)".to_string());

        Ok(CommandResult::Message(lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_compress_command() {
        let ctx = CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            vec![
                h_core::Message::user("hello"),
                h_core::Message::assistant("hi there"),
            ],
            h_core::ModelRef::new(
                h_core::ProviderId::new("test"),
                h_core::ModelId::new("claude-sonnet-4-6"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        );
        let result = CompressCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Context Analysis"));
                assert!(msg.contains("Messages"));
                assert!(msg.contains("1 user"));
                assert!(msg.contains("1 assistant"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_compress_shows_token_estimate() {
        let ctx = CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            vec![
                h_core::Message::user("hello world this is a longer message to test token estimation"),
            ],
            h_core::ModelRef::new(
                h_core::ProviderId::new("test"),
                h_core::ModelId::new("claude-sonnet-4-6"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        );
        let result = CompressCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Estimated tokens"));
                assert!(msg.contains("Context usage"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
