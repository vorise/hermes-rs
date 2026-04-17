use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /nudge — View or control memory and skill nudge settings.
pub struct NudgeCommand;

#[async_trait]
impl SlashCommand for NudgeCommand {
    fn name(&self) -> &str {
        "nudge"
    }

    fn description(&self) -> &str {
        "View or control memory and skill nudge settings"
    }

    fn category(&self) -> &str {
        "memory"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        if args.is_empty() || args == "status" {
            Self::status(ctx)
        } else if let Some(rest) = args.strip_prefix("enable") {
            Self::enable(rest.trim(), ctx)
        } else if let Some(rest) = args.strip_prefix("disable") {
            Self::disable(rest.trim(), ctx)
        } else {
            Ok(CommandResult::Message(format!(
                "Unknown subcommand: {args}. Use /nudge [status|enable|disable]"
            )))
        }
    }
}

impl NudgeCommand {
    fn status(ctx: &CommandContext) -> Result<CommandResult> {
        let nudge = h_core::NudgeSystem::from_hermes_config(&ctx.hermes_config);
        let mut lines = vec!["Nudge System Status:".to_string(), String::new()];

        for line in nudge.status_summary() {
            lines.push(line);
        }

        lines.push(String::new());
        lines.push("Control with: /nudge enable memory|skill <interval>".to_string());
        lines.push("              /nudge disable memory|skill".to_string());

        Ok(CommandResult::Message(lines.join("\n")))
    }

    fn enable(args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.splitn(2, char::is_whitespace).collect();

        let (target, interval) = match parts.as_slice() {
            ["memory", interval] => ("memory", Some(interval)),
            ["memory"] => ("memory", None),
            ["skill", interval] => ("skill", Some(interval)),
            ["skill"] => ("skill", None),
            _ => {
                return Ok(CommandResult::Message(
                    "Usage: /nudge enable memory|skill [interval]".to_string(),
                ));
            }
        };

        let interval = match target {
            "memory" => interval.and_then(|s| s.parse::<u32>().ok()).unwrap_or(10),
            "skill" => interval.and_then(|s| s.parse::<u32>().ok()).unwrap_or(5),
            _ => unreachable!(),
        };

        // Return config change to update the nudge interval
        Ok(CommandResult::ConfigChange(match target {
            "memory" => crate::ConfigChange::SetMemoryNudgeInterval(interval),
            "skill" => crate::ConfigChange::SetSkillNudgeInterval(interval),
            _ => unreachable!(),
        }))
    }

    fn disable(args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let target = args.trim();

        match target {
            "memory" | "skill" => {
                Ok(CommandResult::ConfigChange(match target {
                    "memory" => crate::ConfigChange::SetMemoryNudgeInterval(0),
                    "skill" => crate::ConfigChange::SetSkillNudgeInterval(0),
                    _ => unreachable!(),
                }))
            }
            _ => {
                Ok(CommandResult::Message(
                    "Usage: /nudge disable memory|skill".to_string(),
                ))
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
    async fn test_nudge_status() {
        let result = NudgeCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Nudge System Status"));
                assert!(msg.contains("Memory nudge"));
                assert!(msg.contains("Skill nudge"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_nudge_enable_memory() {
        let result = NudgeCommand.execute("enable memory 15", &make_ctx()).await.unwrap();
        match result {
            CommandResult::ConfigChange(crate::ConfigChange::SetMemoryNudgeInterval(n)) => {
                assert_eq!(n, 15);
            }
            _ => panic!("Expected ConfigChange::SetMemoryNudgeInterval"),
        }
    }

    #[tokio::test]
    async fn test_nudge_disable_skill() {
        let result = NudgeCommand.execute("disable skill", &make_ctx()).await.unwrap();
        match result {
            CommandResult::ConfigChange(crate::ConfigChange::SetSkillNudgeInterval(n)) => {
                assert_eq!(n, 0);
            }
            _ => panic!("Expected ConfigChange::SetSkillNudgeInterval"),
        }
    }
}
