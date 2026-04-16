//! Singularity Environment
//!
//! Singularity/Apptainer container execution backend.

use std::path::{Path, PathBuf};
use std::time::Duration;
use async_trait::async_trait;
use anyhow::{Result, Context, bail};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, info};

use crate::{Environment, EnvError};

/// Singularity configuration.
#[derive(Debug, Clone)]
pub struct SingularityConfig {
    /// Container image (SIF file or library reference).
    pub image: String,

    /// Working directory in container.
    pub working_dir: PathBuf,

    /// Bind mounts (local:container).
    pub binds: Vec<(PathBuf, PathBuf)>,

    /// Environment variables.
    pub env: Vec<(String, String)>,

    /// Use sandbox mode (writable).
    pub sandbox: bool,

    /// Keep container after execution.
    pub keep_container: bool,
}

impl Default for SingularityConfig {
    fn default() -> Self {
        Self {
            image: "library://ubuntu:22.04".to_string(),
            working_dir: PathBuf::from("/workspace"),
            binds: Vec::new(),
            env: Vec::new(),
            sandbox: false,
            keep_container: false,
        }
    }
}

/// Singularity environment for container execution.
///
/// Uses `singularity exec` or `apptainer exec` for command execution.
#[derive(Debug)]
pub struct SingularityEnv {
    /// Configuration.
    config: SingularityConfig,

    /// Whether using Singularity or Apptainer (newer name).
    use_apptainer: bool,
}

impl Default for SingularityEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl SingularityEnv {
    /// Create new Singularity environment.
    pub fn new() -> Self {
        Self {
            config: SingularityConfig::default(),
            use_apptainer: Self::detect_apptainer(),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: SingularityConfig) -> Self {
        Self {
            config,
            use_apptainer: Self::detect_apptainer(),
        }
    }

    /// Detect if Apptainer (new Singularity) is available.
    fn detect_apptainer() -> bool {
        // Check for apptainer first, then singularity
        std::process::Command::new("apptainer")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or_else(|_| {
                std::process::Command::new("singularity")
                    .arg("version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            })
    }

    /// Get executable name (apptainer or singularity).
    fn executable(&self) -> &str {
        if self.use_apptainer {
            "apptainer"
        } else {
            "singularity"
        }
    }

    /// Build exec arguments.
    fn exec_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        args.push("exec".to_string());

        // Bind mounts
        for (local, container) in &self.config.binds {
            args.push("-B".to_string());
            args.push(format!("{}:{}", local.display(), container.display()));
        }

        // Working directory
        args.push("-C".to_string());  // contain
        args.push("-w".to_string());  // workdir
        args.push("--pwd".to_string());
        args.push(self.config.working_dir.display().to_string());

        // Environment variables
        for (key, value) in &self.config.env {
            args.push("-e".to_string());  // clean env
            args.push(format!("--env={}={}", key, value));
        }

        // Sandbox mode
        if self.config.sandbox {
            args.push("-w".to_string());
        }

        // Image
        args.push(self.config.image.clone());

        args
    }

    /// Check if Singularity/Apptainer is available.
    pub async fn is_available() -> bool {
        let exec = if Self::detect_apptainer() {
            "apptainer"
        } else {
            "singularity"
        };

        let result = Command::new(exec)
            .arg("version")
            .output()
            .await;

        result.is_ok() && result.unwrap().status.success()
    }
}

#[async_trait]
impl Environment for SingularityEnv {
    async fn setup(&mut self, task_id: &str) -> Result<()> {
        debug!("Singularity setup for task: {}", task_id);

        // Verify executable exists
        let exec = self.executable();
        let result = Command::new(exec)
            .arg("version")
            .output()
            .await
            .context("Singularity/Apptainer not found")?;

        if !result.status.success() {
            bail!("{} not available", exec);
        }

        info!("Singularity environment ready (using {})", exec);
        Ok(())
    }

    async fn teardown(&mut self, task_id: &str) -> Result<()> {
        debug!("Singularity teardown for task: {}", task_id);
        // Singularity containers are ephemeral by default
        Ok(())
    }

    async fn run_command(&mut self, cmd: &str, timeout_duration: Duration) -> Result<String> {
        let result = timeout(timeout_duration, async {
            let output = Command::new(self.executable())
                .args(self.exec_args())
                .arg("sh")
                .arg("-c")
                .arg(cmd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .await?;

            Ok::<_, anyhow::Error>(output)
        })
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

                if output.status.success() {
                    Ok(stdout.trim_end().to_string())
                } else {
                    let code = output.status.code().unwrap_or(-1);
                    Err(EnvError::CommandFailed { code, stderr }.into())
                }
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                Err(EnvError::Timeout(timeout_duration.as_millis() as u64).into())
            }
        }
    }

    async fn upload_file(&mut self, local: &Path, remote: &Path) -> Result<()> {
        if !local.exists() {
            return Err(EnvError::FileNotFound(local.display().to_string()).into());
        }

        // Singularity uses bind mounts - file should be accessible via bind
        // For direct copy, we need to exec cp inside container
        debug!("Singularity upload: {} -> {}", local.display(), remote.display());

        // Find a bind mount that contains the remote path
        for (bind_local, bind_container) in &self.config.binds {
            if remote.starts_with(bind_container) {
                // Calculate local path
                let relative = remote.strip_prefix(bind_container)?;
                let target_local = bind_local.join(relative);

                tokio::fs::copy(local, &target_local)
                    .await
                    .context("Failed to copy file")?;

                return Ok(());
            }
        }

        // No bind mount found - try exec cp
        let cp_cmd = format!("cp {} {}", local.display(), remote.display());
        self.run_command(&cp_cmd, Duration::from_secs(60)).await?;
        Ok(())
    }

    async fn download_file(&mut self, remote: &Path, local: &Path) -> Result<()> {
        debug!("Singularity download: {} -> {}", remote.display(), local.display());

        // Find bind mount for the remote path
        for (bind_local, bind_container) in &self.config.binds {
            if remote.starts_with(bind_container) {
                let relative = remote.strip_prefix(bind_container)?;
                let source_local = bind_local.join(relative);

                if let Some(parent) = local.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }

                tokio::fs::copy(&source_local, local)
                    .await
                    .context("Failed to copy file")?;

                return Ok(());
            }
        }

        bail!("No bind mount for remote path: {}", remote.display());
    }

    fn working_dir(&self) -> &Path {
        &self.config.working_dir
    }

    fn is_persistent(&self) -> bool {
        false
    }

    fn backend_name(&self) -> &str {
        "singularity"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singularity_env_new() {
        let env = SingularityEnv::new();
        assert_eq!(env.backend_name(), "singularity");
        assert!(!env.is_persistent());
    }

    #[test]
    fn test_singularity_config_default() {
        let config = SingularityConfig::default();
        assert!(config.image.contains("ubuntu"));
        assert!(!config.sandbox);
    }

    #[test]
    fn test_executable() {
        let env = SingularityEnv::new();
        // On systems with apptainer, it will use that; otherwise singularity
        let exec = env.executable();
        assert!(exec == "apptainer" || exec == "singularity");
    }
}