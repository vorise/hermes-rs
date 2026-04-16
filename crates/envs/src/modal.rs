//! Modal Environment
//!
//! Modal sandbox API backend (serverless execution).

use std::path::{Path, PathBuf};
use std::time::Duration;
use async_trait::async_trait;
use anyhow::{Result, bail};
use tracing::debug;

use crate::{Environment, EnvError};

/// Modal configuration.
#[derive(Debug, Clone)]
pub struct ModalConfig {
    /// App name for Modal deployment.
    pub app_name: String,

    /// Image ID or image spec.
    pub image: String,

    /// Working directory in sandbox.
    pub working_dir: PathBuf,

    /// Timeout for sandbox operations.
    pub timeout: Duration,

    /// GPU type (optional).
    pub gpu: Option<String>,
}

impl Default for ModalConfig {
    fn default() -> Self {
        Self {
            app_name: "hermes-sandbox".to_string(),
            image: "python:3.11-slim".to_string(),
            working_dir: PathBuf::from("/workspace"),
            timeout: Duration::from_secs(300),
            gpu: None,
        }
    }
}

/// Modal environment for serverless sandbox execution.
///
/// Note: This is a stub implementation. Full implementation requires
/// the Modal SDK integration via Python bridge or HTTP API.
#[derive(Debug)]
pub struct ModalEnv {
    /// Configuration.
    config: ModalConfig,

    /// Sandbox ID (if running).
    sandbox_id: Option<String>,
}

impl Default for ModalEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl ModalEnv {
    /// Create new Modal environment.
    pub fn new() -> Self {
        Self {
            config: ModalConfig::default(),
            sandbox_id: None,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: ModalConfig) -> Self {
        Self {
            config,
            sandbox_id: None,
        }
    }

    /// Check if Modal CLI is available.
    pub async fn is_modal_available() -> bool {
        // Check for modal CLI or API credentials
        std::env::var("MODAL_TOKEN_ID").is_ok() && std::env::var("MODAL_TOKEN_SECRET").is_ok()
    }
}

#[async_trait]
impl Environment for ModalEnv {
    async fn setup(&mut self, task_id: &str) -> Result<()> {
        debug!("Modal setup for task: {}", task_id);
        // TODO: Create sandbox via Modal API
        // self.sandbox_id = Some(create_sandbox(&self.config).await?);
        bail!("Modal environment not yet implemented - requires Modal SDK integration")
    }

    async fn teardown(&mut self, task_id: &str) -> Result<()> {
        debug!("Modal teardown for task: {}", task_id);
        // TODO: Destroy sandbox via Modal API
        self.sandbox_id = None;
        Ok(())
    }

    async fn run_command(&mut self, cmd: &str, _timeout_duration: Duration) -> Result<String> {
        debug!("Modal run_command: {}", cmd);
        // TODO: Execute via Modal API
        Err(EnvError::NotSupported("Modal command execution not implemented".to_string()).into())
    }

    async fn upload_file(&mut self, local: &Path, remote: &Path) -> Result<()> {
        debug!("Modal upload: {} -> {}", local.display(), remote.display());
        // TODO: Upload via Modal API
        Err(EnvError::NotSupported("Modal file upload not implemented".to_string()).into())
    }

    async fn download_file(&mut self, remote: &Path, local: &Path) -> Result<()> {
        debug!("Modal download: {} -> {}", remote.display(), local.display());
        // TODO: Download via Modal API
        Err(EnvError::NotSupported("Modal file download not implemented".to_string()).into())
    }

    fn working_dir(&self) -> &Path {
        &self.config.working_dir
    }

    fn is_persistent(&self) -> bool {
        false  // Modal sandboxes are ephemeral
    }

    fn backend_name(&self) -> &str {
        "modal"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_env_new() {
        let env = ModalEnv::new();
        assert_eq!(env.backend_name(), "modal");
        assert!(!env.is_persistent());
        assert!(env.sandbox_id.is_none());
    }

    #[test]
    fn test_modal_config_default() {
        let config = ModalConfig::default();
        assert_eq!(config.app_name, "hermes-sandbox");
        assert_eq!(config.image, "python:3.11-slim");
        assert!(config.gpu.is_none());
    }

    #[tokio::test]
    async fn test_modal_setup_fails() {
        let mut env = ModalEnv::new();
        let result = env.setup("test").await;
        assert!(result.is_err());
    }
}