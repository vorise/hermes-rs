use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, ConfigChange, SlashCommand};

/// /personality — Set the agent's personality.
pub struct PersonalityCommand;

#[async_trait]
impl SlashCommand for PersonalityCommand {
    fn name(&self) -> &str {
        "personality"
    }

    fn description(&self) -> &str {
        "Set the agent's personality"
    }

    fn category(&self) -> &str {
        "general"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        if args.is_empty() {
            let current = ctx
                .hermes_config
                .personality
                .as_deref()
                .unwrap_or("default");
            return Ok(CommandResult::Message(format!(
                "Current personality: {current}"
            )));
        }

        Ok(CommandResult::ConfigChange(ConfigChange::Personality(
            args.to_string(),
        )))
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
    async fn test_personality_show_default() {
        let result = PersonalityCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("default")),
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_personality_set() {
        let result = PersonalityCommand
            .execute("kawaii", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::Personality(name)) => {
                assert_eq!(name, "kawaii");
            }
            _ => panic!("Expected ConfigChange::Personality"),
        }
    }
}
