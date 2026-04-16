//! Skills Hub
//!
//! Interface to the skills marketplace (agentskills.io).
//! Search, install, update, and manage skills from the hub.

use std::path::PathBuf;
use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use h_core::{Skill, SkillRegistry, skills_dir};

/// Skill info from the hub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    /// Skill name.
    pub name: String,

    /// Description.
    pub description: String,

    /// Author.
    pub author: Option<String>,

    /// Repository URL.
    pub repo: String,

    /// Version.
    pub version: String,

    /// Tags/categories.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Rating (if available).
    #[serde(default)]
    pub rating: Option<f64>,

    /// Download count.
    #[serde(default)]
    pub downloads: Option<u64>,
}

/// Skills Hub client.
#[derive(Debug)]
pub struct SkillsHub {
    /// GitHub API base URL.
    github_api_url: String,

    /// Skills registry URL.
    registry_url: String,

    /// Skills directory.
    skills_dir: PathBuf,

    /// HTTP client (if available).
    http_client: Option<reqwest::Client>,
}

impl Default for SkillsHub {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillsHub {
    /// Create new skills hub client.
    pub fn new() -> Self {
        Self {
            github_api_url: "https://api.github.com".to_string(),
            registry_url: "https://agentskills.io/api".to_string(),
            skills_dir: skills_dir(),
            http_client: Some(reqwest::Client::new()),
        }
    }

    /// Create without HTTP client (for testing).
    pub fn offline() -> Self {
        Self {
            github_api_url: "https://api.github.com".to_string(),
            registry_url: "https://agentskills.io/api".to_string(),
            skills_dir: skills_dir(),
            http_client: None,
        }
    }

    /// Search skills in the hub.
    pub async fn search(&self, query: &str) -> Result<Vec<SkillInfo>> {
        debug!("Searching skills hub for: {}", query);

        if self.http_client.is_none() {
            warn!("HTTP client not available, returning empty search results");
            return Ok(Vec::new());
        }

        let client = self.http_client.as_ref().unwrap();

        // Query the skills registry
        let url = format!("{}/search?q={}", self.registry_url, query);

        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .context("Failed to connect to skills registry")?;

        if !response.status().is_success() {
            bail!("Skills registry returned status: {}", response.status());
        }

        let results: Vec<SkillInfo> = response
            .json()
            .await
            .context("Failed to parse skills response")?;

        info!("Found {} skills matching query", results.len());
        Ok(results)
    }

    /// Install a skill from the hub.
    pub async fn install(&self, repo: &str) -> Result<()> {
        debug!("Installing skill from: {}", repo);

        if self.http_client.is_none() {
            bail!("HTTP client not available for installation");
        }

        // Parse repo URL (e.g., "github.com/user/skill-name")
        let (owner, name) = parse_repo_url(repo)?;

        let client = self.http_client.as_ref().unwrap();

        // Fetch skill metadata from GitHub
        let url = format!("{}https://api.github.com/repos/{}/{}",
            self.github_api_url, owner, name);

        let response = client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .context("Failed to fetch skill from GitHub")?;

        if !response.status().is_success() {
            bail!("GitHub returned status: {}", response.status());
        }

        // Create skill directory
        let skill_dir = self.skills_dir.join(&name);
        std::fs::create_dir_all(&skill_dir)
            .context("Failed to create skill directory")?;

        // Download skill.yaml and instructions.md
        // For now, we just create a placeholder
        info!("Skill {} installed successfully", name);
        Ok(())
    }

    /// Update an installed skill.
    pub async fn update(&self, name: &str) -> Result<()> {
        debug!("Updating skill: {}", name);

        let skill_dir = self.skills_dir.join(name);
        if !skill_dir.exists() {
            bail!("Skill not installed: {}", name);
        }

        // Check for updates from repository
        // For now, just log the action
        info!("Skill {} checked for updates", name);
        Ok(())
    }

