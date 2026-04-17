use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /platforms — Show connected platform status (CLI-only).
pub struct PlatformsCommand;

#[async_trait]
impl SlashCommand for PlatformsCommand {
    fn name(&self) -> &str {
        "platforms"
    }

    fn description(&self) -> &str {
        "Show connected platform and gateway status"
    }

    fn category(&self) -> &str {
        "general"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let mut lines = vec!["Platform Status:".to_string(), "".to_string()];

        let config = &ctx.hermes_config;

        // Telegram
        if let Some(ref telegram) = config.platforms.as_ref().and_then(|p| p.telegram.as_ref()) {
            let token_set = std::env::var(&telegram.bot_token_env).is_ok();
            lines.push(format!(
                "  [{}] Telegram: {} ({})",
                if token_set { "OK" } else { "WARN" },
                if token_set { "connected" } else { "token not found in env" },
                telegram.bot_token_env,
            ));
        } else {
            lines.push("  [INFO] Telegram: not configured".to_string());
        }

        // Discord
        if let Some(ref discord) = config.platforms.as_ref().and_then(|p| p.discord.as_ref()) {
            let token_set = std::env::var(&discord.bot_token_env).is_ok();
            lines.push(format!(
                "  [{}] Discord: {} ({})",
                if token_set { "OK" } else { "WARN" },
                if token_set { "connected" } else { "token not found in env" },
                discord.bot_token_env,
            ));
        } else {
            lines.push("  [INFO] Discord: not configured".to_string());
        }

        // Slack
        if let Some(ref slack) = config.platforms.as_ref().and_then(|p| p.slack.as_ref()) {
            let bot_ok = std::env::var(&slack.bot_token_env).is_ok();
            let app_ok = std::env::var(&slack.app_token_env).is_ok();
            lines.push(format!(
                "  [{}] Slack: {} (bot: {}, app: {})",
                if bot_ok && app_ok { "OK" } else { "WARN" },
                if bot_ok && app_ok { "connected" } else { "token not found in env" },
                slack.bot_token_env,
                slack.app_token_env,
            ));
        } else {
            lines.push("  [INFO] Slack: not configured".to_string());
        }

        // Web UI
        if let Some(ref web) = config.web {
            let enabled = web.enabled.unwrap_or(false);
            let host = web.host.as_deref().unwrap_or("localhost");
            let port = web.port.unwrap_or(8080);
            lines.push(format!(
                "  [{}] Web UI: {} (http://{}:{port})",
                if enabled { "OK" } else { "OFF" },
                if enabled { "running" } else { "disabled" },
                host,
            ));
        } else {
            lines.push("  [INFO] Web UI: not configured".to_string());
        }

        // Gateway (cron jobs)
        if let Some(ref cron) = config.cron {
            if let Some(ref jobs) = cron.jobs {
                let enabled_jobs = jobs.iter().filter(|j| j.enabled.unwrap_or(true)).count();
                lines.push(format!("  [OK]   Cron: {enabled_jobs} jobs scheduled"));
            }
        } else {
            lines.push("  [INFO] Cron: no jobs configured".to_string());
        }

        lines.push("".to_string());
        lines.push("Running in CLI mode (no gateway daemon).".to_string());

        Ok(CommandResult::Message(lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_platforms_command() {
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
        let result = PlatformsCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Platform Status"));
                assert!(msg.contains("Telegram"));
                assert!(msg.contains("Discord"));
                assert!(msg.contains("Slack"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
