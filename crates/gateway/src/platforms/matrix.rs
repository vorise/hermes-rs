use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, PlatformConfig, StreamConsumer};
use crate::runner::BufferingConsumer;

/// Configuration for the Matrix adapter.
#[derive(Debug, Clone)]
pub struct MatrixConfig {
    /// Matrix homeserver URL (e.g., "https://matrix.org").
    pub homeserver: String,
    /// Matrix access token for the bot user.
    pub access_token: String,
    /// Matrix user ID (e.g., "@hermes_bot:matrix.org").
    pub user_id: String,
    /// Whether to use end-to-end encryption.
    pub enable_encryption: bool,
}

impl MatrixConfig {
    pub fn new(homeserver: &str, access_token: &str, user_id: &str) -> Self {
        Self {
            homeserver: homeserver.to_string(),
            access_token: access_token.to_string(),
            user_id: user_id.to_string(),
            enable_encryption: false,
        }
    }

    /// Enable end-to-end encryption.
    pub fn with_encryption(mut self) -> Self {
        self.enable_encryption = true;
        self
    }

    /// Create from a platform config.
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let homeserver = config.get_str("homeserver")
            .ok_or_else(|| anyhow!("Matrix homeserver not configured"))?;
        let access_token = config.get_str("access_token")
            .ok_or_else(|| anyhow!("Matrix access_token not configured"))?;
        let user_id = config.get_str("user_id")
            .ok_or_else(|| anyhow!("Matrix user_id not configured"))?;
        let mut cfg = Self::new(&homeserver, &access_token, &user_id);
        if config.get_bool("enable_encryption").unwrap_or(false) {
            cfg.enable_encryption = true;
        }
        Ok(cfg)
    }
}

/// Incoming Matrix message.
#[derive(Debug, Clone)]
pub struct MatrixIncomingMessage {
    /// Sender Matrix user ID.
    pub sender: String,
    /// Room ID.
    pub room_id: String,
    /// Message text body.
    pub text: String,
    /// Matrix event ID.
    pub event_id: String,
    /// Whether this is a direct message.
    pub is_dm: bool,
    /// Message type (m.text, m.notice, m.emote).
    pub msgtype: String,
}

/// Matrix platform adapter using the Matrix Client-Server API.
///
/// Connects to a Matrix homeserver via the REST API to send and receive
/// end-to-end encrypted or plain messages.
pub struct MatrixAdapter {
    config: MatrixConfig,
    client: reqwest::Client,
    /// In-memory buffer for received messages.
    received: Arc<parking_lot::Mutex<Vec<MatrixIncomingMessage>>>,
    /// Current sync token (for long-polling).
    sync_token: Arc<parking_lot::Mutex<Option<String>>>,
}

impl MatrixAdapter {
    pub fn new(config: MatrixConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            received: Arc::new(parking_lot::Mutex::new(Vec::new())),
            sync_token: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    /// Matrix Client-Server API base URL.
    fn cs_api_base(&self) -> String {
        format!("{}/_matrix/client/r0", self.config.homeserver.trim_end_matches('/'))
    }

    /// Send a message via the Matrix Client-Server API.
    async fn send_matrix_message(&self, room_id: &str, text: &str) -> Result<String> {
        let base = self.cs_api_base();
        let url = format!("{base}/rooms/{}/send/m.room.message", url_encode(room_id));
        let body = serde_json::json!({
            "msgtype": "m.text",
            "body": text,
        });

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .json(&body)
            .send()
            .await
            .context("Failed to send Matrix message")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Matrix API error ({status}): {body}"));
        }

        let event_id: serde_json::Value = resp.json().await?;
        Ok(event_id["event_id"].as_str().unwrap_or("").to_string())
    }

