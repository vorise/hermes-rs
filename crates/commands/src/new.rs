use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /new — Start a fresh conversation.
pub struct NewCommand;

#[async_trait]
impl SlashCommand for NewCommand {
    fn name(&self) -> &str {
        "new"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["reset"]
    }

    fn description(&self) -> &str {
        "Start a fresh conversation, clearing the current context"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        Ok(CommandResult::ConfigChange(
            crate::ConfigChange::NewSession,
        ))
    }
}

/// /clear — Clear current session messages.
pub struct ClearCommand;

#[async_trait]
impl SlashCommand for ClearCommand {
    fn name(&self) -> &str {
        "clear"
    }

    fn aliases(&self) -> Vec<&str> {
        vec![]
    }

    fn description(&self) -> &str {
        "Clear current session messages"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        Ok(CommandResult::ConfigChange(
            crate::ConfigChange::ClearSession,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandResult;

    #[tokio::test]
    async fn test_new_command_name() {
        assert_eq!(NewCommand.name(), "new");
        assert_eq!(NewCommand.aliases(), vec!["reset"]);
    }

    #[tokio::test]
    async fn test_clear_command_name() {
        assert_eq!(ClearCommand.name(), "clear");
    }

    #[tokio::test]
    async fn test_new_returns_new_session() {
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let ctx = CommandContext::new(
            std::sync::Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            vec![],
            h_core::ModelRef::new(h_core::ProviderId::new("test"), h_core::ModelId::new("test")),
            h_core::CostTracker::default(),
            Some(90),
            false,
            notify,
            h_core::HermesConfig::default(),
        );
        let result = NewCommand.execute("", &ctx).await.unwrap();
        assert!(matches!(result, CommandResult::ConfigChange(crate::ConfigChange::NewSession)));
    }
}
