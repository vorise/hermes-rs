use std::collections::HashMap;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

/// A skill entry from the remote registry.
#[derive(Debug, Clone)]
pub struct RegistrySkill {
    /// Skill identifier (filename without extension).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Short description.
    pub description: String,
    /// Version string.
    pub version: String,
    /// Author or source.
    pub author: String,
    /// Full skill content (markdown instructions).
    pub content: String,
}

/// Client for the agentskills.io skill registry (GitHub-backed).
///
/// Uses the GitHub API to browse, search, and download skill files
/// from the agentskills.io repository.
pub struct SkillsHubClient {
    client: Client,
    /// GitHub owner (e.g., "agentskills-io").
    owner: String,
    /// GitHub repository name (e.g., "skills").
    repo: String,
    /// Path within the repo where skill files live (e.g., "skills/").
    skill_path: String,
    /// GitHub API base URL.
    api_base: String,
}

impl SkillsHubClient {
    /// Create a new client with default agentskills.io settings.
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            owner: "anthropics".to_string(),
            repo: "hermes-agent".to_string(),
            skill_path: "skills/".to_string(),
            api_base: "https://api.github.com".to_string(),
        }
    }

    /// Create a client with custom repository settings.
    pub fn with_repo(owner: &str, repo: &str, skill_path: &str) -> Self {
        Self {
            client: Client::new(),
            owner: owner.to_string(),
            repo: repo.to_string(),
            skill_path: skill_path.trim_end_matches('/').to_string() + "/",
            api_base: "https://api.github.com".to_string(),
        }
    }

    /// Set the GitHub API base URL (for enterprise or proxy setups).
    pub fn with_api_base(mut self, base: &str) -> Self {
        self.api_base = base.to_string();
        self
    }

    /// Set a GitHub personal access token for higher rate limits.
    pub fn with_token(mut self, token: &str) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("hermes-agent"),
        );
        if !token.is_empty() {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")),
            );
        }
        self.client = Client::builder()
            .default_headers(headers)
            .build()
            .unwrap_or_else(|_| Client::new());
        self
    }

    /// List all available skills from the registry.
    pub async fn list_skills(&self) -> Result<Vec<RegistrySkill>> {
        let url = format!(
            "{}/repos/{}/{}/contents/{}",
            self.api_base, self.owner, self.repo, self.skill_path
        );

        let response = self
            .client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
            .send()
            .await
            .with_context(|| format!("Failed to fetch skills from {url}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "GitHub API returned {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            ));
        }

        let items: Vec<GitHubContent> = response
            .json()
            .await
            .with_context(|| "Failed to parse GitHub API response")?;

        // Filter to .md files and fetch each one
        let mut skills = Vec::new();
        for item in items {
            if item.name.ends_with(".md") && item.r#type == "file" {
                if let Ok(skill) = self.fetch_skill(&item).await {
                    skills.push(skill);
                }
            }
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(skills)
    }

    /// Search skills by query (matches name and description).
    pub async fn search_skills(&self, query: &str) -> Result<Vec<RegistrySkill>> {
        let all = self.list_skills().await?;
        let query_lower = query.to_lowercase();

        let results = all
            .into_iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&query_lower)
                    || s.description.to_lowercase().contains(&query_lower)
                    || s.id.to_lowercase().contains(&query_lower)
                    || s.author.to_lowercase().contains(&query_lower)
            })
            .collect();

        Ok(results)
    }

    /// Fetch a single skill by its filename.
    pub async fn fetch_skill_by_name(&self, name: &str) -> Result<RegistrySkill> {
        let filename = if name.ends_with(".md") {
            name.to_string()
        } else {
            format!("{name}.md")
        };

        let url = format!(
            "{}/repos/{}/{}/contents/{}{}",
            self.api_base, self.owner, self.repo, self.skill_path, filename
        );

        let response = self
            .client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
            .send()
            .await
            .with_context(|| format!("Failed to fetch skill '{name}' from {url}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "GitHub API returned {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            ));
        }

        let content: GitHubContent = response
            .json()
            .await
            .with_context(|| "Failed to parse skill file response")?;

        self.decode_skill(&content)
    }

    /// Fetch all skills and return a formatted summary for display.
    pub async fn browse_summary(&self) -> Result<String> {
        let skills = self.list_skills().await?;
        if skills.is_empty() {
            return Ok("No skills found in the registry.".to_string());
        }

        let mut output = format!("## Skills Registry ({})\n\n", skills.len());
        for skill in &skills {
            output.push_str(&format!(
                "- **{}** ({}) — {}\n",
                skill.name, skill.id, skill.description
            ));
        }
        output.push_str("\nInstall with: `/skills install <skill_id>`");
        Ok(output)
    }

    // ── Internal helpers ─────────────────────────────────────────────

    async fn fetch_skill(&self, item: &GitHubContent) -> Result<RegistrySkill> {
        // Fetch the raw content
        if let Some(ref download_url) = item.download_url {
            let response = self
                .client
                .get(download_url)
                .send()
                .await
                .with_context(|| format!("Failed to download skill file: {}", item.name))?;

            let content = response
                .text()
                .await
                .with_context(|| "Failed to read skill file content")?;

            self.parse_skill(&item.name, &content)
        } else {
            // Fall back: decode from base64
            self.decode_skill(item)
        }
    }

    fn parse_skill(&self, filename: &str, content: &str) -> Result<RegistrySkill> {
        let id = filename
            .strip_suffix(".md")
            .unwrap_or(filename)
            .to_string();

        let (metadata, body) = parse_skill_frontmatter(content);

        let id_clone = id.clone();
        Ok(RegistrySkill {
            id,
            name: if metadata.name.is_empty() {
                id_clone.replace('_', " ")
            } else {
                metadata.name
            },
            description: metadata.description,
            version: metadata.version,
            author: metadata.author,
            content: body,
        })
    }

    fn decode_skill(&self, item: &GitHubContent) -> Result<RegistrySkill> {
        let content = item.decode_content()?;
        self.parse_skill(&item.name, &content)
    }
}

