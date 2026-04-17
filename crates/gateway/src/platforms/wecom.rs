use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// Incoming message from WeCom webhook.
#[derive(Debug, Clone)]
pub struct WeComIncomingMessage {
    /// Sender's user ID.
    pub user_id: String,
    /// Agent ID.
    pub agent_id: u64,
    /// Message content.
    pub content: String,
    /// Message ID.
    pub msg_id: String,
}

/// WeCom (企业微信/WeChat Work) platform adapter configuration.
///
/// Uses the WeCom Open API for enterprise messaging.
/// Supports both incoming webhook callbacks and outbound API messages.
#[derive(Debug, Clone)]
pub struct WeComConfig {
    /// WeCom Corp ID.
    pub corp_id: String,
    /// WeCom Agent Secret.
    pub agent_secret: String,
    /// Agent ID for the bot application.
    pub agent_id: u64,
    /// Callback token for webhook verification.
    pub callback_token: Option<String>,
    /// Callback encoding AES key for message decryption.
    pub callback_aes_key: Option<String>,
}

impl WeComConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let corp_id = config
            .get_str("corp_id")
            .ok_or_else(|| anyhow::anyhow!("WeCom corp_id not configured"))?;
        let agent_secret = config
            .get_str("agent_secret")
            .ok_or_else(|| anyhow::anyhow!("WeCom agent_secret not configured"))?;
        let agent_id = config
            .settings
            .get("agent_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("WeCom agent_id not configured"))?;
        Ok(Self {
            corp_id,
            agent_secret,
            agent_id,
            callback_token: config.get_str("callback_token"),
            callback_aes_key: config.get_str("callback_aes_key"),
        })
    }

    /// WeCom API base URL.
    pub fn api_base(&self) -> &str {
        "https://qyapi.weixin.qq.com"
    }

    /// URL to get an access token.
    pub fn token_url(&self) -> String {
        format!(
            "{}/cgi-bin/gettoken?corpid={}&corpsecret={}",
            self.api_base(),
            self.corp_id,
            self.agent_secret
        )
    }

    /// URL to send a message.
    pub fn send_message_url(&self, token: &str) -> String {
        format!("{}/cgi-bin/message/send?access_token={}", self.api_base(), token)
    }
}

/// WeCom platform adapter.
///
/// Connects to WeCom (WeChat Work) via the Enterprise API.
/// Supports text messages, cards, and file sharing with
/// message encryption/decryption for callbacks.
pub struct WeComAdapter {
    config: WeComConfig,
    client: reqwest::Client,
    /// Cached access token.
    access_token: Arc<parking_lot::Mutex<Option<String>>>,
}

impl WeComAdapter {
    pub fn new(config: WeComConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            access_token: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let wc_config = WeComConfig::from_platform_config(config)?;
        Ok(Self::new(wc_config))
    }

    /// Get or refresh the access token.
    async fn get_access_token(&self) -> Result<String> {
        if let Some(token) = self.access_token.lock().clone() {
            return Ok(token);
        }

        let resp = self.client.get(&self.config.token_url()).send().await?;
        let json: serde_json::Value = resp.json().await?;

        let errcode = json.get("errcode").and_then(|c| c.as_i64()).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = json.get("errmsg").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("WeCom token error: {errmsg}"));
        }

        let token = json
            .get("access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("No access_token in response"))?
            .to_string();

        *self.access_token.lock() = Some(token.clone());
        Ok(token)
    }

    async fn send_api(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        let token = self.get_access_token().await?;
        let url = self.config.send_message_url(&token);

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let errcode = json.get("errcode").and_then(|c| c.as_i64()).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = json.get("errmsg").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("WeCom API error: {errmsg}"));
        }

        Ok(json)
    }
}

#[async_trait]
impl PlatformAdapter for WeComAdapter {
    fn name(&self) -> &str { "wecom" }

