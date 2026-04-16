use std::path::PathBuf;

/// Returns the Hermes home directory (~/.hermes).
///
/// Override with `HERMES_HOME` environment variable.
pub fn hermes_home() -> PathBuf {
    if let Ok(home) = std::env::var("HERMES_HOME") {
        return PathBuf::from(home);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".hermes");
    }
    // Fallback for non-Unix platforms
    if let Ok(home) = std::env::var("USERPROFILE") {
        return PathBuf::from(home).join(".hermes");
    }
    PathBuf::from(".hermes")
}

/// Display hermes home path as a string.
pub fn display_hermes_home() -> String {
    hermes_home().to_string_lossy().to_string()
}

/// Config file path: ~/.hermes/config.yaml
pub fn config_path() -> PathBuf {
    hermes_home().join("config.yaml")
}

/// Environment file path: ~/.hermes/.env
pub fn env_path() -> PathBuf {
    hermes_home().join(".env")
}

/// State database path: ~/.hermes/state.db
pub fn state_db_path() -> PathBuf {
    hermes_home().join("state.db")
}

/// Memory directory: ~/.hermes/memory/
pub fn memory_dir() -> PathBuf {
    hermes_home().join("memory")
}

/// Skills directory: ~/.hermes/skills/
pub fn skills_dir() -> PathBuf {
    hermes_home().join("skills")
}

/// Logs directory: ~/.hermes/logs/
pub fn logs_dir() -> PathBuf {
    hermes_home().join("logs")
}

/// SOUL.md path: ~/.hermes/SOUL.md
pub fn soul_path() -> PathBuf {
    hermes_home().join("SOUL.md")
}

/// Ensure all required Hermes home directories exist.
pub fn ensure_hermes_home() -> std::io::Result<()> {
    let home = hermes_home();
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(memory_dir())?;
    std::fs::create_dir_all(skills_dir())?;
    std::fs::create_dir_all(logs_dir())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hermes_home_default() {
        // Should not panic
        let _ = hermes_home();
    }

    #[test]
    fn test_hermes_home_override() {
        // SAFETY: test-only, single-threaded context
        unsafe { std::env::set_var("HERMES_HOME", "/tmp/test_hermes") };
        assert_eq!(hermes_home(), PathBuf::from("/tmp/test_hermes"));
        // SAFETY: test-only, cleanup
        unsafe { std::env::remove_var("HERMES_HOME") };
    }
}