impl Default for SkillsHubClient {
    fn default() -> Self {
        Self::new()
    }
}

/// GitHub API content entry.
#[derive(Debug, Clone, Deserialize)]
struct GitHubContent {
    name: String,
    r#type: String,
    path: String,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    sha: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    encoding: Option<String>,
}

impl GitHubContent {
    /// Decode base64-encoded content.
    fn decode_content(&self) -> Result<String> {
        let encoded = self
            .content
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No content available"))?;

        // GitHub returns base64 with newlines — strip them
        let cleaned: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
        let decoded = base64_decode(&cleaned)
            .ok_or_else(|| anyhow::anyhow!("Failed to decode base64 content"))?;

        String::from_utf8(decoded)
            .with_context(|| "Skill content is not valid UTF-8")
    }
}

/// Parsed frontmatter metadata.
#[derive(Debug, Default)]
struct SkillFrontmatter {
    name: String,
    description: String,
    version: String,
    author: String,
}

/// Parse YAML frontmatter from skill content.
fn parse_skill_frontmatter(content: &str) -> (SkillFrontmatter, String) {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return (SkillFrontmatter::default(), content.to_string());
    }

    let rest = &content[3..];
    if let Some(end) = rest.find("---") {
        let yaml = &rest[..end];
        let body = rest[end + 3..].trim().to_string();

        let mut metadata = SkillFrontmatter::default();
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

        (metadata, body)
    } else {
        (SkillFrontmatter::default(), content.to_string())
    }
}

/// Minimal base64 decoder (supports standard and URL-safe base64).
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    use std::collections::HashMap;

    let table = base64_table();
    let mut output = Vec::new();
    let mut buf: Vec<u8> = Vec::with_capacity(4);

    for ch in input.bytes() {
        if ch == b'=' {
            break;
        }
        if let Some(val) = table.get(&ch) {
            buf.push(*val);
            if buf.len() == 4 {
                output.push((buf[0] << 2) | (buf[1] >> 4));
                output.push((buf[1] << 4) | (buf[2] >> 2));
                output.push((buf[2] << 6) | buf[3]);
                buf.clear();
            }
        }
    }

    // Handle remaining bytes
    if buf.len() == 2 {
        output.push((buf[0] << 2) | (buf[1] >> 4));
    } else if buf.len() == 3 {
        output.push((buf[0] << 2) | (buf[1] >> 4));
        output.push((buf[1] << 4) | (buf[2] >> 2));
    }

    Some(output)
}

fn base64_table() -> HashMap<u8, u8> {
    let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    chars.iter().enumerate().map(|(i, &b)| (b, i as u8)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_frontmatter() {
        let content = r#"---
name: GitHub Auth
description: Authenticate with GitHub API
version: 1.0.0
author: Hermes Team
---

Use this skill to authenticate with the GitHub API.
"#;
        let (metadata, body) = parse_skill_frontmatter(content);
        assert_eq!(metadata.name, "GitHub Auth");
        assert_eq!(metadata.description, "Authenticate with GitHub API");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.author, "Hermes Team");
        assert!(body.contains("authenticate"));
    }

    #[test]
    fn test_parse_skill_frontmatter_no_frontmatter() {
        let content = "Just plain content.";
        let (metadata, body) = parse_skill_frontmatter(content);
        assert!(metadata.name.is_empty());
        assert_eq!(body, "Just plain content.");
    }

    #[test]
    fn test_base64_decode() {
        let decoded = base64_decode("SGVsbG8gV29ybGQ=").unwrap();
        assert_eq!(decoded, b"Hello World");
    }

    #[test]
    fn test_base64_decode_empty() {
        let decoded = base64_decode("").unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_github_content_decode() {
        let item = GitHubContent {
            name: "test.md".to_string(),
            r#type: "file".to_string(),
            path: "skills/test.md".to_string(),
            download_url: None,
            sha: "abc123".to_string(),
            content: Some("dGVzdCBjb250ZW50".to_string()), // base64 of "test content"
            encoding: Some("base64".to_string()),
        };
        let content = item.decode_content().unwrap();
        assert_eq!(content, "test content");
    }

    #[test]
    fn test_skills_hub_client_new() {
        let client = SkillsHubClient::new();
        assert_eq!(client.owner, "anthropics");
        assert_eq!(client.repo, "hermes-agent");
    }

    #[test]
    fn test_skills_hub_client_with_repo() {
        let client = SkillsHubClient::with_repo("test-owner", "test-repo", "skills/");
        assert_eq!(client.owner, "test-owner");
        assert_eq!(client.repo, "test-repo");
        assert_eq!(client.skill_path, "skills/");
    }
}
