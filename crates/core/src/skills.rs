use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// A skill that can be loaded and injected into the system prompt.
///
/// Skills are stored as markdown files in ~/.hermes/skills/ and contain
/// a YAML frontmatter header with metadata followed by skill instructions.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Unique identifier for the skill (filename without .md).
    pub id: String,
    /// Human-readable name of the skill.
    pub name: String,
    /// Short description of what the skill does.
    pub description: String,
    /// Skill version (semver-compatible string).
    pub version: String,
    /// Author or source of the skill.
    pub author: String,
    /// Full skill content (instructions, typically markdown).
    pub content: String,
    /// Path to the skill file.
    pub path: PathBuf,
    /// Whether the skill is currently enabled.
    pub enabled: bool,
}

/// Metadata parsed from skill frontmatter.
#[derive(Debug, Clone, Default)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
}

/// Manages skill discovery, loading, and registry.
///
/// Skills are stored as individual markdown files in the skills directory.
/// Each skill file has a YAML frontmatter with metadata followed by instructions.
pub struct SkillRegistry {
    skills_dir: PathBuf,
    skills: Vec<Skill>,
}

impl SkillRegistry {
    /// Create a new SkillRegistry with the given skills directory.
    pub fn new(skills_dir: PathBuf) -> Self {
        Self {
            skills_dir,
            skills: Vec::new(),
        }
    }

    /// Create a SkillRegistry with the default path (~/.hermes/skills/).
    pub fn default_path() -> Result<Self> {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let skills_dir = home.join(".hermes").join("skills");
        Ok(Self::new(skills_dir))
    }

    /// Ensure the skills directory exists.
    pub fn ensure_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.skills_dir)
            .with_context(|| format!("Failed to create skills dir: {:?}", self.skills_dir))?;
        Ok(())
    }

    /// Load all skills from the skills directory.
    pub fn load_all(&mut self) -> Result<()> {
        self.skills.clear();
        if !self.skills_dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&self.skills_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "md") {
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("Failed to read skill file: {}", path.display()))?;
                let id = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Skip disabled skills (files starting with "_")
                let enabled = !id.starts_with('_');

                let metadata = parse_frontmatter(&content).unwrap_or_default();
                let skill_content = extract_skill_content(&content);

                self.skills.push(Skill {
                    id: id.clone(),
                    name: if metadata.name.is_empty() { id.clone() } else { metadata.name.clone() },
                    description: metadata.description,
                    version: metadata.version,
                    author: metadata.author,
                    content: skill_content,
                    path,
                    enabled,
                });
            }
        }

        self.skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(())
    }

    /// Get all loaded skills.
    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// Get enabled skills only.
    pub fn enabled_skills(&self) -> Vec<&Skill> {
        self.skills.iter().filter(|s| s.enabled).collect()
    }

    /// Get a skill by id.
    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.id == id)
    }

    /// Get the combined skill instructions for all enabled skills.
    ///
    /// Returns a formatted string suitable for injection into the system prompt.
    pub fn instructions(&self) -> String {
        let mut output = String::new();
        for skill in self.enabled_skills() {
            if !skill.content.is_empty() {
                output.push_str(&format!(
                    "## Skill: {}\n{}\n\n",
                    skill.name, skill.content
                ));
            }
        }
        output
    }

    /// Enable a skill by id.
    pub fn enable(&mut self, id: &str) -> Result<()> {
        let skill = self.skills.iter_mut().find(|s| s.id == id)
            .ok_or_else(|| anyhow::anyhow!("Skill not found: {id}"))?;
        skill.enabled = true;
        Ok(())
    }

    /// Disable a skill by id.
    pub fn disable(&mut self, id: &str) -> Result<()> {
        let skill = self.skills.iter_mut().find(|s| s.id == id)
            .ok_or_else(|| anyhow::anyhow!("Skill not found: {id}"))?;
        skill.enabled = false;
        Ok(())
    }

    /// Install a skill from content.
    ///
    /// Writes the skill content to a new file in the skills directory.
    pub fn install(&mut self, id: &str, content: &str) -> Result<()> {
        self.ensure_dir()?;
        let sanitized = sanitize_skill_filename(id);
        let path = self.skills_dir.join(format!("{sanitized}.md"));
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write skill file: {}", path.display()))?;
        // Reload to pick up the new skill
        self.load_all()?;
        Ok(())
    }

    /// Uninstall a skill by id.
    pub fn uninstall(&mut self, id: &str) -> Result<()> {
        let skill = self.skills.iter().find(|s| s.id == id)
            .ok_or_else(|| anyhow::anyhow!("Skill not found: {id}"))?;
        std::fs::remove_file(&skill.path)
            .with_context(|| format!("Failed to remove skill file: {}", skill.path.display()))?;
        self.skills.retain(|s| s.id != id);
        Ok(())
    }

    /// Get the number of loaded skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Check if no skills are loaded.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

