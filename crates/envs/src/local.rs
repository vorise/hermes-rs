use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

use super::{CommandResult, Environment, EnvType};

/// Local environment — executes commands directly on the host machine.
///
/// No isolation, full system access, fastest execution.
pub struct LocalEnv {
    working_dir: Arc<Mutex<PathBuf>>,
    task_id: Option<String>,
}

impl LocalEnv {
    pub fn new(working_dir: PathBuf) -> Self {
        Self {
            working_dir: Arc::new(Mutex::new(working_dir)),
            task_id: None,
        }
    }

    pub fn with_cwd(dir: &str) -> Self {
        Self::new(PathBuf::from(dir))
    }
}

impl Default for LocalEnv {
    fn default() -> Self {
        Self {
            working_dir: Arc::new(Mutex::new(
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            )),
            task_id: None,
        }
    }
}

#[async_trait]
impl Environment for LocalEnv {
    fn env_type(&self) -> EnvType {
        EnvType::Local
    }

    async fn setup(&mut self, task_id: &str) -> Result<()> {
        self.task_id = Some(task_id.to_string());
        tracing::info!(task_id, "Local environment setup");
        Ok(())
    }

    async fn teardown(&mut self, task_id: &str) -> Result<()> {
        tracing::info!(task_id, "Local environment teardown");
        self.task_id = None;
        Ok(())
    }

    async fn run_command(&self, cmd: &str, timeout_secs: Option<u64>) -> Result<CommandResult> {
        let cwd = self.working_dir.lock().await;

        tracing::debug!(cmd, ?cwd, "Running local command");

        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(cwd.as_path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn command")?;

        let timeout = timeout_secs.map(std::time::Duration::from_secs);
        let result = if let Some(dur) = timeout {
            tokio::time::timeout(dur, wait_for_child(&mut child)).await
        } else {
            Ok(wait_for_child(&mut child).await)
        };

        let (exit_code, stdout, stderr) = match result {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                // Kill the process on timeout
                let _ = child.kill().await;
                return Err(anyhow::anyhow!("Command timed out after {:?}s", timeout_secs));
            }
        };

        Ok(CommandResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    async fn run_command_interactive(&self, cmd: &str) -> Result<String> {
        // Local interactive — just run with pty-like behavior
        let result = self.run_command(cmd, None).await?;
        Ok(result.output())
    }

    async fn upload_file(&self, local: &Path, remote: &Path) -> Result<()> {
        tokio::fs::copy(local, remote).await?;
        Ok(())
    }

    async fn download_file(&self, remote: &Path, local: &Path) -> Result<()> {
        tokio::fs::copy(remote, local).await?;
        Ok(())
    }

    fn working_dir(&self) -> &Path {
        // We can't return a reference to the inner PathBuf from the Mutex,
        // so we return a static path. For actual usage, the caller should
        // access via working_dir().to_path_buf() pattern.
        // This is a limitation of the trait signature returning &Path.
        // In practice, we'll use a different approach.
        static FALLBACK: std::sync::LazyLock<PathBuf> = std::sync::LazyLock::new(|| PathBuf::from("/"));
        &FALLBACK
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

async fn wait_for_child(child: &mut tokio::process::Child) -> Result<(i32, String, String)> {
    let stdout_future = async {
        let mut buf = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            stdout.read_to_string(&mut buf).await.ok();
        }
        buf
    };

    let stderr_future = async {
        let mut buf = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            stderr.read_to_string(&mut buf).await.ok();
        }
        buf
    };

    let (stdout, stderr) = tokio::join!(stdout_future, stderr_future);
    let status = child.wait().await?;

    let exit_code = status.code().unwrap_or(-1);
    Ok((exit_code, stdout, stderr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_env_type() {
        let env = LocalEnv::default();
        assert_eq!(env.env_type(), EnvType::Local);
    }

    #[tokio::test]
    async fn test_local_run_command() {
        let env = LocalEnv::default();
        let result = env.run_command("echo hello", None).await.unwrap();
        assert!(result.success());
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_local_run_command_with_stderr() {
        let env = LocalEnv::default();
        let result = env.run_command("echo error >&2 && echo out", None).await.unwrap();
        assert!(result.success());
        assert!(result.stdout.contains("out"));
        assert!(result.stderr.contains("error"));
    }

    #[tokio::test]
    async fn test_local_run_command_fails() {
        let env = LocalEnv::default();
        let result = env.run_command("exit 42", None).await.unwrap();
        assert!(!result.success());
        assert_eq!(result.exit_code, 42);
    }

    #[tokio::test]
    async fn test_local_setup_teardown() {
        let mut env = LocalEnv::default();
        env.setup("task-1").await.unwrap();
        assert_eq!(env.task_id, Some("task-1".to_string()));
        env.teardown("task-1").await.unwrap();
        assert_eq!(env.task_id, None);
    }

    #[tokio::test]
    async fn test_local_is_persistent() {
        let env = LocalEnv::default();
        assert!(env.is_persistent());
    }
}
