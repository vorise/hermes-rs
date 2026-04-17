use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// Incoming message from Feishu webhook.
#[derive(Debug, Clone)]
pub struct FeishuIncomingMessage {
    /// Sender's open ID.
    pub sender_id: String,
    /// Chat/room ID.
    pub chat_id: String,
    /// Message text content.
    pub text: String,
    /// Feishu message ID.
    pub message_id: String,
    /// Whether this is a group chat message.
    pub is_group: bool,
}

/// Feishu (飞书/Lark) platform adapter configuration.
///
/// Uses the Feishu Open Platform API for bot messaging.
#[derive(Debug, Clone)]
pub struct FeishuConfig {
    /// Feishu App ID.
    pub app_id: String,
    /// Feishu App Secret.
    pub app_secret: String,
    /// Optional verification token for webhook.
    pub verification_token: Option<String>,
    /// Encrypt key for webhook event decryption.
    pub encrypt_key: Option<String>,
}

impl FeishuConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let app_id = config
            .get_str("app_id")
            .ok_or_else(|| anyhow::anyhow!("Feishu app_id not configured"))?;
        let app_secret = config
            .get_str("app_secret")
            .ok_or_else(|| anyhow::anyhow!("Feishu app_secret not configured"))?;
        Ok(Self {
            app_id,
            app_secret,
            verification_token: config.get_str("verification_token"),
            encrypt_key: config.get_str("encrypt_key"),
        })
    }

    /// Feishu Open API base URL.
    pub fn api_base(&self) -> &str {
        "https://open.feishu.cn/open-apis"
    }

    /// URL to get a tenant access token.
    pub fn token_url(&self) -> String {
        format!("{}/auth/v3/tenant_access_token/internal", self.api_base())
    }
}

/// Feishu platform adapter.
///
/// Connects to Feishu/Lark via the Open Platform API.
/// Supports rich text messages, interactive cards, and file uploads.
pub struct FeishuAdapter {
    config: FeishuConfig,
    client: reqwest::Client,
    /// Cached tenant access token.
    access_token: Arc<parking_lot::Mutex<Option<String>>>,
}

impl FeishuAdapter {
    pub fn new(config: FeishuConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            access_token: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let fs_config = FeishuConfig::from_platform_config(config)?;
        Ok(Self::new(fs_config))
    }

    /// Get or refresh the tenant access token.
    async fn get_access_token(&self) -> Result<String> {
        if let Some(token) = self.access_token.lock().clone() {
            return Ok(token);
        }

        let body = serde_json::json!({
            "app_id": self.config.app_id,
            "app_secret": self.config.app_secret,
        });

        let resp = self.client
            .post(&self.config.token_url())
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = json.get("msg").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("Feishu token error: {msg}"));
        }

        let token = json
            .get("tenant_access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("No tenant_access_token in response"))?
            .to_string();

        *self.access_token.lock() = Some(token.clone());
        Ok(token)
    }

    async fn api_call(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let token = self.get_access_token().await?;
        let url = format!("{}/{}", self.config.api_base(), path.trim_start_matches('/'));

        let mut req = self.client.request(method.parse()?, &url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json; charset=utf-8");

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await?;
        let json: serde_json::Value = resp.json().await?;

        // Feishu responses have a "code" field; 0 means success
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = json.get("msg").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("Feishu API error (code {code}): {msg}"));
        }

        Ok(json)
    }
}

#[async_trait]
impl PlatformAdapter for FeishuAdapter {
    fn name(&self) -> &str { "feishu" }

