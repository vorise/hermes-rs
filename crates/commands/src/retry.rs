use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, ConfigChange, SlashCommand};

/// /retry — Retry the last turn with the same user input.
pub struct RetryCommand;

#[async_trait]
impl SlashCommand for RetryCommand {
    fn name(&self) -> &str {
        "retry"
    }

    fn description(&self) -> &str {
        "Retry the last turn with the same user input"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        // Need at least one user message + one assistant message
        let len = ctx.messages.len();
        if len < 2 {
            return Ok(CommandResult::Message(
                "Nothing to retry. Send a message first.".to_string(),
            ));
        }

        // Find the last user message before the last assistant response
        let last = ctx.messages.last().unwrap();
        if last.role != h_core::Role::Assistant {
            return Ok(CommandResult::Message(
                "Nothing to retry.".to_string(),
            ));
        }

        // Get the last user message text
        let last_user = ctx.messages.iter().rev().skip(1).find(|m| m.role == h_core::Role::User);
        let _user_text = last_user
            .and_then(|m| m.content.as_ref().and_then(|c| c.as_text()))
            .unwrap_or("(no text)");

        Ok(CommandResult::ConfigChange(ConfigChange::RetryTurn))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_retry_too_few_messages() {
        let ctx = CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            vec![h_core::Message::user("hello")],
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
        let result = RetryCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("Nothing to retry")),
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_retry_with_turns() {
        let msgs = vec![
            h_core::Message::user("hello"),
            h_core::Message::assistant("hi there"),
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
        let result = RetryCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::RetryTurn) => {}
            _ => panic!("Expected RetryTurn"),
        }
    }
}
