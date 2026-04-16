use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use crate::hooks::HookContext;

/// Information about a loaded plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Unique plugin name.
    pub name: String,
    /// Plugin description.
    pub description: String,
    /// Plugin version.
    pub version: String,
    /// Whether the plugin is enabled.
    pub enabled: bool,
}

/// Result of a hook invocation.
pub type HookResult = Result<Vec<Value>>;

/// A plugin that can contribute commands, tools, and hooks to Hermes.
pub trait Plugin: Send + Sync {
    /// Plugin name.
    fn name(&self) -> &str;

    /// Plugin description.
    fn description(&self) -> &str;

    /// Plugin version.
    fn version(&self) -> &str {
        "0.1.0"
    }

    /// Hook names this plugin wants to register for.
    /// Called by the registry to discover hooks.
    fn hook_names(&self) -> Vec<&str> {
        vec![]
    }

    /// Execute a hook by name.
    /// Returns a vector of values to be merged by the dispatcher.
    fn invoke_hook(&self, _hook_name: &str, _ctx: &HookContext) -> HookResult {
        Ok(vec![])
    }

    /// Initialize the plugin. Called once when the plugin is loaded.
    fn initialize(&self) -> Result<()> {
        Ok(())
    }

    /// Get plugin info.
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: self.name().to_string(),
            description: self.description().to_string(),
            version: self.version().to_string(),
            enabled: true,
        }
    }
}

/// Registry for managing loaded plugins.
pub struct PluginRegistry {
    plugins: HashMap<String, Arc<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// Register a plugin.
    pub fn register(&mut self, plugin: Arc<dyn Plugin>) -> Result<()> {
        let name = plugin.name().to_string();
        if self.plugins.contains_key(&name) {
            return Err(anyhow::anyhow!("Plugin already registered: {name}"));
        }
        plugin.initialize()?;
        self.plugins.insert(name, plugin);
        Ok(())
    }

    /// Unregister a plugin by name.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.plugins.remove(name).is_some()
    }

    /// Get a plugin by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Plugin>> {
        self.plugins.get(name)
    }

    /// List all registered plugins.
    pub fn list(&self) -> Vec<PluginInfo> {
        self.plugins.values().map(|p| p.info()).collect()
    }

    /// Count registered plugins.
    pub fn count(&self) -> usize {
        self.plugins.len()
    }

    /// Invoke a hook across all plugins that registered for it.
    pub fn invoke_hook(&self, hook_name: &str, ctx: &HookContext) -> Vec<serde_json::Value> {
        let mut results = Vec::new();
        for (name, plugin) in &self.plugins {
            if plugin.hook_names().contains(&hook_name) {
                match plugin.invoke_hook(hook_name, ctx) {
                    Ok(values) => results.extend(values),
                    Err(e) => tracing::warn!("Plugin {name} hook {hook_name} failed: {e}"),
                }
            }
        }
        results
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience macro for creating a simple plugin.
#[macro_export]
macro_rules! simple_plugin {
    ($name:ident, $plugin_name:expr, $plugin_desc:expr) => {
        pub struct $name;

        impl $crate::Plugin for $name {
            fn name(&self) -> &str {
                $plugin_name
            }

            fn description(&self) -> &str {
                $plugin_desc
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin;

    impl Plugin for TestPlugin {
        fn name(&self) -> &str {
            "test-plugin"
        }

        fn description(&self) -> &str {
            "A test plugin"
        }
    }

    #[test]
    fn test_register_plugin() {
        let mut registry = PluginRegistry::new();
        assert_eq!(registry.count(), 0);

        let plugin = Arc::new(TestPlugin);
        registry.register(plugin).unwrap();
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_duplicate_registration_fails() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(TestPlugin);
        registry.register(plugin.clone()).unwrap();
        let result = registry.register(plugin);
        assert!(result.is_err());
    }

    #[test]
    fn test_unregister_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(TestPlugin);
        registry.register(plugin).unwrap();
        assert!(registry.unregister("test-plugin"));
        assert!(!registry.unregister("test-plugin")); // already removed
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_list_plugins() {
        let mut registry = PluginRegistry::new();
        registry.register(Arc::new(TestPlugin)).unwrap();
        let list = registry.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-plugin");
        assert_eq!(list[0].description, "A test plugin");
    }

    #[test]
    fn test_hook_context_data() {
        let ctx = HookContext::new("test".to_string(), Some("session-1".to_string()));
        assert_eq!(ctx.session_id, Some("session-1".to_string()));
        assert_eq!(ctx.hook_name, "test");

        ctx.set("key", serde_json::json!("value"));
        assert_eq!(ctx.get("key"), Some(serde_json::json!("value")));
        assert_eq!(ctx.get("missing"), None);
    }

    #[test]
    fn test_invoke_hooks() {
        struct HookPlugin;

        impl Plugin for HookPlugin {
            fn name(&self) -> &str {
                "hook-plugin"
            }
            fn description(&self) -> &str {
                "Hook test plugin"
            }
            fn hook_names(&self) -> Vec<&str> {
                vec!["pre_llm_call"]
            }
            fn invoke_hook(&self, hook_name: &str, _ctx: &HookContext) -> HookResult {
                Ok(vec![serde_json::json!({"hook": hook_name, "injected": "context"})])
            }
        }

        let mut registry = PluginRegistry::new();
        registry.register(Arc::new(HookPlugin)).unwrap();

        let ctx = HookContext::new("pre_llm_call".to_string(), None);
        let results = registry.invoke_hook("pre_llm_call", &ctx);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["injected"], "context");

        // Hook that no plugin registered for
        let results2 = registry.invoke_hook("unknown_hook", &ctx);
        assert!(results2.is_empty());
    }
}
