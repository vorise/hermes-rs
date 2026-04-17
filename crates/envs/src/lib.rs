use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

mod daytona;
mod docker;
mod file_sync;
mod local;
mod modal;
mod singularity;
mod ssh;

pub use daytona::DaytonaEnv;
pub use docker::DockerEnv;
pub use file_sync::*;
pub use local::LocalEnv;
pub use modal::ModalEnv;
pub use singularity::SingularityEnv;
pub use ssh::SshEnv;

/// Terminal backend type identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvType {
    Local,
    Docker,
    Ssh,
    Modal,
    Daytona,
    Singularity,
}

impl std::fmt::Display for EnvType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvType::Local => write!(f, "local"),
            EnvType::Docker => write!(f, "docker"),
            EnvType::Ssh => write!(f, "ssh"),
            EnvType::Modal => write!(f, "modal"),
            EnvType::Daytona => write!(f, "daytona"),
            EnvType::Singularity => write!(f, "singularity"),
        }
    }
}

impl std::str::FromStr for EnvType {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "local" => Ok(EnvType::Local),
            "docker" => Ok(EnvType::Docker),
            "ssh" => Ok(EnvType::Ssh),
            "modal" => Ok(EnvType::Modal),
            "daytona" => Ok(EnvType::Daytona),
            "singularity" => Ok(EnvType::Singularity),
            other => Err(format!("Unknown environment type: {other}")),
        }
    }
}

/// SSH connection configuration.
#[derive(Debug, Clone)]
pub struct SshConfig {
    pub host: String,
    pub user: String,
    pub port: Option<u16>,
    pub key_path: Option<PathBuf>,
    pub password: Option<String>,
}

/// Modal environment configuration.
#[derive(Debug, Clone)]
pub struct ModalConfig {
    pub image: Option<String>,
    pub gpu: Option<String>,
    pub memory_mb: Option<u64>,
    pub timeout_secs: Option<u64>,
}

/// Daytona environment configuration.
#[derive(Debug, Clone)]
pub struct DaytonaConfig {
    pub workspace_id: Option<String>,
    pub api_key: Option<String>,
}

impl Default for ModalConfig {
    fn default() -> Self {
        Self {
            image: None,
            gpu: None,
            memory_mb: None,
            timeout_secs: None,
        }
    }
}

impl Default for DaytonaConfig {
    fn default() -> Self {
        Self {
            workspace_id: None,
            api_key: None,
        }
    }
}

/// Result of a command execution.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CommandResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn output(&self) -> String {
        if self.stdout.is_empty() {
            self.stderr.clone()
        } else if self.stderr.is_empty() {
            self.stdout.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }
}

/// Abstract base trait for all terminal backend environments.
#[async_trait]
pub trait Environment: Send + Sync {
    /// The type of this environment.
    fn env_type(&self) -> EnvType;

    /// Initialize the environment.
    async fn setup(&mut self, task_id: &str) -> Result<()>;

    /// Clean up environment resources for a task.
    async fn teardown(&mut self, task_id: &str) -> Result<()>;

    /// Execute a command and return the result.
    async fn run_command(&self, cmd: &str, timeout_secs: Option<u64>) -> Result<CommandResult>;

    /// Execute an interactive command (PTY).
    async fn run_command_interactive(&self, cmd: &str) -> Result<String>;

    /// Upload a file to the environment.
    async fn upload_file(&self, local: &Path, remote: &Path) -> Result<()>;

    /// Download a file from the environment.
    async fn download_file(&self, remote: &Path, local: &Path) -> Result<()>;

    /// Get the current working directory.
    fn working_dir(&self) -> &Path;

    /// Check if the environment persists between turns.
    fn is_persistent(&self) -> bool;
}

/// Environment factory — create an environment from configuration.
pub async fn create_environment(
    env_type: EnvType,
    config: &h_core::HermesConfig,
) -> Result<Arc<dyn Environment>> {
    match env_type {
        EnvType::Local => Ok(Arc::new(LocalEnv::default())),
        EnvType::Docker => {
            let docker_config = config.terminal.as_ref();
            let image = docker_config
                .and_then(|t| t.docker.as_ref())
                .map(|d| d.image.clone())
                .unwrap_or_else(|| "python:3.12-slim".to_string());
            Ok(Arc::new(DockerEnv::new(&image)))
        }
        EnvType::Ssh => {
            let ssh = config.terminal.as_ref().and_then(|t| t.ssh.as_ref());
            if let Some(ssh) = ssh {
                let ssh_config = SshConfig {
                    host: ssh.host.clone(),
                    user: ssh.user.clone(),
                    port: ssh.port,
                    key_path: ssh.key_path.as_ref().map(PathBuf::from),
                    password: None,
                };
                Ok(Arc::new(SshEnv::new(ssh_config)?))
            } else {
                anyhow::bail!("SSH configuration not found");
            }
        }
        EnvType::Modal => {
            let modal = config.terminal.as_ref().and_then(|t| t.modal.as_ref());
            let modal_config = ModalConfig {
                image: modal.and_then(|m| m.image.clone()),
                gpu: None,
                memory_mb: None,
                timeout_secs: modal.and_then(|m| m.timeout_secs),
            };
            Ok(Arc::new(ModalEnv::new(modal_config)))
        }
        EnvType::Daytona => {
            let daytona = config.terminal.as_ref().and_then(|t| t.daytona.as_ref());
            let daytona_config = DaytonaConfig {
                workspace_id: daytona.and_then(|d| d.workspace_id.clone()),
                api_key: None,
            };
            Ok(Arc::new(DaytonaEnv::new(daytona_config)))
        }
        EnvType::Singularity => {
            let singularity = config.terminal.as_ref().and_then(|t| t.singularity.as_ref());
            if let Some(s) = singularity {
                let image = PathBuf::from(&s.image);
                Ok(Arc::new(SingularityEnv::new(&image, s.gpu)))
            } else {
                Ok(Arc::new(SingularityEnv::default()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_type_display() {
        assert_eq!(EnvType::Local.to_string(), "local");
        assert_eq!(EnvType::Docker.to_string(), "docker");
    }

    #[test]
    fn test_env_type_from_str() {
        assert_eq!("local".parse::<EnvType>().unwrap(), EnvType::Local);
        assert_eq!("docker".parse::<EnvType>().unwrap(), EnvType::Docker);
        assert!("invalid".parse::<EnvType>().is_err());
    }

    #[test]
    fn test_command_result_success() {
        let ok = CommandResult {
            stdout: "output".to_string(),
            stderr: String::new(),
            exit_code: 0,
        };
        assert!(ok.success());

        let fail = CommandResult {
            stdout: String::new(),
            stderr: "error".to_string(),
            exit_code: 1,
        };
        assert!(!fail.success());
    }

    #[test]
    fn test_command_result_output() {
        let r1 = CommandResult {
            stdout: "out".to_string(),
            stderr: String::new(),
            exit_code: 0,
        };
        assert_eq!(r1.output(), "out");

        let r2 = CommandResult {
            stdout: "out".to_string(),
            stderr: "err".to_string(),
            exit_code: 1,
        };
        assert!(r2.output().contains("out"));
        assert!(r2.output().contains("err"));
    }
}
