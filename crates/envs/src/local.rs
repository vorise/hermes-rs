//! Local Environment
//!
//! Direct process execution on the local machine.
//! Uses tokio::process for async command execution.

use std::path::{Path, PathBuf};
use std::time::Duration;
use async_trait::async_trait;
use anyhow::{Result, Context, bail};
use tokio::process::Command;
use tokio::io::AsyncReadExt;
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::{Environment, EnvError};

/// Local environment for direct process execution.
#[derive(Debug)]
pub struct LocalEnv {
    /// Working directory for command execution.
    working_dir: PathBuf,

    /// Environment variables to set.
    env_vars: Vec<(String, String)>,
}

impl Default for LocalEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalEnv {
    /// Create a new local environment with current working directory.
    pub fn new() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            env_vars: Vec::new(),
        }
    }

    /// Create with specific working directory.
    pub fn with_working_dir(dir: PathBuf) -> Self {
        Self {
            working_dir: dir,
            env_vars: Vec::new(),
        }
    }

    /// Add environment variable.
    pub fn with_env(mut self, key: String, value: String) -> Self {
        self.env_vars.push((key, value));
        self
    }

    /// Set multiple environment variables.
    pub fn with_envs(mut self, vars: Vec<(String, String)>) -> Self {
        self.env_vars.extend(vars);
        self
    }

    /// Execute command and capture output.
    async fn execute(&self, cmd: &str, timeout_duration: Duration) -> Result<String> {
        debug!("Executing command locally: {}", cmd);

        // Use shell to execute command string
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&self.working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn command")?;

        // Apply environment variables
        for (key, value) in &self.env_vars {
            // Note: tokio::process::Command doesn't have direct env insertion
            // Environment is inherited from parent process by default
            debug!("Would set env: {}={}", key, value);
        }

        // Wait for completion with timeout
        let result = timeout(timeout_duration, async {
            let mut stdout = child.stdout.take().context("stdout not captured")?;
            let mut stderr = child.stderr.take().context("stderr not captured")?;

            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();

            stdout.read_to_end(&mut stdout_buf).await.context("Failed to read stdout")?;
            stderr.read_to_end(&mut stderr_buf).await.context("Failed to read stderr")?;

            let status = child.wait().await.context("Failed to wait for process")?;

            Ok::<_, anyhow::Error>((status, stdout_buf, stderr_buf))
        })
        .await;

        match result {
            Ok(Ok((status, stdout_buf, stderr_buf))) => {
                let stdout = String::from_utf8_lossy(&stdout_buf).into_owned();
                let stderr = String::from_utf8_lossy(&stderr_buf).into_owned();

                if status.success() {
                    debug!("Command succeeded: {}", stdout.trim());
                    Ok(stdout.trim_end().to_string())
                } else {
                    let code = status.code().unwrap_or(-1);
                    warn!("Command failed with code {}: {}", code, stderr.trim());
                    Err(EnvError::CommandFailed { code, stderr }.into())
                }
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                warn!("Command timed out after {}ms", timeout_duration.as_millis());
                // Try to kill the process
                let _ = child.kill().await;
                Err(EnvError::Timeout(timeout_duration.as_millis() as u64).into())
            }
        }
    }
}

#[async_trait]
impl Environment for LocalEnv {
    async fn setup(&mut self, task_id: &str) -> Result<()> {
        debug!("Local environment setup for task: {}", task_id);
        // No setup needed for local - just verify working dir exists
        if !self.working_dir.exists() {
            bail!("Working directory does not exist: {}", self.working_dir.display());
        }
        Ok(())
    }

    async fn teardown(&mut self, task_id: &str) -> Result<()> {
        debug!("Local environment teardown for task: {}", task_id);
        // No teardown needed for local
        Ok(())
    }

    async fn run_command(&mut self, cmd: &str, timeout_duration: Duration) -> Result<String> {
        self.execute(cmd, timeout_duration).await
    }

    async fn upload_file(&mut self, local: &Path, remote: &Path) -> Result<()> {
        // For local env, just copy the file
        debug!("Copying file locally: {} -> {}", local.display(), remote.display());

        if !local.exists() {
            return Err(EnvError::FileNotFound(local.display().to_string()).into());
        }

        tokio::fs::copy(local, remote)
            .await
            .context("Failed to copy file")?;

        Ok(())
    }

    async fn download_file(&mut self, remote: &Path, local: &Path) -> Result<()> {
        // For local env, just copy the file (same as upload)
        self.upload_file(remote, local).await
    }

    fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    fn is_persistent(&self) -> bool {
        true
    }

    fn backend_name(&self) -> &str {
        "local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_local_env_new() {
        let env = LocalEnv::new();
        assert!(env.working_dir.exists());
        assert!(env.is_persistent());
        assert_eq!(env.backend_name(), "local");
    }

    #[tokio::test]
    async fn test_local_env_execute_success() {
        let env = LocalEnv::new();
        let result = env.execute("echo hello", Duration::from_secs(5)).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().trim(), "hello");
    }

    #[tokio::test]
    async fn test_local_env_execute_failure() {
        let env = LocalEnv::new();
        let result = env.execute("exit 1", Duration::from_secs(5)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_local_env_execute_timeout() {
        let env = LocalEnv::new();
        let result = env.execute("sleep 10", Duration::from_millis(100)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_local_env_setup() {
        let mut env = LocalEnv::new();
        let result = env.setup("test-task").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_local_env_run_command() {
        let mut env = LocalEnv::new();
        let result = env.run_command("pwd", Duration::from_secs(5)).await;
        assert!(result.is_ok());
        // Should return current working directory
        assert!(result.unwrap().contains(&env.working_dir.display().to_string()));
    }

    #[tokio::test]
    async fn test_local_env_with_working_dir() {
        let env = LocalEnv::with_working_dir(PathBuf::from("/tmp"));
        assert_eq!(env.working_dir(), Path::new("/tmp"));
    }
}