use anyhow::Result;
use async_trait::async_trait;
use h_api::auxiliary::{AuxiliaryClient, AuxiliaryConfig, AuxiliaryTask, format_compression_input};

use crate::{CommandContext, CommandResult, SlashCommand};

/// /summarize — Summarize the current conversation.
pub struct SummarizeCommand;

#[async_trait]
impl SlashCommand for SummarizeCommand {
    fn name(&self) -> &str {
        "summarize"
    }

    fn description(&self) -> &str {
        "Summarize the current conversation"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let msg_count = ctx.messages.len();
        if msg_count == 0 {
            return Ok(CommandResult::Message(
                "No messages to summarize.".to_string(),
            ));
        }

        // Extract user turns count
        let user_turns = ctx.messages.iter().filter(|m| m.role == h_core::Role::User).count();
        let tool_calls: usize = ctx
            .messages
            .iter()
            .map(|m| m.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0))
            .sum();

        // Format conversation for compression
        let mut conversation_text = String::new();
        for msg in &ctx.messages {
            if let Some(ref content) = msg.content {
                if let Some(text) = content.as_text() {
                    let truncated = if text.len() > 2000 { &text[..2000] } else { text };
                    conversation_text.push_str(&format!("{}: {}\n", msg.role, truncated));
                }
            }
            // Include tool call names
            if let Some(ref tool_calls_list) = msg.tool_calls {
                for tc in tool_calls_list {
                    conversation_text.push_str(&format!("  [Tool Call: {}]\n", tc.function.name));
                }
            }
        }

        let input = format_compression_input(&conversation_text);

        // Try to use auxiliary LLM for summarization
        match Self::try_summarize_with_llm(&input).await {
            Ok(Some(result)) => {
                let msg = format!(
                    "Conversation Summary (LLM-generated, {} tokens):\n\n{}\n\n---\nStats: {user_turns} user turn(s), {msg_count} total message(s), {tool_calls} tool call(s)",
                    result.total_tokens(),
                    result.text
                );
                return Ok(CommandResult::Message(msg));
            }
            Ok(None) => {} // Fall through to basic summary
            Err(e) => {
                return Ok(CommandResult::Message(
                    format!(
                        "Conversation Summary:\n  {user_turns} user turn(s)\n  {msg_count} total message(s)\n  {tool_calls} tool call(s)\n\n(LLM summarization failed: {e})"
                    )
                ));
            }
        }

        // Fallback: basic stats-only summary
        let msg = format!(
            "Conversation Summary:\n  {user_turns} user turn(s)\n  {msg_count} total message(s)\n  {tool_calls} tool call(s)\n\n(Configure ANTHROPIC_API_KEY or OPENAI_API_KEY for LLM summarization)"
        );
        Ok(CommandResult::Message(msg))
    }
}

impl SummarizeCommand {
    async fn try_summarize_with_llm(input: &str) -> Result<Option<h_api::auxiliary::AuxiliaryResult>> {
        let config = AuxiliaryConfig {
            model: String::new(), // Use task-specific default
            provider: "anthropic".to_string(),
            ..Default::default()
        };
        let client = AuxiliaryClient::with_config(config);
        match client.execute(AuxiliaryTask::Compression, input).await {
            Ok(result) => Ok(Some(result)),
            Err(e) => {
                // Try OpenAI if Anthropic fails
                let config2 = AuxiliaryConfig {
                    model: String::new(),
                    provider: "openai".to_string(),
                    ..Default::default()
                };
                let client2 = AuxiliaryClient::with_config(config2);
                match client2.execute(AuxiliaryTask::Compression, input).await {
                    Ok(result) => Ok(Some(result)),
                    Err(e2) => {
                        // If both fail, check if it's just "no API key" vs a real error
                        if e.to_string().contains("No API key") && e2.to_string().contains("No API key") {
                            Ok(None) // No keys configured - silently fall back
                        } else {
                            Err(e) // Return the actual error
                        }
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

    #[tokio::test]
    async fn test_summarize_empty() {
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
        let result = SummarizeCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("No messages")),
            _ => panic!("Expected Message"),
        }
    }

    #[tokio::test]
    async fn test_summarize_with_messages() {
        let msgs = vec![
            h_core::Message::user("hello"),
            h_core::Message::assistant("hi"),
            h_core::Message::user("how are you?"),
            h_core::Message::assistant("fine!"),
        ];
        let ctx = CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            msgs,
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
        let result = SummarizeCommand.execute("", &ctx).await.unwrap();
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("2 user turn"));
                assert!(msg.contains("4 total message"));
            }
            _ => panic!("Expected Message"),
        }
    }
}
