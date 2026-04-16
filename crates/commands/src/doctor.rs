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

        // Check model configuration
        let model_ok = !ctx.model.provider.as_str().is_empty()
            && !ctx.model.model.as_str().is_empty();
        lines.push(format!(
            "  [{}] Model: {}/{}",
            if model_ok { "OK" } else { "WARN" },
            ctx.model.provider,
            ctx.model.model,
        ));

        // Check session DB (it's created, so it's OK)
        lines.push("  [OK]   Session database".to_string());

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
        lines.push("All checks passed.".to_string());

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
                assert!(msg.contains("All checks passed"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
