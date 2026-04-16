use serde::{Deserialize, Serialize};

/// Full user configuration. Mirrors ~/.hermes/config.yaml from the Python codebase.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HermesConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_toolsets: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_toolsets: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegation: Option<DelegationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platforms: Option<PlatformsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<CronConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web: Option<WebConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skin: Option<SkinConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<Profile>>,
}

/// Terminal backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    pub backend: TerminalBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerOpts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshOpts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modal: Option<ModalOpts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daytona: Option<DaytonaOpts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalBackend {
    Local,
    Docker,
    Ssh,
    Modal,
    Daytona,
    Singularity,
}

impl Default for TerminalBackend {
    fn default() -> Self {
        TerminalBackend::Local
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerOpts {
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshOpts {
    pub host: String,
    pub user: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModalOpts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaytonaOpts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

/// Delegation (subagent) configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Memory system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub honcho_url: Option<String>,
}

/// Messaging platforms configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telegram: Option<TelegramConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord: Option<DiscordConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack: Option<SlackConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token_env: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub bot_token_env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub bot_token_env: String,
    pub app_token_env: String,
}

/// MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers: Option<std::collections::HashMap<String, McpServerConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::HashMap<String, String>>,
}

/// Cron scheduler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jobs: Option<Vec<CronJob>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub schedule: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<DeliveryTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryTarget {
    pub platform: String,
    pub chat_id: String,
}

/// Web UI configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

/// Skin/theme configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Plugin configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directories: Option<Vec<String>>,
}

/// Profile for different working contexts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_toolsets: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_toolsets: Option<Vec<String>>,
}

/// Load configuration from YAML, with env var overrides.
pub fn load_config(path: &std::path::Path) -> anyhow::Result<HermesConfig> {
    if !path.exists() {
        tracing::warn!(?path, "Config file not found, using defaults");
        return Ok(HermesConfig::default());
    }

    let contents = std::fs::read_to_string(path)?;
    let config: HermesConfig = serde_yaml::from_str(&contents)?;
    Ok(config)
}

/// Load environment variables from ~/.hermes/.env.
pub fn load_env(env_path: &std::path::Path) -> anyhow::Result<()> {
    if env_path.exists() {
        dotenvy::from_path(env_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HermesConfig::default();
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
    }

    #[test]
    fn test_config_yaml_roundtrip() {
        let config = HermesConfig {
            model: Some("claude-sonnet-4-6".to_string()),
            provider: Some("anthropic".to_string()),
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: HermesConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.model, config.model);
        assert_eq!(parsed.provider, config.provider);
    }
}
