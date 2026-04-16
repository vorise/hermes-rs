//! Docker Environment
//!
//! Docker container execution backend.

use std::path::{Path, PathBuf};
use std::time::Duration;
use async_trait::async_trait;
use anyhow::{Result, Context, bail};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, warn, info};

use crate::{Environment, EnvError};

/// Docker environment configuration.
#[derive(Debug, Clone)]
pub struct DockerConfig {
    /// Container image to use.
    pub image: String,

    /// Container name prefix (task_id will be appended).
    pub name_prefix: String,

    /// Volume mounts (local:container).
    pub volumes: Vec<(PathBuf, PathBuf)>,

    /// Environment variables.
    pub env: Vec<(String, String)>,

    /// Working directory in container.
    pub working_dir: PathBuf,

    /// Keep container after task completion.
    pub keep_container: bool,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            image: "ubuntu:22.04".to_string(),
            name_prefix: "hermes-".to_string(),
            volumes: Vec::new(),
            env: Vec::new(),
            working_dir: PathBuf::from("/workspace"),
            keep_container: false,
        }
    }
}

/// Docker environment for container-based execution.
#[derive(Debug)]
pub struct DockerEnv {
    /// Configuration.
    config: DockerConfig,

    /// Active container ID (if running).
    container_id: Option<String>,

    /// Container name for current task.
    container_name: Option<String>,
}

impl Default for DockerEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerEnv {
    /// Create new Docker environment with default config.
    pub fn new() -> Self {
        Self {
            config: DockerConfig::default(),
            container_id: None,
            container_name: None,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: DockerConfig) -> Self {
        Self {
            config,
            container_id: None,
            container_name: None,
        }
    }

    /// Check if Docker is available.
    pub async fn is_docker_available() -> bool {
        let result = Command::new("docker")
            .arg("version")
            .output()
            .await;
        result.is_ok() && result.unwrap().status.success()
    }

    /// Build container name from task ID.
    fn container_name_for_task(task_id: &str) -> String {
        format!("hermes-{}", task_id.chars().take(20).collect::<String>())
    }

    /// Start container for a task.
    async fn start_container(&mut self, task_id: &str) -> Result<()> {
        let name = Self::container_name_for_task(task_id);
        self.container_name = Some(name.clone());

        debug!("Starting Docker container: {} with image {}", name, self.config.image);

        // Build docker run command
        let mut cmd = Command::new("docker");
        cmd.arg("run")
            .arg("-d")  // detached
            .arg("--name")
            .arg(&name);

        // Add volume mounts
        for (local, container) in &self.config.volumes {
            cmd.arg("-v").arg(format!("{}:{}", local.display(), container.display()));
        }

        // Add working directory
        cmd.arg("-w").arg(self.config.working_dir.display().to_string());

        // Add environment variables
        for (key, value) in &self.config.env {
            cmd.arg("-e").arg(format!("{}={}", key, value));
        }

        // Add image
        cmd.arg(&self.config.image);

        // Keep container running with sleep
        cmd.arg("sleep").arg("infinity");

        let output = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn docker run")?;

        let result = timeout(Duration::from_secs(60), async {
            let output = output.wait_with_output().await?;
            Ok::<_, anyhow::Error>(output)
        })
        .await
        .context("Docker run timed out")??;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            bail!("Failed to start container: {}", stderr);
        }

        // Extract container ID
        let id = String::from_utf8_lossy(&result.stdout).trim().to_string();
        self.container_id = Some(id.clone());

        info!("Container started: {} (ID: {})", name, id);
        Ok(())
    }

    /// Stop and remove container.
    async fn stop_container(&mut self) -> Result<()> {
        if let Some(name) = &self.container_name {
            debug!("Stopping container: {}", name);

            // Stop container
            let stop_result = Command::new("docker")
                .arg("stop")
                .arg(name)
                .output()
                .await;

            if let Ok(output) = stop_result {
                if !output.status.success() {
                    warn!("Failed to stop container: {}", String::from_utf8_lossy(&output.stderr));
                }
            }

            // Remove container (unless keep_container is set)
            if !self.config.keep_container {
                let rm_result = Command::new("docker")
                    .arg("rm")
                    .arg("-f")
                    .arg(name)
                    .output()
                    .await;

                if let Ok(output) = rm_result {
                    if !output.status.success() {
                        warn!("Failed to remove container: {}", String::from_utf8_lossy(&output.stderr));
                    } else {
                        info!("Container removed: {}", name);
                    }
                }
            }

            self.container_id = None;
            self.container_name = None;
        }

        Ok(())
    }

    /// Execute command in container.
    async fn exec_in_container(&self, cmd: &str, _timeout_duration: Duration) -> Result<String> {
        let container = self.container_name.as_ref()
            .context("No container running")?;

        let output = Command::new("docker")
            .arg("exec")
            .arg(container)
            .arg("sh")
            .arg("-c")
            .arg(cmd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .output()
            .await
            .context("Failed to spawn docker exec")?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        if output.status.success() {
            Ok(stdout.trim_end().to_string())
        } else {
            let code = output.status.code().unwrap_or(-1);
            Err(EnvError::CommandFailed { code, stderr }.into())
        }
    }
}

#[async_trait]
impl Environment for DockerEnv {
    async fn setup(&mut self, task_id: &str) -> Result<()> {
        self.start_container(task_id).await
    }

    async fn teardown(&mut self, _task_id: &str) -> Result<()> {
        self.stop_container().await
    }

    async fn run_command(&mut self, cmd: &str, timeout_duration: Duration) -> Result<String> {
        self.exec_in_container(cmd, timeout_duration).await
    }

    async fn upload_file(&mut self, local: &Path, remote: &Path) -> Result<()> {
        let container = self.container_name.as_ref()
            .context("No container running")?;

        if !local.exists() {
            return Err(EnvError::FileNotFound(local.display().to_string()).into());
        }

        debug!("Copying file to container: {} -> {}:{}", local.display(), container, remote.display());

        let output = Command::new("docker")
            .arg("cp")
            .arg(local)
            .arg(format!("{}:{}", container, remote.display()))
            .output()
            .await
            .context("Failed to run docker cp")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to copy file to container: {}", stderr);
        }

        Ok(())
    }

    async fn download_file(&mut self, remote: &Path, local: &Path) -> Result<()> {
        let container = self.container_name.as_ref()
            .context("No container running")?;

        debug!("Copying file from container: {}:{} -> {}", container, remote.display(), local.display());

        // Create parent directories for local path
        if let Some(parent) = local.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let output = Command::new("docker")
            .arg("cp")
            .arg(format!("{}:{}", container, remote.display()))
            .arg(local)
            .output()
            .await
            .context("Failed to run docker cp")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to copy file from container: {}", stderr);
        }

        Ok(())
    }

    fn working_dir(&self) -> &Path {
        &self.config.working_dir
    }

    fn is_persistent(&self) -> bool {
        false
    }

    fn backend_name(&self) -> &str {
        "docker"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_env_new() {
        let env = DockerEnv::new();
        assert_eq!(env.backend_name(), "docker");
        assert!(!env.is_persistent());
        assert_eq!(env.config.image, "ubuntu:22.04");
    }

    #[test]
    fn test_docker_config_default() {
        let config = DockerConfig::default();
        assert_eq!(config.image, "ubuntu:22.04");
        assert_eq!(config.name_prefix, "hermes-");
        assert!(!config.keep_container);
    }

    #[test]
    fn test_container_name_for_task() {
        let name = DockerEnv::container_name_for_task("test-task-123");
        assert!(name.starts_with("hermes-"));
        assert!(name.contains("test-task-123"));
    }
}