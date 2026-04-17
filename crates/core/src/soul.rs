use std::path::{Path, PathBuf};

use anyhow::Result;

/// Default soul content if the user hasn't created one yet.
pub const DEFAULT_SOUL_TEMPLATE: &str =
    "# Your Soul\n\n\
    Define your agent's personality, communication style, and behavioral guidelines here.\n\n\
    ## Personality\n\n\
    Describe how the agent should behave.\n\n\
    ## Communication Style\n\n\
    - Be concise and direct\n\
    - Use markdown formatting\n\
    - Show code examples when helpful\n\n\
    ## Behavioral Guidelines\n\n\
    - Prioritize correctness over speed\n\
    - Ask clarifying questions when uncertain\n\
    ";

/// Represents the agent's personality configuration.
#[derive(Debug, Clone)]
pub struct Soul {
    /// Raw content of the soul file.
    pub content: String,
    /// Path to the soul file.
    pub path: PathBuf,
    /// Whether this is the default template (not user-created).
    pub is_default: bool,
}

impl Soul {
    /// Get a summary of the soul (first non-empty line, truncated).
    pub fn summary(&self) -> String {
        let first_line = self
            .content
            .lines()
            .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .unwrap_or("");
        if first_line.len() > 80 {
            format!("{}...", &first_line[..80])
        } else {
            first_line.to_string()
        }
    }
}

/// Load the soul from the default or custom path.
///
/// Searches in order:
/// 1. `custom_path` if provided
/// 2. `~/.hermes/SOUL.md`
///
/// Returns `None` if no soul file exists.
pub fn load_soul(custom_path: Option<&Path>) -> Result<Option<Soul>> {
    let path = custom_path
        .map(|p| p.to_path_buf())
        .or_else(|| hermes_home().map(|h| h.join("SOUL.md")));

    let Some(path) = path else {
        return Ok(None);
    };

    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)?;
    Ok(Some(Soul {
        content,
        path,
        is_default: false,
    }))
}

/// Load the soul, or return the default template if none exists.
pub fn load_soul_or_default(custom_path: Option<&Path>) -> Result<Soul> {
    if let Some(soul) = load_soul(custom_path)? {
        return Ok(soul);
    }

    // Return default template (not saved to disk)
    Ok(Soul {
        content: DEFAULT_SOUL_TEMPLATE.to_string(),
        path: hermes_home().map(|h| h.join("SOUL.md")).unwrap_or_default(),
        is_default: true,
    })
}

/// Create the default soul file at `~/.hermes/SOUL.md` if it doesn't exist.
///
/// Returns the path to the soul file.
pub fn create_default_soul() -> Result<PathBuf> {
    let home = hermes_home().ok_or_else(|| anyhow::anyhow!("HERMES_HOME not configured"))?;
    let soul_path = home.join("SOUL.md");

    if !soul_path.exists() {
        std::fs::write(&soul_path, DEFAULT_SOUL_TEMPLATE)?;
    }

    Ok(soul_path)
}

/// Update the soul file with new content.
pub fn update_soul(content: &str, custom_path: Option<&Path>) -> Result<PathBuf> {
    let path = custom_path
        .map(|p| p.to_path_buf())
        .or_else(|| hermes_home().map(|h| h.join("SOUL.md")))
        .ok_or_else(|| anyhow::anyhow!("HERMES_HOME not configured"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&path, content)?;
    Ok(path)
}

/// Delete the soul file.
pub fn delete_soul(custom_path: Option<&Path>) -> Result<()> {
    let path = custom_path
        .map(|p| p.to_path_buf())
        .or_else(|| hermes_home().map(|h| h.join("SOUL.md")));

    let Some(path) = path else {
        return Ok(());
    };

    if path.exists() {
        std::fs::remove_file(&path)?;
    }

    Ok(())
}

/// Extract personality traits from soul content for display.
pub fn extract_personality_traits(content: &str) -> Vec<String> {
    let mut traits = Vec::new();

    // Look for bullet points under any section
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let trait_text = trimmed[2..].trim();
            if !trait_text.is_empty() && trait_text.len() < 200 {
                traits.push(trait_text.to_string());
            }
        }
    }

    traits
}

/// Validate soul content (basic sanity checks).
pub fn validate_soul(content: &str) -> Result<()> {
    if content.is_empty() {
        return Err(anyhow::anyhow!("Soul content cannot be empty"));
    }
    if content.len() > 10_000 {
        return Err(anyhow::anyhow!(
            "Soul content too large ({} bytes, max 10000)",
            content.len()
        ));
    }
    Ok(())
}

/// Get the Hermes home directory (~/.hermes or HERMES_HOME env).
fn hermes_home() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("HERMES_HOME") {
        return Some(PathBuf::from(path));
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(PathBuf::from(home).join(".hermes"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_soul_template() {
        assert!(!DEFAULT_SOUL_TEMPLATE.is_empty());
        assert!(DEFAULT_SOUL_TEMPLATE.contains("Personality"));
    }

    #[test]
    fn test_load_soul_missing() {
        let soul = load_soul(Some(Path::new("/nonexistent/SOUL.md"))).unwrap();
        assert!(soul.is_none());
    }

    #[test]
    fn test_load_soul_or_default() {
        let soul = load_soul_or_default(Some(Path::new("/nonexistent/SOUL.md"))).unwrap();
        assert!(soul.is_default);
        assert!(soul.content.contains("Soul"));
    }

    #[test]
    fn test_extract_personality_traits() {
        let content = r#"# My Soul

## Personality
- Be helpful and concise
- Always verify code compiles

## Communication
- Use markdown
- Show examples
"#;
        let traits = extract_personality_traits(content);
        assert_eq!(traits.len(), 4);
        assert_eq!(traits[0], "Be helpful and concise");
        assert_eq!(traits[1], "Always verify code compiles");
    }

    #[test]
    fn test_validate_soul_empty() {
        assert!(validate_soul("").is_err());
    }

    #[test]
    fn test_validate_soul_valid() {
        assert!(validate_soul("Be helpful").is_ok());
    }

    #[test]
    fn test_validate_soul_too_large() {
        let large = "x".repeat(11_000);
        assert!(validate_soul(&large).is_err());
    }

    #[test]
    fn test_soul_summary() {
        let soul = Soul {
            content: "# Title\n\nThis is my agent personality.\n\n- Be nice\n".to_string(),
            path: PathBuf::from("/tmp/SOUL.md"),
            is_default: false,
        };
        let summary = soul.summary();
        assert_eq!(summary, "This is my agent personality.");
    }

    #[test]
    fn test_soul_summary_long_first_line() {
        let content = "# Title\n\n".to_string() + &"a".repeat(100);
        let soul = Soul {
            content,
            path: PathBuf::from("/tmp/SOUL.md"),
            is_default: false,
        };
        let summary = soul.summary();
        assert!(summary.ends_with("..."));
        assert!(summary.len() <= 83); // 80 + "..."
    }

    #[test]
    fn test_create_and_delete_soul() {
        let tmp = std::env::temp_dir().join(format!("hermes_soul_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();

        // Set HERMES_HOME for this test
        std::env::set_var("HERMES_HOME", tmp.to_str().unwrap());

        let path = create_default_soul().unwrap();
        assert!(path.exists());

        let soul = load_soul(None).unwrap().unwrap();
        assert!(!soul.is_default);

        delete_soul(None).unwrap();
        assert!(!path.exists());

        std::env::remove_var("HERMES_HOME");
        std::fs::remove_dir_all(tmp).ok();
    }
}
