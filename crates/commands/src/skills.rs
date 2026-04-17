use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, ConfigChange, SlashCommand};

/// /skills — Browse, search, install, or manage skills.
pub struct SkillsCommand;

#[async_trait]
impl SlashCommand for SkillsCommand {
    fn name(&self) -> &str {
        "skills"
    }

    fn description(&self) -> &str {
        "Browse, search, install, check, or manage skills"
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

                let hub = h_core::skills_hub::SkillsHubClient::new();
                match hub.search_skills(query).await {
                    Ok(results) => {
                        if results.is_empty() {
                            Ok(CommandResult::Message(
                                format!("No skills found for '{query}'."))
                            )
                        } else {
                            let mut lines = vec![format!("Found {} skill(s) for '{query}':\n", results.len())];
                            for skill in &results {
                                lines.push(format!(
                                    "- **{}** ({}) — {}\n  v{} by {}",
                                    skill.name, skill.id, skill.description, skill.version, skill.author
                                ));
                            }
                            lines.push("\nInstall with: `/skills install <skill_id>`".to_string());
                            Ok(CommandResult::Message(lines.join("\n")))
                        }
                    }
                    Err(e) => Ok(CommandResult::Message(
                        format!("Failed to search skills registry: {e}")
                    )),
                }
            }
            Some(&"browse") => {
                let hub = h_core::skills_hub::SkillsHubClient::new();
                match hub.browse_summary().await {
                    Ok(summary) => Ok(CommandResult::Message(summary)),
                    Err(e) => Ok(CommandResult::Message(
                        format!("Failed to browse skills registry: {e}")
                    )),
                }
            }
            Some(&"install") => {
                let skill_id = parts.get(1).map(|s| *s).unwrap_or("");
                if skill_id.is_empty() {
                    return Ok(CommandResult::Message(
                        "Usage: /skills install <skill_id>".to_string(),
                    ));
                }

                let hub = h_core::skills_hub::SkillsHubClient::new();
                match hub.fetch_skill_by_name(skill_id).await {
                    Ok(skill) => {
                        // Build the skill content as it would be stored
                        let content = format!(
                            "---\nname: {}\ndescription: {}\nversion: {}\nauthor: {}\n---\n\n{}",
                            skill.name, skill.description, skill.version, skill.author, skill.content
                        );

                        // Run safety checks before installing
                        let temp_skill = h_core::skills::Skill {
                            id: skill.id.clone(),
                            name: skill.name.clone(),
                            description: skill.description.clone(),
                            version: skill.version.clone(),
                            author: skill.author.clone(),
                            content: skill.content.clone(),
                            path: Default::default(),
                            enabled: true,
                        };
                        let check = h_core::skills_guard::validate_skill_safety(&temp_skill);

                        // If there are blocking errors, reject the install
                        if !check.passed && !check.errors.is_empty() {
                            let error_lines: Vec<_> = check.errors.iter()
                                .map(|e| format!("  ERROR: {e}"))
                                .collect();
                            return Ok(CommandResult::Message(format!(
                                "Skill '{}' blocked by safety checks:\n{}",
                                skill_id,
                                error_lines.join("\n")
                            )));
                        }

                        let mut registry = h_core::skills::SkillRegistry::default_path()
                            .map_err(|e| anyhow::anyhow!("Failed to access skills directory: {e}"))?;
                        registry.load_all().ok();

                        match registry.install(&skill.id, &content) {
                            Ok(()) => {
                                let mut lines = vec![format!(
                                    "Installed skill '{}': {}",
                                    skill.id, skill.description
                                )];
                                if !check.warnings.is_empty() {
                                    lines.push(String::new());
                                    lines.push("Safety warnings:".to_string());
                                    for w in &check.warnings {
                                        lines.push(format!("  WARNING: {w}"));
                                    }
                                }
                                Ok(CommandResult::Message(lines.join("\n")))
                            }
                            Err(e) => Ok(CommandResult::Message(
                                format!("Failed to install skill '{}': {e}", skill.id)
                            )),
                        }
                    }
                    Err(e) => Ok(CommandResult::Message(
                        format!("Failed to fetch skill '{skill_id}' from registry: {e}")
                    )),
                }
            }
            Some(&"enable") => {
                let skill = parts.get(1).map(|s| s.trim()).unwrap_or("");
                if skill.is_empty() {
                    return Ok(CommandResult::Message(
                        "Usage: /skills enable <skill_name>".to_string(),
                    ));
                }
                Ok(CommandResult::ConfigChange(ConfigChange::EnableSkill(skill.to_string())))
            }
            Some(&"disable") => {
                let skill = parts.get(1).map(|s| s.trim()).unwrap_or("");
                if skill.is_empty() {
                    return Ok(CommandResult::Message(
                        "Usage: /skills disable <skill_name>".to_string(),
                    ));
                }
                Ok(CommandResult::ConfigChange(ConfigChange::DisableSkill(skill.to_string())))
            }
            Some(&"check") => {
                // Run safety audit on all installed skills
                let mut registry = match h_core::skills::SkillRegistry::default_path() {
                    Ok(r) => r,
                    Err(e) => {
                        return Ok(CommandResult::Message(
                            format!("Failed to access skills directory: {e}")
                        ));
                    }
                };
                registry.load_all().ok();

                let enabled: Vec<_> = registry.enabled_skills();
                if enabled.is_empty() {
                    return Ok(CommandResult::Message(
                        "No skills installed to check.".to_string()
                    ));
                }

                let (checks, conflicts) = h_core::skills_guard::run_skill_checks(&enabled);

                let mut lines = vec!["Skill Safety Audit:".to_string()];
                let mut any_issues = false;

                for check in &checks {
                    if check.warnings.is_empty() && check.errors.is_empty() {
                        lines.push(format!("  [OK] {}", check.skill_id));
                    } else {
                        any_issues = true;
                        let status = if check.passed { "WARN" } else { "FAIL" };
                        lines.push(format!("  [{status}] {}", check.skill_id));
                        for e in &check.errors {
                            lines.push(format!("    ERROR: {e}"));
                        }
                        for w in &check.warnings {
                            lines.push(format!("    WARNING: {w}"));
                        }
                    }
                }

                if !conflicts.is_empty() {
                    lines.push(String::new());
                    lines.push("Conflicts:".to_string());
                    for (_, field, msg) in &conflicts {
                        lines.push(format!("  {field}: {msg}"));
                    }
                }

                if !any_issues {
                    lines.push(String::new());
                    lines.push("All skills passed safety checks.".to_string());
                }

                Ok(CommandResult::Message(lines.join("\n")))
            }
            Some(other) => {
                Ok(CommandResult::Message(format!(
                    "Unknown subcommand: {other}. Use /skills [search|install|browse|enable|disable]"
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

    #[tokio::test]
    async fn test_skills_enable() {
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
            h_core::HermesConfig::default(),
        );
        let result = SkillsCommand.execute("enable github-auth", &ctx).await.unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::EnableSkill(name)) => {
                assert_eq!(name, "github-auth");
            }
            _ => panic!("Expected ConfigChange::EnableSkill"),
        }
    }

    #[tokio::test]
    async fn test_skills_disable() {
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
            h_core::HermesConfig::default(),
        );
        let result = SkillsCommand.execute("disable github-auth", &ctx).await.unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::DisableSkill(name)) => {
                assert_eq!(name, "github-auth");
            }
            _ => panic!("Expected ConfigChange::DisableSkill"),
        }
    }
}
