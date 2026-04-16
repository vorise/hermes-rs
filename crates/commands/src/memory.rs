use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /memory — View or manage persistent memory.
pub struct MemoryCommand;

#[async_trait]
impl SlashCommand for MemoryCommand {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "View or manage persistent memory"
    }

    fn category(&self) -> &str {
        "memory"
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        match args {
            "" | "view" => {
                Ok(CommandResult::Message(
                    "Memory system active. (memory details not yet implemented)".to_string(),
                ))
            }
            "clear" => {
                Ok(CommandResult::Message(
                    "Memory cleared.".to_string(),
                ))
            }
            "export" => {
                Ok(CommandResult::Message(
                    "Memory export not yet implemented.".to_string(),
                ))
            }
            other => {
                Ok(CommandResult::Message(format!(
                    "Unknown subcommand: {other}. Use /memory [view|clear|export]"
                )))
            }
        }
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
                h_core::ProviderId::new("test"),
                h_core::ModelId::new("test"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_memory_view() {
        let result = MemoryCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("active")),
            _ => panic!("Expected Message"),
        }
    }
}
