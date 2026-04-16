use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A single MCP server configuration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    /// Server display name.
    pub name: String,
    /// Whether the server is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Transport configuration.
    pub transport: TransportConfig,
    /// Server-specific settings.
    #[serde(default)]
    pub settings: HashMap<String, serde_json::Value>,
}

/// Transport configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TransportConfig {
    /// Stdio transport: command + args.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: Option<HashMap<String, String>>,
    },
    /// SSE transport: URL.
    Sse {
        url: String,
    },
}

fn default_true() -> bool {
    true
}

/// Full MCP configuration, typically stored in ~/.hermes/mcp.yaml.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    /// Map of server name to configuration.
    #[serde(default)]
    pub servers: HashMap<String, McpServerEntry>,
}

impl McpConfig {
    /// Load configuration from a YAML file.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read MCP config: {}", path.display()))?;
        serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse MCP config: {}", path.display()))
    }

    /// Save configuration to a YAML file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write MCP config: {}", path.display()))?;
        Ok(())
    }

    /// Get the default config path (~/.hermes/mcp.yaml).
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        home.join(".hermes").join("mcp.yaml")
    }

    /// Load from default path.
    pub fn from_default() -> Result<Self> {
        Self::load(&Self::default_path())
    }

    /// Add or update a server.
    pub fn add_server(&mut self, name: &str, entry: McpServerEntry) {
        self.servers.insert(name.to_string(), entry);
    }

    /// Remove a server.
    pub fn remove_server(&mut self, name: &str) -> Option<McpServerEntry> {
        self.servers.remove(name)
    }

    /// Get a server by name.
    pub fn get_server(&self, name: &str) -> Option<&McpServerEntry> {
        self.servers.get(name)
    }

    /// List all enabled servers.
    pub fn enabled_servers(&self) -> Vec<(&str, &McpServerEntry)> {
        self.servers
            .iter()
            .filter(|(_, entry)| entry.enabled)
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// List all servers.
    pub fn all_servers(&self) -> Vec<(&str, &McpServerEntry)> {
        self.servers.iter().map(|(k, v)| (k.as_str(), v)).collect()
    }

    /// Check if any servers are configured.
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }
}

/// Configuration for MCP server connection, derived from entry config.
pub struct McpServerConfig {
    pub name: String,
    pub transport: crate::transport::McpTransport,
}

impl McpServerConfig {
    /// Convert from a config entry.
    pub fn from_entry(name: &str, entry: &McpServerEntry) -> Result<Self> {
        let transport = match &entry.transport {
            TransportConfig::Stdio { command, args, env } => {
                let env_list = env.as_ref().map(|map| {
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                });
                crate::transport::McpTransport::Stdio {
                    command: command.clone(),
                    args: args.clone(),
                    env: env_list,
                }
            }
            TransportConfig::Sse { url } => {
                crate::transport::McpTransport::Sse { url: url.clone() }
            }
        };

        Ok(Self {
            name: name.to_string(),
            transport,
        })
    }
}
