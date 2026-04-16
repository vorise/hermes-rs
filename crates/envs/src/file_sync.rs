//! File Sync Utilities
//!
//! Synchronize files between local and remote environments.

use std::path::Path;
use anyhow::{Result, Context};
use tracing::{debug, info};
use walkdir::WalkDir;

use crate::Environment;

/// Sync files from local to remote environment.
///
/// Recursively copies all files from local directory to remote.
pub async fn sync_up(
    env: &mut dyn Environment,
    local: &Path,
    remote: &Path,
) -> Result<()> {
    debug!("Sync up: {} -> {}", local.display(), remote.display());

    if !local.exists() {
        return Err(anyhow::anyhow!("Local path does not exist: {}", local.display()));
    }

    if local.is_file() {
        // Single file upload
        env.upload_file(local, remote).await?;
        info!("Uploaded: {} -> {}", local.display(), remote.display());
        return Ok(());
    }

    // Directory sync
    for entry in WalkDir::new(local).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let local_path = entry.path();
            let relative = local_path.strip_prefix(local)
                .context("Failed to compute relative path")?;
            let remote_path = remote.join(relative);

            env.upload_file(local_path, &remote_path).await?;
            debug!("Uploaded: {} -> {}", local_path.display(), remote_path.display());
        }
    }

    info!("Sync complete: {} files uploaded", WalkDir::new(local).into_iter().filter(|_| true).count());
    Ok(())
}

/// Sync files from remote to local environment.
///
/// Recursively copies all files from remote directory to local.
pub async fn sync_down(
    env: &mut dyn Environment,
    remote: &Path,
    local: &Path,
) -> Result<()> {
    debug!("Sync down: {} -> {}", remote.display(), local.display());

    // Create local directory
    tokio::fs::create_dir_all(local).await?;

    // First, list remote files
    let list_cmd = format!("find {} -type f", remote.display());
    let output = env.run_command(&list_cmd, std::time::Duration::from_secs(60)).await?;

    // Parse file list
    for line in output.lines() {
        let remote_file = Path::new(line.trim());
        if remote_file.is_absolute() {
            let relative = remote_file.strip_prefix(remote)
                .context("Failed to compute relative path")?;
            let local_file = local.join(relative);

            // Create parent directories
            if let Some(parent) = local_file.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            env.download_file(remote_file, &local_file).await?;
            debug!("Downloaded: {} -> {}", remote_file.display(), local_file.display());
        }
    }

    info!("Sync complete: files downloaded to {}", local.display());
    Ok(())
}

/// Sync a single file.
pub async fn sync_file(
    env: &mut dyn Environment,
    local: &Path,
    remote: &Path,
    direction: SyncDirection,
) -> Result<()> {
    match direction {
        SyncDirection::Up => env.upload_file(local, remote).await,
        SyncDirection::Down => env.download_file(remote, local).await,
    }
}

/// Sync direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    /// Upload (local -> remote).
    Up,
    /// Download (remote -> local).
    Down,
}

impl std::fmt::Display for SyncDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncDirection::Up => f.write_str("up"),
            SyncDirection::Down => f.write_str("down"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_direction_display() {
        assert_eq!(SyncDirection::Up.to_string(), "up");
        assert_eq!(SyncDirection::Down.to_string(), "down");
    }

    #[tokio::test]
    async fn test_sync_file_placeholder() {
        // This test would need a mock Environment
        // For now, just verify the types are correct
        let direction = SyncDirection::Up;
        assert_eq!(direction, SyncDirection::Up);
    }
}