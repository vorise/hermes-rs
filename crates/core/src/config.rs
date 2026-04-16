//! Configuration module for Hermes Agent.
//!
//! Provides configuration types that mirror the Python codebase's ~/.hermes/config.yaml format.
//! Configuration is loaded from multiple sources in priority order:
//! 1. CLI flags (highest priority)
//! 2. Environment variables (HERMES_*)
//! 3. User config (~/.hermes/config.yaml)
//! 4. Project config (.hermes.yaml)
//! 5. Defaults (lowest priority)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::{ModelId, ProviderId, ModelRef};

/// Full user configuration. Mirrors ~/.hermes/config.yaml from the Python codebase.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HermesConfig {
    /// Default model to use (e.g., "claude-sonnet-4-6")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Default provider to use (e.g., "openrouter", "anthropic")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Custom base URL for API calls
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Personality/persona to use
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personality: Option<String>,

    /// Enabled toolsets
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_toolsets: Option<Vec<String>>,

    /// Disabled toolsets
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_toolsets: Option<Vec<String>>,

    /// Enabled skills
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_skills: Option<Vec<String>>,

    /// Disabled skills
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_skills: Option<Vec<String>>,

    /// Terminal backend configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalConfig>,

    /// Delegation/subagent configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<DelegationConfig>,

    /// Memory system configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryConfig>,

    /// Platform-specific configurations
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platforms: Option<PlatformsConfig>,

    /// MCP server configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpConfig>,

    /// Cron scheduler configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<CronConfig>,

    /// Web UI configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<WebConfig>,

    /// Skin/theme configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<SkinConfig>,

    /// Plugin configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,

    /// User profiles
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<Profile>>,
}

impl HermesConfig {
    /// Load configuration from all sources.
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from_path(None)
    }

    /// Load configuration from a specific config file path.
    pub fn load_from_path(path: Option<PathBuf>) -> anyhow::Result<Self> {
        use crate::home::hermes_home;
        use dotenvy::from_path;

        // Load .env file from hermes home
        let env_path = hermes_home().join(".env");
        if env_path.exists() {
            from_path(&env_path).ok(); // Ignore errors - .env may not exist
        }

        // Load YAML config
        let config_path = path.unwrap_or_else(|| hermes_home().join("config.yaml"));

        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            let config: HermesConfig = serde_yaml::from_str(&contents)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Get the effective model reference (provider + model).
    pub fn model_ref(&self) -> ModelRef {
        let provider = self.provider
            .as_ref()
            .map(|s| ProviderId::new(s))
            .unwrap_or_default();

        let model = self.model
            .as_ref()
            .map(|s| ModelId::new(s))
            .unwrap_or_default();

        ModelRef { provider, model }
    }
}

// ============================================================================
// Terminal Configuration
// ============================================================================

/// Terminal backend type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TerminalBackend {
    #[default]
    Local,
    Docker,
    Ssh,
    Modal,
    Daytona,
    Singularity,
}

impl std::fmt::Display for TerminalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminalBackend::Local => write!(f, "local"),
            TerminalBackend::Docker => write!(f, "docker"),
            TerminalBackend::Ssh => write!(f, "ssh"),
            TerminalBackend::Modal => write!(f, "modal"),
            TerminalBackend::Daytona => write!(f, "daytona"),
            TerminalBackend::Singularity => write!(f, "singularity"),
        }
    }
}

/// Terminal backend configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TerminalConfig {
    #[serde(default)]
    pub backend: TerminalBackend,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerOpts>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshOpts>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modal: Option<ModalOpts>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daytona: Option<DaytonaOpts>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub singularity: Option<SingularityOpts>,
}

/// Docker backend options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DockerOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,

    #[serde(default)]
    pub volumes: Vec<String>,

    #[serde(default)]
    pub env_vars: Vec<String>,

    #[serde(default)]
    pub keep_container: bool,
}

/// SSH backend options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SshOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<PathBuf>,
}

/// Modal backend options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModalOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_name: Option<String>,

    #[serde(default)]
    pub timeout_seconds: u64,

    #[serde(default)]
    pub persistent: bool,
}

/// Daytona backend options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaytonaOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,

    #[serde(default)]
    pub target: Option<String>,
}

/// Singularity backend options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SingularityOpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<PathBuf>,

    #[serde(default)]
    pub bind_paths: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<PathBuf>,
}

