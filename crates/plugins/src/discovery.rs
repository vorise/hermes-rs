//! Plugin Discovery
//!
//! Discover plugins from directories and config.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::registry::PluginMetadata;

/// Discovery config.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryConfig {
    /// Plugin directories to scan.
    pub directories: Vec<PathBuf>,

    /// Whether to scan user plugins.
    pub scan_user_plugins: bool,

    /// Whether to scan system plugins.
    pub scan_system_plugins: bool,

    /// Plugin file patterns.
    pub patterns: Vec<String>,
}

impl DiscoveryConfig {
    /// Create with default directories.
    pub fn default_dirs() -> Self {
        Self {
            directories: vec![
                PathBuf::from("plugins"),
                h_core::hermes_home().join("plugins"),
            ],
            scan_user_plugins: true,
            scan_system_plugins: true,
            patterns: vec!["plugin.yaml".to_string(), "plugin.json".to_string(), "manifest.yaml".to_string()],
        }
    }

    /// Add a directory.
    pub fn add_dir(&mut self, dir: PathBuf) {
        self.directories.push(dir);
    }
}

/// Discovered plugin info.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    /// Plugin directory.
    pub path: PathBuf,

    /// Plugin metadata.
    pub metadata: PluginMetadata,

    /// Discovery source.
    pub source: String,
}

/// Plugin manifest file formats.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin name.
    pub name: String,

    /// Plugin version.
    pub version: String,

    /// Plugin description.
    #[serde(default)]
    pub description: String,

    /// Plugin author.
    #[serde(default)]
    pub author: Option<String>,

    /// Plugin homepage.
    #[serde(default)]
    pub homepage: Option<String>,

    /// Plugin license.
    #[serde(default)]
    pub license: Option<String>,

    /// Required Hermes version.
    #[serde(default)]
    pub hermes_version: Option<String>,

    /// Plugin dependencies.
    #[serde(default)]
    pub dependencies: Vec<String>,

    /// Plugin tags.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Plugin priority.
    #[serde(default)]
    pub priority: u32,

    /// Main entry point (executable or script).
    #[serde(default)]
    pub main: Option<String>,

    /// Plugin hooks.
    #[serde(default)]
    pub hooks: Vec<String>,

    /// Commands provided.
    #[serde(default)]
    pub commands: Vec<String>,

    /// Tools provided.
    #[serde(default)]
    pub tools: Vec<String>,
}

impl PluginManifest {
    /// Convert to PluginMetadata.
    pub fn to_metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            author: self.author.clone(),
            homepage: self.homepage.clone(),
            license: self.license.clone(),
            hermes_version: self.hermes_version.clone(),
            dependencies: self.dependencies.clone(),
            tags: self.tags.clone(),
            priority: self.priority,
        }
    }
}

/// Discover plugins from configured directories.
pub fn discover_plugins(config: &DiscoveryConfig) -> Vec<DiscoveredPlugin> {
    let mut discovered = Vec::new();

    for dir in &config.directories {
        if !dir.exists() {
            info!("Plugin directory does not exist: {}", dir.display());
            continue;
        }

        let plugins = scan_directory(dir, config);
        discovered.extend(plugins);
    }

    info!("Discovered {} plugins", discovered.len());
    discovered
}

/// Scan a directory for plugins.
fn scan_directory(dir: &Path, config: &DiscoveryConfig) -> Vec<DiscoveredPlugin> {
    let mut plugins = Vec::new();

    // Scan for plugin directories
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                // Check for manifest file
                for pattern in &config.patterns {
                    let manifest_path = path.join(pattern);
                    if manifest_path.exists() {
                        if let Some(plugin) = load_manifest(&manifest_path, &path) {
                            plugins.push(plugin);
                            break;  // Found manifest, skip other patterns
                        }
                    }
                }
            }
        }
    }

    plugins
}

/// Load a plugin manifest.
fn load_manifest(manifest_path: &Path, plugin_dir: &Path) -> Option<DiscoveredPlugin> {
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| {
            warn!("Failed to read manifest {}: {}", manifest_path.display(), e);
        })
        .ok()?;

    // Try to parse as YAML first, then JSON
    let manifest: PluginManifest = if manifest_path.extension().map(|e| e == "yaml").unwrap_or(false) {
        // For YAML, use serde_json since serde_yaml might not be available
        // In production, would use proper YAML parser
        serde_json::from_str(&content)
            .map_err(|e| {
                warn!("Failed to parse manifest {}: {}", manifest_path.display(), e);
            })
            .ok()?
    } else {
        serde_json::from_str(&content)
            .map_err(|e| {
                warn!("Failed to parse JSON manifest {}: {}", manifest_path.display(), e);
            })
            .ok()?
    };

    Some(DiscoveredPlugin {
        path: plugin_dir.to_path_buf(),
        metadata: manifest.to_metadata(),
        source: manifest_path.to_string_lossy().to_string(),
    })
}

/// Get installed plugins from Hermes home.
pub fn get_installed_plugins() -> Vec<DiscoveredPlugin> {
    let config = DiscoveryConfig::default_dirs();
    discover_plugins(&config)
}

/// Check if a plugin is installed.
pub fn is_plugin_installed(name: &str) -> bool {
    let plugins = get_installed_plugins();
    plugins.iter().any(|p| p.metadata.name == name)
}

/// Get plugin directory by name.
pub fn get_plugin_dir(name: &str) -> Option<PathBuf> {
    let plugins = get_installed_plugins();
    plugins.iter()
        .find(|p| p.metadata.name == name)
        .map(|p| p.path.clone())
}

/// List available plugins (installed + discoverable).
pub fn list_available_plugins() -> Vec<PluginMetadata> {
    let discovered = get_installed_plugins();
    discovered.iter().map(|p| p.metadata.clone()).collect()
}

/// Validate plugin manifest.
pub fn validate_manifest(manifest: &PluginManifest) -> Result<(), String> {
    if manifest.name.is_empty() {
        return Err("Plugin name is required".to_string());
    }

    if manifest.version.is_empty() {
        return Err("Plugin version is required".to_string());
    }

    // Validate name format (alphanumeric, underscore, hyphen)
    if !manifest.name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err("Plugin name must be alphanumeric with underscores or hyphens".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_config_default() {
        let config = DiscoveryConfig::default_dirs();
        assert!(!config.directories.is_empty());
        assert!(config.scan_user_plugins);
    }

    #[test]
    fn test_plugin_manifest_to_metadata() {
        let manifest = PluginManifest {
            name: "test".to_string(),
            version: "1.0".to_string(),
            description: "Test plugin".to_string(),
            author: Some("test".to_string()),
            priority: 5,
            ..Default::default()
        };
        let meta = manifest.to_metadata();
        assert_eq!(meta.name, "test");
        assert_eq!(meta.priority, 5);
    }

    #[test]
    fn test_validate_manifest_valid() {
        let manifest = PluginManifest {
            name: "valid-plugin".to_string(),
            version: "1.0".to_string(),
            ..Default::default()
        };
        assert!(validate_manifest(&manifest).is_ok());
    }

    #[test]
    fn test_validate_manifest_empty_name() {
        let manifest = PluginManifest {
            name: "".to_string(),
            version: "1.0".to_string(),
            ..Default::default()
        };
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn test_validate_manifest_invalid_name() {
        let manifest = PluginManifest {
            name: "invalid name!".to_string(),
            version: "1.0".to_string(),
            ..Default::default()
        };
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn test_discovery_config_add_dir() {
        let mut config = DiscoveryConfig::default();
        config.add_dir(PathBuf::from("/custom"));
        assert_eq!(config.directories.len(), 1);
    }
}