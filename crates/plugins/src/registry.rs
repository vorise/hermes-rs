//! Plugin Trait and Registry
//!
//! Plugin discovery, loading, and management.

use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;

/// Plugin metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Plugin name.
    pub name: String,

    /// Plugin version.
    pub version: String,

    /// Plugin description.
    pub description: String,

    /// Plugin author.
    pub author: Option<String>,

    /// Plugin homepage.
    pub homepage: Option<String>,

    /// Plugin license.
    pub license: Option<String>,

    /// Required Hermes version.
    pub hermes_version: Option<String>,

    /// Plugin dependencies.
    pub dependencies: Vec<String>,

    /// Plugin tags.
    pub tags: Vec<String>,

    /// Plugin priority (higher = earlier execution).
    pub priority: u32,
}

impl Default for PluginMetadata {
    fn default() -> Self {
        Self {
            name: "unknown".to_string(),
            version: "0.1.0".to_string(),
            description: String::new(),
            author: None,
            homepage: None,
            license: None,
            hermes_version: None,
            dependencies: Vec::new(),
            tags: Vec::new(),
            priority: 0,
        }
    }
}

/// Plugin trait.
///
/// Plugins can provide commands, tools, and hooks.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Get plugin metadata.
    fn metadata(&self) -> &PluginMetadata;

    /// Get plugin name.
    fn name(&self) -> &str {
        &self.metadata().name
    }

    /// Get plugin description.
    fn description(&self) -> &str {
        &self.metadata().description
    }

    /// Get plugin version.
    fn version(&self) -> &str {
        &self.metadata().version
    }

    /// Get plugin priority.
    fn priority(&self) -> u32 {
        self.metadata().priority
    }

    /// Initialize the plugin.
    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Shutdown the plugin.
    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Get slash commands provided by this plugin.
    fn commands(&self) -> Vec<String> {
        Vec::new()
    }

    /// Get tool definitions provided by this plugin.
    fn tools(&self) -> Vec<h_core::ToolDefinition> {
        Vec::new()
    }

    /// Check if plugin is enabled.
    fn is_enabled(&self) -> bool {
        true
    }

    /// Enable/disable the plugin.
    fn set_enabled(&self, enabled: bool);
}

/// Plugin state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// Plugin is not loaded.
    Unloaded,

    /// Plugin is loading.
    Loading,

    /// Plugin is active.
    Active,

    /// Plugin has an error.
    Error,

    /// Plugin is disabled.
    Disabled,
}

/// Plugin entry in registry.
pub struct PluginEntry {
    /// The plugin instance.
    pub plugin: Arc<dyn Plugin>,

    /// Plugin state.
    pub state: PluginState,

    /// Error message (if any).
    pub error: Option<String>,

    /// Load timestamp.
    pub loaded_at: Option<u64>,
}

impl std::fmt::Debug for PluginEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginEntry")
            .field("name", &self.plugin.name())
            .field("state", &self.state)
            .field("error", &self.error)
            .field("loaded_at", &self.loaded_at)
            .finish()
    }
}

/// Plugin registry.
pub struct PluginRegistry {
    /// Registered plugins by name.
    plugins: HashMap<String, PluginEntry>,

    /// Plugin order by priority.
    order: Vec<String>,
}

impl PluginRegistry {
    /// Create new registry.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Register a plugin.
    pub fn register(&mut self, plugin: Arc<dyn Plugin>) {
        let name = plugin.name().to_string();

        let entry = PluginEntry {
            plugin,
            state: PluginState::Unloaded,
            error: None,
            loaded_at: None,
        };

        self.plugins.insert(name.clone(), entry);

        // Update order based on priority
        self.order.push(name.clone());
        self.order.sort_by(|a, b| {
            let pa = self.plugins.get(a).map(|e| e.plugin.priority()).unwrap_or(0);
            let pb = self.plugins.get(b).map(|e| e.plugin.priority()).unwrap_or(0);
            pb.cmp(&pa)  // Higher priority first
        });
    }

    /// Unregister a plugin.
    pub fn unregister(&mut self, name: &str) -> Option<PluginEntry> {
        let entry = self.plugins.remove(name);
        if entry.is_some() {
            self.order.retain(|n| n != name);
        }
        entry
    }

    /// Get a plugin by name.
    pub fn get(&self, name: &str) -> Option<&PluginEntry> {
        self.plugins.get(name)
    }

