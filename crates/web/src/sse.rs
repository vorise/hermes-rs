//! SSE (Server-Sent Events) Support
//!
//! Real-time streaming updates for Hermes Web UI.

use tokio::sync::broadcast;
use serde::Serialize;

/// SSE event types.
#[derive(Debug, Clone, Serialize)]
pub struct SseEvent {
    /// Event type.
    pub event: String,

    /// Event data.
    pub data: String,
}

/// SSE broadcaster for distributing events.
pub struct SseBroadcaster {
    /// Broadcast channel sender.
    tx: broadcast::Sender<SseEvent>,

    /// Channel capacity.
    capacity: usize,
}

impl SseBroadcaster {
    /// Create new broadcaster.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            capacity,
        }
    }

    /// Broadcast an event.
    pub fn broadcast(&self, event_type: &str, data: &str) {
        let event = SseEvent {
            event: event_type.to_string(),
            data: data.to_string(),
        };

        // Ignore send errors (no receivers)
        let _ = self.tx.send(event);
    }

    /// Broadcast a JSON event.
    pub fn broadcast_json<T: Serialize>(&self, event_type: &str, data: &T) {
        let json = serde_json::to_string(data).unwrap_or_default();
        self.broadcast(event_type, &json);
    }

    /// Subscribe to events.
    pub fn subscribe(&self) -> broadcast::Receiver<SseEvent> {
        self.tx.subscribe()
    }

    /// Get capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get subscriber count.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

/// SSE stream message types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamMessageType {
    /// Text delta.
    TextDelta,

    /// Tool start.
    ToolStart,

    /// Tool complete.
    ToolComplete,

    /// Error.
    Error,

    /// Done.
    Done,

    /// Heartbeat.
    Heartbeat,
}

impl StreamMessageType {
    /// Get event name for SSE.
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::TextDelta => "text",
            Self::ToolStart => "tool_start",
            Self::ToolComplete => "tool_complete",
            Self::Error => "error",
            Self::Done => "done",
            Self::Heartbeat => "heartbeat",
        }
    }
}

/// Stream message.
#[derive(Debug, Clone)]
pub struct StreamMessage {
    /// Message type.
    pub msg_type: StreamMessageType,

    /// Message content.
    pub content: String,

    /// Message ID.
    pub message_id: Option<String>,
}

impl StreamMessage {
    /// Create text delta.
    pub fn text_delta(content: String) -> Self {
        Self {
            msg_type: StreamMessageType::TextDelta,
            content,
            message_id: None,
        }
    }

    /// Create tool start.
    pub fn tool_start(tool_name: &str, args: &str) -> Self {
        Self {
            msg_type: StreamMessageType::ToolStart,
            content: format!("{}:{}", tool_name, args),
            message_id: None,
        }
    }

    /// Create tool complete.
    pub fn tool_complete(tool_name: &str, result: &str) -> Self {
        Self {
            msg_type: StreamMessageType::ToolComplete,
            content: format!("{}:{}", tool_name, result),
            message_id: None,
        }
    }

    /// Create error.
    pub fn error(message: String) -> Self {
        Self {
            msg_type: StreamMessageType::Error,
            content: message,
            message_id: None,
        }
    }

    /// Create done signal.
    pub fn done() -> Self {
        Self {
            msg_type: StreamMessageType::Done,
            content: String::new(),
            message_id: None,
        }
    }

    /// Create heartbeat.
    pub fn heartbeat() -> Self {
        Self {
            msg_type: StreamMessageType::Heartbeat,
            content: String::new(),
            message_id: None,
        }
    }

    /// Set message ID.
    pub fn with_message_id(mut self, id: String) -> Self {
        self.message_id = Some(id);
        self
    }

    /// Format as SSE data.
    pub fn to_sse_format(&self) -> String {
        let event = self.msg_type.event_name();
        if self.content.is_empty() {
            format!("event: {}\ndata: \n\n", event)
        } else {
            format!("event: {}\ndata: {}\n\n", event, self.content)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_broadcaster_new() {
        let broadcaster = SseBroadcaster::new(100);
        assert_eq!(broadcaster.capacity(), 100);
        assert_eq!(broadcaster.subscriber_count(), 0);
    }

    #[test]
    fn test_sse_broadcaster_broadcast() {
        let broadcaster = SseBroadcaster::new(100);
        let mut rx = broadcaster.subscribe();

        broadcaster.broadcast("test", "data");

        let event = rx.try_recv().unwrap();
        assert_eq!(event.event, "test");
        assert_eq!(event.data, "data");
    }

    #[test]
    fn test_stream_message_text_delta() {
        let msg = StreamMessage::text_delta("hello".to_string());
        assert_eq!(msg.msg_type, StreamMessageType::TextDelta);
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn test_stream_message_tool() {
        let msg = StreamMessage::tool_start("read_file", "/path");
        assert_eq!(msg.msg_type, StreamMessageType::ToolStart);
        assert!(msg.content.contains("read_file"));
    }

    #[test]
    fn test_stream_message_done() {
        let msg = StreamMessage::done();
        assert_eq!(msg.msg_type, StreamMessageType::Done);
    }

    #[test]
    fn test_stream_message_to_sse_format() {
        let msg = StreamMessage::text_delta("hello".to_string());
        let sse = msg.to_sse_format();
        assert!(sse.contains("event: text"));
        assert!(sse.contains("data: hello"));
    }

    #[test]
    fn test_stream_message_with_id() {
        let msg = StreamMessage::text_delta("hello".to_string())
            .with_message_id("msg-1".to_string());
        assert_eq!(msg.message_id, Some("msg-1".to_string()));
    }

    #[test]
    fn test_message_type_event_name() {
        assert_eq!(StreamMessageType::TextDelta.event_name(), "text");
        assert_eq!(StreamMessageType::ToolStart.event_name(), "tool_start");
        assert_eq!(StreamMessageType::Done.event_name(), "done");
    }
}