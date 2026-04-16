use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::formatting::format_message;
use crate::runner::BufferingConsumer;

/// Slack platform adapter configuration.
#[derive(Debug, Clone)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: Option<String>,
}

impl SlackConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let bot_token = config
            .get_str("bot_token")
            .ok_or_else(|| anyhow::anyhow!("Slack bot_token not configured"))?;
        Ok(Self {
            bot_token,
            app_token: config.get_str("app_token"),
        })
    }

    pub fn api_base(&self) -> &str {
        "https://slack.com/api"
    }
}

/// Slack platform adapter.
///
/// Connects to the Slack Web API for message operations.
pub struct SlackAdapter {
    config: SlackConfig,
    client: reqwest::Client,
}

impl SlackAdapter {
    pub fn new(config: SlackConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let slack_config = SlackConfig::from_platform_config(config)?;
        Ok(Self::new(slack_config))
    }

    async fn api_call(&self, method: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/{}", self.config.api_base(), method);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.bot_token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;
        if body.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            Ok(body)
        } else {
            let error = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            Err(anyhow::anyhow!("Slack API error: {error}"))
        }
    }
}

#[async_trait]
impl PlatformAdapter for SlackAdapter {
    fn name(&self) -> &str { "slack" }

    async fn connect(&self) -> Result<()> {
        self.api_call("auth.test", serde_json::json!({}))
            .await
            .with_context(|| "Failed to connect to Slack API")?;
        tracing::info!("Slack adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Slack adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, channel: &str, text: &str) -> Result<String> {
        let formatted = format_message(text, MessageFormat::Mrkdwn);
        let body = serde_json::json!({
            "channel": channel,
            "text": formatted,
        });

        let resp = self.api_call("chat.postMessage", body).await?;
        let ts = resp
            .get("ts")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(ts)
    }

    async fn send_file(&self, _channel: &str, _path: &Path) -> Result<()> {
        // Slack file upload requires multipart/form-data
        tracing::warn!("Slack file upload not fully implemented");
        Ok(())
    }

    async fn send_animation(&self, channel: &str, path: &Path) -> Result<()> {
        self.send_file(channel, path).await
    }

    async fn send_voice(&self, _channel: &str, _path: &Path) -> Result<()> {
        tracing::warn!("Slack does not support voice messages via bot API");
        Ok(())
    }

    async fn send_sticker(&self, _channel: &str, _sticker_id: &str) -> Result<()> {
        tracing::warn!("Slack does not support sticker sending via bot API");
        Ok(())
    }

    async fn edit_message(&self, channel: &str, ts: &str, text: &str) -> Result<()> {
        let formatted = format_message(text, MessageFormat::Mrkdwn);
        let body = serde_json::json!({
            "channel": channel,
            "ts": ts,
            "text": formatted,
        });
        self.api_call("chat.update", body).await?;
        Ok(())
    }

    async fn delete_message(&self, channel: &str, ts: &str) -> Result<()> {
        let body = serde_json::json!({
            "channel": channel,
            "ts": ts,
        });
        self.api_call("chat.delete", body).await?;
        Ok(())
    }

    async fn is_typing(&self, _channel: &str) -> Result<()> {
        // Slack doesn't have a typing indicator API
        Ok(())
    }

    fn create_consumer(&self, channel: &str, _message_ts: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(BufferingConsumer::new(
            channel.to_string(),
            Arc::new(Self {
                config: self.config.clone(),
                client: self.client.clone(),
            }),
        ))
    }

    fn message_format(&self) -> MessageFormat {
        MessageFormat::Mrkdwn
    }

    fn supports_edit_streaming(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slack_config_from_platform_config() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "bot_token": "xoxb-1234567890-abc",
                "app_token": "xapp-1234567890-def"
            }),
        };

        let slack_config = SlackConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(slack_config.bot_token, "xoxb-1234567890-abc");
        assert_eq!(slack_config.app_token, Some("xapp-1234567890-def".to_string()));
    }

    #[test]
    fn test_slack_config_missing_token() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({}),
        };

        assert!(SlackConfig::from_platform_config(&platform_config).is_err());
    }
}
