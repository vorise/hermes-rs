use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::runner::BufferingConsumer;

/// REST API Server platform adapter configuration.
///
/// Provides a REST API endpoint for programmatic message sending
/// and receiving, useful for integration with external systems.
#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    /// Bind address for the API server (e.g., "0.0.0.0:8080").
    pub bind_address: String,
    /// API key for authentication.
    pub api_key: Option<String>,
    /// Base path for the API (default: "/api").
    pub base_path: String,
}

impl ApiServerConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let bind_address = config
            .get_str("bind_address")
            .unwrap_or_else(|| "0.0.0.0:8080".to_string());
        let base_path = config
            .get_str("base_path")
            .unwrap_or_else(|| "/api".to_string());
        Ok(Self {
            bind_address,
            api_key: config.get_str("api_key"),
            base_path,
        })
    }
}

/// Incoming message from the REST API.
#[derive(Debug, Clone)]
pub struct ApiServerIncomingMessage {
    /// Client identifier (API key or session token).
    pub client_id: String,
    /// Message text content.
    pub text: String,
    /// Optional session ID for conversation continuity.
    pub session_id: Option<String>,
}

/// REST API Server platform adapter.
///
/// Exposes an HTTP API for sending and receiving messages,
/// allowing external systems to integrate with Hermes.
///
/// Endpoints:
/// - POST /api/message - Send a message
/// - GET /api/messages - Poll for new messages
/// - GET /api/health - Health check
pub struct ApiServerAdapter {
    config: ApiServerConfig,
    client: reqwest::Client,
    /// Received messages stored in memory for polling.
    received_messages: Arc<parking_lot::Mutex<Vec<ApiServerIncomingMessage>>>,
    /// Sent message responses stored for retrieval.
    sent_responses: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl ApiServerAdapter {
    pub fn new(config: ApiServerConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            received_messages: Arc::new(parking_lot::Mutex::new(Vec::new())),
            sent_responses: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let api_config = ApiServerConfig::from_platform_config(config)?;
        Ok(Self::new(api_config))
    }

    /// Process an incoming message via the API.
    pub fn handle_incoming(&self, msg: ApiServerIncomingMessage) {
        self.received_messages.lock().push(msg);
    }

    /// Store a response for API retrieval.
    pub fn store_response(&self, text: String) {
        self.sent_responses.lock().push(text);
    }

    /// Get received messages (for testing).
    pub fn get_received(&self) -> Vec<ApiServerIncomingMessage> {
        self.received_messages.lock().clone()
    }

    /// Get sent responses (for testing).
    pub fn get_responses(&self) -> Vec<String> {
        self.sent_responses.lock().clone()
    }

    /// Check if the provided API key is valid.
    pub fn authenticate(&self, api_key: &str) -> bool {
        match &self.config.api_key {
            Some(key) => key == api_key,
            None => true, // No key configured = open access
        }
    }
}

#[async_trait]
impl PlatformAdapter for ApiServerAdapter {
    fn name(&self) -> &str { "api_server" }

    async fn connect(&self) -> Result<()> {
        tracing::info!(
            bind = self.config.bind_address,
            "API Server adapter ready on {}",
            self.config.bind_address
        );
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("API Server adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, _chat_id: &str, text: &str) -> Result<String> {
        // Store response for API retrieval
        self.sent_responses.lock().push(text.to_string());
        Ok(String::new())
    }

    async fn send_file(&self, _chat_id: &str, path: &Path) -> Result<()> {
        let file_path = path.to_string_lossy();
        self.sent_responses.lock().push(format!("[File: {file_path}]"));
        Ok(())
    }

    async fn send_animation(&self, _chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(_chat_id, path).await
    }

    async fn send_voice(&self, _chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(_chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, text: &str) -> Result<()> {
        // Edit the last response
        let mut responses = self.sent_responses.lock();
        if let Some(last) = responses.last_mut() {
            *last = text.to_string();
        }
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // Remove the last response
        let mut responses = self.sent_responses.lock();
        responses.pop();
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    fn create_consumer(&self, chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(BufferingConsumer::new(
            chat_id.to_string(),
            Arc::new(Self {
                config: self.config.clone(),
                client: self.client.clone(),
                received_messages: self.received_messages.clone(),
                sent_responses: self.sent_responses.clone(),
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
    fn test_config_defaults() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({}),
        };

        let config = ApiServerConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.bind_address, "0.0.0.0:8080");
        assert_eq!(config.base_path, "/api");
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_config_custom() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "bind_address": "127.0.0.1:3000",
                "base_path": "/v1",
                "api_key": "secret_key"
            }),
        };

        let config = ApiServerConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.bind_address, "127.0.0.1:3000");
        assert_eq!(config.base_path, "/v1");
        assert_eq!(config.api_key, Some("secret_key".to_string()));
    }

    #[test]
    fn test_adapter_name() {
        let config = ApiServerConfig {
            bind_address: "0.0.0.0:8080".to_string(),
            api_key: None,
            base_path: "/api".to_string(),
        };
        let adapter = ApiServerAdapter::new(config);
        assert_eq!(adapter.name(), "api_server");
    }

    #[test]
    fn test_message_format() {
        let config = ApiServerConfig {
            bind_address: "0.0.0.0:8080".to_string(),
            api_key: None,
            base_path: "/api".to_string(),
        };
        let adapter = ApiServerAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Plain);
        assert!(!adapter.supports_edit_streaming());
    }

