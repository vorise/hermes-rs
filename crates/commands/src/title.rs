use anyhow::Result;
use async_trait::async_trait;
use h_api::auxiliary::{AuxiliaryClient, AuxiliaryConfig, AuxiliaryTask};

use crate::{CommandContext, CommandResult, ConfigChange, SlashCommand};

/// /title — Set or auto-generate the session title.
pub struct TitleCommand;

#[async_trait]
impl SlashCommand for TitleCommand {
    fn name(&self) -> &str {
        "title"
    }

    fn description(&self) -> &str {
        "Set or auto-generate the session title"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();

        if args.is_empty() {
            // Auto-generate title from conversation
            return Self::auto_generate_title(ctx).await;
        }

        // Support "auto" subcommand
        if args == "auto" {
            return Self::auto_generate_title(ctx).await;
        }

        Ok(CommandResult::ConfigChange(ConfigChange::Title(args.to_string())))
    }
}

impl TitleCommand {
    async fn auto_generate_title(ctx: &CommandContext) -> Result<CommandResult> {
        // Get first few messages to generate title
        let first_messages: Vec<_> = ctx.messages.iter().take(6).collect();
        if first_messages.is_empty() {
            return Ok(CommandResult::Message(
                "No messages to generate title from. Use /title <text> to set manually.".to_string(),
            ));
        }

        let mut conversation_text = String::new();
        for msg in &first_messages {
            if let Some(ref content) = msg.content {
                if let Some(text) = content.as_text() {
                    let truncated = if text.len() > 500 { &text[..500] } else { text };
                    conversation_text.push_str(&format!("{}: {}\n", msg.role, truncated));
                }
            }
        }

        let prompt = format!(
            "Generate a short, descriptive title (under 50 characters) for this conversation:\n\n{conversation_text}"
        );

        let config = AuxiliaryConfig {
            model: String::new(),
            provider: "anthropic".to_string(),
            ..Default::default()
        };
        let client = AuxiliaryClient::with_config(config);

        match client.execute(AuxiliaryTask::TitleGeneration, &prompt).await {
            Ok(result) => {
                let title = result.text.trim().trim_matches('"').to_string();
                if title.len() > 100 {
                    return Ok(CommandResult::Message("Generated title too long. Use /title <text> to set manually.".to_string()));
                }
                Ok(CommandResult::ConfigChange(ConfigChange::Title(title)))
            }
            Err(e) => {
                // Try OpenAI fallback
                let config2 = AuxiliaryConfig {
                    model: String::new(),
                    provider: "openai".to_string(),
                    ..Default::default()
                };
                let client2 = AuxiliaryClient::with_config(config2);
                match client2.execute(AuxiliaryTask::TitleGeneration, &prompt).await {
                    Ok(result) => {
                        let title = result.text.trim().trim_matches('"').to_string();
                        Ok(CommandResult::ConfigChange(ConfigChange::Title(title)))
                    }
                    Err(e2) => {
                        Ok(CommandResult::Message(
                            format!("Auto-title failed: {e}\n{e2}\nUse /title <text> to set manually.")
                        ))
                    }
                }
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
    async fn test_title_set() {
        let result = TitleCommand
            .execute("My Session Title", &make_ctx())
            .await
            .unwrap();
        match result {
            CommandResult::ConfigChange(ConfigChange::Title(title)) => {
                assert_eq!(title, "My Session Title");
            }
            _ => panic!("Expected ConfigChange::Title"),
        }
    }

    #[tokio::test]
    async fn test_title_auto_empty_messages() {
        let ctx = make_ctx(); // No messages
        let result = TitleCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("No messages"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_title_auto_subcommand() {
        let ctx = make_ctx(); // No messages
        let result = TitleCommand.execute("auto", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("No messages"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