    /// Get all plugin names.
    pub fn names(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    /// Get plugins in priority order.
    pub fn ordered_names(&self) -> &[String] {
        &self.order
    }

    /// Count plugins.
    pub fn count(&self) -> usize {
        self.plugins.len()
    }

    /// Count active plugins.
    pub fn active_count(&self) -> usize {
        self.plugins.values()
            .filter(|e| e.state == PluginState::Active)
            .count()
    }

    /// Initialize a plugin.
    pub async fn initialize_plugin(&mut self, name: &str) -> anyhow::Result<()> {
        let entry = self.plugins.get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("Plugin not found: {}", name))?;

        entry.state = PluginState::Loading;

        let result = entry.plugin.initialize().await;

        match result {
            Ok(()) => {
                entry.state = PluginState::Active;
                entry.loaded_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                );
                tracing::info!("Plugin {} initialized", name);
                Ok(())
            }
            Err(e) => {
                entry.state = PluginState::Error;
                entry.error = Some(e.to_string());
                tracing::error!("Plugin {} failed to initialize: {}", name, e);
                Err(e)
            }
        }
    }

    /// Initialize all plugins.
    pub async fn initialize_all(&mut self) -> Vec<(String, anyhow::Result<(), anyhow::Error>)> {
        let names = self.order.clone();
        let mut results = Vec::new();

        for name in names {
            let result = self.initialize_plugin(&name).await;
            results.push((name, result));
        }

        results
    }

    /// Shutdown a plugin.
    pub async fn shutdown_plugin(&mut self, name: &str) -> anyhow::Result<()> {
        let entry = self.plugins.get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("Plugin not found: {}", name))?;

        let result = entry.plugin.shutdown().await;

        match result {
            Ok(()) => {
                entry.state = PluginState::Unloaded;
                tracing::info!("Plugin {} shutdown", name);
                Ok(())
            }
            Err(e) => {
                entry.state = PluginState::Error;
                entry.error = Some(e.to_string());
                tracing::error!("Plugin {} failed to shutdown: {}", name, e);
                Err(e)
            }
        }
    }

    /// Enable a plugin.
    pub fn enable(&mut self, name: &str) -> Result<(), PluginError> {
        let entry = self.plugins.get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        entry.plugin.set_enabled(true);
        if entry.state == PluginState::Disabled {
            entry.state = PluginState::Unloaded;
        }
        Ok(())
    }

    /// Disable a plugin.
    pub fn disable(&mut self, name: &str) -> Result<(), PluginError> {
        let entry = self.plugins.get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        entry.plugin.set_enabled(false);
        entry.state = PluginState::Disabled;
        Ok(())
    }

    /// Get all tool definitions from plugins.
    pub fn all_tools(&self) -> Vec<h_core::ToolDefinition> {
        self.plugins.values()
            .filter(|e| e.state == PluginState::Active)
            .flat_map(|e| e.plugin.tools())
            .collect()
    }

    /// Get all command info from plugins.
    pub fn all_commands(&self) -> Vec<String> {
        self.plugins.values()
            .filter(|e| e.state == PluginState::Active)
            .flat_map(|e| e.plugin.commands())
            .collect()
    }

    /// Get plugin state.
    pub fn state(&self, name: &str) -> Option<PluginState> {
        self.plugins.get(name).map(|e| e.state)
    }

    /// Clear all plugins.
    pub fn clear(&mut self) {
        self.plugins.clear();
        self.order.clear();
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Plugin error.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Plugin not found.
    #[error("Plugin not found: {0}")]
    NotFound(String),

    /// Plugin already loaded.
    #[error("Plugin already loaded: {0}")]
    AlreadyLoaded(String),

    /// Plugin load failed.
    #[error("Plugin load failed: {0}")]
    LoadFailed(String),

    /// Plugin initialization failed.
    #[error("Plugin initialization failed: {0}")]
    InitFailed(String),

    /// Invalid plugin.
    #[error("Invalid plugin: {0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPlugin {
        metadata: PluginMetadata,
        enabled: bool,
    }

    impl MockPlugin {
        fn new(name: &str) -> Self {
            Self {
                metadata: PluginMetadata {
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    description: "Mock plugin".to_string(),
                    priority: 0,
                    ..Default::default()
                },
                enabled: true,
            }
        }

        fn with_priority(name: &str, priority: u32) -> Self {
            Self {
                metadata: PluginMetadata {
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    description: "Mock plugin".to_string(),
                    priority,
                    ..Default::default()
                },
                enabled: true,
            }
        }
    }

    #[async_trait]
    impl Plugin for MockPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.metadata
        }

        fn set_enabled(&self, _enabled: bool) {
            // In a real impl, this would need interior mutability
        }

        fn is_enabled(&self) -> bool {
            self.enabled
        }
    }

    #[test]
    fn test_plugin_registry_new() {
        let registry = PluginRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_plugin_registry_register() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("test"));
        registry.register(plugin);
        assert_eq!(registry.count(), 1);
        assert!(registry.get("test").is_some());
    }

    #[test]
    fn test_plugin_registry_unregister() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("test"));
        registry.register(plugin);
        registry.unregister("test");
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_plugin_registry_names() {
        let mut registry = PluginRegistry::new();
        registry.register(Arc::new(MockPlugin::new("a")));
        registry.register(Arc::new(MockPlugin::new("b")));
        let names = registry.names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
    }

    #[test]
    fn test_plugin_metadata_default() {
        let meta = PluginMetadata::default();
        assert_eq!(meta.name, "unknown");
        assert_eq!(meta.version, "0.1.0");
    }

    #[test]
    fn test_plugin_priority_order() {
        let mut registry = PluginRegistry::new();
        registry.register(Arc::new(MockPlugin::with_priority("low", 1)));
        registry.register(Arc::new(MockPlugin::with_priority("high", 10)));
        registry.register(Arc::new(MockPlugin::with_priority("mid", 5)));

        let order = registry.ordered_names();
        assert_eq!(order[0], "high");  // Highest priority first
        assert_eq!(order[1], "mid");
        assert_eq!(order[2], "low");
    }
}