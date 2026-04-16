//! Platform Adapters
//!
//! Collection of all messaging platform adapters.

pub mod base;

pub use base::{PlatformAdapter, PlatformCapabilities};

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// Platform registry.
///
/// Manages all connected platform adapters.
pub struct PlatformRegistry {
    /// Registered adapters.
    adapters: Arc<RwLock<HashMap<String, Arc<dyn PlatformAdapter>>>>,
}

impl PlatformRegistry {
    /// Create new registry.
    pub fn new() -> Self {
        Self {
            adapters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a platform adapter.
    pub fn register(&self, adapter: Arc<dyn PlatformAdapter>) {
        let mut adapters = self.adapters.write();
        adapters.insert(adapter.name().to_string(), adapter);
    }

    /// Unregister a platform.
    pub fn unregister(&self, name: &str) {
        let mut adapters = self.adapters.write();
        adapters.remove(name);
    }

    /// Get platform by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn PlatformAdapter>> {
        let adapters = self.adapters.read();
        adapters.get(name).cloned()
    }

    /// Get all registered platforms.
    pub fn all_platforms(&self) -> Vec<Arc<dyn PlatformAdapter>> {
        let adapters = self.adapters.read();
        adapters.values().cloned().collect()
    }

    /// Get all platform names.
    pub fn platform_names(&self) -> Vec<String> {
        let adapters = self.adapters.read();
        adapters.keys().cloned().collect()
    }

    /// Count platforms.
    pub fn count(&self) -> usize {
        let adapters = self.adapters.read();
        adapters.len()
    }

    /// Connect all platforms.
    pub async fn connect_all(&self) -> Vec<(String, anyhow::Result<(), anyhow::Error>)> {
        let adapters: Vec<Arc<dyn PlatformAdapter>> = self.adapters.read().values().cloned().collect();
        let mut results = Vec::new();
        for adapter in adapters {
            let name = adapter.name().to_string();
            let result = adapter.connect().await;
            results.push((name, result));
        }
        results
    }

    /// Disconnect all platforms.
    pub async fn disconnect_all(&self) -> Vec<(String, anyhow::Result<(), anyhow::Error>)> {
        let adapters: Vec<Arc<dyn PlatformAdapter>> = self.adapters.read().values().cloned().collect();
        let mut results = Vec::new();
        for adapter in adapters {
            let name = adapter.name().to_string();
            let result = adapter.disconnect().await;
            results.push((name, result));
        }
        results
    }
}

impl Default for PlatformRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::Path;

    /// Mock platform adapter for testing.
    struct MockAdapter {
        name: String,
        connected: Arc<RwLock<bool>>,
    }

    impl MockAdapter {
        fn new(name: impl Into<String>) -> Self {
            Self {
                name: name.into(),
                connected: Arc::new(RwLock::new(false)),
            }
        }
    }

    #[async_trait]
    impl PlatformAdapter for MockAdapter {
        fn name(&self) -> &str {
            &self.name
        }

        async fn connect(&self) -> anyhow::Result<()> {
            let mut connected = self.connected.write();
            *connected = true;
            Ok(())
        }

        async fn disconnect(&self) -> anyhow::Result<()> {
            let mut connected = self.connected.write();
            *connected = false;
            Ok(())
        }

        fn is_connected(&self) -> bool {
            *self.connected.read()
        }

        async fn send_message(&self, _chat_id: &str, _text: &str) -> anyhow::Result<String> {
            Ok("msg-123".to_string())
        }

        async fn send_full_message(&self, _chat_id: &str, _message: &crate::event::OutgoingMessage) -> anyhow::Result<String> {
            Ok("msg-123".to_string())
        }

        async fn send_file(&self, _chat_id: &str, _path: &Path, _caption: Option<&str>) -> anyhow::Result<String> {
            Ok("msg-123".to_string())
        }

        async fn send_animation(&self, _chat_id: &str, _path: &Path) -> anyhow::Result<String> {
            Ok("msg-123".to_string())
        }

        async fn send_voice(&self, _chat_id: &str, _path: &Path) -> anyhow::Result<String> {
            Ok("msg-123".to_string())
        }

        async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> anyhow::Result<String> {
            Ok("msg-123".to_string())
        }

        async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn is_typing(&self, _chat_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn stream_consumer(&self, _chat_id: &str, _message_id: &str) -> Arc<dyn crate::stream_consumer::StreamConsumer> {
            Arc::new(crate::stream_consumer::BufferedStreamConsumer::new())
        }

        fn capabilities(&self) -> PlatformCapabilities {
            PlatformCapabilities::text_only()
        }

        async fn start_receiving(&self, _tx: tokio::sync::mpsc::UnboundedSender<crate::event::GatewayMessage>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop_receiving(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_registry_new() {
        let registry = PlatformRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_registry_register() {
        let registry = PlatformRegistry::new();
        let adapter = Arc::new(MockAdapter::new("test"));
        registry.register(adapter);
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_registry_get() {
        let registry = PlatformRegistry::new();
        let adapter = Arc::new(MockAdapter::new("test"));
        registry.register(adapter.clone());

        let found = registry.get("test");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), "test");
    }

    #[test]
    fn test_registry_unregister() {
        let registry = PlatformRegistry::new();
        let adapter = Arc::new(MockAdapter::new("test"));
        registry.register(adapter);
        registry.unregister("test");
        assert_eq!(registry.count(), 0);
    }

    #[tokio::test]
    async fn test_registry_connect_all() {
        let registry = PlatformRegistry::new();
        let adapter = Arc::new(MockAdapter::new("test"));
        registry.register(adapter);

        let results = registry.connect_all().await;
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_ok());
    }

    #[tokio::test]
    async fn test_registry_disconnect_all() {
        let registry = PlatformRegistry::new();
        let adapter = Arc::new(MockAdapter::new("test"));
        registry.register(adapter);

        let results = registry.disconnect_all().await;
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_ok());
    }
}