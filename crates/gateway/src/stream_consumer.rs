//! Stream Consumer
//!
//! Consumes streaming output from the agent and formats for platforms.

use std::sync::Arc;
use async_trait::async_trait;
use anyhow::Result;
use parking_lot::RwLock;
use tokio::sync::mpsc;

use crate::event::GatewayEvent;

/// Stream consumer trait.
///
/// Receives streaming updates from the agent and processes them
/// for display on a specific platform.
#[async_trait]
pub trait StreamConsumer: Send + Sync {
    /// Handle streaming text delta.
    async fn on_text_delta(&self, delta: &str) -> Result<()>;

    /// Handle tool execution start.
    async fn on_tool_start(&self, tool_name: &str, args_preview: &str) -> Result<()>;

    /// Handle tool execution complete.
    async fn on_tool_complete(&self, tool_name: &str, result_preview: &str) -> Result<()>;

    /// Flush accumulated content.
    async fn flush(&self) -> Result<()>;

    /// Get accumulated content.
    fn get_content(&self) -> String;

    /// Clear accumulated content.
    fn clear(&self);
}

/// Gateway stream consumer.
///
/// Accumulates streaming content and sends events to the gateway.
pub struct GatewayStreamConsumer {
    /// Platform identifier.
    platform: String,

    /// Chat identifier.
    chat_id: String,

    /// Message identifier being streamed.
    message_id: String,

    /// Accumulated text content.
    content: Arc<RwLock<String>>,

    /// Event sender.
    event_tx: mpsc::UnboundedSender<GatewayEvent>,

    /// Current tool being executed.
    current_tool: Arc<RwLock<Option<String>>>,
}

impl GatewayStreamConsumer {
    /// Create new stream consumer.
    pub fn new(
        platform: impl Into<String>,
        chat_id: impl Into<String>,
        message_id: impl Into<String>,
        event_tx: mpsc::UnboundedSender<GatewayEvent>,
    ) -> Self {
        Self {
            platform: platform.into(),
            chat_id: chat_id.into(),
            message_id: message_id.into(),
            content: Arc::new(RwLock::new(String::new())),
            event_tx,
            current_tool: Arc::new(RwLock::new(None)),
        }
    }

    /// Send event to gateway.
    fn send_event(&self, event: GatewayEvent) {
        if self.event_tx.send(event).is_err() {
            tracing::warn!("Failed to send gateway event");
        }
    }
}

#[async_trait]
impl StreamConsumer for GatewayStreamConsumer {
    async fn on_text_delta(&self, delta: &str) -> Result<()> {
        // Accumulate content
        {
            let mut content = self.content.write();
            content.push_str(delta);
        }

        // Send stream delta event
        self.send_event(GatewayEvent::StreamDelta {
            platform: self.platform.clone(),
            chat_id: self.chat_id.clone(),
            message_id: self.message_id.clone(),
            delta: delta.to_string(),
        });

        Ok(())
    }

    async fn on_tool_start(&self, tool_name: &str, args_preview: &str) -> Result<()> {
        // Track current tool
        {
            let mut tool = self.current_tool.write();
            *tool = Some(tool_name.to_string());
        }

        // Send tool start event
        self.send_event(GatewayEvent::ToolStart {
            platform: self.platform.clone(),
            chat_id: self.chat_id.clone(),
            tool_name: tool_name.to_string(),
            args_preview: args_preview.chars().take(100).collect(),
        });

        Ok(())
    }

    async fn on_tool_complete(&self, tool_name: &str, result_preview: &str) -> Result<()> {
        // Clear current tool
        {
            let mut tool = self.current_tool.write();
            *tool = None;
        }

        // Send tool complete event
        self.send_event(GatewayEvent::ToolComplete {
            platform: self.platform.clone(),
            chat_id: self.chat_id.clone(),
            tool_name: tool_name.to_string(),
            result_preview: result_preview.chars().take(200).collect(),
        });

        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        // Send final message
        let content = self.get_content();

        if !content.is_empty() {
            self.send_event(GatewayEvent::OutgoingMessage {
                platform: self.platform.clone(),
                chat_id: self.chat_id.clone(),
                message: crate::event::OutgoingMessage::text(&content),
            });
        }

        // Clear accumulator
        self.clear();

        Ok(())
    }

    fn get_content(&self) -> String {
        self.content.read().clone()
    }

    fn clear(&self) {
        let mut content = self.content.write();
        content.clear();
    }
}

/// Buffered stream consumer for testing.
pub struct BufferedStreamConsumer {
    /// Accumulated content.
    content: Arc<RwLock<String>>,

    /// Tool starts.
    tool_starts: Arc<RwLock<Vec<(String, String)>>>,
}

impl BufferedStreamConsumer {
    /// Create new buffered consumer.
    pub fn new() -> Self {
        Self {
            content: Arc::new(RwLock::new(String::new())),
            tool_starts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get tool starts.
    pub fn get_tool_starts(&self) -> Vec<(String, String)> {
        self.tool_starts.read().clone()
    }
}

impl Default for BufferedStreamConsumer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StreamConsumer for BufferedStreamConsumer {
    async fn on_text_delta(&self, delta: &str) -> Result<()> {
        let mut content = self.content.write();
        content.push_str(delta);
        Ok(())
    }

    async fn on_tool_start(&self, tool_name: &str, args_preview: &str) -> Result<()> {
        let mut starts = self.tool_starts.write();
        starts.push((tool_name.to_string(), args_preview.to_string()));
        Ok(())
    }

    async fn on_tool_complete(&self, _tool_name: &str, _result_preview: &str) -> Result<()> {
        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        Ok(())
    }

    fn get_content(&self) -> String {
        self.content.read().clone()
    }

    fn clear(&self) {
        let mut content = self.content.write();
        content.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_buffered_consumer() {
        let consumer = BufferedStreamConsumer::new();

        consumer.on_text_delta("Hello ").await.unwrap();
        consumer.on_text_delta("World").await.unwrap();

        assert_eq!(consumer.get_content(), "Hello World");

        consumer.clear();
        assert_eq!(consumer.get_content(), "");
    }

    #[tokio::test]
    async fn test_buffered_consumer_tool_start() {
        let consumer = BufferedStreamConsumer::new();

        consumer.on_tool_start("read_file", "/path/to/file").await.unwrap();
        consumer.on_tool_start("write_file", "/path/to/output").await.unwrap();

        let starts = consumer.get_tool_starts();
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0].0, "read_file");
    }

    #[test]
    fn test_gateway_stream_consumer_new() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let consumer = GatewayStreamConsumer::new("telegram", "chat1", "msg1", tx);
        assert_eq!(consumer.platform, "telegram");
        assert!(consumer.get_content().is_empty());
    }
}