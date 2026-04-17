use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /speak — Convert text to speech using the TTS tool.
pub struct SpeakCommand;

#[async_trait]
impl SlashCommand for SpeakCommand {
    fn name(&self) -> &str {
        "speak"
    }

    fn description(&self) -> &str {
        "Convert text to speech audio"
    }

    fn category(&self) -> &str {
        "general"
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let text = args.trim();
        if text.is_empty() {
            return Ok(CommandResult::Message(
                "Usage: /speak <text>".to_string(),
            ));
        }

        // Check if a TTS backend is available
        let has_elevenlabs = std::env::var("ELEVENLABS_API_KEY")
            .map(|k| !k.is_empty())
            .unwrap_or(false);

        let backend = if has_elevenlabs {
            "ElevenLabs"
        } else {
            "Edge TTS (run: edge-tts --voice en-US-AriaNeural --text '<text>' --write-media output.mp3)"
        };

        Ok(CommandResult::Message(format!(
            "Text-to-speech requested ({text_len} chars). Use the TTS tool to generate audio.\n\
             Backend: {backend}",
            text_len = text.len()
        )))
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
    async fn test_speak_empty_args() {
        let result = SpeakCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Usage"));
            }
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_speak_with_text() {
        let result = SpeakCommand.execute("Hello world", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("Text-to-speech"));
                assert!(msg.contains("11 chars"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
