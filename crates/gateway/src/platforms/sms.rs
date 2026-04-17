use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// SMS platform adapter configuration.
///
/// Uses a generic SMS gateway API. Supports any provider that
/// exposes an HTTP API for sending/receiving SMS messages.
#[derive(Debug, Clone)]
pub struct SmsConfig {
    /// SMS gateway API endpoint URL.
    pub api_url: String,
    /// API key for authentication.
    pub api_key: String,
    /// Sender phone number or short code.
    pub from_number: String,
}

impl SmsConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let api_url = config
            .get_str("api_url")
            .ok_or_else(|| anyhow::anyhow!("SMS api_url not configured"))?;
        let api_key = config
            .get_str("api_key")
            .ok_or_else(|| anyhow::anyhow!("SMS api_key not configured"))?;
        let from_number = config
            .get_str("from_number")
            .ok_or_else(|| anyhow::anyhow!("SMS from_number not configured"))?;
        Ok(Self {
            api_url,
            api_key,
            from_number,
        })
    }
}

/// SMS platform adapter.
///
/// Connects to an SMS gateway for text and MMS messaging.
/// Supports SMS, MMS (images), and basic media sending.
pub struct SmsAdapter {
    config: SmsConfig,
    client: reqwest::Client,
}

impl SmsAdapter {
    pub fn new(config: SmsConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let sms_config = SmsConfig::from_platform_config(config)?;
        Ok(Self::new(sms_config))
    }

    async fn api_call(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        let resp = self.client
            .post(&self.config.api_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "SMS API error: {status} - {text}"
            ));
        }

        let json: serde_json::Value = resp.json().await?;
        Ok(json)
    }
}

#[async_trait]
impl PlatformAdapter for SmsAdapter {
    fn name(&self) -> &str { "sms" }

    async fn connect(&self) -> Result<()> {
        // Test connection (SMS gateways may not have a direct test endpoint)
        tracing::info!("SMS adapter initialized");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("SMS adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        // SMS messages are limited to 160 characters (standard SMS)
        // or 1530 characters for concatenated SMS (varies by provider)
        // We split at 160 chars and let the API handle concatenation
        let plain = text.lines().next().unwrap_or(text);
        let message = if plain.len() > 1600 {
            format!("{}...", &plain[..1597])
        } else {
            plain.to_string()
        };

        let body = serde_json::json!({
            "to": chat_id,
            "from": self.config.from_number,
            "message": message,
        });

        let resp = self.api_call(body).await?;

        let msg_id = resp
            .get("messageId")
            .or_else(|| resp.get("id"))
            .or_else(|| resp.get("sid"))
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .to_string();

        Ok(msg_id)
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        // MMS: send media attachment
        let file_path = path.to_string_lossy();
        let body = serde_json::json!({
            "to": chat_id,
            "from": self.config.from_number,
            "message": "Media attachment",
            "mediaUrl": file_path,
        });

        self.api_call(body).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        // MMS: send as media
        self.send_file(chat_id, path).await
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        // MMS: send as media
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        // SMS does not support stickers
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // SMS does not support message editing
        tracing::warn!("SMS does not support message editing");
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // SMS does not support message deletion
        tracing::warn!("SMS does not support message deletion");
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        // SMS does not have a typing indicator
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
                "api_url": "https://api.twilio.com/2010-04-01/Accounts/ACxxx/Messages.json",
                "api_key": "twilio_key",
                "from_number": "+1234567890"
            }),
        };

        let config = SmsConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.api_url, "https://api.twilio.com/2010-04-01/Accounts/ACxxx/Messages.json");
        assert_eq!(config.api_key, "twilio_key");
        assert_eq!(config.from_number, "+1234567890");
    }

    #[test]
    fn test_config_missing_api_url() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "api_key": "key",
                "from_number": "+1234567890"
            }),
        };
        assert!(SmsConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_api_key() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "api_url": "https://api.example.com",
                "from_number": "+1234567890"
            }),
        };
        assert!(SmsConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_from_number() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "api_url": "https://api.example.com",
                "api_key": "key"
            }),
        };
        assert!(SmsConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_adapter_name() {
        let config = SmsConfig {
            api_url: "https://api.example.com".to_string(),
            api_key: "key".to_string(),
            from_number: "+1234567890".to_string(),
        };
        let adapter = SmsAdapter::new(config);
        assert_eq!(adapter.name(), "sms");
    }

    #[test]
    fn test_message_format() {
        let config = SmsConfig {
            api_url: "https://api.example.com".to_string(),
            api_key: "key".to_string(),
            from_number: "+1234567890".to_string(),
        };
        let adapter = SmsAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Plain);
        assert!(!adapter.supports_edit_streaming());
    }
}
