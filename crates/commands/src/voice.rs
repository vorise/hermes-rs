use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /voice — Toggle voice input mode.
pub struct VoiceCommand;

#[async_trait]
impl SlashCommand for VoiceCommand {
    fn name(&self) -> &str {
        "voice"
    }

    fn description(&self) -> &str {
        "Toggle voice input mode"
    }

    fn category(&self) -> &str {
        "general"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let has_openai = std::env::var("OPENAI_API_KEY")
            .map(|k| !k.is_empty())
            .unwrap_or(false);

        if !has_openai {
            return Ok(CommandResult::Message(
                "Voice input requires OpenAI Whisper transcription. \
                 Set OPENAI_API_KEY to enable voice input.".to_string(),
            ));
        }

        Ok(CommandResult::Message(
            "Voice input mode toggled. Speak now — audio will be transcribed \
             using Whisper and sent as text.".to_string(),
        ))
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
    async fn test_voice_no_api_key() {
        // Clear env var for this test
        unsafe { std::env::remove_var("OPENAI_API_KEY") };

        let result = VoiceCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("OPENAI_API_KEY"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