// ============================================================================
// Delegation Configuration
// ============================================================================

/// Delegation/subagent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationConfig {
    /// Maximum number of subagents to spawn in parallel
    #[serde(default = "default_max_parallel_subagents")]
    pub max_parallel_subagents: usize,

    /// Default iteration budget per subagent
    #[serde(default = "default_iterations_per_subagent")]
    pub default_iterations_per_subagent: u32,

    /// Model to use for subagents (can differ from main agent)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_model: Option<String>,

    /// Provider to use for subagents
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_provider: Option<String>,

    /// Enable context isolation for subagents
    #[serde(default)]
    pub isolate_context: bool,
}

impl Default for DelegationConfig {
    fn default() -> Self {
        Self {
            max_parallel_subagents: 3,
            default_iterations_per_subagent: 30,
            subagent_model: None,
            subagent_provider: None,
            isolate_context: false,
        }
    }
}

fn default_max_parallel_subagents() -> usize { 3 }
fn default_iterations_per_subagent() -> u32 { 30 }

// ============================================================================
// Memory Configuration
// ============================================================================

/// Memory system configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Enable memory system
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Memory consolidation interval (seconds)
    #[serde(default)]
    pub consolidation_interval_seconds: u64,

    /// Maximum memory entries before consolidation
    #[serde(default)]
    pub max_entries: usize,

    /// Enable Honcho integration
    #[serde(default)]
    pub honcho_enabled: bool,

    /// Honcho API URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub honcho_url: Option<String>,

    /// Memory provider type (local, honcho, custom)
    #[serde(default)]
    pub provider: MemoryProviderType,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProviderType {
    #[default]
    Local,
    Honcho,
    Custom,
}

// ============================================================================
// Platforms Configuration
// ============================================================================

/// Platform-specific configurations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram: Option<TelegramConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<DiscordConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slack: Option<SlackConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whatsapp: Option<WhatsAppConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<MatrixConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homeassistant: Option<HomeAssistantConfig>,
}

/// Telegram platform configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelegramConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<i64>,

    #[serde(default)]
    pub allowed_groups: Vec<i64>,

    #[serde(default)]
    pub use_webhook: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
}

/// Discord platform configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    #[serde(default)]
    pub allowed_channels: Vec<u64>,

    #[serde(default)]
    pub allowed_guilds: Vec<u64>,

    #[serde(default = "default_bot_prefix")]
    pub bot_prefix: String,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            token: None,
            allowed_channels: Vec::new(),
            allowed_guilds: Vec::new(),
            bot_prefix: "!".to_string(),
        }
    }
}

fn default_bot_prefix() -> String { "!".to_string() }

/// Slack platform configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlackConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_token: Option<String>,

    #[serde(default)]
    pub allowed_channels: Vec<String>,

    #[serde(default)]
    pub signing_secret: Option<String>,
}

/// WhatsApp platform configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WhatsAppConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    #[serde(default)]
    pub allowed_numbers: Vec<String>,
}

/// Signal platform configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SignalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_path: Option<PathBuf>,

    #[serde(default)]
    pub phone_number: Option<String>,

    #[serde(default)]
    pub allowed_recipients: Vec<String>,
}

/// Matrix platform configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatrixConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homeserver_url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,

    #[serde(default)]
    pub allowed_rooms: Vec<String>,
}

/// Home Assistant platform configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HomeAssistantConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    #[serde(default)]
    pub entity_whitelist: Vec<String>,
}

// ============================================================================
// MCP Configuration
// ============================================================================

/// MCP server configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,

    #[serde(default)]
    pub auto_discover: bool,
}

/// Individual MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,

    #[serde(flatten)]
    pub transport: McpTransport,

    #[serde(default)]
    pub enabled: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// MCP transport configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: std::collections::HashMap<String, String>,
    },
    Sse {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<std::collections::HashMap<String, String>>,
    },
}

// ============================================================================
// Cron Configuration
// ============================================================================

/// Cron scheduler configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CronConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub jobs: Vec<CronJobConfig>,
}

/// Individual cron job configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobConfig {
    pub id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    pub schedule: String, // Cron expression

    pub prompt: String, // What to ask the agent

    #[serde(default)]
    pub delivery: DeliveryTarget,

    #[serde(default)]
    pub enabled: bool,
}