    /// Sync with the homeserver to receive messages.
    pub async fn sync(&self, timeout_ms: u32) -> Result<Vec<MatrixIncomingMessage>> {
        let base = self.cs_api_base();
        let mut url = format!("{base}/sync?timeout={timeout_ms}", base = base);

        let token = self.sync_token.lock().clone();
        if let Some(ref t) = token {
            url.push_str(&format!("&since={}", url_encode(t)));
        }

        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .send()
            .await
            .context("Failed to sync with Matrix homeserver")?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let body: serde_json::Value = resp.json().await.unwrap_or_default();

        // Update sync token
        if let Some(next_batch) = body.get("next_batch").and_then(|v| v.as_str()) {
            *self.sync_token.lock() = Some(next_batch.to_string());
        }

        // Parse joined room events
        let mut incoming = Vec::new();
        if let Some(rooms) = body.get("rooms").and_then(|r| r.get("join")) {
            if let Some(obj) = rooms.as_object() {
                for (room_id, room_data) in obj {
                    if let Some(events) = room_data.get("timeline").and_then(|t| t.get("events")).and_then(|e| e.as_array()) {
                        for event in events {
                            if event.get("type").and_then(|v| v.as_str()) == Some("m.room.message") {
                                let sender = event.get("sender").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                let event_id = event.get("event_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

                                if let Some(content) = event.get("content") {
                                    let text = content.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let msgtype = content.get("msgtype").and_then(|v| v.as_str()).unwrap_or("m.text").to_string();

                                    // Skip own messages
                                    if sender == self.config.user_id {
                                        continue;
                                    }

                                    let is_dm = room_id.starts_with('!') && room_data.get("summary")
                                        .and_then(|s| s.get("m.joined_member_count"))
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0) <= 2;

                                    let msg = MatrixIncomingMessage {
                                        sender,
                                        room_id: room_id.clone(),
                                        text,
                                        event_id,
                                        is_dm,
                                        msgtype,
                                    };
                                    incoming.push(msg.clone());
                                    self.received.lock().push(msg);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(incoming)
    }

    /// Get received messages (for testing).
    pub fn get_received(&self) -> Vec<MatrixIncomingMessage> {
        self.received.lock().clone()
    }
}

#[async_trait]
impl PlatformAdapter for MatrixAdapter {
    fn name(&self) -> &str { "matrix" }

    async fn connect(&self) -> Result<()> {
        // Verify connectivity by checking whoami
        let base = self.cs_api_base();
        let url = format!("{base}/account/whoami", base = base);
        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                tracing::info!(user_id = %self.config.user_id, homeserver = %self.config.homeserver, "Matrix adapter connected");
            }
            _ => {
                tracing::warn!(user_id = %self.config.user_id, "Matrix whoami check failed, proceeding anyway");
            }
        }
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Matrix adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        self.send_matrix_message(chat_id, text).await
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        // Upload media then send as m.file message
        let base = self.cs_api_base();

        // Read file content
        let content = std::fs::read(path).context("Failed to read file for Matrix upload")?;
        let content_type = mime_guess::from_path(path).first_or_octet_stream().to_string();

        // Upload
        let upload_url = format!("{base}/upload?filename={}", url_encode(path.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
        let upload_resp = self.client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", content_type)
            .body(content)
            .send()
            .await
            .context("Failed to upload file to Matrix")?;

        if !upload_resp.status().is_success() {
            let body = upload_resp.text().await.unwrap_or_default();
            return Err(anyhow!("Matrix upload error: {body}"));
        }

        let upload_body: serde_json::Value = upload_resp.json().await?;
        let mxc_uri = upload_body["content_uri"].as_str().unwrap_or("").to_string();

        // Send file message
        let send_url = format!("{base}/rooms/{}/send/m.room.message", url_encode(chat_id));
        let file_body = serde_json::json!({
            "msgtype": "m.file",
            "body": path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
            "url": mxc_uri,
        });

        self.client
            .post(&send_url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .json(&file_body)
            .send()
            .await?;

        Ok(())
    }

    async fn send_animation(&self, _chat_id: &str, _path: &Path) -> Result<()> {
        // Send as file (Matrix doesn't have native animation type)
        Ok(())
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        let base = self.cs_api_base();

        let content = std::fs::read(path).context("Failed to read voice file")?;
        let upload_url = format!("{base}/upload?filename={}", url_encode(path.file_name().and_then(|n| n.to_str()).unwrap_or("voice.ogg")));
        let upload_resp = self.client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "audio/ogg")
            .body(content)
            .send()
            .await?;

        if !upload_resp.status().is_success() {
            return Ok(());
        }

        let upload_body: serde_json::Value = upload_resp.json().await.unwrap_or_default();
        let mxc_uri = upload_body["content_uri"].as_str().unwrap_or("").to_string();

        let send_url = format!("{base}/rooms/{}/send/m.room.message", url_encode(chat_id));
        let voice_body = serde_json::json!({
            "msgtype": "m.audio",
            "body": "Voice message",
            "url": mxc_uri,
        });

        let _ = self.client.post(&send_url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .json(&voice_body)
            .send()
            .await;

        Ok(())
    }

    async fn send_sticker(&self, chat_id: &str, sticker_id: &str) -> Result<()> {
        let base = self.cs_api_base();
        let url = format!("{base}/rooms/{}/send/m.sticker", url_encode(chat_id));
        let body = serde_json::json!({
            "body": "sticker",
            "url": sticker_id,
        });

        let _ = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .json(&body)
            .send()
            .await;

        Ok(())
    }

    async fn edit_message(&self, chat_id: &str, message_id: &str, text: &str) -> Result<()> {
        let base = self.cs_api_base();
        let url = format!("{base}/rooms/{}/send/m.room.message/{}", url_encode(chat_id), url_encode(message_id));
        let body = serde_json::json!({
            "msgtype": "m.text",
            "body": text,
            "m.new_content": {
                "msgtype": "m.text",
                "body": text,
            },
            "m.relates_to": {
                "rel_type": "m.replace",
                "event_id": message_id,
            },
        });

        self.client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .json(&body)
            .send()
            .await?;

        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // Matrix redaction would require the original event ID and redaction support
        tracing::warn!("Matrix: message redaction not fully implemented");
        Ok(())
    }

    async fn is_typing(&self, chat_id: &str) -> Result<()> {
        let base = self.cs_api_base();
        let url = format!("{base}/rooms/{}/typing/{}", url_encode(chat_id), url_encode(&self.config.user_id));
        let body = serde_json::json!({
            "typing": true,
            "timeout": 30000,
        });

        let _ = self.client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .json(&body)
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
                received: Arc::clone(&self.received),
                sync_token: Arc::clone(&self.sync_token),
            }),
        ))
    }

    fn message_format(&self) -> MessageFormat {
        MessageFormat::Html
    }

    fn supports_edit_streaming(&self) -> bool {
        true
    }
}

fn url_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_config_new() {
        let config = MatrixConfig::new("https://matrix.org", "token123", "@bot:matrix.org");
        assert_eq!(config.homeserver, "https://matrix.org");
        assert_eq!(config.access_token, "token123");
        assert_eq!(config.user_id, "@bot:matrix.org");
        assert!(!config.enable_encryption);
    }

    #[test]
    fn test_matrix_config_with_encryption() {
        let config = MatrixConfig::new("https://matrix.org", "token123", "@bot:matrix.org")
            .with_encryption();
        assert!(config.enable_encryption);
    }

    #[test]
    fn test_matrix_config_from_platform_config() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "homeserver": "https://matrix.org",
                "access_token": "tok123",
                "user_id": "@bot:matrix.org",
            }),
        };
        let mc = MatrixConfig::from_platform_config(&config).unwrap();
        assert_eq!(mc.homeserver, "https://matrix.org");
    }

    #[test]
    fn test_matrix_config_missing_homeserver() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({}),
        };
        assert!(MatrixConfig::from_platform_config(&config).is_err());
    }

    #[test]
    fn test_matrix_adapter_name() {
        let config = MatrixConfig::new("https://matrix.org", "tok", "@bot:matrix.org");
        let adapter = MatrixAdapter::new(config);
        assert_eq!(adapter.name(), "matrix");
    }

    #[test]
    fn test_matrix_message_format() {
        let config = MatrixConfig::new("https://matrix.org", "tok", "@bot:matrix.org");
        let adapter = MatrixAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Html);
    }

    #[test]
    fn test_matrix_supports_edit_streaming() {
        let config = MatrixConfig::new("https://matrix.org", "tok", "@bot:matrix.org");
        let adapter = MatrixAdapter::new(config);
        assert!(adapter.supports_edit_streaming());
    }

    #[test]
    fn test_matrix_received() {
        let config = MatrixConfig::new("https://matrix.org", "tok", "@bot:matrix.org");
        let adapter = MatrixAdapter::new(config);
        assert!(adapter.get_received().is_empty());
    }
}