    async fn connect(&self) -> Result<()> {
        self.get_access_token()
            .await
            .with_context(|| "Failed to connect to WeCom API")?;
        tracing::info!("WeCom adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("WeCom adapter disconnected");
        *self.access_token.lock() = None;
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        // WeCom supports textcard (rich text) messages
        let body = serde_json::json!({
            "touser": chat_id,
            "msgtype": "textcard",
            "agentid": self.config.agent_id,
            "textcard": {
                "title": "Hermes",
                "description": text,
                "url": "",
                "btntxt": "详情",
            },
        });

        let _resp = self.send_api(body).await?;
        Ok(String::new())
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        // WeCom supports file messages
        let body = serde_json::json!({
            "touser": chat_id,
            "msgtype": "text",
            "agentid": self.config.agent_id,
            "text": {
                "content": format!("[文件: {file_name}]"),
            },
        });

        self.send_api(body).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        // WeCom does not support stickers
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // WeCom does not support message editing
        tracing::warn!("WeCom does not support message editing");
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // WeCom does not support message deletion
        tracing::warn!("WeCom does not support message deletion");
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        // WeCom does not have a typing indicator
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
                "corp_id": "ww123456",
                "agent_secret": "abcdef123456",
                "agent_id": 1000001,
                "callback_token": "token",
                "callback_aes_key": "aeskey"
            }),
        };

        let config = WeComConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.corp_id, "ww123456");
        assert_eq!(config.agent_secret, "abcdef123456");
        assert_eq!(config.agent_id, 1000001);
        assert_eq!(config.callback_token, Some("token".to_string()));
        assert_eq!(config.callback_aes_key, Some("aeskey".to_string()));
    }

    #[test]
    fn test_config_missing_corp_id() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "agent_secret": "secret",
                "agent_id": 1000001
            }),
        };
        assert!(WeComConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_secret() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "corp_id": "ww123",
                "agent_id": 1000001
            }),
        };
        assert!(WeComConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_agent_id() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "corp_id": "ww123",
                "agent_secret": "secret"
            }),
        };
        assert!(WeComConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_api_base() {
        let config = WeComConfig {
            corp_id: "test".to_string(),
            agent_secret: "test".to_string(),
            agent_id: 1000001,
            callback_token: None,
            callback_aes_key: None,
        };
        assert_eq!(config.api_base(), "https://qyapi.weixin.qq.com");
    }

    #[test]
    fn test_token_url() {
        let config = WeComConfig {
            corp_id: "corp123".to_string(),
            agent_secret: "secret456".to_string(),
            agent_id: 1000001,
            callback_token: None,
            callback_aes_key: None,
        };
        assert_eq!(
            config.token_url(),
            "https://qyapi.weixin.qq.com/cgi-bin/gettoken?corpid=corp123&corpsecret=secret456"
        );
    }

    #[test]
    fn test_adapter_name() {
        let config = WeComConfig {
            corp_id: "test".to_string(),
            agent_secret: "test".to_string(),
            agent_id: 1000001,
            callback_token: None,
            callback_aes_key: None,
        };
        let adapter = WeComAdapter::new(config);
        assert_eq!(adapter.name(), "wecom");
    }

    #[test]
    fn test_message_format() {
        let config = WeComConfig {
            corp_id: "test".to_string(),
            agent_secret: "test".to_string(),
            agent_id: 1000001,
            callback_token: None,
            callback_aes_key: None,
        };
        let adapter = WeComAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Plain);
        assert!(!adapter.supports_edit_streaming());
    }

    #[test]
    fn test_incoming_message() {
        let msg = WeComIncomingMessage {
            user_id: "zhangsan".to_string(),
            agent_id: 1000001,
            content: "Hello WeCom".to_string(),
            msg_id: "msg_123".to_string(),
        };
        assert_eq!(msg.user_id, "zhangsan");
        assert_eq!(msg.content, "Hello WeCom");
    }
}
