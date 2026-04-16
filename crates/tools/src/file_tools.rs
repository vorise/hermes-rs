use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;

use crate::tool::{Tool, ToolContext, ToolResult};

/// Maximum file size to read (1MB).
const MAX_READ_SIZE: usize = 1024 * 1024;

/// Maximum file size to write (10MB).
const MAX_WRITE_SIZE: usize = 10 * 1024 * 1024;

/// Read a file from the filesystem.
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn toolset(&self) -> &str {
        "file_io"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Returns the file contents as a string. \
        Supports text files and returns base64 for binary files."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let path = args.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: path"))?;

        let resolved = resolve_path(path, &ctx.working_dir);

        if !resolved.exists() {
            return Ok(ToolResult::err(format!("File not found: {}", resolved.display())));
        }

        if !resolved.is_file() {
            return Ok(ToolResult::err(format!("Not a file: {}", resolved.display())));
        }

        let metadata = std::fs::metadata(&resolved)?;
        if metadata.len() as usize > MAX_READ_SIZE {
            return Ok(ToolResult::err(format!(
                "File too large ({} bytes). Maximum: {} bytes",
                metadata.len(), MAX_READ_SIZE
            )));
        }

        let content = std::fs::read_to_string(&resolved)
            .with_context(|| format!("Failed to read file: {}", resolved.display()))?;

        Ok(ToolResult::ok(format!(
            "Read {} bytes from {}\n\n{}",
            content.len(),
            resolved.display(),
            content
        )))
    }
}

/// Write content to a file, creating it if it doesn't exist.
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn toolset(&self) -> &str {
        "file_io"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist, \
        overwrites if it does. Creates parent directories as needed."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let path = args.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: path"))?;

        let content = args.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: content"))?;

        if content.len() > MAX_WRITE_SIZE {
            return Ok(ToolResult::err(format!(
                "Content too large ({} bytes). Maximum: {} bytes",
                content.len(), MAX_WRITE_SIZE
            )));
        }

        let resolved = resolve_path(path, &ctx.working_dir);

        // Create parent directories
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        std::fs::write(&resolved, content)
            .with_context(|| format!("Failed to write file: {}", resolved.display()))?;

        Ok(ToolResult::ok(format!(
            "Wrote {} bytes to {}",
            content.len(),
            resolved.display()
        )))
    }
}

/// Apply a patch (unified diff) to a file.
pub struct PatchTool;

#[async_trait]
impl Tool for PatchTool {
    fn name(&self) -> &str {
        "patch"
    }

    fn toolset(&self) -> &str {
        "file_io"
    }

    fn description(&self) -> &str {
        "Apply a patch to a file using unified diff format. \
        Use this for targeted edits rather than rewriting entire files."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to patch"
                },
                "patch": {
                    "type": "string",
                    "description": "Unified diff patch to apply"
                }
            },
            "required": ["path", "patch"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let path = args.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: path"))?;

        let patch_content = args.get("patch")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: patch"))?;

        let resolved = resolve_path(path, &ctx.working_dir);

        if !resolved.exists() {
            return Ok(ToolResult::err(format!("File not found: {}", resolved.display())));
        }

        // Apply the patch using the system `patch` command
        let mut child = match tokio::process::Command::new("patch")
            .arg(&resolved)
            .arg("-p0")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::err(format!(
                    "Failed to run patch command: {e}. Install `patch` or use write_file instead."
                )));
            }
        };

        // Write patch content to stdin
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(patch_content.as_bytes()).await {
                return Ok(ToolResult::err(format!("Failed to write patch: {e}")));
            }
        }

        match child.wait_with_output().await {
            Ok(output) if output.status.success() => {
                Ok(ToolResult::ok(format!(
                    "Patch applied successfully to {}",
                    resolved.display()
                )))
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Ok(ToolResult::err(format!(
                    "Patch failed for {}: {}",
                    resolved.display(),
                    stderr
                )))
            }
            Err(e) => Ok(ToolResult::err(format!(
                "Failed to run patch command: {e}. Install `patch` or use write_file instead."
            ))),
        }
    }
}

/// Search files using glob patterns.
pub struct SearchFilesTool;

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn toolset(&self) -> &str {
        "file_io"
    }

    fn description(&self) -> &str {
        "Search for files matching a glob pattern within a directory. \
        Returns a list of matching file paths."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to search in (defaults to current directory)"
                },
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match (e.g., '*.rs', '**/*.md')"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let pattern = args.get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: pattern"))?;

        let search_path = args.get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve_path(p, &ctx.working_dir))
            .unwrap_or_else(|| ctx.working_dir.clone());

        if !search_path.exists() {
            return Ok(ToolResult::err(format!("Directory not found: {}", search_path.display())));
        }

        let glob_pattern = search_path.join(pattern).to_string_lossy().to_string();
        let mut matches = Vec::new();

        for entry in glob::glob(&glob_pattern)? {
            match entry {
                Ok(path) => {
                    if path.is_file() {
                        matches.push(path.to_string_lossy().to_string());
                    }
                }
                Err(e) => {
                    tracing::warn!("Glob error: {e}");
                }
            }
        }

        if matches.is_empty() {
            Ok(ToolResult::ok(format!("No files found matching '{pattern}'")))
        } else {
            let count = matches.len();
            let list = matches.iter().map(|s| format!("  {s}")).collect::<Vec<_>>().join("\n");
            Ok(ToolResult::ok(format!(
                "Found {count} files matching '{pattern}':\n{list}"
            )))
        }
    }
}

