use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /summarize — Summarize the current conversation.
pub struct SummarizeCommand;

#[async_trait]
impl SlashCommand for SummarizeCommand {
    fn name(&self) -> &str {
        "summarize"
    }

    fn description(&self) -> &str {
        "Summarize the current conversation"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let msg_count = ctx.messages.len();
        if msg_count == 0 {
            return Ok(CommandResult::Message(
                "No messages to summarize.".to_string(),
            ));
        }

        // Extract user turns count
        let user_turns = ctx.messages.iter().filter(|m| m.role == h_core::Role::User).count();
        let tool_calls = ctx
            .messages
            .iter()
            .map(|m| m.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0))
            .sum::<usize>();

        let msg = format!(
            "Conversation Summary:\n  {user_turns} user turn(s)\n  {msg_count} total message(s)\n  {tool_calls} tool call(s)\n  (Full LLM summarization not yet implemented)"
        );
        Ok(CommandResult::Message(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_summarize_empty() {
        let ctx = CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            vec![],
            h_core::ModelRef::new(
                h_core::ProviderId::new("test"),
                h_core::ModelId::new("test"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        );
        let result = SummarizeCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("No messages")),
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_summarize_with_messages() {
        let msgs = vec![
            h_core::Message::user("hello"),
            h_core::Message::assistant("hi"),
            h_core::Message::user("how are you?"),
            h_core::Message::assistant("fine!"),
        ];
        let ctx = CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            msgs,
            h_core::ModelRef::new(
                h_core::ProviderId::new("test"),
                h_core::ModelId::new("test"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        );
        let result = SummarizeCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("2 user turn"));
                assert!(msg.contains("4 total message"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
