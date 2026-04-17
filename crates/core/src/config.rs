use serde::{Deserialize, Serialize};

/// Current configuration schema version.
/// Incremented when the config structure changes in a backward-incompatible way.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Full user configuration. Mirrors ~/.hermes/config.yaml from the Python codebase.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HermesConfig {
    /// Schema version for migration tracking.
    #[serde(default)]
    pub schema_version: Option<u32>,
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

/// Singularity/Apptainer container options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingularityOpts {
    pub image: String,
    #[serde(default)]
    pub gpu: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind_mounts: Option<Vec<String>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub singularity: Option<SingularityOpts>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            backend: TerminalBackend::Local,
            docker: None,
            ssh: None,
            modal: None,
            daytona: None,
            singularity: None,
        }
    }
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
    /// How often (in turns) to nudge the agent to review/consolidate memories.
    /// 0 or None disables memory nudges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_nudge_interval: Option<u32>,
    /// How often (in tool iterations) to nudge the agent to create skills from novel tasks.
    /// 0 or None disables skill nudges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_nudge_interval: Option<u32>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: Some(true),
            honcho_url: None,
            memory_nudge_interval: Some(10),
            skill_nudge_interval: Some(5),
        }
    }
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

/// Result of a config migration operation.
#[derive(Debug, Clone)]
pub struct MigrationReport {
    /// Whether a migration was performed.
    pub migrated: bool,
    /// Schema version before migration.
    pub from_version: Option<u32>,
    /// Schema version after migration.
    pub to_version: u32,
    /// List of changes made during migration.
    pub changes: Vec<String>,
    /// Path to the backup file (if migrated).
    pub backup_path: Option<std::path::PathBuf>,
}

/// Detect the schema version of a raw config YAML string.
///
/// Returns None if no schema_version field is found (treated as version 0).
fn detect_schema_version(yaml: &str) -> Option<u32> {
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("schema_version:") {
            let value = trimmed.splitn(2, ':').nth(1)?.trim();
            return value.parse().ok();
        }
    }
    None
}

/// Check if a config file needs migration.
///
/// Returns true if the config is missing a schema_version field
/// or has a version lower than the current schema version.
pub fn needs_migration(path: &std::path::Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false); // No file = no migration needed
    }

    let contents = std::fs::read_to_string(path)?;
    let current = detect_schema_version(&contents);
    Ok(current.map_or(true, |v| v < CURRENT_SCHEMA_VERSION))
}

/// Migrate a config file to the current schema version.
///
/// Creates a backup of the original file before migrating.
/// Returns a MigrationReport describing what changed.
pub fn migrate_config(path: &std::path::Path) -> anyhow::Result<MigrationReport> {
    let contents = std::fs::read_to_string(path)?;
    let from_version = detect_schema_version(&contents);

    if from_version.map_or(false, |v| v >= CURRENT_SCHEMA_VERSION) {
        return Ok(MigrationReport {
            migrated: false,
            from_version,
            to_version: from_version.unwrap_or(0),
            changes: Vec::new(),
            backup_path: None,
        });
    }

    // Create backup before migration
    let backup_path = path.with_extension("yaml.bak");
    std::fs::copy(path, &backup_path)?;

    let mut changes = Vec::new();
    let mut migrated_yaml = contents.clone();

    // Apply migrations sequentially from current version to target
    let start_version = from_version.unwrap_or(0);
    for version in start_version..CURRENT_SCHEMA_VERSION {
        let (new_yaml, migration_changes) = apply_migration(&migrated_yaml, version)?;
        migrated_yaml = new_yaml;
        changes.extend(migration_changes);
    }

    // Ensure final schema version is set
    migrated_yaml = set_schema_version(&migrated_yaml, CURRENT_SCHEMA_VERSION);

    // Write migrated config
    std::fs::write(path, &migrated_yaml)?;

    Ok(MigrationReport {
        migrated: true,
        from_version,
        to_version: CURRENT_SCHEMA_VERSION,
        changes,
        backup_path: Some(backup_path),
    })
}