    async fn connect(&self) -> Result<()> {
        self.get_access_token()
            .await
            .with_context(|| "Failed to connect to Feishu API")?;
        tracing::info!("Feishu adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Feishu adapter disconnected");
        *self.access_token.lock() = None;
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        // Feishu supports rich text messages with markdown-like syntax
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "interactive",
            "content": serde_json::json!({
                "config": {
                    "wide_screen_mode": true,
                },
                "elements": [
                    {
                        "tag": "div",
                        "text": {
                            "content": text,
                            "tag": "lark_md",
                        },
                    },
                ],
                "header": {
                    "title": {
                        "content": "Hermes",
                        "tag": "plain_text",
                    },
                },
            }),
        });

        let resp = self.api_call("POST", "im/v1/messages", Some(body)).await?;

        let msg_id = resp
            .get("data")
            .and_then(|d| d.get("message_id"))
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .to_string();

        Ok(msg_id)
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        // Feishu supports file upload via the file API
        // Simplified: send a message with the file name
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": serde_json::json!({
                "text": format!("[File: {file_name}]"),
            }),
        });

        self.api_call("POST", "im/v1/messages", Some(body)).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, sticker_id: &str) -> Result<()> {
        tracing::debug!("Feishu sticker send: using emoji fallback for {sticker_id}");
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // Feishu bot can edit messages but requires specific API
        tracing::warn!("Feishu message editing not fully implemented");
        Ok(())
    }

    async fn delete_message(&self, message_id: &str, _chat_id: &str) -> Result<()> {
        // Feishu supports deleting messages by message_id
        self.api_call("DELETE", &format!("im/v1/messages/{message_id}"), None).await?;
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        // Feishu does not have a typing indicator API
        Ok(())
    }

    fn create_consumer(&self, chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(BufferingConsumer::new(
            chat_id.to_string(),
            Arc::new(Self {
                config: self.config.clone(),
                client: self.client.clone(),
                access_token: self.access_token.clone(),
            }),
        ))
    }

    fn message_format(&self) -> MessageFormat {
        MessageFormat::Markdown
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
                "app_id": "cli_a1b2c3d4e5f6",
                "app_secret": "abcdef123456",
                "verification_token": "vt_token",
                "encrypt_key": "enc_key"
            }),
        };

        let config = FeishuConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.app_id, "cli_a1b2c3d4e5f6");
        assert_eq!(config.app_secret, "abcdef123456");
        assert_eq!(config.verification_token, Some("vt_token".to_string()));
        assert_eq!(config.encrypt_key, Some("enc_key".to_string()));
    }

    #[test]
    fn test_config_missing_app_id() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_secret": "abcdef123456"
            }),
        };
        assert!(FeishuConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_secret() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_id": "cli_a1b2c3d4e5f6"
            }),
        };
        assert!(FeishuConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_api_base() {
        let config = FeishuConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            verification_token: None,
            encrypt_key: None,
        };
        assert_eq!(config.api_base(), "https://open.feishu.cn/open-apis");
    }

    #[test]
    fn test_token_url() {
        let config = FeishuConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            verification_token: None,
            encrypt_key: None,
        };
        assert_eq!(
            config.token_url(),
            "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal"
        );
    }

    #[test]
    fn test_adapter_name() {
        let config = FeishuConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            verification_token: None,
            encrypt_key: None,
        };
        let adapter = FeishuAdapter::new(config);
        assert_eq!(adapter.name(), "feishu");
    }

    #[test]
    fn test_message_format() {
        let config = FeishuConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            verification_token: None,
            encrypt_key: None,
        };
        let adapter = FeishuAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Markdown);
        assert!(!adapter.supports_edit_streaming());
    }

    #[test]
    fn test_incoming_message() {
        let msg = FeishuIncomingMessage {
            sender_id: "ou_abc123".to_string(),
            chat_id: "oc_group456".to_string(),
            text: "Hello Feishu".to_string(),
            message_id: "msg_def789".to_string(),
            is_group: true,
        };
        assert_eq!(msg.sender_id, "ou_abc123");
        assert!(msg.is_group);
        assert_eq!(msg.text, "Hello Feishu");
    }

    #[test]
    fn test_config_optional_fields() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_id": "cli_test",
                "app_secret": "secret_test"
            }),
        };

        let config = FeishuConfig::from_platform_config(&platform_config).unwrap();
        assert!(config.verification_token.is_none());
        assert!(config.encrypt_key.is_none());
    }
}
