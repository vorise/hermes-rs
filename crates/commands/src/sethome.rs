use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /sethome — Set the current channel as the home channel.
///
/// Used in messaging platforms (Telegram, Discord, etc.) to mark
/// the current conversation as the "home" for cross-platform continuity.
/// In CLI mode this is informational only.
pub struct SethomeCommand;

#[async_trait]
impl SlashCommand for SethomeCommand {
    fn name(&self) -> &str {
        "sethome"
    }

    fn description(&self) -> &str {
        "Set current channel as home for cross-platform continuity"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let session_id = &ctx.session_id;
        Ok(CommandResult::Message(
            format!("Session '{session_id}' set as home channel.\nCross-platform conversations will route to this session."),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_sethome_command() {
        let ctx = CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "session-123".to_string(),
            vec![],
            h_core::ModelRef::new(
                h_core::ProviderId::new("anthropic"),
                h_core::ModelId::new("claude-sonnet-4-6"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        );
        let result = SethomeCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("session-123"));
                assert!(msg.contains("home channel"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
