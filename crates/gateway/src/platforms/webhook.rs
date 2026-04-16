use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// Generic webhook platform adapter.
///
/// Receives messages via HTTP POST and delivers responses to a
/// configurable callback URL.
pub struct WebhookAdapter {
    webhook_url: Option<String>,
    client: reqwest::Client,
    received_messages: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
}

impl WebhookAdapter {
    pub fn new(config: &PlatformConfig) -> Self {
        Self {
            webhook_url: config.get_str("callback_url"),
            client: reqwest::Client::new(),
            received_messages: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }

    /// Process an incoming webhook payload.
    pub fn handle_incoming(&self, payload: serde_json::Value) {
        self.received_messages.lock().push(payload);
    }

    /// Get received messages (for testing).
    pub fn get_received(&self) -> Vec<serde_json::Value> {
        self.received_messages.lock().clone()
    }
}

#[async_trait]
impl PlatformAdapter for WebhookAdapter {
    fn name(&self) -> &str { "webhook" }

    async fn connect(&self) -> Result<()> {
        tracing::info!("Webhook adapter ready");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Webhook adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, _chat_id: &str, text: &str) -> Result<String> {
        if let Some(ref url) = self.webhook_url {
            self.client
                .post(url)
                .json(&serde_json::json!({ "text": text }))
                .send()
                .await?;
        }
        Ok(String::new())
    }

    async fn send_file(&self, _chat_id: &str, _path: &Path) -> Result<()> {
        Ok(())
    }

    async fn send_animation(&self, _chat_id: &str, _path: &Path) -> Result<()> {
        Ok(())
    }

    async fn send_voice(&self, _chat_id: &str, _path: &Path) -> Result<()> {
        Ok(())
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, text: &str) -> Result<()> {
        if let Some(ref url) = self.webhook_url {
            self.client
                .post(url)
                .json(&serde_json::json!({ "text": text, "edit": true }))
                .send()
                .await?;
        }
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    fn create_consumer(&self, chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(BufferingConsumer::new(
            chat_id.to_string(),
            Arc::new(Self {
                webhook_url: self.webhook_url.clone(),
                client: self.client.clone(),
                received_messages: Arc::clone(&self.received_messages),
            }),
        ))
    }

    fn message_format(&self) -> MessageFormat {
        MessageFormat::Plain
    }

    fn supports_edit_streaming(&self) -> bool {
        self.webhook_url.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_adapter_name() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({}),
        };
        let adapter = WebhookAdapter::new(&config);
        assert_eq!(adapter.name(), "webhook");
    }

    #[test]
    fn test_webhook_handle_incoming() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({}),
        };
        let adapter = WebhookAdapter::new(&config);

        adapter.handle_incoming(serde_json::json!({ "user": "test", "text": "hello" }));
        let received = adapter.get_received();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0]["text"], "hello");
    }
}
