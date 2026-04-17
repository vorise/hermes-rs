use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, PlatformConfig, StreamConsumer};
use crate::runner::BufferingConsumer;

/// Configuration for the Mattermost adapter.
#[derive(Debug, Clone)]
pub struct MattermostConfig {
    /// Mattermost server URL (e.g., "https://chat.example.com").
    pub server_url: String,
    /// Bot access token.
    pub token: String,
    /// Optional: team name for auto-joining channels.
    pub team_name: Option<String>,
}

impl MattermostConfig {
    pub fn new(server_url: &str, token: &str) -> Self {
        Self {
            server_url: server_url.to_string(),
            token: token.to_string(),
            team_name: None,
        }
    }

    /// Set the team name.
    pub fn with_team(mut self, team_name: &str) -> Self {
        self.team_name = Some(team_name.to_string());
        self
    }

    /// Create from a platform config.
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let server_url = config.get_str("server_url")
            .ok_or_else(|| anyhow!("Mattermost server_url not configured"))?;
        let token = config.get_str("token")
            .ok_or_else(|| anyhow!("Mattermost token not configured"))?;
        let mut cfg = Self::new(&server_url, &token);
        if let Some(team) = config.get_str("team_name") {
            cfg.team_name = Some(team);
        }
        Ok(cfg)
    }
}

/// Incoming Mattermost message.
#[derive(Debug, Clone)]
pub struct MattermostIncomingMessage {
    /// Sender user ID.
    pub sender_id: String,
    /// Sender username.
    pub sender_name: String,
    /// Channel ID.
    pub channel_id: String,
    /// Post ID.
    pub post_id: String,
    /// Message text.
    pub text: String,
    /// Whether this is a direct message.
    pub is_dm: bool,
    /// Root post ID (for thread replies).
    pub root_id: Option<String>,
}

/// Mattermost platform adapter using the REST API v4.
///
/// Connects to a Mattermost server via the REST API v4 to send and receive
/// messages. Supports channels, direct messages, and threaded conversations.
pub struct MattermostAdapter {
    config: MattermostConfig,
    client: reqwest::Client,
    /// Bot's own user ID (resolved on connect).
    bot_user_id: Arc<parking_lot::Mutex<Option<String>>>,
    /// In-memory buffer for received messages.
    received: Arc<parking_lot::Mutex<Vec<MattermostIncomingMessage>>>,
}

impl MattermostAdapter {
    pub fn new(config: MattermostConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            bot_user_id: Arc::new(parking_lot::Mutex::new(None)),
            received: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }

    /// API base URL.
    fn api_base(&self) -> String {
        format!("{}/api/v4", self.config.server_url.trim_end_matches('/'))
    }

    /// Create an authenticated request builder.
    #[allow(dead_code)]
    fn authed_get(&self) -> reqwest::RequestBuilder {
        self.client
            .get("")
            .header("Authorization", format!("Bearer {}", self.config.token))
    }

    /// Get the bot's user ID.
    pub async fn resolve_bot_user_id(&self) -> Result<String> {
        let url = format!("{}/users/me", self.api_base());
        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .send()
            .await
            .context("Failed to resolve bot user ID")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Mattermost API error ({status}): {body}"));
        }

        let body: serde_json::Value = resp.json().await?;
        let user_id = body["id"].as_str()
            .ok_or_else(|| anyhow!("Missing user ID in response"))?
            .to_string();

        *self.bot_user_id.lock() = Some(user_id.clone());
        Ok(user_id)
    }

    /// Send a post (message) to a channel.
    async fn send_post(&self, channel_id: &str, text: &str, root_id: Option<&str>) -> Result<String> {
        let url = format!("{}/posts", self.api_base());
        let mut body = serde_json::json!({
            "channel_id": channel_id,
            "message": text,
        });
        if let Some(rid) = root_id {
            body["root_id"] = serde_json::json!(rid);
        }

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .json(&body)
            .send()
            .await
            .context("Failed to send Mattermost post")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Mattermost API error ({status}): {body}"));
        }

        let result: serde_json::Value = resp.json().await?;
        Ok(result["id"].as_str().unwrap_or("").to_string())
    }

    /// Poll posts in a channel since a given timestamp.
    pub async fn poll_posts(&self, channel_id: &str, after: i64) -> Result<Vec<MattermostIncomingMessage>> {
        let url = format!("{}/channels/{}/posts?after={}&per_page=60",
            self.api_base(), channel_id, after);

        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .send()
            .await
            .context("Failed to poll Mattermost posts")?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let posts: serde_json::Value = resp.json().await.unwrap_or_default();
        let bot_id = self.bot_user_id.lock().clone().unwrap_or_default();

        let mut messages = Vec::new();

        if let Some(obj) = posts.as_object() {
            for (_key, post) in obj {
                let user_id = post["user_id"].as_str().unwrap_or("").to_string();
                if user_id == bot_id {
                    continue;
                }

                let text = post["message"].as_str().unwrap_or("").to_string();
                let channel_id_val = post["channel_id"].as_str().unwrap_or("").to_string();
                let post_id = post["id"].as_str().unwrap_or("").to_string();
                let root_id = post["root_id"].as_str().map(String::from);
                let sender_name = post["props"]["sender_name"].as_str()
                    .or_else(|| post["username"].as_str())
                    .unwrap_or("unknown")
                    .to_string();

                // Determine if DM: channel type would be 'D' or 'G'
                let is_dm = channel_id.len() == 26; // heuristic: DM channel IDs are typically shorter

                let msg = MattermostIncomingMessage {
                    sender_id: user_id,
                    sender_name,
                    channel_id: channel_id_val,
                    post_id,
                    text,
                    is_dm,
                    root_id,
                };
                self.received.lock().push(msg.clone());
                messages.push(msg);
            }
        }

        Ok(messages)
    }

    /// Get received messages (for testing).
    pub fn get_received(&self) -> Vec<MattermostIncomingMessage> {
        self.received.lock().clone()
    }
}

