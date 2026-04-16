//! Gateway Events
//!
//! Events flowing through the gateway system.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Gateway event types.
#[derive(Debug, Clone)]
pub enum GatewayEvent {
    /// Incoming message from a platform.
    IncomingMessage(GatewayMessage),

    /// Outgoing message to a platform.
    OutgoingMessage {
        platform: String,
        chat_id: String,
        message: OutgoingMessage,
    },

    /// Streaming text delta.
    StreamDelta {
        platform: String,
        chat_id: String,
        message_id: String,
        delta: String,
    },

    /// Tool execution started.
    ToolStart {
        platform: String,
        chat_id: String,
        tool_name: String,
        args_preview: String,
    },

    /// Tool execution completed.
    ToolComplete {
        platform: String,
        chat_id: String,
        tool_name: String,
        result_preview: String,
    },

    /// Message edit request.
    EditMessage {
        platform: String,
        chat_id: String,
        message_id: String,
        text: String,
    },

    /// Typing indicator.
    TypingIndicator {
        platform: String,
        chat_id: String,
    },

    /// Platform connected.
    PlatformConnected {
        platform: String,
    },

    /// Platform disconnected.
    PlatformDisconnected {
        platform: String,
    },

    /// Error occurred.
    Error {
        platform: String,
        chat_id: String,
        error: String,
    },

    /// Session created.
    SessionCreated {
        platform: String,
        chat_id: String,
        session_id: String,
    },

    /// Session ended.
    SessionEnded {
        platform: String,
        chat_id: String,
    },

    /// Cron job triggered.
    CronTrigger {
        job_id: String,
        prompt: String,
        delivery: crate::cron::DeliveryTarget,
    },
}

/// Incoming message from a platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    /// Platform identifier (e.g., "telegram", "discord").
    pub platform: String,

    /// Chat/channel/user identifier.
    pub chat_id: String,

    /// Message identifier from platform.
    pub message_id: String,

    /// User who sent the message.
    pub user_id: String,

    /// User display name.
    pub user_name: Option<String>,

    /// Message text content.
    pub text: String,

    /// Attached files (if any).
    #[serde(default)]
    pub attachments: Vec<Attachment>,

    /// Message timestamp.
    pub timestamp: u64,

    /// Is this a reply to another message?
    #[serde(default)]
    pub reply_to: Option<String>,

    /// Is this an edit of an existing message?
    #[serde(default)]
    pub is_edit: bool,

    /// Is this a private/direct message?
    #[serde(default)]
    pub is_private: bool,

    /// Thread ID (for platforms with threads).
    #[serde(default)]
    pub thread_id: Option<String>,
}

/// File attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// File name.
    pub name: String,

    /// MIME type.
    #[serde(default)]
    pub mime_type: Option<String>,

    /// File size in bytes.
    #[serde(default)]
    pub size: Option<u64>,

    /// URL or path to file.
    pub url: Option<String>,

    /// Local path if downloaded.
    #[serde(default)]
    pub local_path: Option<PathBuf>,
}

/// Outgoing message to a platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingMessage {
    /// Text content.
    pub text: String,

    /// Files to attach.
    #[serde(default)]
    pub attachments: Vec<PathBuf>,

    /// Is this a reply?
    #[serde(default)]
    pub reply_to: Option<String>,

    /// Use animation/GIF?
    #[serde(default)]
    pub animation: Option<PathBuf>,

    /// Use voice message?
    #[serde(default)]
    pub voice: Option<PathBuf>,

    /// Sticker ID.
    #[serde(default)]
    pub sticker: Option<String>,
}

impl GatewayMessage {
    /// Create a new gateway message.
    pub fn new(
        platform: impl Into<String>,
        chat_id: impl Into<String>,
        message_id: impl Into<String>,
        user_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            platform: platform.into(),
            chat_id: chat_id.into(),
            message_id: message_id.into(),
            user_id: user_id.into(),
            user_name: None,
            text: text.into(),
            attachments: Vec::new(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            reply_to: None,
            is_edit: false,
            is_private: false,
            thread_id: None,
        }
    }

    /// Get the unique key for this message source.
    pub fn source_key(&self) -> String {
        format!("{}:{}", self.platform, self.chat_id)
    }

    /// Check if message contains attachments.
    pub fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }
}

impl OutgoingMessage {
    /// Create simple text message.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            attachments: Vec::new(),
            reply_to: None,
            animation: None,
            voice: None,
            sticker: None,
        }
    }

    /// Create message with file attachment.
    pub fn with_file(text: impl Into<String>, file: PathBuf) -> Self {
        Self {
            text: text.into(),
            attachments: vec![file],
            reply_to: None,
            animation: None,
            voice: None,
            sticker: None,
        }
    }

    /// Create reply message.
    pub fn reply(text: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            attachments: Vec::new(),
            reply_to: Some(to.into()),
            animation: None,
            voice: None,
            sticker: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_message_new() {
        let msg = GatewayMessage::new("telegram", "chat1", "msg1", "user1", "Hello");
        assert_eq!(msg.platform, "telegram");
        assert_eq!(msg.chat_id, "chat1");
        assert_eq!(msg.text, "Hello");
        assert!(!msg.is_private);
    }

    #[test]
    fn test_gateway_message_source_key() {
        let msg = GatewayMessage::new("telegram", "chat1", "msg1", "user1", "Hello");
        assert_eq!(msg.source_key(), "telegram:chat1");
    }

    #[test]
    fn test_outgoing_message_text() {
        let msg = OutgoingMessage::text("Hello");
        assert_eq!(msg.text, "Hello");
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn test_outgoing_message_reply() {
        let msg = OutgoingMessage::reply("Reply", "msg1");
        assert_eq!(msg.reply_to, Some("msg1".to_string()));
    }

    #[test]
    fn test_attachment_new() {
        let att = Attachment {
            name: "test.png".to_string(),
            mime_type: Some("image/png".to_string()),
            size: Some(1024),
            url: Some("http://example.com/test.png".to_string()),
            local_path: None,
        };
        assert_eq!(att.name, "test.png");
    }

    #[test]
    fn test_gateway_event_incoming() {
        let msg = GatewayMessage::new("telegram", "chat1", "msg1", "user1", "Hello");
        let event = GatewayEvent::IncomingMessage(msg);
        assert!(matches!(event, GatewayEvent::IncomingMessage(_)));
    }
}