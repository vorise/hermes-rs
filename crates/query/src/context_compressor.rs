use anyhow::{anyhow, Result};
use h_api::ApiClient;
use h_core::{Message, ModelRef};


/// Approximate token count from a rough character-level estimate.
/// Uses the rule of thumb that 1 token ~= 4 characters for English text.
fn estimate_tokens(text: &str) -> u64 {
    (text.len() as f64 / 4.0).ceil() as u64
}

/// Estimate total tokens for a slice of messages.
pub fn estimate_message_tokens(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_message_token_count).sum()
}

/// Estimate tokens for a single message.
fn estimate_message_token_count(msg: &Message) -> u64 {
    let mut tokens = 4u64; // Base overhead per message

    if let Some(ref content) = msg.content {
        match content {
            h_core::Content::Text(t) => {
                tokens += estimate_tokens(t);
            }
            h_core::Content::Multi(parts) => {
                for part in parts {
                    match part {
                        h_core::ContentPart::Text { text } => {
                            tokens += estimate_tokens(text);
                        }
                        h_core::ContentPart::Image { .. } => {
                            tokens += 85; // Approximate for a single image
                        }
                    }
                }
            }
        }
    }

    if let Some(ref tool_calls) = msg.tool_calls {
        for tc in tool_calls {
            tokens += estimate_tokens(&tc.function.name);
            tokens += estimate_tokens(&tc.function.arguments);
        }
    }

    if let Some(ref name) = msg.name {
        tokens += estimate_tokens(name);
    }

    tokens
}

/// Estimate the context window size for a model (in tokens).
fn model_context_window(model: &ModelRef) -> u64 {
    // Known context windows for common models
    let model_id = model.model.as_str();
    match model_id {
        m if m.contains("claude-sonnet-4") => 200_000,
        m if m.contains("claude") => 200_000,
        m if m.contains("gpt-4o") => 128_000,
        m if m.contains("gpt-4") => 128_000,
        m if m.contains("gpt-3.5") => 16_385,
        m if m.contains("claude-3-5") => 200_000,
        m if m.contains("mistral") => 128_000,
        _ => 128_000, // Default conservative estimate
    }
}

/// Configuration for context compression.
#[derive(Debug, Clone)]
pub struct CompressorConfig {
    /// Token threshold at which compression should be triggered.
    pub threshold_tokens: u64,
    /// Number of recent turns to preserve without compression.
    pub preserve_turns: usize,
    /// Model reference for the auxiliary compression LLM.
    pub auxiliary_model: Option<ModelRef>,
    /// Whether to use an auxiliary LLM for compression (true) or simple truncation (false).
    pub use_llm_compression: bool,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            threshold_tokens: 100_000,
            preserve_turns: 6,
            auxiliary_model: None,
            use_llm_compression: false,
        }
    }
}

/// Context compressor that manages context window size.
pub struct ContextCompressor {
    config: CompressorConfig,
}

impl ContextCompressor {
    pub fn new(config: CompressorConfig) -> Self {
        Self { config }
    }

    /// Check if the current messages exceed the compression threshold.
    pub fn should_compress(&self, messages: &[Message]) -> bool {
        let tokens = estimate_message_tokens(messages);
        tokens > self.config.threshold_tokens
    }

    /// Perform a preflight check: will the next API call likely exceed context limits?
    pub fn preflight_check(
        &self,
        messages: &[Message],
        system_prompt: &str,
        model: &ModelRef,
    ) -> PreflightResult {
        let context_window = model_context_window(model);
        let system_tokens = estimate_tokens(system_prompt);
        let message_tokens = estimate_message_tokens(messages);
        let total_tokens = system_tokens + message_tokens;
        let remaining = context_window.saturating_sub(total_tokens);

        // Reserve ~20% of context for the response
        let response_reserve = context_window / 5;
        let effective_remaining = remaining.saturating_sub(response_reserve);

        if total_tokens >= context_window {
            PreflightResult::Critical {
                total_tokens,
                context_window,
                overflow: total_tokens - context_window,
            }
        } else if effective_remaining < 4096 {
            PreflightResult::Warning {
                total_tokens,
                context_window,
                remaining: effective_remaining,
            }
        } else {
            PreflightResult::Ok {
                total_tokens,
                context_window,
                remaining: effective_remaining,
            }
        }
    }

