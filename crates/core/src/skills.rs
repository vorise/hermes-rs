//! Skills System
//!
//! Skills are reusable instruction modules for the Hermes agent.
//! Each skill provides specialized capabilities and instructions.

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::fs;
use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::home::skills_dir;

/// Skill definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Skill name.
    pub name: String,

    /// Human-readable description.
    pub description: String,

    /// Skill instructions (markdown).
    pub instructions: String,

    /// Version string.
    #[serde(default)]
    pub version: String,

    /// Required tools for this skill.
    #[serde(default)]
    pub tool_requirements: Vec<String>,

    /// Required toolsets for this skill.
    #[serde(default)]
    pub toolset_requirements: Vec<String>,

    /// Whether skill is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Platforms where this skill is active.
    #[serde(default)]
    pub platforms: Vec<String>,

    /// Author/creator.
    #[serde(default)]
    pub author: Option<String>,

    /// Repository URL.
    #[serde(default)]
    pub repo: Option<String>,
}

fn default_enabled() -> bool { true }

impl Skill {
    /// Load skill from directory.
    pub fn from_dir(dir: &Path) -> Result<Self> {
        let skill_file = dir.join("skill.yaml");

        if !skill_file.exists() {
            // Try skill.md as fallback
            let md_file = dir.join("skill.md");
            if md_file.exists() {
                return Self::from_md_file(&md_file);
            }
            bail!("No skill.yaml or skill.md found in {}", dir.display());
        }

        let content = fs::read_to_string(&skill_file)
            .context("Failed to read skill.yaml")?;

        let mut skill: Skill = serde_yaml::from_str(&content)
            .context("Failed to parse skill.yaml")?;

        // Load instructions from instructions.md if exists
        let instructions_file = dir.join("instructions.md");
        if instructions_file.exists() {
            skill.instructions = fs::read_to_string(&instructions_file)
                .context("Failed to read instructions.md")?;
        }

        skill.name = dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(skill)
    }

    /// Load skill from markdown file (simple format).
    pub fn from_md_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .context("Failed to read skill.md")?;

        let name = path.parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        // Parse frontmatter if present
        let (frontmatter, instructions) = parse_skill_frontmatter(&content)?;

        Ok(Self {
            name: name.to_string(),
            description: frontmatter.get("description")
                .and_then(|v| v.as_str())
                .unwrap_or(name)
                .to_string(),
            instructions,
            version: frontmatter.get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("1.0")
                .to_string(),
            tool_requirements: frontmatter.get("tools")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            toolset_requirements: frontmatter.get("toolsets")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            enabled: frontmatter.get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            platforms: frontmatter.get("platforms")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            author: frontmatter.get("author")
                .and_then(|v| v.as_str())
                .map(String::from),
            repo: None,
        })
    }

    /// Get skill system prompt injection.
    pub fn to_system_prompt(&self) -> String {
        format!(
            "# Skill: {}\n\n{}\n\n{}",
            self.name,
            self.description,
            self.instructions
        )
    }

    /// Check if skill is compatible with current platform.
    pub fn is_compatible(&self, platform: &str) -> bool {
        if self.platforms.is_empty() {
            return true;  // No platform restriction
        }
        self.platforms.contains(&platform.to_lowercase())
    }

    /// Validate skill dependencies.
    pub fn validate_tools(&self, available_tools: &[String], available_toolsets: &[String]) -> Result<()> {
        for tool in &self.tool_requirements {
            if !available_tools.contains(tool) {
                warn!("Skill {} requires tool {} which is not available", self.name, tool);
            }
        }

        for toolset in &self.toolset_requirements {
            if !available_toolsets.contains(toolset) {
                warn!("Skill {} requires toolset {} which is not available", self.name, toolset);
            }
        }

        Ok(())
    }

    /// Save skill to directory.
    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;

        // Save skill.yaml
        let yaml_content = serde_yaml::to_string(&self)
            .context("Failed to serialize skill")?;

        fs::write(dir.join("skill.yaml"), yaml_content)?;

        // Save instructions.md
        fs::write(dir.join("instructions.md"), &self.instructions)?;

        info!("Saved skill: {} to {}", self.name, dir.display());
        Ok(())
    }
}

