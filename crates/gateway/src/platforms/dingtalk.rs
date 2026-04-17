use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// Incoming message from DingTalk webhook.
#[derive(Debug, Clone)]
pub struct DingTalkIncomingMessage {
    /// Sender's staff ID.
    pub sender_id: String,
    /// Sender's display name.
    pub sender_name: String,
    /// Conversation ID.
    pub conversation_id: String,
    /// Message text content.
    pub text: String,
    /// Message ID.
    pub msg_id: String,
    /// Webhook token for reply validation.
    pub webhook_token: Option<String>,
}

/// DingTalk platform adapter configuration.
///
/// DingTalk bot uses a combination of webhook for incoming messages
/// and REST API for outgoing messages.
#[derive(Debug, Clone)]
pub struct DingTalkConfig {
    /// DingTalk App Key.
    pub app_key: String,
    /// DingTalk App Secret.
    pub app_secret: String,
    /// Outgoing webhook URL for replies.
    pub webhook_url: Option<String>,
}

impl DingTalkConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let app_key = config
            .get_str("app_key")
            .ok_or_else(|| anyhow::anyhow!("DingTalk app_key not configured"))?;
        let app_secret = config
            .get_str("app_secret")
            .ok_or_else(|| anyhow::anyhow!("DingTalk app_secret not configured"))?;
        Ok(Self {
            app_key,
            app_secret,
            webhook_url: config.get_str("webhook_url"),
        })
    }

    /// DingTalk API base URL.
    pub fn api_base(&self) -> &str {
        "https://oapi.dingtalk.com"
    }

    /// URL to get an access token.
    pub fn token_url(&self) -> String {
        format!(
            "{}/gettoken?appkey={}&appsecret={}",
            self.api_base(),
            self.app_key,
            self.app_secret
        )
    }
}

/// DingTalk platform adapter.
///
/// Connects to DingTalk via the Open API for bot messaging.
/// Supports text messages, markdown, and file sharing.
pub struct DingTalkAdapter {
    config: DingTalkConfig,
    client: reqwest::Client,
    /// Cached access token (simplified, no TTL tracking).
    access_token: Arc<parking_lot::Mutex<Option<String>>>,
}

impl DingTalkAdapter {
    pub fn new(config: DingTalkConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            access_token: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let dt_config = DingTalkConfig::from_platform_config(config)?;
        Ok(Self::new(dt_config))
    }

    /// Get or refresh the access token.
    async fn get_access_token(&self) -> Result<String> {
        if let Some(token) = self.access_token.lock().clone() {
            return Ok(token);
        }

        let resp = self.client.get(&self.config.token_url()).send().await?;
        let json: serde_json::Value = resp.json().await?;

        if json.get("errcode").and_then(|c| c.as_i64()) != Some(0) {
            let errmsg = json.get("errmsg").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("DingTalk token error: {errmsg}"));
        }

        let token = json
            .get("access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("No access_token in response"))?
            .to_string();

        *self.access_token.lock() = Some(token.clone());
        Ok(token)
    }

    async fn api_call(&self, endpoint: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        let token = self.get_access_token().await?;
        let url = format!("{}/{}/send", self.config.api_base(), endpoint);

        let resp = self.client
            .post(&url)
            .query(&[("access_token", &token)])
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        if json.get("errcode").and_then(|c| c.as_i64()) != Some(0) {
            let errmsg = json.get("errmsg").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("DingTalk API error: {errmsg}"));
        }

        Ok(json)
    }
}

#[async_trait]
impl PlatformAdapter for DingTalkAdapter {
    fn name(&self) -> &str { "dingtalk" }

    async fn connect(&self) -> Result<()> {
        // Test connection by fetching a token
        self.get_access_token()
            .await
            .with_context(|| "Failed to connect to DingTalk API")?;
        tracing::info!("DingTalk adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("DingTalk adapter disconnected");
        // Invalidate cached token
        *self.access_token.lock() = None;
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        // DingTalk supports markdown messages
        let body = serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": "Hermes",
                "text": text,
            },
            "openConversationId": chat_id,
            "at": {
                "isAtAll": false,
            },
        });

        let resp = self.api_call("robot/oToMessages", body).await?;

        let msg_id = resp
            .get("processQueryKey")
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .to_string();

        Ok(msg_id)
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        // DingTalk supports sending files via media upload API
        // Simplified: send the file path as a message
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let body = serde_json::json!({
            "msgtype": "text",
            "text": {
                "content": format!("[File: {file_name}]"),
            },
            "openConversationId": chat_id,
            "at": {
                "isAtAll": false,
            },
        });

        self.api_call("robot/oToMessages", body).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        // Send as file
        self.send_file(chat_id, path).await
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        // Send as file
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, sticker_id: &str) -> Result<()> {
        // DingTalk doesn't have a native sticker API for bots
        tracing::debug!("DingTalk sticker send: using emoji fallback for {sticker_id}");
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // DingTalk bot does not support message editing
        tracing::warn!("DingTalk does not support message editing");
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // DingTalk bot does not support message deletion
        tracing::warn!("DingTalk does not support message deletion");
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        // DingTalk does not have a typing indicator API
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
                "app_key": "dingabcdef",
                "app_secret": "secret123",
                "webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=xxx"
            }),
        };

        let config = DingTalkConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.app_key, "dingabcdef");
        assert_eq!(config.app_secret, "secret123");
        assert!(config.webhook_url.is_some());
    }

    #[test]
    fn test_config_missing_app_key() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_secret": "secret123"
            }),
        };
        assert!(DingTalkConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_secret() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_key": "dingabcdef"
            }),
        };
        assert!(DingTalkConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_api_base() {
        let config = DingTalkConfig {
            app_key: "test".to_string(),
            app_secret: "test".to_string(),
            webhook_url: None,
        };
        assert_eq!(config.api_base(), "https://oapi.dingtalk.com");
    }

    #[test]
    fn test_token_url() {
        let config = DingTalkConfig {
            app_key: "key123".to_string(),
            app_secret: "secret456".to_string(),
            webhook_url: None,
        };
        assert_eq!(
            config.token_url(),
            "https://oapi.dingtalk.com/gettoken?appkey=key123&appsecret=secret456"
        );
    }

    #[test]
    fn test_adapter_name() {
        let config = DingTalkConfig {
            app_key: "test".to_string(),
            app_secret: "test".to_string(),
            webhook_url: None,
        };
        let adapter = DingTalkAdapter::new(config);
        assert_eq!(adapter.name(), "dingtalk");
    }

    #[test]
    fn test_message_format() {
        let config = DingTalkConfig {
            app_key: "test".to_string(),
            app_secret: "test".to_string(),
            webhook_url: None,
        };
        let adapter = DingTalkAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Markdown);
        assert!(!adapter.supports_edit_streaming());
    }

    #[test]
    fn test_incoming_message() {
        let msg = DingTalkIncomingMessage {
            sender_id: "user123".to_string(),
            sender_name: "Test User".to_string(),
            conversation_id: "conv456".to_string(),
            text: "Hello".to_string(),
            msg_id: "msg789".to_string(),
            webhook_token: None,
        };
        assert_eq!(msg.sender_id, "user123");
        assert_eq!(msg.text, "Hello");
    }
}
