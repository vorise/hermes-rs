use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use parking_lot::Mutex;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// Telegram Network (TgNet) platform adapter configuration.
///
/// TgNet enables distributed Hermes deployments where multiple instances
/// communicate via Telegram as a mesh network. Each instance has its own
/// bot token and can relay messages to other instances in the network.
#[derive(Debug, Clone)]
pub struct TgNetConfig {
    /// Bot token for this instance.
    pub bot_token: String,
    /// Network identifier (shared across all instances in the mesh).
    pub network_id: String,
    /// This instance's node ID within the network.
    pub node_id: String,
    /// Other nodes' bot tokens in the network for message relaying.
    pub peer_tokens: Vec<String>,
}

impl TgNetConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let bot_token = config
            .get_str("bot_token")
            .ok_or_else(|| anyhow::anyhow!("TgNet bot_token not configured"))?;
        let network_id = config
            .get_str("network_id")
            .unwrap_or_else(|| "default".to_string());
        let node_id = config
            .get_str("node_id")
            .unwrap_or_else(|| "node-1".to_string());
        let peer_tokens = config
            .get_str("peer_tokens")
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
            .unwrap_or_default();

        Ok(Self {
            bot_token,
            network_id,
            node_id,
            peer_tokens,
        })
    }

    pub fn api_base(&self) -> String {
        format!("https://api.telegram.org/bot{}", self.bot_token)
    }
}

/// Incoming message from TgNet.
#[derive(Debug, Clone)]
pub struct TgNetIncomingMessage {
    /// Sender's open ID.
    pub from_user: String,
    /// Message content.
    pub content: String,
    /// Source node ID.
    pub source_node: String,
    /// Target node ID (empty = broadcast).
    pub target_node: String,
    /// Network identifier.
    pub network_id: String,
}

/// Telegram Network (TgNet) platform adapter.
///
/// Connects multiple Hermes instances via Telegram for distributed deployments.
/// Each instance runs its own bot and relays messages to peers in the network.
pub struct TgNetAdapter {
    config: TgNetConfig,
    client: reqwest::Client,
    sent_message_id: Mutex<Option<String>>,
}

impl TgNetAdapter {
    pub fn new(config: TgNetConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            sent_message_id: Mutex::new(None),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let tg_config = TgNetConfig::from_platform_config(config)?;
        Ok(Self::new(tg_config))
    }

    /// Make an API call to Telegram.
    async fn api_call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/{}", self.config.api_base(), method);
        let resp = self.client.post(&url).json(&params).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "TgNet Telegram API error: {} - {}",
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
            Err(anyhow::anyhow!("TgNet Telegram API error: {desc}"))
        }
    }

    /// Relay a message to all peer nodes in the network.
    #[allow(dead_code)]
    async fn relay_to_peers(&self, _content: &str) -> Result<()> {
        for peer_token in &self.config.peer_tokens {
            let peer_api = format!("https://api.telegram.org/bot{peer_token}/sendMessage");
            let body = serde_json::json!({
                "chat_id": "self",
                "text": format!("[TgNet relay from {}] {}", self.config.node_id, _content),
                "parse_mode": "MarkdownV2",
            });
            // Fire-and-forget relay — log errors but don't fail the response
            match self.client.post(&peer_api).json(&body).send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        tracing::warn!(peer_token = %peer_token.chars().take(8).collect::<String>(), "TgNet relay to peer failed");
                    }
                }
                Err(e) => {
                    tracing::warn!(peer_token = %peer_token.chars().take(8).collect::<String>(), error = %e, "TgNet relay network error");
                }
            }
        }
        Ok(())
    }

    /// Process an incoming relayed message.
    pub fn handle_incoming(&self, msg: TgNetIncomingMessage) {
        tracing::info!(
            source_node = %msg.source_node,
            target_node = %msg.target_node,
            network = %msg.network_id,
            "TgNet received relayed message"
        );
        // In production, this would dispatch to the query loop.
        // For now, we log the incoming message.
    }

    /// Get the network ID.
    pub fn network_id(&self) -> &str {
        &self.config.network_id
    }

    /// Get the node ID.
    pub fn node_id(&self) -> &str {
        &self.config.node_id
    }

    /// Get the number of configured peers.
    pub fn peer_count(&self) -> usize {
        self.config.peer_tokens.len()
    }
}