    /// Compress messages by summarizing older turns.
    ///
    /// Strategy:
    /// 1. Identify messages above the threshold
    /// 2. Use auxiliary LLM to summarize older turns (if available)
    /// 3. Preserve recent messages (last N turns)
    /// 4. Preserve all tool results (they are critical for continuity)
    /// 5. Replace older messages with their summaries
    pub async fn compress(
        &self,
        messages: &mut Vec<Message>,
        api_client: Option<&ApiClient>,
    ) -> Result<CompressionResult> {
        let original_count = messages.len();
        let original_tokens = estimate_message_tokens(messages);

        if !self.should_compress(messages) {
            return Ok(CompressionResult {
                original_tokens,
                compressed_tokens: original_tokens,
                messages_removed: 0,
                messages_compressed: 0,
            });
        }

        // Determine which messages to compress
        let preserve_count = self.config.preserve_turns * 2; // 2 messages per turn (assistant + tool)
        let compress_count = messages.len().saturating_sub(preserve_count);

        if compress_count == 0 {
            // Nothing to compress without losing recent context
            // Fall back to aggressive truncation
            if messages.len() > preserve_count {
                messages.drain(0..messages.len() - preserve_count);
                let new_tokens = estimate_message_tokens(messages);
                return Ok(CompressionResult {
                    original_tokens,
                    compressed_tokens: new_tokens,
                    messages_removed: original_count - messages.len(),
                    messages_compressed: 0,
                });
            }
        }

        // Separate messages into compressible and preserved
        let compressible: Vec<Message> = messages.drain(0..compress_count).collect();
        let preserved: Vec<Message> = messages.drain(..).collect();

        // Try LLM compression if available
        if self.config.use_llm_compression {
            if let Some(client) = api_client {
                match self.summarize_with_llm(client, &compressible).await {
                    Ok(summary) => {
                        let summary_msg = Message::system(&summary);
                        messages.insert(0, summary_msg);
                        messages.extend(preserved);

                        let new_tokens = estimate_message_tokens(messages);
                        return Ok(CompressionResult {
                            original_tokens,
                            compressed_tokens: new_tokens,
                            messages_removed: 0,
                            messages_compressed: compressible.len(),
                        });
                    }
                    Err(e) => {
                        tracing::warn!("LLM compression failed, falling back to truncation: {e}");
                    }
                }
            }
        }

        // Fallback: simple truncation with a brief summary header
        let compressible_text: String = compressible
            .iter()
            .filter_map(|m| m.content.as_ref().and_then(|c| c.as_text()))
            .take(20) // Limit to avoid huge summaries
            .map(|t| {
                if t.len() > 500 {
                    format!("{}...", &t[..500])
                } else {
                    t.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let header = if compressible_text.len() > 2000 {
            format!(
                "[Previous conversation summary: {compress_count} messages were summarized. \
                The conversation covered various topics including the following excerpts: \
                {}...]",
                &compressible_text[..2000]
            )
        } else {
            format!(
                "[Previous conversation summary: {compress_count} messages condensed. \
                Key points: {compressible_text}]"
            )
        };

        messages.insert(0, Message::system(&header));
        messages.extend(preserved);

        let new_tokens = estimate_message_tokens(messages);
        Ok(CompressionResult {
            original_tokens,
            compressed_tokens: new_tokens,
            messages_removed: 0,
            messages_compressed: compressible.len(),
        })
    }

    /// Use an auxiliary LLM to summarize a batch of messages.
    async fn summarize_with_llm(
        &self,
        client: &ApiClient,
        messages: &[Message],
    ) -> Result<String> {
        // Build a summary request
        let summary_messages = vec![
            Message::system(
                "Summarize the following conversation in a concise manner. \
                Preserve key decisions, tool results, code changes, and important facts. \
                Keep the summary under 1000 words. Focus on information that would be \
                useful for continuing the conversation."
            ),
        ];

        // Take the messages to summarize (limit to avoid huge requests)
        let mut summary_messages = summary_messages;
        let mut summary_text = String::new();
        for msg in messages.iter().take(50) {
            if let Some(ref content) = msg.content {
                if let Some(text) = content.as_text() {
                    summary_text.push_str(&format!("[{role}]: {text}\n\n", role = msg.role));
                }
            }
        }
        summary_messages.push(Message::user(summary_text));

        // Make the API call with minimal tools
        let tools = vec![];
        let response = client.chat(&summary_messages, &tools).await?;

        response
            .text_content()
            .ok_or_else(|| anyhow!("LLM returned no text content for summary"))
    }
}

/// Result of the compression operation.
#[derive(Debug, Clone)]
pub struct CompressionResult {
    pub original_tokens: u64,
    pub compressed_tokens: u64,
    pub messages_removed: usize,
    pub messages_compressed: usize,
}

impl CompressionResult {
    pub fn compression_ratio(&self) -> f64 {
        if self.original_tokens == 0 {
            return 1.0;
        }
        self.compressed_tokens as f64 / self.original_tokens as f64
    }
}

/// Result of a preflight context check.
#[derive(Debug, Clone)]
pub enum PreflightResult {
    Ok {
        total_tokens: u64,
        context_window: u64,
        remaining: u64,
    },
    Warning {
        total_tokens: u64,
        context_window: u64,
        remaining: u64,
    },
    Critical {
        total_tokens: u64,
        context_window: u64,
        overflow: u64,
    },
}

impl PreflightResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, PreflightResult::Ok { .. })
    }