/// Delivery target for cron job results.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeliveryTarget {
    #[default]
    Log,
    Telegram { chat_id: i64 },
    Discord { channel_id: u64 },
    Slack { channel: String },
    Email { to: String },
    Webhook { url: String },
}

// ============================================================================
// Web UI Configuration
// ============================================================================

/// Web UI server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_web_host")]
    pub host: String,

    #[serde(default = "default_web_port")]
    pub port: u16,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_dir: Option<PathBuf>,

    #[serde(default)]
    pub cors_origins: Vec<String>,

    #[serde(default)]
    pub auth_enabled: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_secret: Option<String>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".to_string(),
            port: 8080,
            static_dir: None,
            cors_origins: Vec::new(),
            auth_enabled: false,
            auth_secret: None,
        }
    }
}

fn default_web_host() -> String { "127.0.0.1".to_string() }
fn default_web_port() -> u16 { 8080 }

// ============================================================================
// Skin Configuration
// ============================================================================

/// Skin/theme configuration for TUI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkinConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default)]
    pub colors: ColorConfig,

    #[serde(default)]
    pub spinner: SpinnerConfig,

    #[serde(default)]
    pub banner: BannerConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ColorConfig {
    #[serde(default)]
    pub primary: Option<String>,

    #[serde(default)]
    pub secondary: Option<String>,

    #[serde(default)]
    pub accent: Option<String>,

    #[serde(default)]
    pub background: Option<String>,

    #[serde(default)]
    pub text: Option<String>,

    #[serde(default)]
    pub error: Option<String>,

    #[serde(default)]
    pub success: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpinnerConfig {
    #[serde(default = "default_spinner_frames")]
    pub frames: Vec<String>,

    #[serde(default = "default_spinner_interval")]
    pub interval_ms: u64,
}

impl Default for SpinnerConfig {
    fn default() -> Self {
        Self {
            frames: default_spinner_frames(),
            interval_ms: 80,
        }
    }
}

fn default_spinner_frames() -> Vec<String> {
    vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}
fn default_spinner_interval() -> u64 { 80 }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BannerConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub custom_text: Option<String>,

    #[serde(default)]
    pub show_version: bool,
}

// ============================================================================
// Plugins Configuration
// ============================================================================

/// Plugin system configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub auto_discover: bool,

    #[serde(default)]
    pub plugin_dirs: Vec<PathBuf>,

    #[serde(default)]
    pub disabled_plugins: Vec<String>,
}

// ============================================================================
// Profile Configuration
// ============================================================================

/// User profile for different contexts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default)]
    pub config: HermesConfig,

    #[serde(default)]
    pub default: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_yaml_roundtrip() {
        let config = HermesConfig {
            model: Some("claude-sonnet-4-6".to_string()),
            provider: Some("openrouter".to_string()),
            personality: Some("friendly".to_string()),
            terminal: Some(TerminalConfig {
                backend: TerminalBackend::Docker,
                docker: Some(DockerOpts {
                    image: Some("hermes-agent:latest".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: HermesConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(config.model, parsed.model);
        assert_eq!(config.provider, parsed.provider);
        assert_eq!(config.terminal.unwrap().backend, TerminalBackend::Docker);
    }

    #[test]
    fn test_terminal_backend_default() {
        let config = HermesConfig::default();
        assert!(config.terminal.is_none());
    }

    #[test]
    fn test_model_ref() {
        let config = HermesConfig {
            model: Some("gpt-4o".to_string()),
            provider: Some("openai".to_string()),
            ..Default::default()
        };

        let model_ref = config.model_ref();
        assert_eq!(model_ref.provider.as_str(), "openai");
        assert_eq!(model_ref.model.as_str(), "gpt-4o");
    }

    #[test]
    fn test_mcp_transport_stdio() {
        let yaml = r#"
name: test-server
type: stdio
command: python
args: ["-m", "mcp_server"]
enabled: true
"#;

        let server: McpServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(server.name, "test-server");
        assert!(matches!(server.transport, McpTransport::Stdio { .. }));
    }

    #[test]
    fn test_mcp_transport_sse() {
        let yaml = r#"
name: remote-server
type: sse
url: http://localhost:3000/mcp
enabled: true
"#;

        let server: McpServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(server.name, "remote-server");
        assert!(matches!(server.transport, McpTransport::Sse { .. }));
    }
}