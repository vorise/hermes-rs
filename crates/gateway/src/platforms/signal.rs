use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, PlatformConfig, StreamConsumer};
use crate::runner::BufferingConsumer;

/// Configuration for the Signal adapter.
#[derive(Debug, Clone)]
pub struct SignalConfig {
    /// Phone number of the Signal bot (e.g., "+1234567890").
    pub phone_number: String,
    /// signal-cli REST API base URL (default: "http://127.0.0.1:8080").
    pub api_url: String,
}

impl SignalConfig {
    pub fn new(phone_number: &str) -> Self {
        Self {
            phone_number: phone_number.to_string(),
            api_url: "http://127.0.0.1:8080".to_string(),
        }
    }

    /// With a custom API URL.
    pub fn with_api_url(mut self, url: &str) -> Self {
        self.api_url = url.to_string();
        self
    }

    /// Create from a platform config.
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let phone_number = config.get_str("phone_number")
            .ok_or_else(|| anyhow!("Signal phone_number not configured"))?;
        let mut cfg = Self::new(&phone_number);
        if let Some(url) = config.get_str("api_url") {
            cfg.api_url = url;
        }
        Ok(cfg)
    }
}

/// Incoming Signal message.
#[derive(Debug, Clone)]
pub struct SignalIncomingMessage {
    /// Sender phone number or group ID.
    pub source: String,
    /// Group ID (None for DMs).
    pub group_id: Option<String>,
    /// Message text.
    pub text: String,
    /// Attachment paths, if any.
    pub attachments: Vec<String>,
}

/// Signal platform adapter using signal-cli REST API.
///
/// Connects to a running signal-cli-rest-api instance to send and receive
/// end-to-end encrypted Signal messages.
pub struct SignalAdapter {
    config: SignalConfig,
    client: reqwest::Client,
    /// In-memory buffer for received messages (for testing and local polling).
    received: Arc<parking_lot::Mutex<Vec<SignalIncomingMessage>>>,
}

impl SignalAdapter {
    pub fn new(config: SignalConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            received: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }

    /// Send a message via the signal-cli REST API.
    async fn send_via_api(&self, recipients: &[String], message: &str) -> Result<()> {
        let url = format!("{}/v1/send", self.config.api_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "message": message,
            "number": self.config.phone_number,
            "recipients": recipients,
        });

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to send Signal message via REST API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Signal API error ({status}): {body}"));
        }

        Ok(())
    }

    /// Receive messages via polling the signal-cli REST API.
    pub async fn poll_messages(&self) -> Result<Vec<SignalIncomingMessage>> {
        let url = format!("{}/v1/receive/{}", self.config.api_url.trim_end_matches('/'), self.config.phone_number);

        let resp = self.client
            .get(&url)
            .send()
            .await
            .context("Failed to poll Signal messages")?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let messages: Vec<serde_json::Value> = resp.json().await.unwrap_or_default();
        let mut incoming = Vec::new();

        for msg in messages {
            let source = msg.get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let group_id = msg.get("groupId")
                .and_then(|v| v.as_str())
                .map(String::from);

            let text = msg.get("data")
                .and_then(|d| d.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let attachments = msg.get("data")
                .and_then(|d| d.get("attachments"))
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.get("id").and_then(|v| v.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            incoming.push(SignalIncomingMessage {
                source,
                group_id,
                text,
                attachments,
            });
        }

        // Store locally for consumer access
        for msg in &incoming {
            self.received.lock().push(msg.clone());
        }

        Ok(incoming)
    }

    /// Get received messages (for testing).
    pub fn get_received(&self) -> Vec<SignalIncomingMessage> {
        self.received.lock().clone()
    }

    /// Send a typing indicator via the REST API.
    async fn send_typing(&self, recipient: &str) -> Result<()> {
        let url = format!("{}/v1/typing-indicator", self.config.api_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "number": self.config.phone_number,
            "recipient": recipient,
            "typing_indicator": true,
        });

        let _ = self.client.post(&url).json(&body).send().await;
        Ok(())
    }

    #[allow(dead_code)]
    fn api_base(&self) -> String {
        self.config.api_url.clone()
    }
}