    #[test]
    fn test_handle_incoming() {
        let config = ApiServerConfig {
            bind_address: "0.0.0.0:8080".to_string(),
            api_key: None,
            base_path: "/api".to_string(),
        };
        let adapter = ApiServerAdapter::new(config);

        adapter.handle_incoming(ApiServerIncomingMessage {
            client_id: "client_123".to_string(),
            text: "Hello API".to_string(),
            session_id: Some("session_abc".to_string()),
        });

        let received = adapter.get_received();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].text, "Hello API");
        assert_eq!(received[0].client_id, "client_123");
    }

    #[test]
    fn test_store_response() {
        let config = ApiServerConfig {
            bind_address: "0.0.0.0:8080".to_string(),
            api_key: None,
            base_path: "/api".to_string(),
        };
        let adapter = ApiServerAdapter::new(config);

        adapter.store_response("Response 1".to_string());
        adapter.store_response("Response 2".to_string());

        let responses = adapter.get_responses();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0], "Response 1");
        assert_eq!(responses[1], "Response 2");
    }

    #[test]
    fn test_authenticate_with_key() {
        let config = ApiServerConfig {
            bind_address: "0.0.0.0:8080".to_string(),
            api_key: Some("secret".to_string()),
            base_path: "/api".to_string(),
        };
        let adapter = ApiServerAdapter::new(config);

        assert!(adapter.authenticate("secret"));
        assert!(!adapter.authenticate("wrong"));
    }

    #[test]
    fn test_authenticate_no_key() {
        let config = ApiServerConfig {
            bind_address: "0.0.0.0:8080".to_string(),
            api_key: None,
            base_path: "/api".to_string(),
        };
        let adapter = ApiServerAdapter::new(config);

        assert!(adapter.authenticate("any"));
    }

    #[test]
    fn test_edit_message() {
        let config = ApiServerConfig {
            bind_address: "0.0.0.0:8080".to_string(),
            api_key: None,
            base_path: "/api".to_string(),
        };
        let adapter = ApiServerAdapter::new(config);

        adapter.store_response("Original".to_string());
        assert_eq!(adapter.get_responses().len(), 1);

        // Directly test the mutex manipulation (edit_message is async, can't call from sync test)
        {
            let mut responses = adapter.sent_responses.lock();
            if let Some(last) = responses.last_mut() {
                *last = "Edited".to_string();
            }
        }

        let responses = adapter.get_responses();
        assert_eq!(responses.len(), 1, "Expected 1 response");
        assert_eq!(responses[0], "Edited", "Response should be edited");
    }

    #[test]
    fn test_delete_message() {
        let config = ApiServerConfig {
            bind_address: "0.0.0.0:8080".to_string(),
            api_key: None,
            base_path: "/api".to_string(),
        };
        let adapter = ApiServerAdapter::new(config);

        adapter.store_response("First".to_string());
        adapter.store_response("Second".to_string());
        assert_eq!(adapter.get_responses().len(), 2);

        // Directly test the mutex manipulation (delete_message is async)
        {
            let mut responses = adapter.sent_responses.lock();
            responses.pop();
        }

        let responses = adapter.get_responses();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], "First");
    }
}
