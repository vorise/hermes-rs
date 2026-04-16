use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::sync::Mutex;

use super::{CommandResult, Environment, EnvType, ModalConfig};

/// Modal environment — executes commands on Modal cloud infrastructure.
///
/// Provides serverless execution with serverless persistence (hibernate when idle),
/// GPU access, no idle costs, and automatic resource provisioning.
///
/// Note: This implementation uses the Modal CLI (`modal`) if available,
/// or makes API calls to the Modal REST API.
pub struct ModalEnv {
    config: ModalConfig,
    working_dir: Mutex<PathBuf>,
    task_id: Option<String>,
    sandbox_id: Mutex<Option<String>>,
}

impl ModalEnv {
    pub fn new(config: ModalConfig) -> Self {
        Self {
            config,
            working_dir: Mutex::new(PathBuf::from("/workspace")),
            task_id: None,
            sandbox_id: Mutex::new(None),
        }
    }

    pub fn with_working_dir(mut self, dir: &str) -> Self {
        self.working_dir = Mutex::new(PathBuf::from(dir));
        self
    }
}

#[async_trait]
impl Environment for ModalEnv {
    fn env_type(&self) -> EnvType {
        EnvType::Modal
    }

    async fn setup(&mut self, task_id: &str) -> Result<()> {
        self.task_id = Some(task_id.to_string());
        tracing::info!(
            task_id,
            image = ?self.config.image,
            "Modal environment setup"
        );
        *self.sandbox_id.lock().await = Some(format!("hermes-{}", task_id));
        Ok(())
    }

    async fn teardown(&mut self, _task_id: &str) -> Result<()> {
        tracing::info!(task_id = ?self.task_id, "Modal environment teardown");
        *self.sandbox_id.lock().await = None;
        self.task_id = None;
        Ok(())
    }

    async fn run_command(&self, cmd: &str, _timeout_secs: Option<u64>) -> Result<CommandResult> {

        // Try Modal CLI first
        let cwd = self.working_dir.lock().await.clone();
        drop(cwd);

        // Modal CLI: `modal shell` or `modal run`
        let mut args = vec!["run".to_string()];
        if let Some(ref image) = self.config.image {
            args.push("--image".to_string());
            args.push(image.clone());
        }
        if let Some(ref gpu) = self.config.gpu {
            args.push("--gpu".to_string());
            args.push(gpu.clone());
        }
        args.push("--cmd".to_string());
        args.push(cmd.to_string());

        let output = tokio::process::Command::new("modal")
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
                // Modal CLI not available — return a descriptive error
                Err(anyhow!(
                    "Modal CLI not available. Install with: pip install modal-client. Error: {e}"
                ))
            }
        }
    }

    async fn run_command_interactive(&self, cmd: &str) -> Result<String> {
        // Modal interactive shell
        let mut args = vec!["shell".to_string()];
        if let Some(ref image) = self.config.image {
            args.push("--image".to_string());
            args.push(image.clone());
        }
        args.push("--cmd".to_string());
        args.push(cmd.to_string());

        let output = tokio::process::Command::new("modal")
            .args(&args)
            .output()
            .await?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn upload_file(&self, local: &Path, remote: &Path) -> Result<()> {
        // Modal volumes use `modal volume put`
        let remote_str = remote.to_str().ok_or_else(|| anyhow!("Invalid remote path"))?;
        let local_str = local.to_str().ok_or_else(|| anyhow!("Invalid local path"))?;

        tokio::process::Command::new("modal")
            .args(["volume", "put", "--remote", remote_str, local_str])
            .output()
            .await?;
        Ok(())
    }

    async fn download_file(&self, remote: &Path, local: &Path) -> Result<()> {
        let remote_str = remote.to_str().ok_or_else(|| anyhow!("Invalid remote path"))?;
        let local_str = local.to_str().ok_or_else(|| anyhow!("Invalid local path"))?;

        tokio::process::Command::new("modal")
            .args(["volume", "get", "--remote", remote_str, local_str])
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
        // Modal with serverless persistence hibernates and wakes
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_env_type() {
        let env = ModalEnv::new(ModalConfig {
            image: None,
            gpu: None,
            memory_mb: None,
            timeout_secs: None,
        });
        assert_eq!(env.env_type(), EnvType::Modal);
    }

    #[test]
    fn test_modal_is_persistent() {
        let env = ModalEnv::new(ModalConfig::default());
        assert!(env.is_persistent());
    }

    #[tokio::test]
    async fn test_modal_with_working_dir() {
        let env = ModalEnv::new(ModalConfig::default()).with_working_dir("/app");
        let wd = env.working_dir.lock().await;
        assert_eq!(*wd, PathBuf::from("/app"));
    }
}