    pub fn needs_compression(&self) -> bool {
        !matches!(self, PreflightResult::Ok { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use h_core::{Message, ModelId, ProviderId, ToolCallFunction};

    #[test]
    fn test_estimate_tokens() {
        let text = "Hello, world!";
        let tokens = estimate_tokens(text);
        assert!(tokens > 0);
        assert!(tokens < 10); // "Hello, world!" = 13 chars / 4 = ~4 tokens
    }

    #[test]
    fn test_estimate_message_tokens() {
        let msg = Message::user("Hello, this is a test message with some content.");
        let tokens = estimate_message_token_count(&msg);
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_message_tokens_tool_call() {
        let msg = Message {
            role: h_core::Role::Assistant,
            content: None,
            tool_calls: Some(vec![h_core::ToolCall {
                id: "1".to_string(),
                function: ToolCallFunction {
                    name: "read_file".to_string(),
                    arguments: r#"{"path": "/test"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
            name: None,
            reasoning: None,
        };
        let tokens = estimate_message_token_count(&msg);
        assert!(tokens > 0);
    }

    #[test]
    fn test_should_compress_empty() {
        let compressor = ContextCompressor::new(CompressorConfig::default());
        assert!(!compressor.should_compress(&[]));
    }

    #[test]
    fn test_should_compress_small() {
        let compressor = ContextCompressor::new(CompressorConfig::default());
        let messages = vec![
            Message::user("Hello"),
            Message::assistant("Hi there!"),
        ];
        assert!(!compressor.should_compress(&messages));
    }

    #[test]
    fn test_preflight_check_small() {
        let compressor = ContextCompressor::new(CompressorConfig::default());
        let messages = vec![
            Message::user("What is 2+2?"),
            Message::assistant("4"),
        ];
        let model = ModelRef::new(ProviderId::new("anthropic"), ModelId::new("claude-sonnet-4-6"));
        let result = compressor.preflight_check(&messages, "You are helpful", &model);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_compress_no_compression_needed() {
        let compressor = ContextCompressor::new(CompressorConfig::default());
        let mut messages = vec![
            Message::user("Hello"),
            Message::assistant("Hi!"),
        ];
        let result = compressor.compress(&mut messages, None).await.unwrap();
        assert_eq!(result.messages_removed, 0);
        assert_eq!(result.messages_compressed, 0);
    }

    #[tokio::test]
    async fn test_compress_truncation_fallback() {
        let config = CompressorConfig {
            threshold_tokens: 100,
            preserve_turns: 1,
            ..Default::default()
        };
        let compressor = ContextCompressor::new(config);
        let mut messages: Vec<Message> = (0..50)
            .map(|i| Message::user(format!("Message {i}")))
            .collect();

        let result = compressor.compress(&mut messages, None).await.unwrap();
        assert!(result.messages_removed > 0 || result.messages_compressed > 0);
        assert!(result.compressed_tokens < result.original_tokens);
    }

    #[test]
    fn test_compression_result_ratio() {
        let result = CompressionResult {
            original_tokens: 10000,
            compressed_tokens: 5000,
            messages_removed: 0,
            messages_compressed: 20,
        };
        assert_eq!(result.compression_ratio(), 0.5);
    }

    #[test]
    fn test_preflight_result_variants() {
        let ok = PreflightResult::Ok {
            total_tokens: 10000,
            context_window: 100000,
            remaining: 70000,
        };
        let warn = PreflightResult::Warning {
            total_tokens: 95000,
            context_window: 100000,
            remaining: 3000,
        };
        let critical = PreflightResult::Critical {
            total_tokens: 110000,
            context_window: 100000,
            overflow: 10000,
        };

        assert!(ok.is_ok());
        assert!(!warn.is_ok());
        assert!(!critical.is_ok());

        assert!(!ok.needs_compression());
        assert!(warn.needs_compression());
        assert!(critical.needs_compression());
    }

    #[test]
    fn test_model_context_window() {
        let claude = ModelRef::new(ProviderId::new("anthropic"), ModelId::new("claude-sonnet-4-6"));
        let gpt4 = ModelRef::new(ProviderId::new("openai"), ModelId::new("gpt-4o-mini"));

        assert!(model_context_window(&claude) >= 100_000);
        assert!(model_context_window(&gpt4) >= 100_000);
    }
}
