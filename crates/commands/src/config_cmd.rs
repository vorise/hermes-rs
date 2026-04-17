use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /config — View or set configuration values.
pub struct ConfigCommand;

#[async_trait]
impl SlashCommand for ConfigCommand {
    fn name(&self) -> &str {
        "config"
    }

    fn description(&self) -> &str {
        "View or set configuration values"
    }

    fn category(&self) -> &str {
        "general"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();

        if args.is_empty() {
            // Show full config summary
            let config = &ctx.hermes_config;
            let mut lines = vec!["Configuration:".to_string(), "".to_string()];

            lines.push(format!("  Model:        {}", config.model.as_deref().unwrap_or("(not set)")));
            lines.push(format!("  Provider:     {}", config.provider.as_deref().unwrap_or("(not set)")));
            lines.push(format!("  Personality:  {}", config.personality.as_deref().unwrap_or("(default)")));

            if let Some(ref toolsets) = config.enabled_toolsets {
                lines.push(format!("  Toolsets:     {}", toolsets.join(", ")));
            }

            if let Some(ref terminal) = config.terminal {
                lines.push(format!("  Terminal:     {:?}", terminal.backend));
            }

            if let Some(ref memory) = config.memory {
                lines.push(format!("  Memory:       {}", if memory.enabled.unwrap_or(true) { "enabled" } else { "disabled" }));
            }

            lines.push("".to_string());
            lines.push("Use /config <key> to view a specific value.".to_string());

            return Ok(CommandResult::Message(lines.join("\n")));
        }

        // Parse sub-command: `config <key>` or `config set <key> <value>`
        let parts: Vec<&str> = args.splitn(3, ' ').collect();

        if parts.len() >= 2 && parts[0] == "set" {
            // Setting a value
            let key = parts[1];
            let value = parts.get(2).copied().unwrap_or("");
            Self::set_config(key, value, ctx)
        } else {
            // View a specific key
            let key = parts[0];
            let value = match key {
                "model" => ctx.hermes_config.model.as_deref().unwrap_or("(not set)"),
                "provider" => ctx.hermes_config.provider.as_deref().unwrap_or("(not set)"),
                "personality" => ctx.hermes_config.personality.as_deref().unwrap_or("(default)"),
                "terminal" => {
                    match &ctx.hermes_config.terminal {
                        Some(t) => return Ok(CommandResult::Message(format!("  terminal:\n    backend: {:?}", t.backend))),
                        None => "(default: local)",
                    }
                }
                "memory" => {
                    match &ctx.hermes_config.memory {
                        Some(m) => return Ok(CommandResult::Message(format!("  memory:\n    enabled: {}", m.enabled.unwrap_or(true)))),
                        None => "(defaults)",
                    }
                }
                _ => return Ok(CommandResult::Message(
                    format!("Unknown config key: '{key}'. Available keys: model, provider, personality, terminal, memory"),
                )),
            };
            Ok(CommandResult::Message(format!("  {key}: {value}")))
        }
    }
}

impl ConfigCommand {
    fn set_config(key: &str, value: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let config_path = h_core::home::config_path();

        // Load existing config or start with empty
        let mut config: serde_yaml::Value = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            serde_yaml::from_str(&content).unwrap_or(serde_yaml::Value::Mapping(Default::default()))
        } else {
            serde_yaml::Value::Mapping(Default::default())
        };

        let mapping = config
            .as_mapping_mut()
            .ok_or_else(|| anyhow::anyhow!("Failed to parse config as mapping"))?;

        match key {
            "model" => {
                mapping.insert(serde_yaml::Value::String("model".to_string()), serde_yaml::Value::String(value.to_string()));
                Ok(CommandResult::Message(format!("Set model to '{value}'. Saved to config.")))
            }
            "provider" => {
                mapping.insert(serde_yaml::Value::String("provider".to_string()), serde_yaml::Value::String(value.to_string()));
                Ok(CommandResult::Message(format!("Set provider to '{value}'. Saved to config.")))
            }
            "personality" => {
                mapping.insert(serde_yaml::Value::String("personality".to_string()), serde_yaml::Value::String(value.to_string()));
                Ok(CommandResult::ConfigChange(
                    crate::ConfigChange::Personality(value.to_string()),
                ))
            }
            "terminal" => {
                mapping.insert(
                    serde_yaml::Value::String("terminal".to_string()),
                    serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([(
                        serde_yaml::Value::String("backend".to_string()),
                        serde_yaml::Value::String(value.to_string()),
                    )])),
                );
                Ok(CommandResult::Message(format!("Set terminal backend to '{value}'. Saved to config.")))
            }
            _ => Ok(CommandResult::Message(
                format!("Unknown config key: '{key}'. Settable keys: model, provider, personality, terminal"),
            )),
        }.map(|result| {
            // Persist to disk for non-personality changes (personality goes through ConfigChange)
            if !matches!(result, CommandResult::ConfigChange(_)) {
                if let Ok(yaml) = serde_yaml::to_string(&config) {
                    let _ = std::fs::write(&config_path, yaml);
                }
            }
            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    fn test_ctx() -> CommandContext {
        CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
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
        )
    }

    #[tokio::test]
    async fn test_config_empty_shows_summary() {
        let result = ConfigCommand.execute("", &test_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Configuration"));
                assert!(msg.contains("Model"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_config_view_key() {
        let result = ConfigCommand.execute("model", &test_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("model"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_config_unknown_key() {
        let result = ConfigCommand.execute("nonexistent", &test_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Unknown config key"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_config_set_model() {
        let result = ConfigCommand.execute("set model claude-opus-4-6", &test_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Set model to"));
                assert!(msg.contains("claude-opus-4-6"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_config_set_personality() {
        let result = ConfigCommand.execute("set personality friendly", &test_ctx()).await.unwrap();
        match result {
            CommandResult::ConfigChange(crate::ConfigChange::Personality(name)) => {
                assert_eq!(name, "friendly");
            }
            _ => panic!("Expected ConfigChange::Personality"),
        }
    }
}
