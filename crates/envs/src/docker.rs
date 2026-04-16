use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

use super::{CommandResult, Environment, EnvType};

/// Docker environment — executes commands inside Docker containers.
pub struct DockerEnv {
    image: String,
    container_name: Arc<Mutex<Option<String>>>,
    working_dir: PathBuf,
    volumes: Vec<(String, String)>,
}

impl DockerEnv {
    pub fn new(image: &str) -> Self {
        Self {
            image: image.to_string(),
            container_name: Arc::new(Mutex::new(None)),
            working_dir: PathBuf::from("/workspace"),
            volumes: Vec::new(),
        }
    }

    pub fn with_volume(mut self, host: &str, container: &str) -> Self {
        self.volumes.push((host.to_string(), container.to_string()));
        self
    }

    pub fn with_working_dir(mut self, dir: &str) -> Self {
        self.working_dir = PathBuf::from(dir);
        self
    }

    async fn docker_exec(&self, args: &[&str]) -> Result<String> {
        let mut child = tokio::process::Command::new("docker")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn docker command. Is docker installed?")?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        if let Some(ref mut s) = child.stdout {
            s.read_to_string(&mut stdout).await.ok();
        }
        if let Some(ref mut s) = child.stderr {
            s.read_to_string(&mut stderr).await.ok();
        }
        let status = child.wait().await?;
        if !status.success() {
            return Err(anyhow!("docker command failed: {stderr}"));
        }
        Ok(stdout.trim().to_string())
    }
}

#[async_trait]
impl Environment for DockerEnv {
    fn env_type(&self) -> EnvType {
        EnvType::Docker
    }

    async fn setup(&mut self, task_id: &str) -> Result<()> {
        let container = format!("hermes-{}", task_id);
        let mut name_guard = self.container_name.lock().await;

        tracing::info!(image = %self.image, container, "Docker environment setup");

        let volume_args: Vec<String> = self
            .volumes
            .iter()
            .map(|(h, c)| format!("-v:{h}:{c}"))
            .collect();

        let mut create_args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            container.clone(),
            "-w".to_string(),
            self.working_dir.to_string_lossy().to_string(),
        ];
        create_args.extend(volume_args);
        create_args.extend([
            "--rm".to_string(),
            self.image.clone(),
            "sleep".to_string(),
            "infinity".to_string(),
        ]);

        let create_args_refs: Vec<&str> = create_args.iter().map(|s| s.as_str()).collect();

        match self.docker_exec(&create_args_refs).await {
            Ok(id) => {
                *name_guard = Some(container);
                tracing::info!(container_id = %id, "Docker container started");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Docker container creation failed, will exec inline");
                *name_guard = None;
            }
        }

        Ok(())
    }

    async fn teardown(&mut self, _task_id: &str) -> Result<()> {
        let name_guard = self.container_name.lock().await;
        if let Some(ref container) = *name_guard {
            tracing::info!(container, "Docker environment teardown");
            let _ = self.docker_exec(&["stop", container]).await;
        }
        Ok(())
    }

    async fn run_command(&self, cmd: &str, timeout_secs: Option<u64>) -> Result<CommandResult> {
        let container_name = self.container_name.lock().await.clone();

        let exec_args: Vec<String> = if let Some(ref container) = container_name {
            vec![
                "exec".to_string(),
                container.clone(),
                "sh".to_string(),
                "-c".to_string(),
                cmd.to_string(),
            ]
        } else {
            vec![
                "run".to_string(),
                "--rm".to_string(),
                "-w".to_string(),
                self.working_dir.to_string_lossy().to_string(),
                self.image.clone(),
                "sh".to_string(),
                "-c".to_string(),
                cmd.to_string(),
            ]
        };

        let timeout = timeout_secs.map(std::time::Duration::from_secs);

        let mut child = tokio::process::Command::new("docker")
            .args(&exec_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn docker command")?;

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
        let container_name = self.container_name.lock().await.clone();
        if let Some(container) = container_name {
            let remote_str = format!("{container}:{}", remote.display());
            self.docker_exec(&["cp", local.to_str().unwrap_or(""), &remote_str])
                .await?;
        } else {
            return Err(anyhow!("No running container to upload to"));
        }
        Ok(())
    }

    async fn download_file(&self, remote: &Path, local: &Path) -> Result<()> {
        let container_name = self.container_name.lock().await.clone();
        if let Some(container) = container_name {
            let remote_str = format!("{container}:{}", remote.display());
            self.docker_exec(&["cp", &remote_str, local.to_str().unwrap_or("")])
                .await?;
        } else {
            return Err(anyhow!("No running container to download from"));
        }
        Ok(())
    }

    fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    fn is_persistent(&self) -> bool {
        false
    }
}

async fn collect_child_output(child: &mut tokio::process::Child) -> Result<(i32, String, String)> {
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

    #[test]
    fn test_docker_env_type() {
        let env = DockerEnv::new("python:3.12-slim");
        assert_eq!(env.env_type(), EnvType::Docker);
    }

    #[test]
    fn test_docker_builder() {
        let env = DockerEnv::new("ubuntu:latest")
            .with_volume("/tmp", "/workspace")
            .with_working_dir("/app");
        assert_eq!(env.volumes.len(), 1);
    }

    #[test]
    fn test_docker_not_persistent() {
        let env = DockerEnv::new("python:3.12-slim");
        assert!(!env.is_persistent());
    }
}