/// Parse skill markdown frontmatter.
fn parse_skill_frontmatter(content: &str) -> Result<(HashMap<String, serde_json::Value>, String)> {
    if !content.starts_with("---") {
        return Ok((HashMap::new(), content.to_string()));
    }

    let end_marker = content[3..].find("---");
    if end_marker.is_none() {
        return Ok((HashMap::new(), content.to_string()));
    }

    let frontmatter_str = &content[3..end_marker.unwrap() + 3];
    let body = &content[end_marker.unwrap() + 6..];

    let mut frontmatter = HashMap::new();

    for line in frontmatter_str.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            let json_value = if value.starts_with('"') && value.ends_with('"') {
                serde_json::Value::String(value[1..value.len()-1].to_string())
            } else if value.starts_with('[') && value.ends_with(']') {
                // Parse array
                let items: Vec<serde_json::Value> = value[1..value.len()-1]
                    .split(',')
                    .map(|s| serde_json::Value::String(s.trim().to_string()))
                    .collect();
                serde_json::Value::Array(items)
            } else if value == "true" {
                serde_json::Value::Bool(true)
            } else if value == "false" {
                serde_json::Value::Bool(false)
            } else {
                serde_json::Value::String(value.to_string())
            };

            frontmatter.insert(key.to_string(), json_value);
        }
    }

    Ok((frontmatter, body.trim().to_string()))
}

/// Skill registry for managing installed skills.
#[derive(Debug)]
pub struct SkillRegistry {
    /// Skills directory.
    skills_dir: PathBuf,

    /// Loaded skills.
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    /// Create new skill registry.
    pub fn new() -> Self {
        Self {
            skills_dir: skills_dir(),
            skills: HashMap::new(),
        }
    }

