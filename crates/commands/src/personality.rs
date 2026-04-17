use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, ConfigChange, SlashCommand};

/// /personality — Set or view the agent's personality.
pub struct PersonalityCommand;

#[async_trait]
impl SlashCommand for PersonalityCommand {
    fn name(&self) -> &str {
        "personality"
    }

    fn description(&self) -> &str {
        "Set or view the agent's personality"
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

            // Also show SOUL.md status
            let hermes_home = h_core::home::hermes_home();
            let soul_path = hermes_home.join("SOUL.md");
            let soul_status = if soul_path.exists() {
                if let Ok(soul) = h_core::soul::load_soul_or_default(None) {
                    format!(
                        " | SOUL.md loaded ({}, {} bytes)",
                        if soul.is_default { "default" } else { "custom" },
                        soul.content.len()
                    )
                } else {
                    String::new()
                }
            } else {
                " | SOUL.md not found".to_string()
            };

            return Ok(CommandResult::Message(format!(
                "Current personality: {current}{soul_status}"
            )));
        }

        // Handle subcommands
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        match parts[0] {
            "set" => {
                let name = parts.get(1).map(|s| s.trim()).unwrap_or("");
                if name.is_empty() {
                    return Ok(CommandResult::Message(
                        "Usage: /personality set <name>".to_string(),
                    ));
                }
                Ok(CommandResult::ConfigChange(ConfigChange::Personality(
                    name.to_string(),
                )))
            }
            "show" => {
                let hermes_home = h_core::home::hermes_home();
                let soul_path = hermes_home.join("SOUL.md");
                if soul_path.exists() {
                    match h_core::soul::load_soul_or_default(None) {
                        Ok(soul) => {
                            let preview = if soul.content.len() > 500 {
                                format!("{}...", &soul.content[..500])
                            } else {
                                soul.content.clone()
                            };
                            Ok(CommandResult::Message(format!(
                                "SOUL.md ({}) ({} bytes):\n\n{preview}",
                                soul_path.display(),
                                soul.content.len()
                            )))
                        }
                        Err(e) => Ok(CommandResult::Message(format!("Error loading SOUL.md: {e}"))),
                    }
                } else {
                    Ok(CommandResult::Message(
                        "SOUL.md not found. Create one with /personality init.".to_string(),
                    ))
                }
            }
            "init" => {
                match h_core::soul::create_default_soul() {
                    Ok(path) => Ok(CommandResult::Message(format!(
                        "Created default SOUL.md at {}",
                        path.display()
                    ))),
                    Err(e) => Ok(CommandResult::Message(format!("Failed to create SOUL.md: {e}"))),
                }
            }
            other => {
                // Treat as shorthand for "set <name>"
                Ok(CommandResult::ConfigChange(ConfigChange::Personality(
                    other.to_string(),
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
            .execute("set kawaii", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::Personality(name)) => {
                assert_eq!(name, "kawaii");
            }
            _ => panic!("Expected ConfigChange::Personality"),
        }
    }

    #[tokio::test]
    async fn test_personality_set_shorthand() {
        let result = PersonalityCommand
            .execute("friendly", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::Personality(name)) => {
                assert_eq!(name, "friendly");
            }
            _ => panic!("Expected ConfigChange::Personality"),
        }
    }

    #[tokio::test]
    async fn test_personality_show_soul() {
        let result = PersonalityCommand
            .execute("show", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::Message(msg) => {
                // Either shows SOUL.md not found or shows content if it exists
                assert!(msg.contains("SOUL.md") || msg.contains("personality"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