#[async_trait]
impl PlatformAdapter for TgNetAdapter {
    fn name(&self) -> &str {
        "telegram_network"
    }

    async fn connect(&self) -> Result<()> {
        // Test connection by getting bot info
        self.api_call("getMe", serde_json::json!({}))
            .await
            .with_context(|| "Failed to connect TgNet to Telegram API")?;
        tracing::info!(
            network_id = %self.config.network_id,
            node_id = %self.config.node_id,
            peers = self.config.peer_tokens.len(),
            "TgNet adapter connected"
        );
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!(
            network_id = %self.config.network_id,
            node_id = %self.config.node_id,
            "TgNet adapter disconnected"
        );
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        let params = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "MarkdownV2",
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

        let params = serde_json::json!({
            "chat_id": chat_id,
            "caption": file_name,
        });

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
        let params = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "parse_mode": "MarkdownV2",
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

    fn create_consumer(&self, chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(BufferingConsumer::new(
            chat_id.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_platform_config() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "bot_token": "12345:ABC-def-tgnet",
                "network_id": "prod-cluster",
                "node_id": "node-alpha",
                "peer_tokens": "peer_token_1,peer_token_2"
            }),
        };

        let config = TgNetConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.bot_token, "12345:ABC-def-tgnet");
        assert_eq!(config.network_id, "prod-cluster");
        assert_eq!(config.node_id, "node-alpha");
        assert_eq!(config.peer_tokens.len(), 2);
        assert_eq!(config.peer_tokens[0], "peer_token_1");
        assert_eq!(config.peer_tokens[1], "peer_token_2");
    }

    #[test]
    fn test_config_defaults() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "bot_token": "12345:ABC"
            }),
        };

        let config = TgNetConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.network_id, "default");
        assert_eq!(config.node_id, "node-1");
        assert!(config.peer_tokens.is_empty());
    }

    #[test]
    fn test_config_missing_token() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({}),
        };
        assert!(TgNetConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_api_base() {
        let config = TgNetConfig {
            bot_token: "test_token".to_string(),
            network_id: "default".to_string(),
            node_id: "node-1".to_string(),
            peer_tokens: vec![],
        };
        assert_eq!(
            config.api_base(),
            "https://api.telegram.org/bottest_token"
        );
    }

    #[test]
    fn test_adapter_name() {
        let config = TgNetConfig {
            bot_token: "test".to_string(),
            network_id: "test-net".to_string(),
            node_id: "node-1".to_string(),
            peer_tokens: vec![],
        };
        let adapter = TgNetAdapter::new(config);
        assert_eq!(adapter.name(), "telegram_network");
    }

    #[test]
    fn test_message_format() {
        let config = TgNetConfig {
            bot_token: "test".to_string(),
            network_id: "test-net".to_string(),
            node_id: "node-1".to_string(),
            peer_tokens: vec![],
        };
        let adapter = TgNetAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Markdown);
        assert!(adapter.supports_edit_streaming());
    }

    #[test]
    fn test_incoming_message() {
        let msg = TgNetIncomingMessage {
            from_user: "user123".to_string(),
            content: "Hello from peer".to_string(),
            source_node: "node-beta".to_string(),
            target_node: "node-alpha".to_string(),
            network_id: "prod-cluster".to_string(),
        };
        assert_eq!(msg.from_user, "user123");
        assert_eq!(msg.content, "Hello from peer");
        assert_eq!(msg.source_node, "node-beta");
        assert_eq!(msg.target_node, "node-alpha");
    }

    #[test]
    fn test_peer_count() {
        let config = TgNetConfig {
            bot_token: "test".to_string(),
            network_id: "test-net".to_string(),
            node_id: "node-1".to_string(),
            peer_tokens: vec!["token1".to_string(), "token2".to_string(), "token3".to_string()],
        };
        let adapter = TgNetAdapter::new(config);
        assert_eq!(adapter.peer_count(), 3);
    }

    #[test]
    fn test_network_id_and_node_id() {
        let config = TgNetConfig {
            bot_token: "test".to_string(),
            network_id: "my-network".to_string(),
            node_id: "node-gamma".to_string(),
            peer_tokens: vec![],
        };
        let adapter = TgNetAdapter::new(config);
        assert_eq!(adapter.network_id(), "my-network");
        assert_eq!(adapter.node_id(), "node-gamma");
    }
}
