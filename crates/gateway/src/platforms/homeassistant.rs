use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, PlatformConfig, StreamConsumer};
use h_core::NoOpConsumer;

/// Configuration for the Home Assistant adapter.
#[derive(Debug, Clone)]
pub struct HomeAssistantConfig {
    /// Home Assistant server URL (e.g., "http://homeassistant.local:8123").
    pub url: String,
    /// Long-lived access token.
    pub token: String,
    /// Default notification target (entity_id, notify service, or conversation agent).
    pub default_target: Option<String>,
}

impl HomeAssistantConfig {
    pub fn new(url: &str, token: &str) -> Self {
        Self {
            url: url.to_string(),
            token: token.to_string(),
            default_target: None,
        }
    }

    /// Set the default notification target.
    pub fn with_default_target(mut self, target: &str) -> Self {
        self.default_target = Some(target.to_string());
        self
    }

    /// Create from a platform config.
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let url = config.get_str("url")
            .ok_or_else(|| anyhow!("Home Assistant url not configured"))?;
        let token = config.get_str("token")
            .ok_or_else(|| anyhow!("Home Assistant token not configured"))?;
        let mut cfg = Self::new(&url, &token);
        if let Some(target) = config.get_str("default_target") {
            cfg.default_target = Some(target);
        }
        Ok(cfg)
    }
}

/// Incoming Home Assistant event.
#[derive(Debug, Clone)]
pub struct HomeAssistantEvent {
    /// Event type (e.g., "state_changed", "call_service").
    pub event_type: String,
    /// Event data as raw JSON.
    pub data: serde_json::Value,
    /// Optional chat_id for routing.
    pub chat_id: Option<String>,
}

/// Home Assistant platform adapter using the REST API.
///
/// Connects to a Home Assistant instance via the REST API to deliver
/// notifications, query entity states, and invoke services.
///
/// Unlike traditional chat platforms, Home Assistant is primarily used
/// for notifications and entity control rather than conversational chat.
pub struct HomeAssistantAdapter {
    config: HomeAssistantConfig,
    client: reqwest::Client,
    /// In-memory buffer for received events.
    received: Arc<parking_lot::Mutex<Vec<HomeAssistantEvent>>>,
}

impl HomeAssistantAdapter {
    pub fn new(config: HomeAssistantConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            received: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }

    /// API base URL.
    fn api_base(&self) -> String {
        format!("{}/api", self.config.url.trim_end_matches('/'))
    }

    /// Call a Home Assistant service.
    ///
    /// Example: call_service("notify", "persistent_notification", &json!({"message": "Hello"}))
    pub async fn call_service(&self, domain: &str, service: &str, data: &serde_json::Value) -> Result<()> {
        let url = format!("{}/services/{}/{}", self.api_base(), domain, service);
        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .header("Content-Type", "application/json")
            .json(data)
            .send()
            .await
            .context("Failed to call Home Assistant service")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Home Assistant API error ({status}): {body}"));
        }

        Ok(())
    }

    /// Get the state of an entity.
    pub async fn get_entity_state(&self, entity_id: &str) -> Result<String> {
        let url = format!("{}/states/{}", self.api_base(), entity_id);
        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .send()
            .await
            .context("Failed to get Home Assistant entity state")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Home Assistant API error ({status}): {body}"));
        }

        let state: serde_json::Value = resp.json().await?;
        Ok(state["state"].as_str().unwrap_or("unknown").to_string())
    }

    /// Send a notification via the persistent_notification service.
    pub async fn send_notification(&self, message: &str, title: Option<&str>) -> Result<()> {
        let data = serde_json::json!({
            "message": message,
            "title": title.unwrap_or("Hermes"),
        });
        self.call_service("persistent_notification", "create", &data).await
    }

    /// Send a message to a notify service (e.g., notify.mobile_app, notify.telegram).
    pub async fn send_notify_message(&self, service: &str, message: &str) -> Result<()> {
        let data = serde_json::json!({
            "message": message,
        });
        self.call_service("notify", service, &data).await
    }

    /// Process a conversation via the conversation API (Home Assistant built-in AI).
    pub async fn conversation_process(&self, text: &str, agent_id: Option<&str>) -> Result<String> {
        let url = format!("{}/conversation/process", self.api_base());
        let mut body = serde_json::json!({
            "text": text,
        });
        if let Some(aid) = agent_id {
            body["agent_id"] = serde_json::json!(aid);
        }

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to process Home Assistant conversation")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Home Assistant conversation error ({status}): {body}"));
        }

        let result: serde_json::Value = resp.json().await?;
        let response = result["response"]["speech"]["plain"]["speech"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(response)
    }

    /// Record an incoming event.
    pub fn record_event(&self, event: HomeAssistantEvent) {
        self.received.lock().push(event);
    }

    /// Get received events (for testing).
    pub fn get_received(&self) -> Vec<HomeAssistantEvent> {
        self.received.lock().clone()
    }
}

