use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use tokio::io::AsyncReadExt;

use super::{CommandResult, Environment, EnvType};

/// Singularity/Apptainer environment — executes commands inside Singularity containers.
///
/// HPC-grade container isolation, no root required, GPU support,
/// shared filesystem access.
pub struct SingularityEnv {
    image_path: PathBuf,
    working_dir: PathBuf,
    gpu_enabled: bool,
    task_id: Option<String>,
}

impl SingularityEnv {
    pub fn new(image_path: &Path, gpu_enabled: bool) -> Self {
        Self {
            image_path: image_path.to_path_buf(),
            working_dir: PathBuf::from("/workspace"),
            gpu_enabled,
            task_id: None,
        }
    }

    pub fn with_working_dir(mut self, dir: &str) -> Self {
        self.working_dir = PathBuf::from(dir);
        self
    }
}

impl Default for SingularityEnv {
    fn default() -> Self {
        Self {
            image_path: PathBuf::from("/opt/hermes.sif"),
            working_dir: PathBuf::from("/workspace"),
            gpu_enabled: false,
            task_id: None,
        }
    }
}

#[async_trait]
impl Environment for SingularityEnv {
    fn env_type(&self) -> EnvType {
        EnvType::Singularity
    }

    async fn setup(&mut self, task_id: &str) -> Result<()> {
        self.task_id = Some(task_id.to_string());
        tracing::info!(
            image = ?self.image_path,
            task_id,
            "Singularity environment setup"
        );
        Ok(())
    }

    async fn teardown(&mut self, _task_id: &str) -> Result<()> {
        tracing::info!(task_id = ?self.task_id, "Singularity environment teardown");
        self.task_id = None;
        Ok(())
    }

    async fn run_command(&self, cmd: &str, timeout_secs: Option<u64>) -> Result<CommandResult> {
        // Build singularity exec arguments
        let image = self
            .image_path
            .to_str()
            .ok_or_else(|| anyhow!("Invalid image path"))?;

        let wd = self.working_dir.to_string_lossy().to_string();
        let mut args = vec!["exec", "--bind", "/:/host"];
        if self.gpu_enabled {
            args.push("--nv");
        }
        args.extend(["--pwd", &wd, image, "sh", "-c", cmd]);

        let timeout = timeout_secs.map(std::time::Duration::from_secs);

        let mut child = tokio::process::Command::new("singularity")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn singularity command. Is singularity installed?")?;

        let result = if let Some(dur) = timeout {
            tokio::time::timeout(dur, collect_child_output(&mut child)).await
        } else {
            Ok(collect_child_output(&mut child).await)
        };

        let (exit_code, stdout, stderr) = match result {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                let _ = child.kill().await;
                return Err(anyhow!("Command timed out after {:?}s", timeout_secs));
            }
        };

        Ok(CommandResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    async fn run_command_interactive(&self, cmd: &str) -> Result<String> {
        let result = self.run_command(cmd, None).await?;
        Ok(result.output())
    }

    async fn upload_file(&self, local: &Path, remote: &Path) -> Result<()> {
        // Singularity shares the host filesystem, so upload is just a copy
        let remote_host_path = PathBuf::from("/host").join(remote.strip_prefix("/").unwrap_or(remote));
        tokio::fs::copy(local, &remote_host_path).await?;
        Ok(())
    }

    async fn download_file(&self, remote: &Path, local: &Path) -> Result<()> {
        // Singularity shares the host filesystem
        let remote_host_path = PathBuf::from("/host").join(remote.strip_prefix("/").unwrap_or(remote));
        tokio::fs::copy(&remote_host_path, local).await?;
        Ok(())
    }

    fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

async fn collect_child_output(child: &mut tokio::process::Child) -> Result<(i32, String, String)> {
    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    if let Some(ref mut s) = child.stdout {
        s.read_to_string(&mut stdout_buf).await.ok();
    }
    if let Some(ref mut s) = child.stderr {
        s.read_to_string(&mut stderr_buf).await.ok();
    }
    let status = child.wait().await?;
    let exit_code = status.code().unwrap_or(-1);
    Ok((exit_code, stdout_buf, stderr_buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singularity_env_type() {
        let env = SingularityEnv::default();
        assert_eq!(env.env_type(), EnvType::Singularity);
    }

    #[test]
    fn test_singularity_builder() {
        let env = SingularityEnv::new(Path::new("/tmp/test.sif"), true)
            .with_working_dir("/app");
        assert!(env.gpu_enabled);
        assert_eq!(env.working_dir, PathBuf::from("/app"));
    }

    #[test]
    fn test_singularity_is_persistent() {
        let env = SingularityEnv::default();
        assert!(env.is_persistent());
    }
}
