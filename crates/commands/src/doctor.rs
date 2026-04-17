use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /doctor — Run diagnostics.
pub struct DoctorCommand;

#[async_trait]
impl SlashCommand for DoctorCommand {
    fn name(&self) -> &str {
        "doctor"
    }

    fn description(&self) -> &str {
        "Run diagnostics to check configuration and connectivity"
    }

    fn category(&self) -> &str {
        "general"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let mut lines = vec!["Hermes Diagnostics:".to_string(), "".to_string()];
        let mut all_ok = true;

        // Check model configuration
        let model_ok = !ctx.model.provider.as_str().is_empty()
            && !ctx.model.model.as_str().is_empty();
        if !model_ok { all_ok = false; }
        lines.push(format!(
            "  [{}] Model: {}/{}",
            if model_ok { "OK" } else { "WARN" },
            ctx.model.provider,
            ctx.model.model,
        ));

        // Check API key
        let api_key_env = match ctx.model.provider.as_str() {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "openrouter" => "OPENROUTER_API_KEY",
            "nous" => "NOUS_API_KEY",
            "mistral" => "MISTRAL_API_KEY",
            "moonshot" | "kimi" => "MOONSHOT_API_KEY",
            "minimax" => "MINIMAX_API_KEY",
            "huggingface" => "HUGGINGFACE_API_KEY",
            "ollama" => "", // local, no key needed
            _ => "",
        };
        if api_key_env.is_empty() {
            if ctx.model.provider.as_str() == "ollama" {
                lines.push("  [OK]   API key: not required (local Ollama)".to_string());
            } else {
                lines.push(format!("  [WARN] API key: unknown provider '{}', cannot verify", ctx.model.provider));
            }
        } else if std::env::var(api_key_env).ok().filter(|k| !k.is_empty()).is_some() {
            lines.push(format!("  [OK]   API key: {api_key_env} is set"));
        } else {
            all_ok = false;
            lines.push(format!("  [ERR]  API key: {api_key_env} is not set"));
        }

        // Check Hermes home directory
        let hermes_home = h_core::home::hermes_home();
        if hermes_home.exists() {
            lines.push(format!("  [OK]   Hermes home: {}", hermes_home.display()));
        } else {
            all_ok = false;
            lines.push(format!("  [ERR]  Hermes home: {} does not exist", hermes_home.display()));
        }

        // Check config file
        let config_path = h_core::home::config_path();
        if config_path.exists() {
            lines.push(format!("  [OK]   Config: {}", config_path.display()));
        } else {
            lines.push(format!("  [INFO] Config: {} not found (using defaults)", config_path.display()));
        }

        // Check session database
        let session_db = hermes_home.join("sessions.db");
        if session_db.exists() {
            lines.push(format!("  [OK]   Session database: {}", session_db.display()));
        } else {
            lines.push("  [INFO] Session database: will be created on first use".to_string());
        }

        // Check checkpoint database
        let checkpoint_db = hermes_home.join("checkpoints.db");
        if checkpoint_db.exists() {
            lines.push(format!("  [OK]   Checkpoint database: {}", checkpoint_db.display()));
        } else {
            lines.push("  [INFO] Checkpoint database: will be created on first checkpoint".to_string());
        }

        // Check SOUL.md
        let soul_path = hermes_home.join("SOUL.md");
        if soul_path.exists() {
            lines.push(format!("  [OK]   Personality: {}", soul_path.display()));
        } else {
            lines.push("  [INFO] Personality: SOUL.md not found (using default)".to_string());
        }

        // Check memory directory
        let memory_dir = h_core::home::memory_dir();
        if memory_dir.exists() {
            lines.push(format!("  [OK]   Memory: {}", memory_dir.display()));
        } else {
            lines.push("  [INFO] Memory: directory not found".to_string());
        }

        // Check skills directory
        let skills_dir = h_core::home::skills_dir();
        if skills_dir.exists() {
            lines.push(format!("  [OK]   Skills: {}", skills_dir.display()));
        } else {
            lines.push("  [INFO] Skills: directory not found".to_string());
        }

        // Check context files in current directory
        if let Ok(cwd) = std::env::current_dir() {
            for file in ["AGENTS.md", "CLAUDE.md", ".cursorrules"] {
                let path = cwd.join(file);
                if path.exists() {
                    lines.push(format!("  [OK]   Context file: {}", file));
                }
            }
        }

        // Check toolsets
        let enabled = &ctx.hermes_config.enabled_toolsets;
        let disabled = &ctx.hermes_config.disabled_toolsets;
        let total_tools = enabled.as_ref().map(|v| v.len()).unwrap_or(0)
            + disabled.as_ref().map(|v| v.len()).unwrap_or(0);
        lines.push(format!(
            "  [OK]   Tools: {total_tools} configured",
        ));

        // Check skills
        let skills = &ctx.hermes_config.enabled_skills;
        let skill_count = skills.as_ref().map(|v| v.len()).unwrap_or(0);
        lines.push(format!("  [OK]   Skills: {skill_count} enabled"));

        // Check terminal config
        if let Some(terminal) = &ctx.hermes_config.terminal {
            lines.push(format!(
                "  [OK]   Terminal backend: {:?}",
                terminal.backend
            ));
        } else {
            lines.push("  [INFO] Terminal backend: default (local)".to_string());
        }

        // Check MCP
        if let Some(mcp) = &ctx.hermes_config.mcp {
            if let Some(servers) = &mcp.servers {
                lines.push(format!("  [OK]   MCP servers: {} configured", servers.len()));
            }
        } else {
            lines.push("  [INFO] MCP: no servers configured".to_string());
        }

        lines.push("".to_string());
        if all_ok {
            lines.push("All critical checks passed.".to_string());
        } else {
            lines.push("Some checks failed. Review the errors above.".to_string());
        }

        Ok(CommandResult::Message(lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_doctor_command() {
        let ctx = CommandContext::new(
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
        );
        let result = DoctorCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Diagnostics"));
                assert!(msg.contains("claude-sonnet-4-6"));
                assert!(msg.contains("API key"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
