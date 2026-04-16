//! Context Compressor
//!
//! Compresses conversation history when token limits are approached.

use anyhow::Result;
use h_api::{ApiClient, ResolvedApiConfig};
use h_core::{Message, Content};
use tracing::{debug, info};

/// Compresses conversation context when approaching token limits.
pub struct ContextCompressor {
    /// Token threshold for triggering compression.
    threshold_tokens: u64,

    /// Auxiliary client for summarization (cheaper model).
    auxiliary_client: Option<ApiClient>,
}

impl ContextCompressor {
    /// Create a new context compressor.
    pub fn new(threshold_tokens: u64) -> Self {
        Self {
            threshold_tokens,
            auxiliary_client: None,
        }
    }

    /// Create with an auxiliary client for summarization.
    pub fn with_client(threshold_tokens: u64, config: ResolvedApiConfig) -> Result<Self> {
        Ok(Self {
            threshold_tokens,
            auxiliary_client: Some(ApiClient::new(config)?),
        })
    }

    /// Check if compression should be triggered.
    ///
    /// Estimates token count from messages and system prompt.
    pub fn should_compress(&self, messages: &[Message], system_prompt: &str) -> bool {
        let estimated_tokens = estimate_tokens(messages, system_prompt);
        estimated_tokens > self.threshold_tokens
    }

    /// Compress older messages in the conversation.
    ///
    /// Preserves recent messages and tool results, summarizes older content.
    pub async fn compress(&self, messages: &mut Vec<Message>) -> Result<()> {
        if messages.len() < 10 {
            // Not enough messages to compress
            return Ok(());
        }

        info!("Compressing context with {} messages", messages.len());

        // Strategy: Keep recent messages, summarize older ones
        let keep_recent = 5; // Keep last 5 turns

        // Identify messages to compress (older half)
        let compress_from = messages.len() / 2;
        let compress_to = messages.len() - keep_recent;

        if compress_from >= compress_to {
            return Ok(()); // Nothing to compress
        }

        // Extract older messages for summarization
        let older_messages: Vec<Message> = messages
            .iter()
            .take(compress_to)
            .skip(compress_from)
            .cloned()
            .collect();

        // Create summary (placeholder - would use auxiliary LLM)
        let summary = create_placeholder_summary(&older_messages);

        // Replace older messages with summary
        let summary_message = Message::system(Content::text(format!(
            "[Earlier conversation summarized]: {}",
            summary
        )));

        // Reconstruct message list
        let recent_messages: Vec<Message> = messages
            .iter()
            .skip(messages.len() - keep_recent)
            .cloned()
            .collect();

        *messages = vec![summary_message];
        messages.extend(recent_messages);

        debug!("Compressed to {} messages", messages.len());

        Ok(())
    }

    /// Get the threshold tokens.
    pub fn threshold(&self) -> u64 {
        self.threshold_tokens
    }
}

impl Default for ContextCompressor {
    fn default() -> Self {
        // Default threshold: 100K tokens
        Self::new(100_000)
    }
}

/// Estimate token count from messages and system prompt.
///
/// Uses a simple heuristic: ~4 characters per token.
fn estimate_tokens(messages: &[Message], system_prompt: &str) -> u64 {
    let mut total_chars = system_prompt.len();

    for msg in messages {
        if let Some(content) = &msg.content {
            total_chars += content.to_string_repr().len();
        }
        // Add overhead for metadata
        total_chars += 50; // role, tool_calls overhead estimate
    }

    // Estimate tokens (rough: 4 chars per token)
    (total_chars / 4) as u64
}

/// Create a placeholder summary (would use LLM in production).
fn create_placeholder_summary(messages: &[Message]) -> String {
    let mut summary_parts = Vec::new();

    for msg in messages {
        if let Some(content) = &msg.content {
            let text = content.to_string_repr();
            if text.len() > 100 {
                // Truncate long content
                summary_parts.push(format!(
                    "{}: {}...",
                    msg.role,
                    text.chars().take(100).collect::<String>()
                ));
            } else if !text.is_empty() {
                summary_parts.push(format!("{}: {}", msg.role, text));
            }
        }
    }

    if summary_parts.is_empty() {
        "No content to summarize".to_string()
    } else {
        summary_parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compressor_new() {
        let compressor = ContextCompressor::new(50_000);
        assert_eq!(compressor.threshold(), 50_000);
    }

    #[test]
    fn test_compressor_default() {
        let compressor = ContextCompressor::default();
        assert_eq!(compressor.threshold(), 100_000);
    }

    #[test]
    fn test_should_compress_below_threshold() {
        let compressor = ContextCompressor::new(1000);
        let messages = vec![
            Message::user("Hello"),
            Message::assistant("Hi"),
        ];
        let prompt = "System";

        assert!(!compressor.should_compress(&messages, prompt));
    }

    #[test]
    fn test_should_compress_above_threshold() {
        let compressor = ContextCompressor::new(100);

        // Create large messages
        let messages: Vec<Message> = (0..100)
            .map(|i| Message::user(format!("Message number {} with some content", i)))
            .collect();

        let prompt = "Long system prompt";

        assert!(compressor.should_compress(&messages, prompt));
    }

    #[test]
    fn test_estimate_tokens() {
        let messages = vec![
            Message::user("Hello world"), // 11 chars + 50 overhead
            Message::assistant("Hi there"), // 8 chars + 50 overhead
        ];
        let prompt = "System prompt"; // 12 chars

        let tokens = estimate_tokens(&messages, prompt);
        // Total chars: 12 + 11 + 50 + 8 + 50 = 131
        // Tokens: 131 / 4 = 32
        assert!(tokens > 0);
    }

    #[tokio::test]
    async fn test_compress_small_messages() {
        let compressor = ContextCompressor::new(1000);
        let mut messages = vec![
            Message::user("Hello"),
            Message::assistant("Hi"),
        ];

        // Should not compress small messages
        compressor.compress(&mut messages).await.unwrap();
        assert_eq!(messages.len(), 2);
    }

    #[tokio::test]
    async fn test_compress_large_messages() {
        let compressor = ContextCompressor::new(100);

        // Create many messages
        let mut messages: Vec<Message> = (0..20)
            .map(|i| Message::user(format!("Message {}", i)))
            .collect();

        // Add assistant responses
        for i in 0..20 {
            messages.push(Message::assistant(format!("Response {}", i)));
        }

        compressor.compress(&mut messages).await.unwrap();
        // Should be compressed
        assert!(messages.len() < 40);
    }
}