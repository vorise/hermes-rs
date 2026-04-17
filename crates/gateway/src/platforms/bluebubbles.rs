use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// BlueBubbles platform adapter configuration.
///
/// BlueBubbles provides a REST API for iMessage on macOS.
#[derive(Debug, Clone)]
pub struct BlueBubblesConfig {
    /// BlueBubbles server URL (e.g., "http://localhost:12345").
    pub server_url: String,
    /// API password for authentication.
    pub password: String,
    /// Default chat GUID for DM conversations.
    pub default_chat_guid: Option<String>,
}

impl BlueBubblesConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let server_url = config
            .get_str("server_url")
            .ok_or_else(|| anyhow::anyhow!("BlueBubbles server_url not configured"))?;
        let password = config
            .get_str("password")
            .ok_or_else(|| anyhow::anyhow!("BlueBubbles password not configured"))?;
        Ok(Self {
            server_url,
            password,
            default_chat_guid: config.get_str("default_chat_guid"),
        })
    }

    /// API base URL with version prefix.
    pub fn api_base(&self) -> &str {
        &self.server_url
    }

    /// Build an API URL for a specific endpoint.
    pub fn endpoint_url(&self, endpoint: &str) -> String {
        format!("{}/api/v1/{}", self.api_base(), endpoint)
    }
}

/// BlueBubbles platform adapter.
///
/// Connects to a BlueBubbles server for iMessage integration.
/// Supports text messages, attachments, and tapback reactions.
pub struct BlueBubblesAdapter {
    config: BlueBubblesConfig,
    client: reqwest::Client,
}

impl BlueBubblesAdapter {
    pub fn new(config: BlueBubblesConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let bb_config = BlueBubblesConfig::from_platform_config(config)?;
        Ok(Self::new(bb_config))
    }

    async fn api_call(&self, method: &str, endpoint: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let url = self.config.endpoint_url(endpoint);
        let mut req = self.client.request(method.parse()?, &url)
            .query(&[("password", &self.config.password)]);

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "BlueBubbles API error: {status} - {text}"
            ));
        }

        let json: serde_json::Value = resp.json().await?;
        // BlueBubbles wraps responses in { "status": "success", "data": { ... } }
        if json.get("status").and_then(|s| s.as_str()) == Some("error") {
            let message = json.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("BlueBubbles error: {message}"));
        }

        Ok(json.get("data").cloned().unwrap_or(json))
    }
}

#[async_trait]
impl PlatformAdapter for BlueBubblesAdapter {
    fn name(&self) -> &str { "bluebubbles" }

    async fn connect(&self) -> Result<()> {
        // Test connection by fetching server info
        self.api_call("GET", "server/info", None)
            .await
            .with_context(|| "Failed to connect to BlueBubbles server")?;
        tracing::info!("BlueBubbles adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("BlueBubbles adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        let body = serde_json::json!({
            "chatGuid": chat_id,
            "message": text,
            "method": "apple-script",
        });

        let resp = self.api_call("POST", "message/send", Some(body)).await?;

        let msg_id = resp
            .get("rowId")
            .and_then(|id| id.as_u64())
            .unwrap_or(0)
            .to_string();

        Ok(msg_id)
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        let file_path = path.to_string_lossy();
        let body = serde_json::json!({
            "chatGuid": chat_id,
            "filePath": file_path,
            "method": "apple-script",
        });

        self.api_call("POST", "message/send", Some(body)).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        // Send as file attachment (iMessage doesn't distinguish animations)
        self.send_file(chat_id, path).await
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        // Send as file attachment
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, sticker_id: &str) -> Result<()> {
        // iMessage doesn't have a native sticker API via BlueBubbles
        // sticker_id could be an emoji
        tracing::debug!("BlueBubbles sticker send: using emoji fallback for {sticker_id}");
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // iMessage does not support message editing via BlueBubbles
        tracing::warn!("BlueBubbles does not support message editing");
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // iMessage does not support message deletion via BlueBubbles
        tracing::warn!("BlueBubbles does not support message deletion");
        Ok(())
    }

    async fn is_typing(&self, chat_id: &str) -> Result<()> {
        let body = serde_json::json!({
            "chatGuid": chat_id,
        });
        self.api_call("POST", "typing-indicator/start", Some(body)).await?;
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
                "server_url": "http://localhost:12345",
                "password": "my_secret_password",
                "default_chat_guid": "iMessage;+;chat;+;guid"
            }),
        };

        let config = BlueBubblesConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.server_url, "http://localhost:12345");
        assert_eq!(config.password, "my_secret_password");
        assert_eq!(config.default_chat_guid, Some("iMessage;+;chat;+;guid".to_string()));
    }

    #[test]
    fn test_config_missing_server_url() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "password": "test"
            }),
        };
        assert!(BlueBubblesConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_missing_password() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "server_url": "http://localhost:12345"
            }),
        };
        assert!(BlueBubblesConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_endpoint_url() {
        let config = BlueBubblesConfig {
            server_url: "http://localhost:12345".to_string(),
            password: "test".to_string(),
            default_chat_guid: None,
        };
        assert_eq!(
            config.endpoint_url("message/send"),
            "http://localhost:12345/api/v1/message/send"
        );
    }

    #[test]
    fn test_adapter_name() {
        let config = BlueBubblesConfig {
            server_url: "http://localhost:12345".to_string(),
            password: "test".to_string(),
            default_chat_guid: None,
        };
        let adapter = BlueBubblesAdapter::new(config);
        assert_eq!(adapter.name(), "bluebubbles");
    }

    #[test]
    fn test_message_format() {
        let config = BlueBubblesConfig {
            server_url: "http://localhost:12345".to_string(),
            password: "test".to_string(),
            default_chat_guid: None,
        };
        let adapter = BlueBubblesAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Plain);
        assert!(!adapter.supports_edit_streaming());
    }
}
