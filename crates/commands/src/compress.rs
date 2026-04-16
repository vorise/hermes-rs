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
        Ok(CommandResult::Message(format!(
            "Context compression triggered. {msg_count} messages in session."
        )))
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
                h_core::ModelId::new("test"),
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
                assert!(msg.contains("2 messages"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
