use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::formatting::format_message;

/// Discord platform adapter configuration.
#[derive(Debug, Clone)]
pub struct DiscordConfig {
    pub bot_token: String,
}

impl DiscordConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let bot_token = config
            .get_str("bot_token")
            .ok_or_else(|| anyhow::anyhow!("Discord bot_token not configured"))?;
        Ok(Self { bot_token })
    }

    pub fn api_base(&self) -> &str {
        "https://discord.com/api/v10"
    }
}

/// Discord platform adapter.
///
/// Connects to the Discord REST API for message operations.
/// In a production implementation, this would use the gateway websocket
/// for receiving messages.
pub struct DiscordAdapter {
    config: DiscordConfig,
    client: reqwest::Client,
}

impl DiscordAdapter {
    pub fn new(config: DiscordConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let dc_config = DiscordConfig::from_platform_config(config)?;
        Ok(Self::new(dc_config))
    }

    async fn api_call(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let url = format!("{}/{}", self.config.api_base(), path);
        let mut req = self.client.request(method.parse()?, &url)
            .header("Authorization", format!("Bot {}", self.config.bot_token))
            .header("Content-Type", "application/json");

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Discord API error: {} - {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body)
    }
}

#[async_trait]
impl PlatformAdapter for DiscordAdapter {
    fn name(&self) -> &str { "discord" }

    async fn connect(&self) -> Result<()> {
        // Test connection by fetching current user
        self.api_call("GET", "users/@me", None)
            .await
            .with_context(|| "Failed to connect to Discord API")?;
        tracing::info!("Discord adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Discord adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, channel_id: &str, text: &str) -> Result<String> {
        let formatted = format_message(text, MessageFormat::Markdown);
        let body = serde_json::json!({
            "content": formatted,
        });

        let resp = self
            .api_call("POST", &format!("channels/{channel_id}/messages"), Some(body))
            .await?;

        let msg_id = resp
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(msg_id)
    }

    async fn send_file(&self, _channel_id: &str, _path: &Path) -> Result<()> {
        // Discord file upload requires multipart/form-data
        // Simplified: would use reqwest::multipart in production
        tracing::warn!("Discord file upload not fully implemented");
        Ok(())
    }

    async fn send_animation(&self, channel_id: &str, path: &Path) -> Result<()> {
        self.send_file(channel_id, path).await
    }

    async fn send_voice(&self, _channel_id: &str, _path: &Path) -> Result<()> {
        tracing::warn!("Discord does not support voice messages via bot API");
        Ok(())
    }

    async fn send_sticker(&self, channel_id: &str, sticker_id: &str) -> Result<()> {
        let body = serde_json::json!({
            "sticker_ids": [sticker_id],
        });
        self.api_call("POST", &format!("channels/{channel_id}/messages"), Some(body))
            .await?;
        Ok(())
    }

    async fn edit_message(&self, channel_id: &str, message_id: &str, text: &str) -> Result<()> {
        let formatted = format_message(text, MessageFormat::Markdown);
        let body = serde_json::json!({
            "content": formatted,
        });
        self.api_call(
            "PATCH",
            &format!("channels/{channel_id}/messages/{message_id}"),
            Some(body),
        )
        .await?;
        Ok(())
    }

    async fn delete_message(&self, channel_id: &str, message_id: &str) -> Result<()> {
        self.api_call(
            "DELETE",
            &format!("channels/{channel_id}/messages/{message_id}"),
            None,
        )
        .await?;
        Ok(())
    }

    async fn is_typing(&self, channel_id: &str) -> Result<()> {
        self.api_call(
            "POST",
            &format!("channels/{channel_id}/typing"),
            None,
        )
        .await?;
        Ok(())
    }

    fn create_consumer(&self, channel_id: &str, message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(DiscordConsumer::new(
            channel_id.to_string(),
            message_id,
            Arc::new(Self {
                config: self.config.clone(),
                client: self.client.clone(),
            }),
        ))
    }

    fn message_format(&self) -> MessageFormat {
        MessageFormat::Markdown
    }

    fn supports_edit_streaming(&self) -> bool {
        true
    }
}

/// Discord stream consumer that edits messages for streaming.
struct DiscordConsumer {
    channel_id: String,
    message_id: Option<String>,
    adapter: Arc<DiscordAdapter>,
    buffer: std::sync::Mutex<String>,
}

impl DiscordConsumer {
    fn new(channel_id: String, message_id: Option<String>, adapter: Arc<DiscordAdapter>) -> Self {
        Self {
            channel_id,
            message_id,
            adapter,
            buffer: std::sync::Mutex::new(String::new()),
        }
    }
}

#[async_trait]
impl StreamConsumer for DiscordConsumer {
    async fn on_text_delta(&self, delta: &str) -> Result<()> {
        let text = {
            let mut buf = self.buffer.lock().unwrap();
            buf.push_str(delta);
            buf.clone()
        };

        if let Some(msg_id) = &self.message_id {
            let _ = self.adapter.edit_message(&self.channel_id, msg_id, &text).await;
        } else {
            let _ = self.adapter.send_message(&self.channel_id, &text).await;
        }
        Ok(())
    }

    async fn on_tool_start(&self, tool_name: &str, _args: &str) -> Result<()> {
        let text = {
            let mut buf = self.buffer.lock().unwrap();
            buf.push_str(&format!("\n> _Running tool: {tool_name}_\n"));
            buf.clone()
        };
        if let Some(msg_id) = &self.message_id {
            let _ = self.adapter.edit_message(&self.channel_id, msg_id, &text).await;
        }
        Ok(())
    }

    async fn on_tool_complete(&self, tool_name: &str, _result: &str) -> Result<()> {
        let text = {
            let mut buf = self.buffer.lock().unwrap();
            buf.push_str(&format!("\n> _Tool {tool_name} completed_\n"));
            buf.clone()
        };
        if let Some(msg_id) = &self.message_id {
            let _ = self.adapter.edit_message(&self.channel_id, msg_id, &text).await;
        }
        Ok(())
    }

    async fn on_tool_error(&self, tool_name: &str, error: &str) -> Result<()> {
        let text = {
            let mut buf = self.buffer.lock().unwrap();
            buf.push_str(&format!("\n> _Tool {tool_name} failed: {error}_\n"));
            buf.clone()
        };
        if let Some(msg_id) = &self.message_id {
            let _ = self.adapter.edit_message(&self.channel_id, msg_id, &text).await;
        }
        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discord_config_from_platform_config() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "bot_token": "MTk4NjIyODA3NjM4MDI4Mjg4.GxKj0Q.abc123"
            }),
        };

        let dc_config = DiscordConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(dc_config.bot_token, "MTk4NjIyODA3NjM4MDI4Mjg4.GxKj0Q.abc123");
    }

    #[test]
    fn test_discord_config_missing_token() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({}),
        };

        assert!(DiscordConfig::from_platform_config(&platform_config).is_err());
    }
}