/// Parse YAML frontmatter from skill content.
///
/// Returns metadata parsed from the frontmatter block,
/// or None if no frontmatter is found.
fn parse_frontmatter(content: &str) -> Option<SkillMetadata> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    let end = rest.find("---")?;
    let yaml = &rest[..end];

    let mut metadata = SkillMetadata::default();
    for line in yaml.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_lowercase();
            let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
            match key.as_str() {
                "name" => metadata.name = value,
                "description" => metadata.description = value,
                "version" => metadata.version = value,
                "author" => metadata.author = value,
                _ => {}
            }
        }
    }

    Some(metadata)
}

/// Extract the skill content (everything after the frontmatter).
fn extract_skill_content(content: &str) -> String {
    let content = content.trim_start();
    if content.starts_with("---") {
        if let Some(pos) = content[3..].find("---") {
            return content[3 + pos + 3..].trim().to_string();
        }
    }
    content.to_string()
}

/// Sanitize a skill name for use as a filename.
fn sanitize_skill_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .collect::<String>()
        .to_lowercase()
        .replace(' ', "_")
}

/// Validate a skill file has correct frontmatter.
///
/// Returns Ok(()) if the skill is valid, or an error describing the issue.
pub fn validate_skill(path: &Path) -> Result<SkillMetadata> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read skill file: {}", path.display()))?;

    let metadata = parse_frontmatter(&content)
        .ok_or_else(|| anyhow::anyhow!("Skill file missing frontmatter: {}", path.display()))?;

    if metadata.name.is_empty() {
        anyhow::bail!("Skill file missing 'name' field in frontmatter: {}", path.display());
    }
    if metadata.description.is_empty() {
        anyhow::bail!("Skill file missing 'description' field in frontmatter: {}", path.display());
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_skills_dir() -> PathBuf {
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let dir = std::env::temp_dir().join(format!("hermes_skills_{id}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn test_parse_frontmatter() {
        let content = r#"---
name: Test Skill
description: A test skill
version: 1.0.0
author: Test Author
---

This is the skill content.
"#;
        let metadata = parse_frontmatter(content).unwrap();
        assert_eq!(metadata.name, "Test Skill");
        assert_eq!(metadata.description, "A test skill");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.author, "Test Author");
    }

    #[test]
    fn test_parse_frontmatter_missing() {
        let content = "Just content, no frontmatter.";
        assert!(parse_frontmatter(content).is_none());
    }

    #[test]
    fn test_extract_skill_content() {
        let content = "---\nname: Test\n---\nThis is the content.";
        let result = extract_skill_content(content);
        assert_eq!(result, "This is the content.");
    }

    #[test]
    fn test_extract_skill_content_no_frontmatter() {
        let content = "Just plain content.";
        let result = extract_skill_content(content);
        assert_eq!(result, "Just plain content.");
    }

    #[test]
    fn test_sanitize_skill_filename() {
        assert_eq!(sanitize_skill_filename("My Skill"), "my_skill");
        assert_eq!(sanitize_skill_filename("test-skill"), "test-skill");
        assert_eq!(sanitize_skill_filename("My Awesome Skill!"), "my_awesome_skill");
    }

    #[test]
    fn test_skill_registry_load_all_empty() {
        let dir = temp_skills_dir();
        let mut registry = SkillRegistry::new(dir.clone());
        registry.load_all().unwrap();
        assert!(registry.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_skill_registry_load_and_install() {
        let dir = temp_skills_dir();
        let mut registry = SkillRegistry::new(dir.clone());

        let skill_content = r#"---
name: Test Skill
description: A test skill for registry
version: 0.1.0
author: Tester
---

Do something useful.
"#;
        registry.install("test_skill", skill_content).unwrap();
        assert_eq!(registry.len(), 1);

        let skill = registry.get("test_skill").unwrap();
        assert_eq!(skill.name, "Test Skill");
        assert!(skill.enabled);
        assert!(skill.content.contains("Do something useful"));

        // Test instructions output
        let instructions = registry.instructions();
        assert!(instructions.contains("## Skill: Test Skill"));
        assert!(instructions.contains("Do something useful"));

        // Test enable/disable
        registry.disable("test_skill").unwrap();
        assert!(!registry.get("test_skill").unwrap().enabled);
        let instructions = registry.instructions();
        assert!(!instructions.contains("Test Skill"));

        registry.enable("test_skill").unwrap();
        assert!(registry.get("test_skill").unwrap().enabled);

        // Test uninstall
        registry.uninstall("test_skill").unwrap();
        assert_eq!(registry.len(), 0);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_validate_skill() {
        let dir = temp_skills_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("valid.md");
        fs::write(&path, "---\nname: Valid\ndescription: A valid skill\nversion: 1.0\n---\ncontent\n").unwrap();

        let metadata = validate_skill(&path).unwrap();
        assert_eq!(metadata.name, "Valid");

        // Missing description should fail
        let path2 = dir.join("no_desc.md");
        fs::write(&path2, "---\nname: NoDesc\nversion: 1.0\n---\ncontent\n").unwrap();
        assert!(validate_skill(&path2).is_err());

        let _ = fs::remove_dir_all(dir);
    }
}