/// Apply a single migration step from version `from` to `from + 1`.
fn apply_migration(yaml: &str, from: u32) -> anyhow::Result<(String, Vec<String>)> {
    match from {
        0 => migrate_0_to_1(yaml),
        _ => anyhow::bail!("Unknown config schema version: {from}"),
    }
}

/// Migration 0 -> 1:
/// - Add schema_version field
/// - Migrate deprecated `toolsets.enabled` to `enabled_toolsets`
/// - Migrate deprecated `toolsets.disabled` to `disabled_toolsets`
/// - Normalize model string (remove leading "openrouter/" prefix)
/// - Migrate deprecated `personality` format to SOUL.md hint
fn migrate_0_to_1(yaml: &str) -> anyhow::Result<(String, Vec<String>)> {
    let mut changes = Vec::new();
    let mut doc: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    let map = doc.as_mapping_mut().ok_or_else(|| anyhow::anyhow!("Config root must be a mapping"))?;

    // Set schema version
    map.insert(
        serde_yaml::Value::String("schema_version".to_string()),
        serde_yaml::Value::Number(1.into()),
    );
    changes.push("Added schema_version: 1".to_string());

    // Migrate toolsets.enabled -> enabled_toolsets
    if let Some(toolsets) = map.remove(&serde_yaml::Value::String("toolsets".to_string())) {
        if let Some(toolsets_map) = toolsets.as_mapping() {
            if let Some(enabled) = toolsets_map.get(&serde_yaml::Value::String("enabled".to_string())) {
                map.insert(
                    serde_yaml::Value::String("enabled_toolsets".to_string()),
                    enabled.clone(),
                );
                changes.push("Migrated toolsets.enabled → enabled_toolsets".to_string());
            }
            if let Some(disabled) = toolsets_map.get(&serde_yaml::Value::String("disabled".to_string())) {
                map.insert(
                    serde_yaml::Value::String("disabled_toolsets".to_string()),
                    disabled.clone(),
                );
                changes.push("Migrated toolsets.disabled → disabled_toolsets".to_string());
            }
        }
    }

    // Normalize model string: remove "openrouter/" prefix
    let model_value = map.get(&serde_yaml::Value::String("model".to_string())).cloned();
    if let Some(model) = model_value {
        if let Some(model_str) = model.as_str() {
            if model_str.starts_with("openrouter/") {
                let normalized = model_str.trim_start_matches("openrouter/");
                let change_msg = format!("Normalized model: '{model_str}' → '{normalized}'");
                map.insert(
                    serde_yaml::Value::String("model".to_string()),
                    serde_yaml::Value::String(normalized.to_string()),
                );
                changes.push(change_msg);
            }
        }
    }

    let output = serde_yaml::to_string(&doc)?;
    Ok((output, changes))
}

/// Set or update the schema_version field in a YAML string.
fn set_schema_version(yaml: &str, version: u32) -> String {
    // Try to parse as YAML and set the version
    if let Ok(mut doc) = serde_yaml::from_str::<serde_yaml::Value>(yaml) {
        if let Some(map) = doc.as_mapping_mut() {
            map.insert(
                serde_yaml::Value::String("schema_version".to_string()),
                serde_yaml::Value::Number(version.into()),
            );
            if let Ok(output) = serde_yaml::to_string(&doc) {
                return output;
            }
        }
    }
    // Fallback: prepend schema_version line
    format!("schema_version: {version}\n{yaml}")
}

