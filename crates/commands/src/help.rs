use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, CommandRegistry, SlashCommand};

/// /help — Show help information for all commands or a specific command.
pub struct HelpCommand;

#[async_trait]
impl SlashCommand for HelpCommand {
    fn name(&self) -> &str {
        "help"
    }

    fn description(&self) -> &str {
        "Show help information for commands"
    }

    fn category(&self) -> &str {
        "general"
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        if args.is_empty() {
            // Show all commands grouped by category
            let registry = all_commands();
            let groups = registry.list_commands();

            let mut lines = vec!["Available Commands:".to_string(), "".to_string()];
            for (category, cmds) in groups {
                lines.push(format!("[{category}]"));
                for cmd in cmds {
                    let aliases = if cmd.aliases().is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", cmd.aliases().join(", "))
                    };
                    lines.push(format!("  /{}{} — {}", cmd.name(), aliases, cmd.description()));
                }
                lines.push("".to_string());
            }
            lines.push("Use /help <command> for more info.".to_string());
            Ok(CommandResult::Message(lines.join("\n")))
        } else {
            // Show specific command help
            let registry = all_commands();
            if let Some(cmd) = registry.find(args) {
                let aliases = if cmd.aliases().is_empty() {
                    String::new()
                } else {
                    format!("\nAliases: /{}", cmd.aliases().join(", /"))
                };
                let msg = format!(
                    "/{name} — {desc}\nCategory: {category}{aliases}",
                    name = cmd.name(),
                    desc = cmd.description(),
                    category = cmd.category(),
                );
                Ok(CommandResult::Message(msg))
            } else {
                Ok(CommandResult::Message(format!("Unknown command: /{args}")))
            }
        }
    }
}

/// Build the standard registry for help display.
fn all_commands() -> CommandRegistry {
    CommandRegistry::new(crate::all_commands())
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
    async fn test_help_lists_commands() {
        let result = HelpCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Available Commands"));
                assert!(msg.contains("/new"));
                assert!(msg.contains("/model"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_help_specific_command() {
        let result = HelpCommand.execute("model", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("/model"));
                assert!(msg.contains("Switch"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_help_unknown_command() {
        let result = HelpCommand.execute("nonexistent", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("Unknown command")),
            _ => panic!("Expected Message"),
        }
    }
}
