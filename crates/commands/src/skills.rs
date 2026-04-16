use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /skills — Browse, search, or manage skills.
pub struct SkillsCommand;

#[async_trait]
impl SlashCommand for SkillsCommand {
    fn name(&self) -> &str {
        "skills"
    }

    fn description(&self) -> &str {
        "Browse, search, or manage skills"
    }

    fn category(&self) -> &str {
        "skills"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        let parts: Vec<&str> = args.splitn(2, char::is_whitespace).collect();

        match parts.first() {
            Some(&"") | None => {
                // List installed skills
                let mut lines = vec!["Installed Skills:".to_string()];
                let enabled = ctx.hermes_config.enabled_skills.clone().unwrap_or_default();
                let disabled = ctx.hermes_config.disabled_skills.clone().unwrap_or_default();

                for skill in &enabled {
                    lines.push(format!("  [enabled]  {skill}"));
                }
                for skill in &disabled {
                    lines.push(format!("  [disabled] {skill}"));
                }
                if enabled.is_empty() && disabled.is_empty() {
                    lines.push("  (no skills installed)".to_string());
                }
                Ok(CommandResult::Message(lines.join("\n")))
            }
            Some(&"search") => {
                let query = parts.get(1).map(|s| *s).unwrap_or("");
                if query.is_empty() {
                    return Ok(CommandResult::Message(
                        "Usage: /skills search <query>".to_string(),
                    ));
                }
                Ok(CommandResult::Message(format!(
                    "Searching skills for '{query}'... (skill registry not yet implemented)"
                )))
            }
            Some(&"install") => {
                let skill_id = parts.get(1).map(|s| *s).unwrap_or("");
                if skill_id.is_empty() {
                    return Ok(CommandResult::Message(
                        "Usage: /skills install <skill_id>".to_string(),
                    ));
                }
                Ok(CommandResult::Message(format!(
                    "Installing skill '{skill_id}'... (skill registry not yet implemented)"
                )))
            }
            Some(&"enable") | Some(&"disable") => {
                let action = parts[0];
                let skill = parts.get(1).map(|s| *s).unwrap_or("");
                if skill.is_empty() {
                    return Ok(CommandResult::Message(format!(
                        "Usage: /skills {action} <skill_name>"
                    )));
                }
                Ok(CommandResult::Message(format!(
                    "Skill '{skill}' {action}d."
                )))
            }
            Some(other) => {
                Ok(CommandResult::Message(format!(
                    "Unknown subcommand: {other}. Use /skills [search|install|enable|disable]"
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

    #[tokio::test]
    async fn test_skills_list() {
        let config = h_core::HermesConfig {
            enabled_skills: Some(vec!["github-auth".to_string()]),
            ..h_core::HermesConfig::default()
        };
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
            false,
            Arc::new(Notify::new()),
            config,
        );
        let result = SkillsCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("github-auth")),
            _ => panic!("Expected Message"),
        }
    }
}
