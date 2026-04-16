//! Platform Adapter Trait
//!
//! Base trait for all messaging platform adapters.

use std::path::Path;
use std::sync::Arc;
use async_trait::async_trait;
use anyhow::Result;

use crate::event::{GatewayMessage, OutgoingMessage};
use crate::stream_consumer::StreamConsumer;

/// Platform adapter trait.
///
/// All messaging platforms must implement this trait for unified
/// message handling and sending.
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// Platform name (e.g., "telegram", "discord").
    fn name(&self) -> &str;

    /// Connect to the platform.
    async fn connect(&self) -> Result<()>;

    /// Disconnect from the platform.
    async fn disconnect(&self) -> Result<()>;

    /// Check if connected.
    fn is_connected(&self) -> bool;

    /// Send a text message.
    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String>;

    /// Send a message with full control.
    async fn send_full_message(&self, chat_id: &str, message: &OutgoingMessage) -> Result<String>;

    /// Send a file attachment.
    async fn send_file(&self, chat_id: &str, path: &Path, caption: Option<&str>) -> Result<String>;

    /// Send an animation/GIF.
    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<String>;

    /// Send a voice message.
    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<String>;

    /// Send a sticker.
    async fn send_sticker(&self, chat_id: &str, sticker_id: &str) -> Result<String>;

    /// Edit an existing message.
    async fn edit_message(&self, chat_id: &str, message_id: &str, text: &str) -> Result<()>;

    /// Delete a message.
    async fn delete_message(&self, chat_id: &str, message_id: &str) -> Result<()>;

    /// Send typing indicator.
    async fn is_typing(&self, chat_id: &str) -> Result<()>;

    /// Get stream consumer for this platform.
    fn stream_consumer(&self, chat_id: &str, message_id: &str) -> Arc<dyn StreamConsumer>;

    /// Get platform capabilities.
    fn capabilities(&self) -> PlatformCapabilities;

    /// Start receiving messages.
    async fn start_receiving(&self, tx: tokio::sync::mpsc::UnboundedSender<GatewayMessage>) -> Result<()>;

    /// Stop receiving messages.
    async fn stop_receiving(&self) -> Result<()>;
}

/// Platform capabilities.
#[derive(Debug, Clone, Default)]
pub struct PlatformCapabilities {
    /// Supports text messages.
    pub text: bool,

    /// Supports file attachments.
    pub files: bool,

    /// Supports images.
    pub images: bool,

    /// Supports animations/GIFs.
    pub animations: bool,

    /// Supports voice messages.
    pub voice: bool,

    /// Supports stickers.
    pub stickers: bool,

    /// Supports message editing.
    pub edit_messages: bool,

    /// Supports message deletion.
    pub delete_messages: bool,

    /// Supports typing indicator.
    pub typing_indicator: bool,

    /// Supports replies.
    pub replies: bool,

    /// Supports threads.
    pub threads: bool,

    /// Supports inline buttons.
    pub inline_buttons: bool,

    /// Supports markdown formatting.
    pub markdown: bool,

    /// Supports HTML formatting.
    pub html: bool,

    /// Maximum message length.
    pub max_message_length: Option<usize>,
}

impl PlatformCapabilities {
    /// Create capabilities for a basic text-only platform.
    pub fn text_only() -> Self {
        Self {
            text: true,
            max_message_length: Some(4096),
            ..Default::default()
        }
    }

    /// Create capabilities for a full-featured platform.
    pub fn full() -> Self {
        Self {
            text: true,
            files: true,
            images: true,
            animations: true,
            voice: true,
            stickers: true,
            edit_messages: true,
            delete_messages: true,
            typing_indicator: true,
            replies: true,
            threads: true,
            inline_buttons: true,
            markdown: true,
            max_message_length: Some(4096),
            ..Default::default()
        }
    }

    /// Check if platform supports a feature.
    pub fn supports(&self, feature: &str) -> bool {
        match feature {
            "text" => self.text,
            "files" => self.files,
            "images" => self.images,
            "animations" => self.animations,
            "voice" => self.voice,
            "stickers" => self.stickers,
            "edit" => self.edit_messages,
            "delete" => self.delete_messages,
            "typing" => self.typing_indicator,
            "replies" => self.replies,
            "threads" => self.threads,
            "buttons" => self.inline_buttons,
            "markdown" => self.markdown,
            "html" => self.html,
            _ => false,
        }
    }
}