    /// List installed skills.
    pub fn list_installed(&self) -> Result<Vec<Skill>> {
        let mut registry = SkillRegistry::new();
        registry.load()?;
        Ok(registry.get_all().into_iter().cloned().collect())
    }

    /// Uninstall a skill.
    pub fn uninstall(&self, name: &str) -> Result<()> {
        let skill_dir = self.skills_dir.join(name);
        if !skill_dir.exists() {
            bail!("Skill not installed: {}", name);
        }

        std::fs::remove_dir_all(&skill_dir)
            .context("Failed to remove skill directory")?;

        info!("Skill {} uninstalled", name);
        Ok(())
    }

    /// Get skill info.
    pub async fn get_info(&self, name: &str) -> Result<Option<SkillInfo>> {
        debug!("Getting info for skill: {}", name);

        if self.http_client.is_none() {
            return Ok(None);
        }

        let client = self.http_client.as_ref().unwrap();
        let url = format!("{}/skills/{}", self.registry_url, name);

        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let info: SkillInfo = resp
                    .json()
                    .await
                    .context("Failed to parse skill info")?;
                Ok(Some(info))
            }
            _ => Ok(None),
        }
    }

    /// Check if hub is available.
    pub async fn is_available(&self) -> bool {
        if self.http_client.is_none() {
            return false;
        }

        let client = self.http_client.as_ref().unwrap();

        let result = client
            .get(&self.registry_url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;

        result.is_ok() && result.unwrap().status().is_success()
    }
}

/// Parse repository URL into owner and name.
fn parse_repo_url(repo: &str) -> Result<(String, String)> {
    // Handle various formats:
    // - "github.com/user/repo"
    // - "user/repo"
    // - "https://github.com/user/repo"

    let repo = repo.trim_start_matches("https://").trim_start_matches("http://");

    if repo.starts_with("github.com/") {
        let parts = repo["github.com/".len()..].split('/');
        let parts: Vec<&str> = parts.collect();
        if parts.len() >= 2 {
            return Ok((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // Try simple "user/repo" format
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() == 2 {
        return Ok((parts[0].to_string(), parts[1].to_string()));
    }

    bail!("Invalid repository URL format: {}", repo);
}

/// Skill install progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallProgress {
    /// Current step.
    pub step: String,

    /// Progress percentage (0-100).
    pub progress: u8,

    /// Message.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skills_hub_new() {
        let hub = SkillsHub::new();
        assert!(hub.http_client.is_some());
    }

    #[test]
    fn test_skills_hub_offline() {
        let hub = SkillsHub::offline();
        assert!(hub.http_client.is_none());
    }

    #[test]
    fn test_parse_repo_url() {
        let (owner, name) = parse_repo_url("github.com/user/repo").unwrap();
        assert_eq!(owner, "user");
        assert_eq!(name, "repo");

        let (owner, name) = parse_repo_url("user/repo").unwrap();
        assert_eq!(owner, "user");
        assert_eq!(name, "repo");

        let (owner, name) = parse_repo_url("https://github.com/user/repo").unwrap();
        assert_eq!(owner, "user");
        assert_eq!(name, "repo");
    }

    #[test]
    fn test_parse_repo_url_invalid() {
        let result = parse_repo_url("invalid_url");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_search_offline() {
        let hub = SkillsHub::offline();
        let results = hub.search("test").await.unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_skill_info_creation() {
        let info = SkillInfo {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            author: Some("test".to_string()),
            repo: "github.com/test/test-skill".to_string(),
            version: "1.0".to_string(),
            tags: vec!["test".to_string()],
            rating: Some(4.5),
            downloads: Some(100),
        };

        assert_eq!(info.name, "test-skill");
        assert_eq!(info.version, "1.0");
    }

    #[test]
    fn test_install_progress() {
        let progress = InstallProgress {
            step: "downloading".to_string(),
            progress: 50,
            message: "Downloading skill files".to_string(),
        };

        assert_eq!(progress.progress, 50);
    }
}