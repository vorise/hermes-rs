use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use parking_lot::Mutex;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;

/// Telegram platform adapter configuration.
#[derive(Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub webhook_url: Option<String>,
    pub webhook_port: Option<u16>,
    pub parse_mode: Option<String>,
}

impl TelegramConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let bot_token = config
            .get_str("bot_token")
            .ok_or_else(|| anyhow::anyhow!("Telegram bot_token not configured"))?;

        Ok(Self {
            bot_token,
            webhook_url: config.get_str("webhook_url"),
            webhook_port: config.get_bool("webhook_port").map(|v| v as u16),
            parse_mode: config.get_str("parse_mode"),
        })
    }

    pub fn api_base(&self) -> String {
        format!("https://api.telegram.org/bot{}", self.bot_token)
    }
}

/// Telegram platform adapter.
///
/// Connects to the Telegram Bot API via HTTP.
/// Supports long polling or webhook mode.
pub struct TelegramAdapter {
    config: TelegramConfig,
    client: reqwest::Client,
    sent_message_id: Mutex<Option<String>>,
}

impl TelegramAdapter {
    pub fn new(config: TelegramConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            sent_message_id: Mutex::new(None),
        }
    }

    /// Try to create a Telegram adapter from a platform config.
    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let tg_config = TelegramConfig::from_platform_config(config)?;
        Ok(Self::new(tg_config))
    }

    /// Make an API call to Telegram.
    async fn api_call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/{}", self.config.api_base(), method);
        let resp = self.client.post(&url).json(&params).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Telegram API error: {} - {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        if body.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            Ok(body)
        } else {
            let desc = body
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            Err(anyhow::anyhow!("Telegram API error: {desc}"))
        }
    }
}

#[async_trait]
impl PlatformAdapter for TelegramAdapter {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn connect(&self) -> Result<()> {
        // Test connection by getting bot info
        self.api_call("getMe", serde_json::json!({}))
            .await
            .with_context(|| "Failed to connect to Telegram API")?;
        tracing::info!("Telegram adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Telegram adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        let parse_mode = self.config.parse_mode.as_deref().unwrap_or("MarkdownV2");
        let params = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": parse_mode,
        });

        let resp = self.api_call("sendMessage", params).await?;
        let msg_id = resp
            .get("result")
            .and_then(|r| r.get("message_id"))
            .and_then(|id| id.as_u64())
            .unwrap_or(0)
            .to_string();

        *self.sent_message_id.lock() = Some(msg_id.clone());
        Ok(msg_id)
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        // Use sendDocument
        let params = serde_json::json!({
            "chat_id": chat_id,
            "caption": file_name,
        });

        // For file upload, we'd need multipart/form-data in production
        self.api_call("sendDocument", params).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("animation");

        let params = serde_json::json!({
            "chat_id": chat_id,
            "caption": file_name,
        });

        self.api_call("sendAnimation", params).await?;
        Ok(())
    }

    async fn send_voice(&self, chat_id: &str, _path: &Path) -> Result<()> {
        let params = serde_json::json!({
            "chat_id": chat_id,
        });
        self.api_call("sendVoice", params).await?;
        Ok(())
    }

    async fn send_sticker(&self, chat_id: &str, sticker_id: &str) -> Result<()> {
        let params = serde_json::json!({
            "chat_id": chat_id,
            "sticker": sticker_id,
        });
        self.api_call("sendSticker", params).await?;
        Ok(())
    }

    async fn edit_message(&self, chat_id: &str, message_id: &str, text: &str) -> Result<()> {
        let parse_mode = self.config.parse_mode.as_deref().unwrap_or("MarkdownV2");
        let params = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "parse_mode": parse_mode,
        });
        self.api_call("editMessageText", params).await?;
        Ok(())
    }

    async fn delete_message(&self, chat_id: &str, message_id: &str) -> Result<()> {
        let params = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
        });
        self.api_call("deleteMessage", params).await?;
        Ok(())
    }

    async fn is_typing(&self, chat_id: &str) -> Result<()> {
        let params = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing",
        });
        self.api_call("sendChatAction", params).await?;
        Ok(())
    }

    fn create_consumer(&self, chat_id: &str, message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(TelegramConsumer::new(
            chat_id.to_string(),
            message_id,
            Arc::new(Self {
                config: self.config.clone(),
                client: self.client.clone(),
                sent_message_id: Mutex::new(None),
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

/// Telegram-specific stream consumer that edits messages for streaming.
struct TelegramConsumer {
    chat_id: String,
    message_id: Option<String>,
    adapter: Arc<TelegramAdapter>,
    buffer: std::sync::Mutex<String>,
}

impl TelegramConsumer {
    fn new(chat_id: String, message_id: Option<String>, adapter: Arc<TelegramAdapter>) -> Self {
        Self {
            chat_id,
            message_id,
            adapter,
            buffer: std::sync::Mutex::new(String::new()),
        }
    }
}

#[async_trait]
impl StreamConsumer for TelegramConsumer {
    async fn on_text_delta(&self, delta: &str) -> Result<()> {
        let text = {
            let mut buf = self.buffer.lock().unwrap();
            buf.push_str(delta);
            buf.clone()
        };

        if let Some(msg_id) = &self.message_id {
            let _ = self.adapter.edit_message(&self.chat_id, msg_id, &text).await;
        } else {
            let _ = self.adapter.send_message(&self.chat_id, &text).await;
        }
        Ok(())
    }

    async fn on_tool_start(&self, tool_name: &str, _args_preview: &str) -> Result<()> {
        let text = {
            let mut buf = self.buffer.lock().unwrap();
            buf.push_str(&format!("\n[_Running: {tool_name}_]\n"));
            buf.clone()
        };
        if let Some(msg_id) = &self.message_id {
            let _ = self.adapter.edit_message(&self.chat_id, msg_id, &text).await;
        }
        Ok(())
    }

    async fn on_tool_complete(&self, tool_name: &str, _result_preview: &str) -> Result<()> {
        let text = {
            let mut buf = self.buffer.lock().unwrap();
            buf.push_str(&format!("\n[_Done: {tool_name}_]\n"));
            buf.clone()
        };
        if let Some(msg_id) = &self.message_id {
            let _ = self.adapter.edit_message(&self.chat_id, msg_id, &text).await;
        }
        Ok(())
    }

    async fn on_tool_error(&self, tool_name: &str, error: &str) -> Result<()> {
        let text = {
            let mut buf = self.buffer.lock().unwrap();
            buf.push_str(&format!("\n[_Error: {tool_name}: {error}_]\n"));
            buf.clone()
        };
        if let Some(msg_id) = &self.message_id {
            let _ = self.adapter.edit_message(&self.chat_id, msg_id, &text).await;
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
    fn test_telegram_config_from_platform_config() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "bot_token": "12345:ABC-def",
                "parse_mode": "Markdown"
            }),
        };

        let tg_config = TelegramConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(tg_config.bot_token, "12345:ABC-def");
        assert_eq!(tg_config.parse_mode, Some("Markdown".to_string()));
        assert!(tg_config.webhook_url.is_none());
    }

    #[test]
    fn test_telegram_config_missing_token() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({}),
        };

        assert!(TelegramConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_telegram_api_base() {
        let config = TelegramConfig {
            bot_token: "test_token".to_string(),
            webhook_url: None,
            webhook_port: None,
            parse_mode: None,
        };
        assert_eq!(
            config.api_base(),
            "https://api.telegram.org/bottest_token"
        );
    }
}
