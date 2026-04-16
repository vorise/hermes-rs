//! Hermes Envs Crate
//!
//! Terminal backend implementations for execution environments.
//! Supports Local, Docker, SSH, Modal, Daytona, and Singularity backends.

use std::path::Path;
use std::time::Duration;
use async_trait::async_trait;
use anyhow::Result;

/// Environment trait for execution backends.
///
/// All terminal backends must implement this trait for unified
/// command execution, file transfer, and lifecycle management.
#[async_trait]
pub trait Environment: Send + Sync {
    /// Initialize environment for a task.
    ///
    /// For ephemeral environments (Docker, Modal), this may create
    /// a new container/sandbox. For persistent environments (Local, SSH),
    /// this validates connectivity.
    async fn setup(&mut self, task_id: &str) -> Result<()>;

    /// Cleanup environment after task completion.
    ///
    /// For ephemeral environments, this destroys the container/sandbox.
    /// For persistent environments, this is a no-op.
    async fn teardown(&mut self, task_id: &str) -> Result<()>;

    /// Execute a command in the environment.
    ///
    /// Returns stdout on success, or error with stderr on failure.
    /// Commands are executed with timeout protection.
    async fn run_command(&mut self, cmd: &str, timeout: Duration) -> Result<String>;

    /// Upload a file from local to remote environment.
    async fn upload_file(&mut self, local: &Path, remote: &Path) -> Result<()>;

    /// Download a file from remote environment to local.
    async fn download_file(&mut self, remote: &Path, local: &Path) -> Result<()>;

    /// Get the working directory for command execution.
    fn working_dir(&self) -> &Path;

    /// Check if environment is persistent (Local, SSH) or ephemeral (Docker, Modal).
    fn is_persistent(&self) -> bool;

    /// Get environment type name for logging.
    fn backend_name(&self) -> &str;
}

/// Environment backend type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    Local,
    Docker,
    Ssh,
    Modal,
    Daytona,
    Singularity,
}

impl BackendType {
    /// Get backend name as string.
    pub fn as_str(&self) -> &'static str {
        match self {
            BackendType::Local => "local",
            BackendType::Docker => "docker",
            BackendType::Ssh => "ssh",
            BackendType::Modal => "modal",
            BackendType::Daytona => "daytona",
            BackendType::Singularity => "singularity",
        }
    }
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Environment creation error.
#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    #[error("Command timed out after {0}ms")]
    Timeout(u64),

    #[error("Command failed with exit code {code}: {stderr}")]
    CommandFailed { code: i32, stderr: String },

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Container error: {0}")]
    ContainerError(String),

    #[error("SSH error: {0}")]
    SshError(String),

    #[error("Sandbox API error: {0}")]
    SandboxError(String),

    #[error("Not supported on this backend: {0}")]
    NotSupported(String),
}

pub mod local;
pub mod docker;
pub mod ssh;
pub mod modal;
pub mod daytona;
pub mod singularity;
pub mod file_sync;

pub use local::LocalEnv;
pub use docker::DockerEnv;
pub use ssh::SshEnv;
pub use modal::ModalEnv;
pub use daytona::DaytonaEnv;
pub use singularity::SingularityEnv;

/// Create environment from backend type and config.
pub fn create_env(backend: BackendType) -> Result<Box<dyn Environment>> {
    match backend {
        BackendType::Local => Ok(Box::new(LocalEnv::new())),
        BackendType::Docker => Ok(Box::new(DockerEnv::new())),
        BackendType::Ssh => Ok(Box::new(SshEnv::new())),
        BackendType::Modal => Ok(Box::new(ModalEnv::new())),
        BackendType::Daytona => Ok(Box::new(DaytonaEnv::new())),
        BackendType::Singularity => Ok(Box::new(SingularityEnv::new())),
    }
}

/// Check if environment is ephemeral (needs cleanup).
pub fn is_ephemeral(backend: BackendType) -> bool {
    match backend {
        BackendType::Local | BackendType::Ssh => false,
        BackendType::Docker | BackendType::Modal | BackendType::Daytona | BackendType::Singularity => true,
    }
}