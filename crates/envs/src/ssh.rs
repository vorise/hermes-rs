//! SSH Environment
//!
//! SSH-based remote execution backend.

use std::path::{Path, PathBuf};
use std::time::Duration;
use async_trait::async_trait;
use anyhow::{Result, Context, bail};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, info};

use crate::{Environment, EnvError};

/// SSH configuration.
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// Host to connect to.
    pub host: String,

    /// SSH port (default 22).
    pub port: u16,

    /// Username for authentication.
    pub user: String,

    /// SSH key path (optional, uses default if not set).
    pub key_path: Option<PathBuf>,

    /// Working directory on remote host.
    pub working_dir: PathBuf,

    /// Connection timeout.
    pub connect_timeout: Duration,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 22,
            user: "root".to_string(),
            key_path: None,
            working_dir: PathBuf::from("/workspace"),
            connect_timeout: Duration::from_secs(30),
        }
    }
}

/// SSH environment for remote execution.
#[derive(Debug)]
pub struct SshEnv {
    /// Configuration.
    config: SshConfig,

    /// Connection state.
    connected: bool,
}

impl Default for SshEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl SshEnv {
    /// Create new SSH environment with default config.
    pub fn new() -> Self {
        Self {
            config: SshConfig::default(),
            connected: false,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: SshConfig) -> Self {
        Self {
            config,
            connected: false,
        }
    }

    /// Build SSH command arguments.
    fn ssh_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        // Port
        args.push("-p".to_string());
        args.push(self.config.port.to_string());

        // Connection timeout
        args.push("-o".to_string());
        args.push(format!("ConnectTimeout={}", self.config.connect_timeout.as_secs()));

        // Strict host key checking (off for development)
        args.push("-o".to_string());
        args.push("StrictHostKeyChecking=no".to_string());

        // SSH key if specified
        if let Some(key) = &self.config.key_path {
            args.push("-i".to_string());
            args.push(key.display().to_string());
        }

        // Batch mode (no password prompts)
        args.push("-o".to_string());
        args.push("BatchMode=yes".to_string());

        args
    }

    /// Build SSH destination string.
    fn destination(&self) -> String {
        format!("{}@{}", self.config.user, self.config.host)
    }

    /// Test SSH connection.
    async fn test_connection(&mut self) -> Result<()> {
        debug!("Testing SSH connection to {}", self.destination());

        let mut cmd = Command::new("ssh");
        cmd.args(self.ssh_args())
            .arg(self.destination())
            .arg("echo 'connection ok'");

        let output = timeout(self.config.connect_timeout, async {
            cmd.output().await
        })
        .await
        .context("SSH connection timed out")??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("SSH connection failed: {}", stderr);
        }

        self.connected = true;
        info!("SSH connection established to {}", self.destination());
        Ok(())
    }

    /// Execute command via SSH.
    async fn ssh_exec(&self, cmd: &str, timeout_duration: Duration) -> Result<String> {
        if !self.connected {
            bail!("SSH not connected");
        }

        let result = timeout(timeout_duration, async {
            let output = Command::new("ssh")
                .args(self.ssh_args())
                .arg(self.destination())
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
}

#[async_trait]
impl Environment for SshEnv {
    async fn setup(&mut self, task_id: &str) -> Result<()> {
        debug!("SSH setup for task: {}", task_id);
        self.test_connection().await
    }

    async fn teardown(&mut self, task_id: &str) -> Result<()> {
        debug!("SSH teardown for task: {}", task_id);
        self.connected = false;
        Ok(())
    }

    async fn run_command(&mut self, cmd: &str, timeout_duration: Duration) -> Result<String> {
        self.ssh_exec(cmd, timeout_duration).await
    }

    async fn upload_file(&mut self, local: &Path, remote: &Path) -> Result<()> {
        if !local.exists() {
            return Err(EnvError::FileNotFound(local.display().to_string()).into());
        }

        debug!("SCP upload: {} -> {}:{}", local.display(), self.destination(), remote.display());

        let mut cmd = Command::new("scp");
        cmd.args(self.ssh_args());

        // Source
        cmd.arg(local);

        // Destination
        cmd.arg(format!("{}:{}", self.destination(), remote.display()));

        let output = timeout(self.config.connect_timeout, async {
            cmd.output().await
        })
        .await
        .context("SCP upload timed out")??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("SCP upload failed: {}", stderr);
        }

        Ok(())
    }

    async fn download_file(&mut self, remote: &Path, local: &Path) -> Result<()> {
        debug!("SCP download: {}:{} -> {}", self.destination(), remote.display(), local.display());

        // Create parent directories
        if let Some(parent) = local.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut cmd = Command::new("scp");
        cmd.args(self.ssh_args());

        // Source
        cmd.arg(format!("{}:{}", self.destination(), remote.display()));

        // Destination
        cmd.arg(local);

        let output = timeout(self.config.connect_timeout, async {
            cmd.output().await
        })
        .await
        .context("SCP download timed out")??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("SCP download failed: {}", stderr);
        }

        Ok(())
    }

    fn working_dir(&self) -> &Path {
        &self.config.working_dir
    }

    fn is_persistent(&self) -> bool {
        true
    }

    fn backend_name(&self) -> &str {
        "ssh"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_env_new() {
        let env = SshEnv::new();
        assert_eq!(env.backend_name(), "ssh");
        assert!(env.is_persistent());
        assert!(!env.connected);
    }

    #[test]
    fn test_ssh_config_default() {
        let config = SshConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 22);
        assert_eq!(config.user, "root");
    }

    #[test]
    fn test_ssh_destination() {
        let env = SshEnv::with_config(SshConfig {
            host: "example.com".to_string(),
            user: "user".to_string(),
            ..Default::default()
        });
        assert_eq!(env.destination(), "user@example.com");
    }

    #[test]
    fn test_ssh_args() {
        let env = SshEnv::new();
        let args = env.ssh_args();
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"22".to_string()));
    }
}