/// Load configuration from YAML, with env var overrides and automatic migration.
///
/// If the config file has an outdated schema, it will be automatically migrated.
/// A backup of the original file is created before migration.
pub fn load_config(path: &std::path::Path) -> anyhow::Result<HermesConfig> {
    if !path.exists() {
        tracing::warn!(?path, "Config file not found, using defaults");
        return Ok(HermesConfig::default());
    }

    // Check if migration is needed
    if needs_migration(path)? {
        let report = migrate_config(path)?;
        if report.migrated {
            tracing::info!(
                from_version = ?report.from_version,
                to_version = report.to_version,
                changes = ?report.changes,
                "Config migrated to current schema version"
            );
            for change in &report.changes {
                tracing::info!("Config migration: {change}");
            }
        }
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
            schema_version: Some(CURRENT_SCHEMA_VERSION),
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: HermesConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.model, config.model);
        assert_eq!(parsed.provider, config.provider);
    }

    #[test]
    fn test_detect_schema_version_present() {
        let yaml = "schema_version: 1\nmodel: claude-sonnet-4-6\n";
        assert_eq!(detect_schema_version(yaml), Some(1));
    }

    #[test]
    fn test_detect_schema_version_missing() {
        let yaml = "model: claude-sonnet-4-6\nprovider: anthropic\n";
        assert_eq!(detect_schema_version(yaml), None);
    }

    #[test]
    fn test_detect_schema_version_zero() {
        let yaml = "schema_version: 0\nmodel: test\n";
        assert_eq!(detect_schema_version(yaml), Some(0));
    }

    #[test]
    fn test_needs_migration_no_file() {
        let result = needs_migration(std::path::Path::new("/nonexistent/config.yaml"));
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_needs_migration_without_schema_version() {
        let tmp = std::env::temp_dir().join(format!("hermes_config_test_{}.yaml", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, "model: claude-sonnet-4-6\n").unwrap();
        assert!(needs_migration(&tmp).unwrap());
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_needs_migration_with_current_version() {
        let tmp = std::env::temp_dir().join(format!("hermes_config_test_{}.yaml", uuid::Uuid::new_v4()));
        let yaml = format!("schema_version: {}\nmodel: claude-sonnet-4-6\n", CURRENT_SCHEMA_VERSION);
        std::fs::write(&tmp, &yaml).unwrap();
        assert!(!needs_migration(&tmp).unwrap());
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_migrate_0_to_1_adds_schema_version() {
        let yaml = "model: claude-sonnet-4-6\nprovider: anthropic\n";
        let (output, changes) = migrate_0_to_1(yaml).unwrap();
        assert!(output.contains("schema_version: 1"));
        assert!(changes.iter().any(|c| c.contains("schema_version")));
    }

    #[test]
    fn test_migrate_0_to_1_toolsets_enabled() {
        let yaml = "model: claude-sonnet-4-6\ntoolsets:\n  enabled:\n    - file\n    - terminal\n";
        let (output, changes) = migrate_0_to_1(yaml).unwrap();
        assert!(output.contains("enabled_toolsets"));
        assert!(changes.iter().any(|c| c.contains("toolsets.enabled")));
        // Ensure the old toolsets key is gone
        let parsed: serde_yaml::Value = serde_yaml::from_str(&output).unwrap();
        assert!(parsed.get("toolsets").is_none());
    }

    #[test]
    fn test_migrate_0_to_1_toolsets_disabled() {
        let yaml = "toolsets:\n  disabled:\n    - browser\n";
        let (output, changes) = migrate_0_to_1(yaml).unwrap();
        assert!(output.contains("disabled_toolsets"));
        assert!(changes.iter().any(|c| c.contains("toolsets.disabled")));
    }

    #[test]
    fn test_migrate_0_to_1_normalize_model() {
        let yaml = "model: openrouter/anthropic/claude-sonnet-4-6\n";
        let (output, changes) = migrate_0_to_1(yaml).unwrap();
        assert!(output.contains("anthropic/claude-sonnet-4-6"));
        assert!(changes.iter().any(|c| c.contains("Normalized model")));
        assert!(!output.contains("openrouter/"));
    }

    #[test]
    fn test_migrate_0_to_1_no_change_for_clean_model() {
        let yaml = "model: claude-sonnet-4-6\n";
        let (output, changes) = migrate_0_to_1(yaml).unwrap();
        assert!(output.contains("model: claude-sonnet-4-6"));
        // Should still have schema_version change
        assert!(changes.iter().any(|c| c.contains("schema_version")));
    }

    #[test]
    fn test_migrate_full_flow() {
        let tmp = std::env::temp_dir().join(format!("hermes_config_test_{}.yaml", uuid::Uuid::new_v4()));
        let old_yaml = r#"model: openrouter/anthropic/claude-sonnet-4-6
toolsets:
  enabled:
    - file
    - terminal
  disabled:
    - browser
provider: openrouter
"#;
        std::fs::write(&tmp, old_yaml).unwrap();

        let report = migrate_config(&tmp).unwrap();
        assert!(report.migrated);
        assert_eq!(report.from_version, None);
        assert_eq!(report.to_version, CURRENT_SCHEMA_VERSION);
        assert!(report.backup_path.is_some());
        assert!(report.backup_path.as_ref().unwrap().exists());
        assert!(report.changes.len() >= 3); // schema + model + 2 toolsets

        // Verify backup contains original content
        let backup = std::fs::read_to_string(report.backup_path.unwrap()).unwrap();
        assert!(backup.contains("openrouter/anthropic"));
        assert!(backup.contains("toolsets:"));

        // Verify migrated config
        let migrated = std::fs::read_to_string(&tmp).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&migrated).unwrap();
        assert_eq!(parsed.get("schema_version").and_then(|v| v.as_u64()), Some(CURRENT_SCHEMA_VERSION as u64));
        assert!(parsed.get("toolsets").is_none());
        assert!(parsed.get("enabled_toolsets").is_some());
        assert!(parsed.get("disabled_toolsets").is_some());
        // Model should be normalized (openrouter/ prefix removed)
        let model = parsed.get("model").and_then(|v| v.as_str()).unwrap();
        assert!(!model.starts_with("openrouter/"));

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_load_config_with_migration() {
        let tmp = std::env::temp_dir().join(format!("hermes_config_test_{}.yaml", uuid::Uuid::new_v4()));
        let old_yaml = "model: claude-sonnet-4-6\nprovider: anthropic\n";
        std::fs::write(&tmp, old_yaml).unwrap();

        let config = load_config(&tmp).unwrap();
        assert_eq!(config.model, Some("claude-sonnet-4-6".to_string()));
        assert_eq!(config.provider, Some("anthropic".to_string()));
        assert_eq!(config.schema_version, Some(CURRENT_SCHEMA_VERSION));

        // Verify file was updated with schema_version
        let content = std::fs::read_to_string(&tmp).unwrap();
        assert!(content.contains("schema_version: 1"));

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_load_config_no_migration_needed() {
        let tmp = std::env::temp_dir().join(format!("hermes_config_test_{}.yaml", uuid::Uuid::new_v4()));
        let yaml = format!("schema_version: {}\nmodel: claude-sonnet-4-6\n", CURRENT_SCHEMA_VERSION);
        std::fs::write(&tmp, &yaml).unwrap();

        let config = load_config(&tmp).unwrap();
        assert_eq!(config.model, Some("claude-sonnet-4-6".to_string()));

        // No backup should be created
        let backup = tmp.with_extension("yaml.bak");
        assert!(!backup.exists());

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_load_config_missing_file() {
        let config = load_config(std::path::Path::new("/nonexistent/config.yaml"));
        assert!(config.is_ok());
        let config = config.unwrap();
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(config.schema_version.is_none());
    }

    #[test]
    fn test_set_schema_version() {
        let yaml = "model: test\n";
        let result = set_schema_version(yaml, 5);
        assert!(result.contains("schema_version: 5"));
    }

    #[test]
    fn test_migration_report_fields() {
        let report = MigrationReport {
            migrated: true,
            from_version: None,
            to_version: 1,
            changes: vec!["test change".to_string()],
            backup_path: Some(std::path::PathBuf::from("/tmp/backup.yaml")),
        };
        assert!(report.migrated);
        assert!(report.from_version.is_none());
        assert_eq!(report.to_version, 1);
        assert_eq!(report.changes.len(), 1);
        assert!(report.backup_path.is_some());
    }
}
