use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::sync::Mutex;

use super::{CommandResult, DaytonaConfig, Environment, EnvType};

/// Daytona environment — executes commands on Daytona cloud infrastructure.
///
/// Serverless execution with serverless persistence and automatic
/// resource provisioning.
///
/// Note: This implementation uses the Daytona CLI (`daytona`) if available.
pub struct DaytonaEnv {
    config: DaytonaConfig,
    working_dir: Mutex<PathBuf>,
    task_id: Option<String>,
    workspace_id: Mutex<Option<String>>,
}

impl DaytonaEnv {
    pub fn new(config: DaytonaConfig) -> Self {
        Self {
            config,
            working_dir: Mutex::new(PathBuf::from("/workspace")),
            task_id: None,
            workspace_id: Mutex::new(None),
        }
    }

    pub fn with_working_dir(mut self, dir: &str) -> Self {
        self.working_dir = Mutex::new(PathBuf::from(dir));
        self
    }
}

#[async_trait]
impl Environment for DaytonaEnv {
    fn env_type(&self) -> EnvType {
        EnvType::Daytona
    }

    async fn setup(&mut self, task_id: &str) -> Result<()> {
        self.task_id = Some(task_id.to_string());
        tracing::info!(task_id, "Daytona environment setup");

        let ws_id = self.config.workspace_id.clone().unwrap_or_else(|| {
            format!("hermes-{}", task_id)
        });
        *self.workspace_id.lock().await = Some(ws_id);

        Ok(())
    }

    async fn teardown(&mut self, _task_id: &str) -> Result<()> {
        tracing::info!(task_id = ?self.task_id, "Daytona environment teardown");
        *self.workspace_id.lock().await = None;
        self.task_id = None;
        Ok(())
    }

    async fn run_command(&self, cmd: &str, _timeout_secs: Option<u64>) -> Result<CommandResult> {
        // Try Daytona CLI
        let workspace = self.workspace_id.lock().await.clone();

        let mut args = vec!["ssh".to_string()];
        if let Some(ref ws_id) = workspace {
            args.push(ws_id.clone());
        }
        args.push("--command".to_string());
        args.push(cmd.to_string());

        let output = tokio::process::Command::new("daytona")
            .args(&args)
            .output()
            .await;

        match output {
            Ok(out) => {
                let exit_code = out.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();

                Ok(CommandResult {
                    stdout,
                    stderr,
                    exit_code,
                })
            }
            Err(e) => {
                Err(anyhow!(
                    "Daytona CLI not available. Install daytona CLI. Error: {e}"
                ))
            }
        }
    }

    async fn run_command_interactive(&self, cmd: &str) -> Result<String> {
        let result = self.run_command(cmd, None).await?;
        Ok(result.output())
    }

    async fn upload_file(&self, local: &Path, remote: &Path) -> Result<()> {
        let workspace = self.workspace_id.lock().await.clone();
        let ws_id = workspace.as_deref().ok_or_else(|| anyhow!("No workspace configured"))?;

        let remote_str = remote.to_str().ok_or_else(|| anyhow!("Invalid remote path"))?;
        let local_str = local.to_str().ok_or_else(|| anyhow!("Invalid local path"))?;

        tokio::process::Command::new("daytona")
            .args(["scp", ws_id, local_str, &format!("/workspace/{remote_str}")])
            .output()
            .await?;
        Ok(())
    }

    async fn download_file(&self, remote: &Path, local: &Path) -> Result<()> {
        let workspace = self.workspace_id.lock().await.clone();
        let ws_id = workspace.as_deref().ok_or_else(|| anyhow!("No workspace configured"))?;

        let remote_str = remote.to_str().ok_or_else(|| anyhow!("Invalid remote path"))?;
        let local_str = local.to_str().ok_or_else(|| anyhow!("Invalid local path"))?;

        tokio::process::Command::new("daytona")
            .args(["scp", ws_id, &format!("/workspace/{remote_str}"), local_str])
            .output()
            .await?;
        Ok(())
    }

    fn working_dir(&self) -> &Path {
        static FALLBACK: std::sync::LazyLock<PathBuf> =
            std::sync::LazyLock::new(|| PathBuf::from("/workspace"));
        &FALLBACK
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daytona_env_type() {
        let env = DaytonaEnv::new(DaytonaConfig::default());
        assert_eq!(env.env_type(), EnvType::Daytona);
    }

    #[test]
    fn test_daytona_is_persistent() {
        let env = DaytonaEnv::new(DaytonaConfig::default());
        assert!(env.is_persistent());
    }

    #[tokio::test]
    async fn test_daytona_with_working_dir() {
        let env = DaytonaEnv::new(DaytonaConfig::default()).with_working_dir("/app");
        let wd = env.working_dir.lock().await;
        assert_eq!(*wd, PathBuf::from("/app"));
    }
}
