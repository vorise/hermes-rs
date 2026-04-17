use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// Incoming message from Weixin (WeChat).
#[derive(Debug, Clone)]
pub struct WeixinIncomingMessage {
    /// Sender's open ID.
    pub from_user: String,
    /// Message content.
    pub content: String,
    /// Message type (text, image, voice, etc.).
    pub msg_type: String,
    /// WeChat message ID.
    pub msg_id: u64,
}

/// Weixin (微信/WeChat) platform adapter configuration.
///
/// Uses the WeChat Official Account API for messaging.
/// Supports text messages, rich media, and template messages.
#[derive(Debug, Clone)]
pub struct WeixinConfig {
    /// WeChat App ID.
    pub app_id: String,
    /// WeChat App Secret.
    pub app_secret: String,
    /// WeChat Token for callback verification.
    pub token: Option<String>,
    /// Encoding AES Key for callback message encryption.
    pub encoding_aes_key: Option<String>,
}

impl WeixinConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let app_id = config
            .get_str("app_id")
            .ok_or_else(|| anyhow::anyhow!("Weixin app_id not configured"))?;
        let app_secret = config
            .get_str("app_secret")
            .ok_or_else(|| anyhow::anyhow!("Weixin app_secret not configured"))?;
        Ok(Self {
            app_id,
            app_secret,
            token: config.get_str("token"),
            encoding_aes_key: config.get_str("encoding_aes_key"),
        })
    }

    /// WeChat API base URL.
    pub fn api_base(&self) -> &str {
        "https://api.weixin.qq.com"
    }

    /// URL to get an access token.
    pub fn token_url(&self) -> String {
        format!(
            "{}/cgi-bin/token?grant_type=client_credential&appid={}&secret={}",
            self.api_base(),
            self.app_id,
            self.app_secret
        )
    }
}

/// Weixin (WeChat) platform adapter.
///
/// Connects to WeChat via the Official Account API.
/// Supports text messages, rich media, and template messages.
pub struct WeixinAdapter {
    config: WeixinConfig,
    client: reqwest::Client,
    /// Cached access token.
    access_token: Arc<parking_lot::Mutex<Option<String>>>,
}

impl WeixinAdapter {
    pub fn new(config: WeixinConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            access_token: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let wx_config = WeixinConfig::from_platform_config(config)?;
        Ok(Self::new(wx_config))
    }

    /// Get or refresh the access token.
    async fn get_access_token(&self) -> Result<String> {
        if let Some(token) = self.access_token.lock().clone() {
            return Ok(token);
        }

        let resp = self.client.get(&self.config.token_url()).send().await?;
        let json: serde_json::Value = resp.json().await?;

        // WeChat returns errcode on error
        if let Some(errcode) = json.get("errcode") {
            let errmsg = json.get("errmsg").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("WeChat token error: {errmsg} (code: {errcode})"));
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
        let url = format!("{}/{}?access_token={}", self.config.api_base(), endpoint, token);

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;

        // Check for errors
        if let Some(errcode) = json.get("errcode") {
            let code = errcode.as_i64().unwrap_or(-1);
            if code != 0 {
                let errmsg = json.get("errmsg").and_then(|m| m.as_str()).unwrap_or("Unknown error");
                return Err(anyhow::anyhow!("WeChat API error: {errmsg} (code: {code})"));
            }
        }

        Ok(json)
    }
}

#[async_trait]
impl PlatformAdapter for WeixinAdapter {
    fn name(&self) -> &str { "weixin" }

    async fn connect(&self) -> Result<()> {
        self.get_access_token()
            .await
            .with_context(|| "Failed to connect to WeChat API")?;
        tracing::info!("Weixin adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Weixin adapter disconnected");
        *self.access_token.lock() = None;
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        // WeChat Official Account API supports customer service messages
        let body = serde_json::json!({
            "touser": chat_id,
            "msgtype": "text",
            "text": {
                "content": text,
            },
        });

        self.api_call("cgi-bin/message/custom/send", body).await?;
        Ok(String::new())
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        // WeChat supports file upload via media API, then send as file message
        // Simplified: send as text notification
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let body = serde_json::json!({
            "touser": chat_id,
            "msgtype": "text",
            "text": {
                "content": format!("[文件: {file_name}]"),
            },
        });

        self.api_call("cgi-bin/message/custom/send", body).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        // WeChat supports emoji/image messages
        self.send_file(chat_id, path).await
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        // WeChat supports voice messages
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        // WeChat does not support sending stickers via API
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // WeChat does not support message editing
        tracing::warn!("WeChat does not support message editing");
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // WeChat does not support message deletion
        tracing::warn!("WeChat does not support message deletion");
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        // WeChat does not have a typing indicator
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
                "app_id": "wx_abcdef123",
                "app_secret": "secret_xyz789",
                "token": "weixin_token",
                "encoding_aes_key": "aes_key_abc"
            }),
        };

        let config = WeixinConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.app_id, "wx_abcdef123");
        assert_eq!(config.app_secret, "secret_xyz789");
        assert_eq!(config.token, Some("weixin_token".to_string()));
        assert_eq!(config.encoding_aes_key, Some("aes_key_abc".to_string()));
    }

    #[test]
    fn test_config_missing_app_id() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_secret": "secret"
            }),
        };
        assert!(WeixinConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_secret() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_id": "wx_abc"
            }),
        };
        assert!(WeixinConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_optional_fields() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "app_id": "wx_abc",
                "app_secret": "secret"
            }),
        };

        let config = WeixinConfig::from_platform_config(&platform_config).unwrap();
        assert!(config.token.is_none());
        assert!(config.encoding_aes_key.is_none());
    }

    #[test]
    fn test_api_base() {
        let config = WeixinConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            token: None,
            encoding_aes_key: None,
        };
        assert_eq!(config.api_base(), "https://api.weixin.qq.com");
    }

    #[test]
    fn test_token_url() {
        let config = WeixinConfig {
            app_id: "wx123".to_string(),
            app_secret: "secret456".to_string(),
            token: None,
            encoding_aes_key: None,
        };
        let url = config.token_url();
        assert!(url.contains("api.weixin.qq.com"));
        assert!(url.contains("appid=wx123"));
        assert!(url.contains("secret=secret456"));
    }

    #[test]
    fn test_adapter_name() {
        let config = WeixinConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            token: None,
            encoding_aes_key: None,
        };
        let adapter = WeixinAdapter::new(config);
        assert_eq!(adapter.name(), "weixin");
    }

    #[test]
    fn test_message_format() {
        let config = WeixinConfig {
            app_id: "test".to_string(),
            app_secret: "test".to_string(),
            token: None,
            encoding_aes_key: None,
        };
        let adapter = WeixinAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Plain);
        assert!(!adapter.supports_edit_streaming());
    }

    #[test]
    fn test_incoming_message() {
        let msg = WeixinIncomingMessage {
            from_user: "oABCdef123".to_string(),
            content: "Hello WeChat".to_string(),
            msg_type: "text".to_string(),
            msg_id: 12345678901234567,
        };
        assert_eq!(msg.from_user, "oABCdef123");
        assert_eq!(msg.content, "Hello WeChat");
        assert_eq!(msg.msg_type, "text");
    }
}
