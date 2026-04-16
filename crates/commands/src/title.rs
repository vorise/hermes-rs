use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, ConfigChange, SlashCommand};

/// /title — Set the session title.
pub struct TitleCommand;

#[async_trait]
impl SlashCommand for TitleCommand {
    fn name(&self) -> &str {
        "title"
    }

    fn description(&self) -> &str {
        "Set the session title"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        if args.is_empty() {
            return Ok(CommandResult::Message(
                "Usage: /title <text>".to_string(),
            ));
        }
        Ok(CommandResult::ConfigChange(ConfigChange::Title(args.to_string())))
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
    async fn test_title_set() {
        let result = TitleCommand
            .execute("My Session Title", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::Title(title)) => {
                assert_eq!(title, "My Session Title");
            }
            _ => panic!("Expected ConfigChange::Title"),
        }
    }
}
