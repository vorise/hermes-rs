use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::base::PlatformConfig;

/// Full gateway configuration.
///
/// Loaded from ~/.hermes/gateway.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Platform configurations keyed by platform name.
    #[serde(default)]
    pub platforms: std::collections::HashMap<String, PlatformConfig>,
    /// Maximum concurrent gateway instances.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    /// Default session timeout in seconds.
    #[serde(default = "default_session_timeout")]
    pub session_timeout_secs: u64,
    /// Gateway HTTP API port (for REST API and webhook platform).
    #[serde(default = "default_api_port")]
    pub api_port: u16,
}

fn default_max_concurrent() -> u32 { 10 }
fn default_session_timeout() -> u64 { 3600 }
fn default_api_port() -> u16 { 8080 }

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            platforms: std::collections::HashMap::new(),
            max_concurrent: default_max_concurrent(),
            session_timeout_secs: default_session_timeout(),
            api_port: default_api_port(),
        }
    }
}

impl GatewayConfig {
    /// Load configuration from a YAML file.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read gateway config: {}", path.display()))?;
        serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse gateway config: {}", path.display()))
    }

    /// Save configuration to a YAML file.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write gateway config: {}", path.display()))?;
        Ok(())
    }

    /// Get the default config path (~/.hermes/gateway.yaml).
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        home.join(".hermes").join("gateway.yaml")
    }

    /// Load from default path.
    pub fn from_default() -> Result<Self> {
        Self::load(&Self::default_path())
    }

    /// Get a platform configuration by name.
    pub fn get_platform(&self, name: &str) -> Option<&PlatformConfig> {
        self.platforms.get(name)
    }

    /// List all enabled platforms.
    pub fn enabled_platforms(&self) -> Vec<(&str, &PlatformConfig)> {
        self.platforms
            .iter()
            .filter(|(_, cfg)| cfg.enabled)
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// Check if any platforms are configured.
    pub fn is_empty(&self) -> bool {
        self.platforms.is_empty()
    }

    /// Get the number of configured platforms.
    pub fn platform_count(&self) -> usize {
        self.platforms.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_gateway_config() {
        let config = GatewayConfig::default();
        assert!(config.is_empty());
        assert_eq!(config.max_concurrent, 10);
        assert_eq!(config.session_timeout_secs, 3600);
        assert_eq!(config.api_port, 8080);
    }

    #[test]
    fn test_gateway_config_platform_filtering() {
        use std::collections::HashMap;
        let mut platforms = HashMap::new();
        platforms.insert("telegram".to_string(), PlatformConfig {
            enabled: true,
            settings: serde_json::json!({"bot_token": "abc"}),
        });
        platforms.insert("discord".to_string(), PlatformConfig {
            enabled: false,
            settings: serde_json::json!({"bot_token": "xyz"}),
        });

        let config = GatewayConfig { platforms, ..Default::default() };
        let enabled = config.enabled_platforms();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].0, "telegram");
    }

    #[test]
    fn test_gateway_config_platform_count() {
        use std::collections::HashMap;
        let mut platforms = HashMap::new();
        platforms.insert("telegram".to_string(), PlatformConfig::default());
        platforms.insert("discord".to_string(), PlatformConfig::default());

        let config = GatewayConfig { platforms, ..Default::default() };
        assert_eq!(config.platform_count(), 2);
    }
}
