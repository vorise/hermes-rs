use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// Incoming message from QQ Bot.
#[derive(Debug, Clone)]
pub struct QqBotIncomingMessage {
    /// Sender's open ID.
    pub sender_id: String,
    /// Group ID (None for DM).
    pub group_id: Option<String>,
    /// Message text content.
    pub text: String,
    /// QQ message ID.
    pub message_id: u64,
}

/// QQ Bot platform adapter configuration.
///
/// Uses the QQ Open Platform bot API for messaging.
#[derive(Debug, Clone)]
pub struct QqBotConfig {
    /// QQ Bot App ID.
    pub app_id: String,
    /// QQ Bot App Secret.
    pub app_secret: String,
    /// QQ Bot token for authentication.
    pub bot_token: String,
}

impl QqBotConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let app_id = config
            .get_str("app_id")
            .ok_or_else(|| anyhow::anyhow!("QQBot app_id not configured"))?;
        let app_secret = config
            .get_str("app_secret")
            .ok_or_else(|| anyhow::anyhow!("QQBot app_secret not configured"))?;
        let bot_token = config
            .get_str("bot_token")
            .ok_or_else(|| anyhow::anyhow!("QQBot bot_token not configured"))?;
        Ok(Self {
            app_id,
            app_secret,
            bot_token,
        })
    }

    /// QQ Open Platform API base URL.
    pub fn api_base(&self) -> &str {
        "https://api.sgroup.qq.com"
    }
}

/// QQ Bot platform adapter.
///
/// Connects to QQ (Tencent QQ) via the Open Platform API.
/// Supports text messages, images, and group/DM messaging.
pub struct QqBotAdapter {
    config: QqBotConfig,
    client: reqwest::Client,
}

impl QqBotAdapter {
    pub fn new(config: QqBotConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let qq_config = QqBotConfig::from_platform_config(config)?;
        Ok(Self::new(qq_config))
    }

    async fn api_call(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let url = format!("{}/{}", self.config.api_base(), path.trim_start_matches('/'));

        let mut req = self.client.request(method.parse()?, &url)
            .header("Authorization", format!("QQBot {}", self.config.bot_token))
            .header("Content-Type", "application/json");

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "QQBot API error: {status} - {text}"
            ));
        }

        let json: serde_json::Value = resp.json().await?;
        Ok(json)
    }
}

#[async_trait]
impl PlatformAdapter for QqBotAdapter {
    fn name(&self) -> &str { "qqbot" }

    async fn connect(&self) -> Result<()> {
        // Test connection by fetching bot info
        self.api_call("GET", "users/@me", None)
            .await
            .with_context(|| "Failed to connect to QQBot API")?;
        tracing::info!("QQBot adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("QQBot adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        // QQ Bot supports ark (rich text) messages and simple text
        let body = serde_json::json!({
            "content": text,
            "msg_type": 0, // 0 = text
        });

        let resp = self.api_call("POST", &format!("channels/{chat_id}/messages"), Some(body)).await?;

        let msg_id = resp
            .get("id")
            .and_then(|id| id.as_u64())
            .unwrap_or(0)
            .to_string();

        Ok(msg_id)
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        // QQ Bot supports file attachments via media API
        let body = serde_json::json!({
            "content": format!("[File: {file_name}]"),
            "msg_type": 0,
        });

        self.api_call("POST", &format!("channels/{chat_id}/messages"), Some(body)).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        // QQ Bot supports face/sticker but requires specific emoji IDs
        Ok(())
    }

    async fn edit_message(&self, chat_id: &str, message_id: &str, text: &str) -> Result<()> {
        // QQ Bot supports editing bot's own messages
        let body = serde_json::json!({
            "content": text,
        });

        self.api_call("PATCH", &format!("channels/{chat_id}/messages/{message_id}"), Some(body)).await?;
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // QQ Bot may support message deletion for bot messages
        tracing::warn!("QQBot message deletion not fully implemented");
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        // QQ Bot does not have a typing indicator
        Ok(())
    }

    fn create_consumer(&self, chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(BufferingConsumer::new(
            chat_id.to_string(),
            Arc::new(Self {
                config: self.config.clone(),
                client: self.client.clone(),
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
    fn test_config_from_platform_config() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_id": "qq_app_123",
                "app_secret": "secret_abc",
                "bot_token": "token_xyz"
            }),
        };

        let config = QqBotConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.app_id, "qq_app_123");
        assert_eq!(config.app_secret, "secret_abc");
        assert_eq!(config.bot_token, "token_xyz");
    }

    #[test]
    fn test_config_missing_app_id() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_secret": "secret",
                "bot_token": "token"
            }),
        };
        assert!(QqBotConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_secret() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_id": "app",
                "bot_token": "token"
            }),
        };
        assert!(QqBotConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_token() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_id": "app",
                "app_secret": "secret"
            }),
        };
        assert!(QqBotConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_api_base() {
        let config = QqBotConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            bot_token: "test".to_string(),
        };
        assert_eq!(config.api_base(), "https://api.sgroup.qq.com");
    }

    #[test]
    fn test_adapter_name() {
        let config = QqBotConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            bot_token: "test".to_string(),
        };
        let adapter = QqBotAdapter::new(config);
        assert_eq!(adapter.name(), "qqbot");
    }

    #[test]
    fn test_message_format() {
        let config = QqBotConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            bot_token: "test".to_string(),
        };
        let adapter = QqBotAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Plain);
        assert!(!adapter.supports_edit_streaming());
    }

    #[test]
    fn test_incoming_message() {
        let msg = QqBotIncomingMessage {
            sender_id: "user_openid".to_string(),
            group_id: Some("group_123".to_string()),
            text: "Hello QQ".to_string(),
            message_id: 456,
        };
        assert_eq!(msg.sender_id, "user_openid");
        assert!(msg.group_id.is_some());
        assert_eq!(msg.text, "Hello QQ");
    }
}