#[async_trait]
impl PlatformAdapter for HomeAssistantAdapter {
    fn name(&self) -> &str { "homeassistant" }

    async fn connect(&self) -> Result<()> {
        // Verify connectivity by fetching config
        let url = format!("{}/config", self.api_base());
        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                let location = body.get("location_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                tracing::info!(location = %location, url = %self.config.url, "Home Assistant adapter connected");
            }
            _ => {
                tracing::warn!(url = %self.config.url, "Home Assistant health check failed, proceeding anyway");
            }
        }
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Home Assistant adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        // If chat_id looks like a notify service, use that; otherwise use persistent_notification
        if chat_id.starts_with("notify.") {
            let service = chat_id.trim_start_matches("notify.");
            self.send_notify_message(service, text).await?;
        } else {
            self.send_notification(text, None).await?;
        }
        Ok(String::new())
    }

    async fn send_file(&self, _chat_id: &str, _path: &Path) -> Result<()> {
        // Home Assistant file sending would require uploading to a media source
        tracing::warn!("Home Assistant: file sending not fully implemented");
        Ok(())
    }

    async fn send_animation(&self, _chat_id: &str, _path: &Path) -> Result<()> {
        Ok(())
    }

    async fn send_voice(&self, _chat_id: &str, _path: &Path) -> Result<()> {
        // Could use TTS via Home Assistant: media_player.play_media
        tracing::warn!("Home Assistant: voice sending not fully implemented");
        Ok(())
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        // Not applicable to Home Assistant
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // Home Assistant doesn't support message editing
        tracing::warn!("Home Assistant: message editing not supported");
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // Could dismiss persistent notifications, but requires notification_id tracking
        tracing::warn!("Home Assistant: message deletion not fully implemented");
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        // Home Assistant doesn't have typing indicators
        Ok(())
    }

    fn create_consumer(&self, _chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
        // Home Assistant uses event listeners, not streaming consumers
        Box::new(NoOpConsumer)
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
    fn test_hass_config_new() {
        let config = HomeAssistantConfig::new("http://homeassistant.local:8123", "token123");
        assert_eq!(config.url, "http://homeassistant.local:8123");
        assert_eq!(config.token, "token123");
        assert!(config.default_target.is_none());
    }

    #[test]
    fn test_hass_config_with_target() {
        let config = HomeAssistantConfig::new("http://homeassistant.local:8123", "token123")
            .with_default_target("notify.mobile_app_phone");
        assert_eq!(config.default_target, Some("notify.mobile_app_phone".to_string()));
    }

    #[test]
    fn test_hass_config_from_platform_config() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "url": "http://ha.local:8123",
                "token": "secret",
                "default_target": "notify.telegram"
            }),
        };
        let hc = HomeAssistantConfig::from_platform_config(&config).unwrap();
        assert_eq!(hc.url, "http://ha.local:8123");
        assert_eq!(hc.default_target, Some("notify.telegram".to_string()));
    }

    #[test]
    fn test_hass_config_missing_url() {
        let config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "token": "secret"
            }),
        };
        assert!(HomeAssistantConfig::from_platform_config(&config).is_err());
    }

    #[test]
    fn test_hass_adapter_name() {
        let config = HomeAssistantConfig::new("http://localhost:8123", "tok");
        let adapter = HomeAssistantAdapter::new(config);
        assert_eq!(adapter.name(), "homeassistant");
    }

    #[test]
    fn test_hass_message_format() {
        let config = HomeAssistantConfig::new("http://localhost:8123", "tok");
        let adapter = HomeAssistantAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Plain);
    }

    #[test]
    fn test_hass_no_edit_streaming() {
        let config = HomeAssistantConfig::new("http://localhost:8123", "tok");
        let adapter = HomeAssistantAdapter::new(config);
        assert!(!adapter.supports_edit_streaming());
    }

    #[test]
    fn test_hass_received() {
        let config = HomeAssistantConfig::new("http://localhost:8123", "tok");
        let adapter = HomeAssistantAdapter::new(config);
        assert!(adapter.get_received().is_empty());
    }

    #[test]
    fn test_hass_record_event() {
        let config = HomeAssistantConfig::new("http://localhost:8123", "tok");
        let adapter = HomeAssistantAdapter::new(config);

        adapter.record_event(HomeAssistantEvent {
            event_type: "state_changed".to_string(),
            data: serde_json::json!({"entity_id": "light.living_room"}),
            chat_id: None,
        });
        assert_eq!(adapter.get_received().len(), 1);
    }

    #[test]
    fn test_hass_api_base_trailing_slash() {
        let config = HomeAssistantConfig::new("http://localhost:8123/", "tok");
        let adapter = HomeAssistantAdapter::new(config);
        assert_eq!(adapter.api_base(), "http://localhost:8123/api");
    }
}
