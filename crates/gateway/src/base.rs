use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// Re-export StreamConsumer from h-core so the gateway uses the same trait
pub use h_core::StreamConsumer;

use std::path::PathBuf;

/// Message formatting style for a platform.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageFormat {
    Plain,
    #[default]
    Markdown,
    Html,
    Mrkdwn,
}

/// An incoming message from a platform user.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// Platform-specific user identifier.
    pub user_id: String,
    /// Platform-specific conversation/channel identifier.
    pub chat_id: String,
    /// Platform identifier (e.g., "telegram", "discord").
    pub platform: String,
    /// Message text content.
    pub text: String,
    /// Whether this is a direct message.
    pub is_dm: bool,
    /// Platform-specific message ID (for editing/deleting).
    pub message_id: Option<String>,
    /// Attachment file paths, if any.
    pub attachments: Vec<PathBuf>,
}

/// Abstract base trait for all messaging platform adapters.
///
/// Each platform (Telegram, Discord, Slack, etc.) implements this trait
/// to connect, receive messages, and send responses.
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// Platform name (e.g., "telegram", "discord").
    fn name(&self) -> &str;

    /// Connect to the platform.
    async fn connect(&self) -> Result<()>;

    /// Disconnect from the platform.
    async fn disconnect(&self) -> Result<()>;

    /// Send a text message to a chat.
    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String>;

    /// Send a file to a chat.
    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()>;

    /// Send an animation/GIF to a chat.
    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()>;

    /// Send a voice message to a chat.
    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()>;

    /// Send a sticker/emoji to a chat.
    async fn send_sticker(&self, chat_id: &str, sticker_id: &str) -> Result<()>;

    /// Edit an existing message.
    async fn edit_message(&self, chat_id: &str, message_id: &str, text: &str) -> Result<()>;

    /// Delete a message.
    async fn delete_message(&self, chat_id: &str, message_id: &str) -> Result<()>;

    /// Show typing indicator in a chat.
    async fn is_typing(&self, chat_id: &str) -> Result<()>;

    /// Create a new stream consumer for a specific chat.
    fn create_consumer(&self, chat_id: &str, message_id: Option<String>) -> Box<dyn StreamConsumer>;

    /// Message format supported by this platform.
    fn message_format(&self) -> MessageFormat {
        MessageFormat::Markdown
    }

    /// Whether this platform supports streaming edits.
    fn supports_edit_streaming(&self) -> bool {
        false
    }
}

/// Gateway configuration for a single platform.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlatformConfig {
    /// Whether this platform is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Platform-specific settings stored as a JSON object.
    #[serde(flatten)]
    pub settings: serde_json::Value,
}

impl PlatformConfig {
    /// Get a string setting by key.
    pub fn get_str(&self, key: &str) -> Option<String> {
        self.settings.get(key).and_then(|v| v.as_str()).map(String::from)
    }

    /// Get a bool setting by key.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.settings.get(key).and_then(|v| v.as_bool())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_config_get_str() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "bot_token": "abc123",
                "webhook_port": 8443
            }),
        };
        assert_eq!(config.get_str("bot_token"), Some("abc123".to_string()));
        assert_eq!(config.get_str("missing"), None);
    }

    #[test]
    fn test_platform_config_get_bool() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "use_webhook": true
            }),
        };
        assert_eq!(config.get_bool("use_webhook"), Some(true));
        assert_eq!(config.get_bool("missing"), None);
    }

    #[test]
    fn test_message_format_defaults() {
        assert_eq!(MessageFormat::default(), MessageFormat::Markdown);
    }
}
