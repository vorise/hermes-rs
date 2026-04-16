use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, ConfigChange, SlashCommand};

/// /undo — Undo the last turn (remove last user+assistant exchange).
pub struct UndoCommand;

#[async_trait]
impl SlashCommand for UndoCommand {
    fn name(&self) -> &str {
        "undo"
    }

    fn description(&self) -> &str {
        "Undo the last conversation turn"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        if ctx.messages.is_empty() {
            return Ok(CommandResult::Message(
                "Nothing to undo.".to_string(),
            ));
        }

        // Count how many messages to remove (assistant + tool calls)
        let mut count = 0;
        let msgs: Vec<&h_core::Message> = ctx.messages.iter().rev().collect();

        // Remove assistant message
        if let Some(msg) = msgs.first() {
            if msg.role == h_core::Role::Assistant {
                count += 1;
            }
        }

        // Check if we have a user message before it
        if count > 0 && msgs.len() > 1 {
            if msgs[1].role == h_core::Role::User {
                count += 1;
            }
        }

        if count == 0 {
            return Ok(CommandResult::Message(
                "Nothing to undo.".to_string(),
            ));
        }

        Ok(CommandResult::ConfigChange(ConfigChange::UndoTurn))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_undo_empty() {
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
        let result = UndoCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("Nothing")),
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_undo_with_turns() {
        let msgs = vec![
            h_core::Message::user("hello"),
            h_core::Message::assistant("hi"),
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
        let result = UndoCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::UndoTurn) => {}
            _ => panic!("Expected UndoTurn"),
        }
    }
}
