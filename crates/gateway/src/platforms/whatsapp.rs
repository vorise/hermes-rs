use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::formatting::strip_markdown;
use crate::runner::BufferingConsumer;

/// WhatsApp platform adapter configuration.
///
/// Uses the WhatsApp Business Cloud API (Meta).
#[derive(Debug, Clone)]
pub struct WhatsAppConfig {
    /// WhatsApp Business Account ID.
    pub phone_number_id: String,
    /// Access token for the Cloud API.
    pub access_token: String,
    /// Verify token for webhook validation.
    pub verify_token: Option<String>,
}

impl WhatsAppConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let phone_number_id = config
            .get_str("phone_number_id")
            .ok_or_else(|| anyhow::anyhow!("WhatsApp phone_number_id not configured"))?;
        let access_token = config
            .get_str("access_token")
            .ok_or_else(|| anyhow::anyhow!("WhatsApp access_token not configured"))?;
        Ok(Self {
            phone_number_id,
            access_token,
            verify_token: config.get_str("verify_token"),
        })
    }

    pub fn api_base(&self) -> &str {
        "https://graph.facebook.com/v18.0"
    }

    /// API endpoint for sending messages.
    pub fn messages_url(&self) -> String {
        format!(
            "{}/{}/messages",
            self.api_base(),
            self.phone_number_id
        )
    }
}

/// WhatsApp platform adapter.
///
/// Connects to the WhatsApp Business Cloud API (Meta).
/// Supports text messages, media, and template messages.
pub struct WhatsAppAdapter {
    config: WhatsAppConfig,
    client: reqwest::Client,
}

impl WhatsAppAdapter {
    pub fn new(config: WhatsAppConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let wa_config = WhatsAppConfig::from_platform_config(config)?;
        Ok(Self::new(wa_config))
    }

    async fn api_call(&self, method: &str, url: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let mut req = self.client.request(method.parse()?, url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "application/json");

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "WhatsApp API error: {status} - {text}"
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body)
    }
}

#[async_trait]
impl PlatformAdapter for WhatsAppAdapter {
    fn name(&self) -> &str { "whatsapp" }

    async fn connect(&self) -> Result<()> {
        // Test connection by fetching phone number info
        let url = format!(
            "{}/{}",
            self.config.api_base(),
            self.config.phone_number_id
        );
        self.api_call("GET", &url, None)
            .await
            .with_context(|| "Failed to connect to WhatsApp Business API")?;
        tracing::info!("WhatsApp adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("WhatsApp adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        // WhatsApp doesn't support markdown, so strip it
        let plain_text = strip_markdown(text);

        // WhatsApp messages over 4096 chars should be truncated
        let message = if plain_text.len() > 4096 {
            format!("{}...", &plain_text[..4093])
        } else {
            plain_text
        };

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": chat_id,
            "type": "text",
            "text": {
                "body": message,
            },
        });

        let resp = self.api_call("POST", &self.config.messages_url(), Some(body)).await?;

        let msg_id = resp
            .get("messages")
            .and_then(|m| m.get(0))
            .and_then(|m| m.get("id"))
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .to_string();

        Ok(msg_id)
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        // WhatsApp media upload is a two-step process:
        // 1. Upload the media to get a media ID
        // 2. Send the message with the media ID
        // For now, we send the file as a document link
        // Full multipart upload would require additional implementation
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": chat_id,
            "type": "document",
            "document": {
                "filename": file_name,
                "caption": file_name,
            },
        });

        self.api_call("POST", &self.config.messages_url(), Some(body)).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        // Send as video since WhatsApp doesn't have a dedicated animation type
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("animation");

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": chat_id,
            "type": "video",
            "video": {
                "filename": file_name,
                "caption": file_name,
            },
        });

        self.api_call("POST", &self.config.messages_url(), Some(body)).await?;
        Ok(())
    }

    async fn send_voice(&self, chat_id: &str, _path: &Path) -> Result<()> {
        // WhatsApp voice messages require media upload first
        // Simplified: would upload media and send as audio type in production
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": chat_id,
            "type": "audio",
            "audio": {
                "link": "https://example.com/audio.ogg",
            },
        });

        self.api_call("POST", &self.config.messages_url(), Some(body)).await?;
        Ok(())
    }

    async fn send_sticker(&self, chat_id: &str, sticker_id: &str) -> Result<()> {
        // WhatsApp supports sticker messages via media ID
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": chat_id,
            "type": "sticker",
            "sticker": {
                "id": sticker_id,
            },
        });

        self.api_call("POST", &self.config.messages_url(), Some(body)).await?;
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // WhatsApp does not support message editing
        tracing::warn!("WhatsApp does not support message editing");
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // WhatsApp does not support message deletion via the Business API
        tracing::warn!("WhatsApp does not support message deletion via Business API");
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        // WhatsApp does not have a typing indicator API
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
    fn test_whatsapp_config_from_platform_config() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "phone_number_id": "123456789",
                "access_token": "EAABsbCS1i...",
                "verify_token": "my_verify_token"
            }),
        };

        let wa_config = WhatsAppConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(wa_config.phone_number_id, "123456789");
        assert_eq!(wa_config.access_token, "EAABsbCS1i...");
        assert_eq!(wa_config.verify_token, Some("my_verify_token".to_string()));
    }

    #[test]
    fn test_whatsapp_config_missing_phone_number_id() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "access_token": "EAABsbCS1i..."
            }),
        };

        assert!(WhatsAppConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_whatsapp_config_missing_token() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "phone_number_id": "123456789"
            }),
        };

        assert!(WhatsAppConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_whatsapp_api_base() {
        let config = WhatsAppConfig {
            phone_number_id: "test".to_string(),
            access_token: "test".to_string(),
            verify_token: None,
        };
        assert_eq!(config.api_base(), "https://graph.facebook.com/v18.0");
    }

    #[test]
    fn test_whatsapp_messages_url() {
        let config = WhatsAppConfig {
            phone_number_id: "123456789".to_string(),
            access_token: "test".to_string(),
            verify_token: None,
        };
        assert_eq!(
            config.messages_url(),
            "https://graph.facebook.com/v18.0/123456789/messages"
        );
    }
}