#[async_trait]
impl PlatformAdapter for MattermostAdapter {
    fn name(&self) -> &str { "mattermost" }

    async fn connect(&self) -> Result<()> {
        let user_id = self.resolve_bot_user_id().await?;
        tracing::info!(user_id = %user_id, server = %self.config.server_url, "Mattermost adapter connected");
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Mattermost adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        self.send_post(chat_id, text, None).await
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        let api_base = self.api_base();

        // Upload file
        let upload_url = format!("{}/files", api_base);
        let content = std::fs::read(path).context("Failed to read file")?;
        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let file_name_for_msg = file_name.clone();

        let form = reqwest::multipart::Form::new()
            .text("channel_id", chat_id.to_string())
            .part("files", reqwest::multipart::Part::bytes(content)
                .file_name(file_name));

        let resp = self.client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file to Mattermost")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Mattermost upload error ({status}): {body}"));
        }

        // Send post with file info
        let msg_text = format!("Shared a file: {file_name_for_msg}");
        self.send_post(chat_id, &msg_text, None).await?;

        Ok(())
    }

    async fn send_animation(&self, _chat_id: &str, _path: &Path) -> Result<()> {
        // Send as file (Mattermost doesn't have native animation type)
        Ok(())
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        // Mattermost custom emoji would require emoji_id, not supported generically
        tracing::warn!("Mattermost: sticker sending not supported");
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, message_id: &str, text: &str) -> Result<()> {
        let url = format!("{}/posts/{}", self.api_base(), message_id);
        let body = serde_json::json!({
            "id": message_id,
            "message": text,
        });

        let resp = self.client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .json(&body)
            .send()
            .await
            .context("Failed to edit Mattermost post")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Mattermost edit error ({status}): {body}"));
        }

        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, message_id: &str) -> Result<()> {
        let url = format!("{}/posts/{}", self.api_base(), message_id);
        let body = serde_json::json!({});

        let _ = self.client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .json(&body)
            .send()
            .await;

        Ok(())
    }

    async fn is_typing(&self, chat_id: &str) -> Result<()> {
        let url = format!("{}/channels/{}/typing", self.api_base(), chat_id);
        let _ = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .send()
            .await;
        Ok(())
    }

    fn create_consumer(&self, chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(BufferingConsumer::new(
            chat_id.to_string(),
            Arc::new(Self {
                config: self.config.clone(),
                client: self.client.clone(),
                bot_user_id: Arc::clone(&self.bot_user_id),
                received: Arc::clone(&self.received),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mattermost_config_new() {
        let config = MattermostConfig::new("https://chat.example.com", "token123");
        assert_eq!(config.server_url, "https://chat.example.com");
        assert_eq!(config.token, "token123");
        assert!(config.team_name.is_none());
    }

    #[test]
    fn test_mattermost_config_with_team() {
        let config = MattermostConfig::new("https://chat.example.com", "token123")
            .with_team("myteam");
        assert_eq!(config.team_name, Some("myteam".to_string()));
    }

    #[test]
    fn test_mattermost_config_from_platform_config() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "server_url": "https://chat.example.com",
                "token": "tok123",
                "team_name": "engineering"
            }),
        };
        let mc = MattermostConfig::from_platform_config(&config).unwrap();
        assert_eq!(mc.server_url, "https://chat.example.com");
        assert_eq!(mc.team_name, Some("engineering".to_string()));
    }

    #[test]
    fn test_mattermost_config_missing_server() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "token": "tok123"
            }),
        };
        assert!(MattermostConfig::from_platform_config(&config).is_err());
    }

    #[test]
    fn test_mattermost_adapter_name() {
        let config = MattermostConfig::new("https://chat.example.com", "tok");
        let adapter = MattermostAdapter::new(config);
        assert_eq!(adapter.name(), "mattermost");
    }

    #[test]
    fn test_mattermost_message_format() {
        let config = MattermostConfig::new("https://chat.example.com", "tok");
        let adapter = MattermostAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Markdown);
    }

    #[test]
    fn test_mattermost_supports_edit_streaming() {
        let config = MattermostConfig::new("https://chat.example.com", "tok");
        let adapter = MattermostAdapter::new(config);
        assert!(adapter.supports_edit_streaming());
    }

    #[test]
    fn test_mattermost_received() {
        let config = MattermostConfig::new("https://chat.example.com", "tok");
        let adapter = MattermostAdapter::new(config);
        assert!(adapter.get_received().is_empty());
    }

    #[test]
    fn test_mattermost_api_base() {
        let config = MattermostConfig::new("https://chat.example.com/", "tok");
        let adapter = MattermostAdapter::new(config);
        assert_eq!(adapter.api_base(), "https://chat.example.com/api/v4");
    }

    #[test]
    fn test_mattermost_config_trailing_slash() {
        let config = MattermostConfig::new("https://chat.example.com/", "tok");
        let adapter = MattermostAdapter::new(config);
        assert_eq!(adapter.api_base(), "https://chat.example.com/api/v4");
    }
}
