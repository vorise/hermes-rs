use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, ConfigChange, SlashCommand};

/// /tools — List, enable, or disable tools.
pub struct ToolsCommand;

#[async_trait]
impl SlashCommand for ToolsCommand {
    fn name(&self) -> &str {
        "tools"
    }

    fn description(&self) -> &str {
        "List, enable, or disable toolsets"
    }

    fn category(&self) -> &str {
        "tools"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        let parts: Vec<&str> = args.splitn(2, char::is_whitespace).collect();

        match parts.first() {
            Some(&"") | None => {
                // List all toolsets with their status
                let mut lines = vec!["Tools:".to_string()];
                let enabled = ctx.hermes_config.enabled_toolsets.clone().unwrap_or_default();
                let disabled = ctx.hermes_config.disabled_toolsets.clone().unwrap_or_default();

                for tool in &enabled {
                    lines.push(format!("  [enabled]  {tool}"));
                }
                for tool in &disabled {
                    lines.push(format!("  [disabled] {tool}"));
                }
                if enabled.is_empty() && disabled.is_empty() {
                    lines.push("  (no toolsets configured)".to_string());
                }
                Ok(CommandResult::Message(lines.join("\n")))
            }
            Some(&"enable") => {
                let tool_name = parts.get(1).map(|s| s.trim()).unwrap_or("");
                if tool_name.is_empty() {
                    return Ok(CommandResult::Message(
                        "Usage: /tools enable <toolset>".to_string(),
                    ));
                }
                Ok(CommandResult::ConfigChange(ConfigChange::EnableToolset(tool_name.to_string())))
            }
            Some(&"disable") => {
                let tool_name = parts.get(1).map(|s| s.trim()).unwrap_or("");
                if tool_name.is_empty() {
                    return Ok(CommandResult::Message(
                        "Usage: /tools disable <toolset>".to_string(),
                    ));
                }
                Ok(CommandResult::ConfigChange(ConfigChange::DisableToolset(tool_name.to_string())))
            }
            Some(&"list") => {
                self.execute("", ctx).await
            }
            Some(other) => {
                Ok(CommandResult::Message(format!(
                    "Unknown subcommand: {other}. Use /tools [list|enable|disable]"
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
        let config = h_core::HermesConfig {
            enabled_toolsets: Some(vec![
                "read_file".to_string(),
                "write_file".to_string(),
            ]),
            disabled_toolsets: Some(vec!["browser".to_string()]),
            ..h_core::HermesConfig::default()
        };
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
            config,
        )
    }

    #[tokio::test]
    async fn test_tools_list() {
        let result = ToolsCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("read_file"));
                assert!(msg.contains("browser"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_tools_enable() {
        let result = ToolsCommand
            .execute("enable terminal", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::EnableToolset(name)) => {
                assert_eq!(name, "terminal");
            }
            _ => panic!("Expected ConfigChange::EnableToolset"),
        }
    }

    #[tokio::test]
    async fn test_tools_disable() {
        let result = ToolsCommand
            .execute("disable read_file", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::DisableToolset(name)) => {
                assert_eq!(name, "read_file");
            }
            _ => panic!("Expected ConfigChange::DisableToolset"),
        }
    }
}