    /// Create with custom directory.
    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            skills_dir: dir,
            skills: HashMap::new(),
        }
    }

    /// Load all skills from directory.
    pub fn load(&mut self) -> Result<()> {
        if !self.skills_dir.exists() {
            debug!("Skills directory does not exist: {}", self.skills_dir.display());
            return Ok(());
        }

        for entry in fs::read_dir(&self.skills_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Ok(skill) = Skill::from_dir(&path) {
                    let name = skill.name.clone();
                    self.skills.insert(name.clone(), skill);
                    debug!("Loaded skill: {}", name);
                }
            }
        }

        info!("Loaded {} skills", self.skills.len());
        Ok(())
    }

    /// Get all skills.
    pub fn get_all(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Get enabled skills.
    pub fn get_enabled(&self) -> Vec<&Skill> {
        self.skills.values().filter(|s| s.enabled).collect()
    }

    /// Get skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// Get skill mutably.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Skill> {
        self.skills.get_mut(name)
    }

    /// Add skill.
    pub fn insert(&mut self, skill: Skill) {
        self.skills.insert(skill.name.clone(), skill);
    }

    /// Remove skill.
    pub fn remove(&mut self, name: &str) -> Option<Skill> {
        self.skills.remove(name)
    }

    /// Enable/disable skill.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<()> {
        let skill = self.skills.get_mut(name)
            .context("Skill not found")?;
        skill.enabled = enabled;
        Ok(())
    }

    /// Check if skill exists.
    pub fn contains(&self, name: &str) -> bool {
        self.skills.contains_key(name)
    }

    /// Count skills.
    pub fn count(&self) -> usize {
        self.skills.len()
    }

    /// Count enabled skills.
    pub fn count_enabled(&self) -> usize {
        self.skills.values().filter(|s| s.enabled).count()
    }

    /// Build combined system prompt from all enabled skills.
    pub fn build_skills_prompt(&self) -> String {
        let enabled = self.get_enabled();

        if enabled.is_empty() {
            return String::new();
        }

        let mut prompt = String::from("# Active Skills\n\n");

        for skill in enabled {
            prompt.push_str(&skill.to_system_prompt());
            prompt.push_str("\n\n---\n\n");
        }

        prompt
    }

    /// Validate all skills against available tools.
    pub fn validate_all(&self, tools: &[String], toolsets: &[String]) -> Result<()> {
        for skill in self.skills.values() {
            skill.validate_tools(tools, toolsets)?;
        }
        Ok(())
    }

    /// Get skills compatible with platform.
    pub fn get_compatible(&self, platform: &str) -> Vec<&Skill> {
        self.skills.values()
            .filter(|s| s.enabled && s.is_compatible(platform))
            .collect()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Skill nudge configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillNudgeConfig {
    /// Interval between nudges (in turns).
    pub interval_turns: u32,

    /// Message template.
    pub message_template: String,

    /// Maximum skills to suggest creating.
    pub max_suggestions: usize,
}

impl Default for SkillNudgeConfig {
    fn default() -> Self {
        Self {
            interval_turns: 100,
            message_template: "Have you developed a reusable pattern? Consider creating a skill for it.".to_string(),
            max_suggestions: 2,
        }
    }
}

/// Skills guard for safety checks.
#[derive(Debug, Clone)]
pub struct SkillsGuard {
    /// Blocked skill operations.
    blocked_operations: Vec<String>,

    /// Require approval for these skills.
    approval_required: Vec<String>,
}

impl Default for SkillsGuard {
    fn default() -> Self {
        Self {
            blocked_operations: Vec::new(),
            approval_required: Vec::new(),
        }
    }
}

impl SkillsGuard {
    /// Create new guard.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if skill operation is allowed.
    pub fn is_allowed(&self, skill_name: &str, operation: &str) -> bool {
        let combined = format!("{}:{}", skill_name, operation);
        !self.blocked_operations.iter().any(|s| s == &combined)
            && !self.blocked_operations.iter().any(|s| s == operation)
    }

    /// Check if operation requires approval.
    pub fn needs_approval(&self, skill_name: &str) -> bool {
        self.approval_required.iter().any(|s| s == skill_name)
    }

    /// Block an operation.
    pub fn block(&mut self, operation: String) {
        self.blocked_operations.push(operation);
    }

    /// Require approval for a skill.
    pub fn require_approval(&mut self, skill_name: String) {
        self.approval_required.push(skill_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_creation() {
        let skill = Skill {
            name: "code-review".to_string(),
            description: "Code review patterns".to_string(),
            instructions: "Review code for quality".to_string(),
            version: "1.0".to_string(),
            tool_requirements: vec!["read_file".to_string()],
            toolset_requirements: vec!["filesystem".to_string()],
            enabled: true,
            platforms: vec!["github".to_string()],
            author: Some("hermes".to_string()),
            repo: None,
        };

        assert_eq!(skill.name, "code-review");
        assert!(skill.enabled);
        assert!(skill.is_compatible("github"));
        assert!(!skill.is_compatible("slack"));
    }

    #[test]
    fn test_skill_system_prompt() {
        let skill = Skill {
            name: "test".to_string(),
            description: "Test skill".to_string(),
            instructions: "Instructions here".to_string(),
            version: "1.0".to_string(),
            tool_requirements: vec![],
            toolset_requirements: vec![],
            enabled: true,
            platforms: vec![],
            author: None,
            repo: None,
        };

        let prompt = skill.to_system_prompt();
        assert!(prompt.contains("# Skill: test"));
        assert!(prompt.contains("Instructions here"));
    }

    #[test]
    fn test_skill_registry_new() {
        let registry = SkillRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_skill_registry_insert() {
        let mut registry = SkillRegistry::new();
        let skill = Skill {
            name: "test".to_string(),
            description: "Test".to_string(),
            instructions: "Test".to_string(),
            version: "1.0".to_string(),
            tool_requirements: vec![],
            toolset_requirements: vec![],
            enabled: true,
            platforms: vec![],
            author: None,
            repo: None,
        };

        registry.insert(skill);
        assert_eq!(registry.count(), 1);
        assert!(registry.contains("test"));
    }

    #[test]
    fn test_skill_registry_enable_disable() {
        let mut registry = SkillRegistry::new();
        let skill = Skill {
            name: "test".to_string(),
            description: "Test".to_string(),
            instructions: "Test".to_string(),
            version: "1.0".to_string(),
            tool_requirements: vec![],
            toolset_requirements: vec![],
            enabled: true,
            platforms: vec![],
            author: None,
            repo: None,
        };

        registry.insert(skill);
        registry.set_enabled("test", false).unwrap();
        assert_eq!(registry.count_enabled(), 0);
    }

    #[test]
    fn test_skill_nudge_config_default() {
        let config = SkillNudgeConfig::default();
        assert_eq!(config.interval_turns, 100);
        assert!(!config.message_template.is_empty());
    }

    #[test]
    fn test_skills_guard() {
        let mut guard = SkillsGuard::new();
        guard.block("dangerous_operation".to_string());
        guard.require_approval("sensitive_skill".to_string());

        assert!(!guard.is_allowed("any_skill", "dangerous_operation"));
        assert!(guard.needs_approval("sensitive_skill"));
        assert!(!guard.needs_approval("safe_skill"));
    }

    #[test]
    fn test_parse_skill_frontmatter() {
        let content = "---\nname: test\nversion: 2.0\ntools: [read, write]\n---\n\nSkill instructions";
        let (fm, body) = parse_skill_frontmatter(content).unwrap();
        assert_eq!(fm.get("name").unwrap().as_str().unwrap(), "test");
        assert_eq!(body, "Skill instructions");
    }

    #[test]
    fn test_registry_build_skills_prompt() {
        let mut registry = SkillRegistry::new();
        registry.insert(Skill {
            name: "test".to_string(),
            description: "Test skill".to_string(),
            instructions: "Do something useful".to_string(),
            version: "1.0".to_string(),
            tool_requirements: vec![],
            toolset_requirements: vec![],
            enabled: true,
            platforms: vec![],
            author: None,
            repo: None,
        });

        let prompt = registry.build_skills_prompt();
        assert!(prompt.contains("# Active Skills"));
        assert!(prompt.contains("test"));
    }
}