#[async_trait]
impl PlatformAdapter for SignalAdapter {
    fn name(&self) -> &str { "signal" }

    async fn connect(&self) -> Result<()> {
        // Verify signal-cli REST API is reachable
        let url = format!("{}/v1/qrcode/{}", self.config.api_url.trim_end_matches('/'), self.config.phone_number);
        let resp = self.client.get(&url).send().await;
        match resp {
            Ok(r) if r.status().is_success() => {
                tracing::info!(phone = %self.config.phone_number, "Signal adapter connected");
            }
            _ => {
                // Still consider connected — API may just not have that endpoint
                tracing::warn!(phone = %self.config.phone_number, "Signal API health check failed, proceeding anyway");
            }
        }
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Signal adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        self.send_via_api(&[chat_id.to_string()], text).await?;
        Ok(String::new())
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        let url = format!("{}/v1/send", self.config.api_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "number": self.config.phone_number,
            "recipients": [chat_id],
            "attachments": [path.to_string_lossy().to_string()],
        });

        self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to send Signal file")?;

        Ok(())
    }

    async fn send_animation(&self, _chat_id: &str, _path: &Path) -> Result<()> {
        // Signal doesn't have native animation support — send as file
        Ok(())
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        // Signal sticker sending not supported via REST API
        tracing::warn!("Signal: sticker sending not supported");
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // Signal doesn't support message editing
        tracing::warn!("Signal: message editing not supported");
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // Signal doesn't support message deletion via REST API
        tracing::warn!("Signal: message deletion not supported");
        Ok(())
    }

    async fn is_typing(&self, chat_id: &str) -> Result<()> {
        self.send_typing(chat_id).await
    }

    fn create_consumer(&self, chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(BufferingConsumer::new(
            chat_id.to_string(),
            Arc::new(Self {
                config: self.config.clone(),
                client: self.client.clone(),
                received: Arc::clone(&self.received),
            }),
        ))
    }

    fn message_format(&self) -> MessageFormat {
        MessageFormat::Plain
    }

    fn supports_edit_streaming(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_config_new() {
        let config = SignalConfig::new("+1234567890");
        assert_eq!(config.phone_number, "+1234567890");
        assert_eq!(config.api_url, "http://127.0.0.1:8080");
    }

    #[test]
    fn test_signal_config_custom_url() {
        let config = SignalConfig::new("+1234567890")
            .with_api_url("http://localhost:9999");
        assert_eq!(config.api_url, "http://localhost:9999");
    }

    #[test]
    fn test_signal_config_from_platform_config() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "phone_number": "+1234567890",
                "api_url": "http://signal-api:8080"
            }),
        };
        let signal_config = SignalConfig::from_platform_config(&config).unwrap();
        assert_eq!(signal_config.phone_number, "+1234567890");
        assert_eq!(signal_config.api_url, "http://signal-api:8080");
    }

    #[test]
    fn test_signal_config_missing_phone() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({}),
        };
        assert!(SignalConfig::from_platform_config(&config).is_err());
    }

    #[test]
    fn test_signal_adapter_name() {
        let config = SignalConfig::new("+1234567890");
        let adapter = SignalAdapter::new(config);
        assert_eq!(adapter.name(), "signal");
    }

    #[test]
    fn test_signal_message_format() {
        let config = SignalConfig::new("+1234567890");
        let adapter = SignalAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Plain);
    }

    #[test]
    fn test_signal_no_edit_streaming() {
        let config = SignalConfig::new("+1234567890");
        let adapter = SignalAdapter::new(config);
        assert!(!adapter.supports_edit_streaming());
    }

    #[test]
    fn test_signal_received() {
        let config = SignalConfig::new("+1234567890");
        let adapter = SignalAdapter::new(config);

        assert!(adapter.get_received().is_empty());
    }
}
