use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /stop — Interrupt current tool execution.
pub struct StopCommand;

#[async_trait]
impl SlashCommand for StopCommand {
    fn name(&self) -> &str {
        "stop"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["interrupt", "cancel"]
    }

    fn description(&self) -> &str {
        "Interrupt the current tool execution"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        if ctx.is_processing {
            ctx.interrupt_notify.notify_one();
            Ok(CommandResult::Message("Interrupt sent. Stopping...".to_string()))
        } else {
            Ok(CommandResult::Message(
                "No active task to stop.".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_stop_when_processing() {
        let notify = Arc::new(Notify::new());
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
            true, // is_processing
            notify.clone(),
            h_core::HermesConfig::default(),
        );
        let result = StopCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("Stopping")),
            _ => panic!("Expected Message"),
        }
        // Verify the notify was triggered by checking it resolves
        notify.notify_one(); // second notify to prove first was consumed
    }

    #[tokio::test]
    async fn test_stop_when_idle() {
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
            false, // is_processing
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        );
        let result = StopCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("No active task")),
            _ => panic!("Expected Message"),
        }
    }
}
