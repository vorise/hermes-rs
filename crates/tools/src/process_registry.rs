use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use tokio::process::{Child, Command};
use uuid::Uuid;

/// Handle to a spawned process.
#[derive(Debug, Clone)]
pub struct ProcessHandle {
    pub id: String,
}

/// A running process with its output.
struct RunningProcess {
    child: Child,
    output: Arc<Mutex<String>>,
}

/// Registry for managing background processes.
pub struct ProcessRegistry {
    processes: HashMap<String, RunningProcess>,
}

impl Default for ProcessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
        }
    }

    /// Spawn a command. If background is true, the process runs asynchronously.
    pub async fn spawn(
        &mut self,
        cmd: &str,
        background: bool,
        working_dir: Option<&std::path::Path>,
    ) -> Result<ProcessHandle> {
        let id = Uuid::new_v4().to_string();

        let mut command = Command::new("sh");
        command.arg("-c").arg(cmd);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        if let Some(dir) = working_dir {
            command.current_dir(dir);
        }

        let child = command.spawn()?;
        let output = Arc::new(Mutex::new(String::new()));

        if background {
            self.processes.insert(
                id.clone(),
                RunningProcess {
                    child,
                    output: output.clone(),
                },
            );
            Ok(ProcessHandle { id })
        } else {
            // Wait for foreground process and return output
            let output_str = child.wait_with_output().await?;
            let stdout = String::from_utf8_lossy(&output_str.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output_str.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout
            } else {
                format!("{stdout}\n{stderr}")
            };
            let output = Arc::new(Mutex::new(combined));
            // Spawn a dummy completed process to store the output
            let dummy = Command::new("true").spawn()?;
            self.processes.insert(
                id.clone(),
                RunningProcess {
                    child: dummy,
                    output: output.clone(),
                },
            );
            Ok(ProcessHandle { id })
        }
    }

    /// Get the output of a process.
    pub fn get_output(&self, handle: &ProcessHandle) -> Result<String> {
        let proc = self
            .processes
            .get(&handle.id)
            .ok_or_else(|| anyhow::anyhow!("process not found: {}", handle.id))?;
        Ok(proc.output.lock().clone())
    }

    /// Kill a running process.
    pub async fn kill(&mut self, handle: &ProcessHandle) -> Result<()> {
        if let Some(proc) = self.processes.get_mut(&handle.id) {
            let _ = proc.child.kill().await;
        }
        self.processes.remove(&handle.id);
        Ok(())
    }

    /// Clean up all processes.
    pub async fn cleanup_all(&mut self) {
        for (_, proc) in self.processes.iter_mut() {
            let _ = proc.child.kill().await;
        }
        self.processes.clear();
    }
}

impl Drop for ProcessRegistry {
    fn drop(&mut self) {
        // Best-effort cleanup - in async context this won't work properly,
        // but it's a safety net for sync drops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_foreground_process() {
        let mut registry = ProcessRegistry::new();
        let handle = registry
            .spawn("echo hello", false, None)
            .await
            .unwrap();
        let output = registry.get_output(&handle).unwrap();
        assert!(output.contains("hello"));
    }

    #[tokio::test]
    async fn test_process_not_found() {
        let registry = ProcessRegistry::new();
        let handle = ProcessHandle {
            id: "nonexistent".to_string(),
        };
        assert!(registry.get_output(&handle).is_err());
    }
}
