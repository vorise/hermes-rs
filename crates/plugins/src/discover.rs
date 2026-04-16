use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::hooks::HookContext;
use crate::plugin::{HookResult, Plugin};

/// Plugin manifest loaded from a plugin directory.
#[derive(Debug, Clone)]
pub struct PluginManifest {
    /// Plugin name.
    pub name: String,
    /// Plugin description.
    pub description: String,
    /// Plugin version.
    pub version: String,
    /// Entry point script (for executable plugins).
    pub entry_point: Option<String>,
    /// Plugin directory.
    pub dir: PathBuf,
}

impl PluginManifest {
    /// Load manifest from a plugin.toml file.
    pub fn load(dir: &Path) -> Result<Self> {
        let manifest_path = dir.join("plugin.toml");
        let content = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read manifest at {}", manifest_path.display()))?;

        let toml: toml::Value = toml::from_str(&content)
            .with_context(|| format!("Failed to parse manifest at {}", manifest_path.display()))?;

        let plugin = toml.get("plugin").context("Missing [plugin] section in manifest")?;

        Ok(Self {
            name: plugin.get("name")
                .and_then(|v| v.as_str())
                .context("Missing plugin name in manifest")?
                .to_string(),
            description: plugin.get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            version: plugin.get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.1.0")
                .to_string(),
            entry_point: plugin.get("entry_point")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            dir: dir.to_path_buf(),
        })
    }
}

/// Discover plugins in the given directory.
///
/// Looks for directories containing a `plugin.toml` manifest file.
pub fn discover_plugins(plugins_dir: &Path) -> Result<Vec<PluginManifest>> {
    if !plugins_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plugins = Vec::new();

    for entry in fs::read_dir(plugins_dir)
        .with_context(|| format!("Failed to read plugins directory: {}", plugins_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() && path.join("plugin.toml").exists() {
            match PluginManifest::load(&path) {
                Ok(manifest) => {
                    tracing::info!("Discovered plugin: {} v{}", manifest.name, manifest.version);
                    plugins.push(manifest);
                }
                Err(e) => {
                    tracing::warn!("Failed to load plugin from {}: {e}", path.display());
                }
            }
        }
    }

    Ok(plugins)
}

/// Load a plugin from a directory.
///
/// For directory-based plugins (no entry point), reads configuration from manifest.
/// For executable plugins (with entry point), spawns the process.
///
/// Note: This function returns metadata only. Actual plugin execution for
/// directory-based plugins requires compile-time linking. Executable plugins
/// communicate via stdio.
pub fn load_plugin_from_dir(dir: &Path) -> Result<PluginManifest> {
    PluginManifest::load(dir)
}

/// In-memory plugin for testing / built-in plugins.
/// These don't require a filesystem directory.
pub struct BuiltinPlugin<F> {
    name: String,
    description: String,
    version: String,
    hook_names: Vec<String>,
    hook_fn: F,
}

impl<F> BuiltinPlugin<F>
where
    F: Fn(&str, &HookContext) -> HookResult + Send + Sync,
{
    pub fn new(
        name: &str,
        description: &str,
        hook_names: Vec<&str>,
        hook_fn: F,
    ) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            version: "0.1.0".to_string(),
            hook_names: hook_names.into_iter().map(|s| s.to_string()).collect(),
            hook_fn,
        }
    }
}

impl<F> Plugin for BuiltinPlugin<F>
where
    F: Fn(&str, &HookContext) -> HookResult + Send + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn hook_names(&self) -> Vec<&str> {
        self.hook_names.iter().map(|s| s.as_str()).collect()
    }

    fn invoke_hook(&self, hook_name: &str, ctx: &crate::hooks::HookContext) -> crate::plugin::HookResult {
        (self.hook_fn)(hook_name, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_test_plugin(dir: &Path, name: &str, version: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let manifest = format!(
            r#"[plugin]
name = "{name}"
description = "A test plugin"
version = "{version}"
"#
        );
        let mut file = std::fs::File::create(dir.join("plugin.toml")).unwrap();
        file.write_all(manifest.as_bytes()).unwrap();
    }

    #[test]
    fn test_discover_plugins_empty_dir() {
        let temp = std::env::temp_dir().join("hermes_test_empty");
        std::fs::create_dir_all(&temp).unwrap();
        let plugins = discover_plugins(&temp).unwrap();
        assert!(plugins.is_empty());
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_discover_single_plugin() {
        let temp = std::env::temp_dir().join("hermes_test_single");
        let plugin_dir = temp.join("my-plugin");
        create_test_plugin(&plugin_dir, "my-plugin", "1.0.0");

        let plugins = discover_plugins(&temp).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "my-plugin");
        assert_eq!(plugins[0].version, "1.0.0");

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_discover_multiple_plugins() {
        let temp = std::env::temp_dir().join("hermes_test_multi");

        create_test_plugin(&temp.join("plugin-a"), "plugin-a", "0.1.0");
        create_test_plugin(&temp.join("plugin-b"), "plugin-b", "0.2.0");
        // Non-plugin directory (no manifest)
        std::fs::create_dir_all(temp.join("not-a-plugin")).unwrap();

        let plugins = discover_plugins(&temp).unwrap();
        assert_eq!(plugins.len(), 2);

        let names: Vec<_> = plugins.iter().map(|p| &p.name).collect();
        assert!(names.contains(&&"plugin-a".to_string()));
        assert!(names.contains(&&"plugin-b".to_string()));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_discover_nonexistent_dir() {
        let plugins = discover_plugins(Path::new("/nonexistent/path")).unwrap();
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_load_plugin_manifest() {
        let temp = std::env::temp_dir().join("hermes_test_load");
        create_test_plugin(&temp, "load-test", "2.0.0");

        let manifest = load_plugin_from_dir(&temp).unwrap();
        assert_eq!(manifest.name, "load-test");
        assert_eq!(manifest.version, "2.0.0");
        assert_eq!(manifest.description, "A test plugin");
        assert!(manifest.entry_point.is_none());

        let _ = std::fs::remove_dir_all(&temp);
    }
}
