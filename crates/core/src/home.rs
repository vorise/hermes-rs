//! Home directory utilities for Hermes Agent.
//!
//! Provides the hermes_home() function that returns the path to ~/.hermes
//! with support for HERMES_HOME environment variable override.

use std::path::PathBuf;
use once_cell::sync::Lazy;

/// Get the Hermes home directory.
///
/// Default: ~/.hermes
/// Override via HERMES_HOME environment variable.
pub fn hermes_home() -> PathBuf {
    HERMES_HOME.clone()
}

/// Cached Hermes home directory.
static HERMES_HOME: Lazy<PathBuf> = Lazy::new(|| {
    std::env::var("HERMES_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".hermes")
        })
});

/// Get the sessions database path.
pub fn sessions_db_path() -> PathBuf {
    hermes_home().join("sessions.db")
}

/// Get the config file path.
pub fn config_path() -> PathBuf {
    hermes_home().join("config.yaml")
}

/// Get the .env file path.
pub fn env_file_path() -> PathBuf {
    hermes_home().join(".env")
}

/// Get the memory directory path.
pub fn memory_dir() -> PathBuf {
    hermes_home().join("memory")
}

/// Get the skills directory path.
pub fn skills_dir() -> PathBuf {
    hermes_home().join("skills")
}

/// Get the plugins directory path.
pub fn plugins_dir() -> PathBuf {
    hermes_home().join("plugins")
}

/// Get the logs directory path.
pub fn logs_dir() -> PathBuf {
    hermes_home().join("logs")
}

/// Ensure the Hermes home directory exists.
pub fn ensure_hermes_home() -> anyhow::Result<()> {
    let home = hermes_home();
    if !home.exists() {
        std::fs::create_dir_all(&home)?;
    }
    Ok(())
}

/// Ensure all required subdirectories exist.
pub fn ensure_all_dirs() -> anyhow::Result<()> {
    ensure_hermes_home()?;

    let dirs = [
        memory_dir(),
        skills_dir(),
        plugins_dir(),
        logs_dir(),
    ];

    for dir in dirs {
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hermes_home_default() {
        // Without HERMES_HOME override (using unsafe as remove_var is unsafe)
        unsafe { std::env::remove_var("HERMES_HOME") };
        let home = hermes_home();

        // Should be ~/.hermes
        assert!(home.to_string_lossy().contains(".hermes"));
    }

    #[test]
    fn test_sessions_db_path() {
        let db_path = sessions_db_path();
        assert!(db_path.to_string_lossy().ends_with("sessions.db"));
    }

    #[test]
    fn test_config_path() {
        let config = config_path();
        assert!(config.to_string_lossy().ends_with("config.yaml"));
    }
}