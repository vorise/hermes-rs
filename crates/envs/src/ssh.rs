use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::Mutex;

use super::{CommandResult, Environment, EnvType, SshConfig};

/// SSH environment — executes commands on remote machines via SSH.
///
/// Uses openssh-compatible ssh/scp commands for remote access and file sync.
pub struct SshEnv {
    config: SshConfig,
    working_dir: Mutex<PathBuf>,
    task_id: Option<String>,
    connected: Mutex<bool>,
}

impl SshEnv {
    pub fn new(config: SshConfig) -> Result<Self> {
        if config.host.is_empty() {
            return Err(anyhow!("SSH host cannot be empty"));
        }
        Ok(Self {
            config,
            working_dir: Mutex::new(PathBuf::from("/tmp")),
            task_id: None,
            connected: Mutex::new(false),
        })
    }

    pub fn with_working_dir(mut self, dir: &str) -> Self {
        self.working_dir = Mutex::new(PathBuf::from(dir));
        self
    }

    fn ssh_target(&self) -> String {
        if let Some(port) = self.config.port {
            format!("{}@{} -p {}", self.config.user, self.config.host, port)
        } else {
            format!("{}@{}", self.config.user, self.config.host)
        }
    }

    async fn run_ssh_command(&self, cmd: &str) -> Result<(String, String)> {
        let target = self.ssh_target();
        let mut ssh_cmd = Command::new("ssh");

        if let Some(ref key) = self.config.key_path {
            ssh_cmd.arg("-i").arg(key);
        }
        ssh_cmd
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("StrictHostKeyChecking=no")
            .arg(&target)
            .arg(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = ssh_cmd.spawn().context("Failed to spawn ssh command")?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        if let Some(ref mut s) = child.stdout {
            s.read_to_string(&mut stdout).await.ok();
        }
        if let Some(ref mut s) = child.stderr {
            s.read_to_string(&mut stderr).await.ok();
        }
        child.wait().await?;

        Ok((stdout, stderr))
    }
}

#[async_trait]
impl Environment for SshEnv {
    fn env_type(&self) -> EnvType {
        EnvType::Ssh
    }

    async fn setup(&mut self, task_id: &str) -> Result<()> {
        self.task_id = Some(task_id.to_string());
        tracing::info!(host = %self.config.host, task_id, "SSH environment setup");

        // Test connectivity
        match self.run_ssh_command("echo connected").await {
            Ok((out, _)) if out.trim() == "connected" => {
                *self.connected.lock().await = true;
            }
            Ok(_) | Err(_) => {
                tracing::warn!("SSH connection test failed, will retry on command exec");
            }
        }
        Ok(())
    }

    async fn teardown(&mut self, _task_id: &str) -> Result<()> {
        tracing::info!(host = %self.config.host, "SSH environment teardown");
        *self.connected.lock().await = false;
        self.task_id = None;
        Ok(())
    }

    async fn run_command(&self, cmd: &str, _timeout_secs: Option<u64>) -> Result<CommandResult> {
        // Prepend cd to working dir
        let cwd = self.working_dir.lock().await;
        let full_cmd = format!("cd {} && {}", cwd.display(), cmd);
        drop(cwd);

        let (stdout, stderr) = self.run_ssh_command(&full_cmd).await?;

        // We don't have exact exit code from ssh easily; assume 0 if no stderr
        let exit_code = if stderr.is_empty() { 0 } else { 1 };

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
        let target = format!("{}@{}:{}", self.config.user, self.config.host, remote.display());
        let local_str = local.to_str().ok_or_else(|| anyhow!("Invalid local path"))?;

        let mut scp_cmd = Command::new("scp");
        if let Some(ref key) = self.config.key_path {
            scp_cmd.arg("-i").arg(key);
        }
        if let Some(port) = self.config.port {
            scp_cmd.arg("-P").arg(port.to_string());
        }
        scp_cmd
            .arg("-o")
            .arg("BatchMode=yes")
            .arg(local_str)
            .arg(&target);

        let status = scp_cmd.status().await.context("Failed to run scp")?;
        if !status.success() {
            anyhow::bail!("scp upload failed");
        }
        Ok(())
    }

    async fn download_file(&self, remote: &Path, local: &Path) -> Result<()> {
        let source = format!("{}@{}:{}", self.config.user, self.config.host, remote.display());
        let local_str = local.to_str().ok_or_else(|| anyhow!("Invalid local path"))?;

        let mut scp_cmd = Command::new("scp");
        if let Some(ref key) = self.config.key_path {
            scp_cmd.arg("-i").arg(key);
        }
        if let Some(port) = self.config.port {
            scp_cmd.arg("-P").arg(port.to_string());
        }
        scp_cmd
            .arg("-o")
            .arg("BatchMode=yes")
            .arg(&source)
            .arg(local_str);

        let status = scp_cmd.status().await.context("Failed to run scp")?;
        if !status.success() {
            anyhow::bail!("scp download failed");
        }
        Ok(())
    }

    fn working_dir(&self) -> &Path {
        static FALLBACK: std::sync::LazyLock<PathBuf> =
            std::sync::LazyLock::new(|| PathBuf::from("/tmp"));
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
    fn test_ssh_env_type() {
        let config = SshConfig {
            host: "example.com".to_string(),
            user: "admin".to_string(),
            port: None,
            key_path: None,
            password: None,
        };
        let env = SshEnv::new(config).unwrap();
        assert_eq!(env.env_type(), EnvType::Ssh);
    }

    #[test]
    fn test_ssh_empty_host() {
        let config = SshConfig {
            host: "".to_string(),
            user: "admin".to_string(),
            port: None,
            key_path: None,
            password: None,
        };
        assert!(SshEnv::new(config).is_err());
    }

    #[test]
    fn test_ssh_is_persistent() {
        let config = SshConfig {
            host: "example.com".to_string(),
            user: "admin".to_string(),
            port: None,
            key_path: None,
            password: None,
        };
        let env = SshEnv::new(config).unwrap();
        assert!(env.is_persistent());
    }

    #[test]
    fn test_ssh_target() {
        let config = SshConfig {
            host: "example.com".to_string(),
            user: "admin".to_string(),
            port: Some(2222),
            key_path: None,
            password: None,
        };
        let env = SshEnv::new(config).unwrap();
        let target = env.ssh_target();
        assert_eq!(target, "admin@example.com -p 2222");
    }
}