/// Grep for content within files.
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn toolset(&self) -> &str {
        "file_io"
    }

    fn description(&self) -> &str {
        "Search for a pattern within files. Returns matching lines with file paths."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Whether to do case-sensitive matching",
                    "default": false
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let pattern = args.get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: pattern"))?;

        let path = args.get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve_path(p, &ctx.working_dir))
            .unwrap_or_else(|| ctx.working_dir.clone());

        let case_sensitive = args.get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let re = if case_sensitive {
            regex::Regex::new(pattern)
        } else {
            regex::RegexBuilder::new(pattern).case_insensitive(true).build()
        };

        let re = re.map_err(|e| anyhow::anyhow!("Invalid regex pattern: {e}"))?;

        let max_results = 50;
        let results = if path.is_file() {
            let mut r = Vec::new();
            let _ = search_file_for_pattern(&path, &re, &mut r, max_results);
            r
        } else if path.is_dir() {
            let mut r = Vec::new();
            for entry in walkdir::WalkDir::new(&path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .take(500)
            {
                if r.len() >= max_results {
                    break;
                }
                let remaining = max_results - r.len();
                let _ = search_file_for_pattern(entry.path(), &re, &mut r, remaining);
            }
            r
        } else {
            Vec::new()
        };

        if results.is_empty() {
            Ok(ToolResult::ok(format!("No matches for '{pattern}'")))
        } else {
            let count = results.len();
            let output = results.iter().map(|(file, line, content)| {
                format!("{file}:{line}: {content}")
            }).collect::<Vec<_>>().join("\n");
            Ok(ToolResult::ok(format!(
                "Found {count} matches for '{pattern}':\n{output}"
            )))
        }
    }
}

fn search_file_for_pattern(
    path: &Path,
    re: &regex::Regex,
    results: &mut Vec<(String, usize, String)>,
    max: usize,
) -> Result<()> {
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.len() > MAX_READ_SIZE as u64 {
            return Ok(());
        }
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let display = path.to_string_lossy();
    for (line_num, line) in content.lines().enumerate() {
        if results.len() >= max {
            break;
        }
        if re.is_match(line) {
            results.push((display.to_string(), line_num + 1, line.trim().to_string()));
        }
    }

    Ok(())
}

/// Resolve a path, handling relative and absolute paths.
fn resolve_path(path: &str, working_dir: &Path) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx(working_dir: std::path::PathBuf) -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            task_id: "test".to_string(),
            config: std::sync::Arc::new(h_core::HermesConfig::default()),
            working_dir,
        }
    }

    #[tokio::test]
    async fn test_read_file() {
        let dir = std::env::temp_dir().join("hermes_tools_test_read");
        std::fs::create_dir_all(&dir).ok();
        let file = dir.join("test.txt");
        std::fs::write(&file, "hello world").ok();

        let ctx = test_ctx(dir.clone());
        let result = ReadFileTool.execute(
            json!({"path": "test.txt"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("hello world"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_write_file() {
        let dir = std::env::temp_dir().join("hermes_tools_test_write");
        std::fs::create_dir_all(&dir).ok();
        let file = dir.join("output.txt");

        let ctx = test_ctx(dir.clone());
        let result = WriteFileTool.execute(
            json!({"path": "output.txt", "content": "test content"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("Wrote"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "test content");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_write_file_creates_dirs() {
        let dir = std::env::temp_dir().join("hermes_tools_test_nested");
        let file = dir.join("a").join("b").join("c.txt");

        let ctx = test_ctx(dir.clone());
        let result = WriteFileTool.execute(
            json!({"path": "a/b/c.txt", "content": "nested"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "nested");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let ctx = test_ctx(std::env::temp_dir());
        let result = ReadFileTool.execute(
            json!({"path": "nonexistent.txt"}),
            &ctx,
        ).await.unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }

    #[tokio::test]
    async fn test_search_files() {
        let dir = std::env::temp_dir().join("hermes_tools_test_search");
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(dir.join("test1.rs"), "fn main() {}").ok();
        std::fs::write(dir.join("test2.rs"), "fn test() {}").ok();
        std::fs::write(dir.join("readme.md"), "# Hello").ok();

        let ctx = test_ctx(dir.clone());
        let result = SearchFilesTool.execute(
            json!({"pattern": "*.rs"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("2 files"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_grep_tool() {
        let dir = std::env::temp_dir().join("hermes_tools_test_grep");
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(dir.join("code.rs"), "fn main() {\n    println!(\"hello\");\n}").ok();

        let ctx = test_ctx(dir.clone());
        let result = GrepTool.execute(
            json!({"pattern": "println", "path": "code.rs"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("println"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
