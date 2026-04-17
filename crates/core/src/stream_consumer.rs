use anyhow::Result;
use async_trait::async_trait;

/// Trait for consuming streamed LLM responses.
///
/// Implementations receive streaming deltas from the query loop and
/// deliver them to the appropriate platform (e.g., by editing a message
/// in Telegram or posting incremental updates in Discord).
#[async_trait]
pub trait StreamConsumer: Send + Sync {
    /// Handle a text delta from the LLM stream.
    async fn on_text_delta(&self, delta: &str) -> Result<()>;

    /// Called when a tool execution starts.
    async fn on_tool_start(&self, tool_name: &str, args_preview: &str) -> Result<()>;

    /// Called when a tool execution completes.
    async fn on_tool_complete(&self, tool_name: &str, result_preview: &str) -> Result<()>;

    /// Called when an error occurs during tool execution.
    async fn on_tool_error(&self, tool_name: &str, error: &str) -> Result<()>;

    /// Flush any buffered content and finalize the response.
    async fn flush(&self) -> Result<()>;
}

/// No-op consumer that discards all deltas.
pub struct NoOpConsumer;

#[async_trait]
impl StreamConsumer for NoOpConsumer {
    async fn on_text_delta(&self, _delta: &str) -> Result<()> { Ok(()) }
    async fn on_tool_start(&self, _tool_name: &str, _args_preview: &str) -> Result<()> { Ok(()) }
    async fn on_tool_complete(&self, _tool_name: &str, _result_preview: &str) -> Result<()> { Ok(()) }
    async fn on_tool_error(&self, _tool_name: &str, _error: &str) -> Result<()> { Ok(()) }
    async fn flush(&self) -> Result<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_no_op_consumer() {
        let consumer = NoOpConsumer;
        consumer.on_text_delta("hello").await.unwrap();
        consumer.on_tool_start("test", "{}").await.unwrap();
        consumer.on_tool_complete("test", "ok").await.unwrap();
        consumer.on_tool_error("test", "failed").await.unwrap();
        consumer.flush().await.unwrap();
    }
}
