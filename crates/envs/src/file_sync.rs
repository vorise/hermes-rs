use std::path::Path;

use anyhow::{Context, Result};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Copy files from local to a remote host via scp.
pub async fn sync_up_scp(
    host: &str,
    user: &str,
    local: &Path,
    remote: &str,
    key_path: Option<&Path>,
) -> Result<()> {
    let target = format!("{user}@{host}:{remote}");
    let local_str = local.to_str().context("Invalid local path")?;

    let mut cmd = Command::new("scp");
    cmd.arg(local_str).arg(&target);
    if let Some(key) = key_path {
        cmd.arg("-i").arg(key.to_str().context("Invalid key path")?);
    }

    let status = cmd.status().await.context("Failed to run scp")?;
    if !status.success() {
        anyhow::bail!("scp upload failed with exit code {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

/// Copy files from a remote host to local via scp.
pub async fn sync_down_scp(
    host: &str,
    user: &str,
    remote: &str,
    local: &Path,
    key_path: Option<&Path>,
) -> Result<()> {
    let source = format!("{user}@{host}:{remote}");
    let local_str = local.to_str().context("Invalid local path")?;

    let mut cmd = Command::new("scp");
    cmd.arg(&source).arg(local_str);
    if let Some(key) = key_path {
        cmd.arg("-i").arg(key.to_str().context("Invalid key path")?);
    }

    let status = cmd.status().await.context("Failed to run scp")?;
    if !status.success() {
        anyhow::bail!("scp download failed with exit code {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

/// Synchronize a directory up to a remote host using rsync (if available).
/// Falls back to scp for individual files if rsync is not installed.
pub async fn sync_dir_up(
    host: &str,
    user: &str,
    local_dir: &Path,
    remote_dir: &str,
    key_path: Option<&Path>,
) -> Result<()> {
    let target = format!("{user}@{host}:{remote_dir}");
    let local_str = local_dir.to_str().context("Invalid local path")?;

    // Try rsync first
    match Command::new("rsync")
        .arg("-az")
        .arg("--delete")
        .arg(local_str)
        .arg(&target)
        .status()
        .await
    {
        Ok(status) if status.success() => return Ok(()),
        _ => {}
    }

    // Fallback: use scp -r
    let mut cmd = Command::new("scp");
    cmd.arg("-r").arg(local_str).arg(&target);
    if let Some(key) = key_path {
        cmd.arg("-i").arg(key.to_str().context("Invalid key path")?);
    }

    let status = cmd.status().await.context("Failed to run scp")?;
    if !status.success() {
        anyhow::bail!("scp directory upload failed with exit code {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

/// Synchronize a directory down from a remote host using rsync.
pub async fn sync_dir_down(
    host: &str,
    user: &str,
    remote_dir: &str,
    local_dir: &Path,
    key_path: Option<&Path>,
) -> Result<()> {
    let source = format!("{user}@{host}:{remote_dir}");
    let local_str = local_dir.to_str().context("Invalid local path")?;

    // Try rsync first
    match Command::new("rsync")
        .arg("-az")
        .arg("--delete")
        .arg(&source)
        .arg(local_str)
        .status()
        .await
    {
        Ok(status) if status.success() => return Ok(()),
        _ => {}
    }

    // Fallback: use scp -r
    let mut cmd = Command::new("scp");
    cmd.arg("-r").arg(&source).arg(local_str);
    if let Some(key) = key_path {
        cmd.arg("-i").arg(key.to_str().context("Invalid key path")?);
    }

    let status = cmd.status().await.context("Failed to run scp")?;
    if !status.success() {
        anyhow::bail!("scp directory download failed with exit code {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

/// Sync a file to a Docker container using docker cp.
pub async fn docker_sync_up(container: &str, local: &Path, remote: &Path) -> Result<()> {
    let remote_str = format!("{container}:{}", remote.display());
    let local_str = local.to_str().context("Invalid local path")?;

    let status = Command::new("docker")
        .arg("cp")
        .arg(local_str)
        .arg(&remote_str)
        .status()
        .await
        .context("Failed to run docker cp")?;

    if !status.success() {
        anyhow::bail!("docker cp upload failed");
    }
    Ok(())
}

/// Sync a file from a Docker container using docker cp.
pub async fn docker_sync_down(container: &str, remote: &Path, local: &Path) -> Result<()> {
    let remote_str = format!("{container}:{}", remote.display());
    let local_str = local.to_str().context("Invalid local path")?;

    let status = Command::new("docker")
        .arg("cp")
        .arg(&remote_str)
        .arg(local_str)
        .status()
        .await
        .context("Failed to run docker cp")?;

    if !status.success() {
        anyhow::bail!("docker cp download failed");
    }
    Ok(())
}

/// Check if two files differ by comparing their sizes and checksums.
pub async fn files_differ(a: &Path, b: &Path) -> Result<bool> {
    let mut fa = tokio::fs::File::open(a).await?;
    let mut fb = tokio::fs::File::open(b).await?;

    let mut buf_a = Vec::new();
    let mut buf_b = Vec::new();
    fa.read_to_end(&mut buf_a).await?;
    fb.read_to_end(&mut buf_b).await?;

    Ok(buf_a != buf_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_sync_up_invalid_path() {
        // docker cp with invalid paths should fail
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(docker_sync_up(
            "nonexistent-container",
            Path::new("/tmp/hermes_test_nonexistent"),
            Path::new("/tmp/test"),
        ));
        // Should fail because the container doesn't exist
        assert!(result.is_err());
    }
}
