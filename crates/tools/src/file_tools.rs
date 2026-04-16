//! File I/O Tools for Hermes Agent.
//!
//! Provides tools for reading, writing, patching, and searching files.

use crate::{Tool, ToolContext};
use anyhow::{Result, Context};
use async_trait::async_trait;
use h_core::ToolResult;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::fs;

// ============================================================================
// Read File Tool
// ============================================================================

/// Tool for reading file contents.
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Returns the file content as a string. \
         Supports reading from any path within the working directory."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to read (relative to working directory or absolute)"
                },
                "offset": {
                    "type": "integer",
                    "description": "Optional line number to start reading from (1-based)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Optional maximum number of lines to read"
                }
            },
            "required": ["path"]
        })
    }

    fn max_result_size_chars(&self) -> Option<usize> {
        Some(50_000) // Limit to 50K chars
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let path: String = args.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' argument")?
            .to_string();

        let offset: Option<usize> = args.get("offset")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);

        let limit: Option<usize> = args.get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);

        // Resolve path relative to working directory
        let full_path = if path.starts_with('/') || path.starts_with('~') {
            PathBuf::from(&path)
        } else {
            ctx.working_dir.join(&path)
        };

        // Check if path is within allowed bounds (security check)
        // For now, allow all paths - can add restrictions later

        // Read file content
        let content = fs::read_to_string(&full_path)
            .await
            .with_context(|| format!("Failed to read file: {}", full_path.display()))?;

        // Apply offset and limit
        let lines: Vec<&str> = content.lines().collect();
        let start = offset.map(|o| o.saturating_sub(1)).unwrap_or(0);
        let end = limit.map(|l| start + l).unwrap_or(lines.len());

        let result_content = lines.iter()
            .skip(start)
            .take(end - start)
            .copied()  // Convert &&str to &str
            .collect::<Vec<&str>>()
            .join("\n");

        // Check result size
        if result_content.len() > self.max_result_size_chars().unwrap_or(usize::MAX) {
            // Persist to temp file and return path
            let temp_path = ctx.working_dir.join(format!(
                ".hermes_large_output_{}.txt",
                uuid::Uuid::new_v4()
            ));
            fs::write(&temp_path, &result_content).await?;
            return Ok(ToolResult::with_persisted_path(
                format!("Output too large, saved to: {}", temp_path.display()),
                temp_path,
            ));
        }

        Ok(ToolResult::success(result_content))
    }
}

// ============================================================================
// Write File Tool
// ============================================================================

/// Tool for writing content to a file.
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist, \
         overwrites if it does. Creates parent directories if needed."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to write to (relative to working directory or absolute)"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let path: String = args.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' argument")?
            .to_string();

        let content: String = args.get("content")
            .and_then(|v| v.as_str())
            .context("Missing 'content' argument")?
            .to_string();

        // Resolve path
        let full_path = if path.starts_with('/') || path.starts_with('~') {
            PathBuf::from(&path)
        } else {
            ctx.working_dir.join(&path)
        };

        // Create parent directories if needed
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        // Write content
        fs::write(&full_path, &content)
            .await
            .with_context(|| format!("Failed to write file: {}", full_path.display()))?;

        Ok(ToolResult::success(format!(
            "Successfully wrote {} bytes to {}",
            content.len(),
            full_path.display()
        )))
    }
}

// ============================================================================
// Patch Tool
// ============================================================================

/// Tool for applying patches/edits to files.
pub struct PatchTool;

#[async_trait]
impl Tool for PatchTool {
    fn name(&self) -> &str {
        "patch"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Apply a patch to a file by replacing specific text. \
         The patch replaces old_string with new_string in the file."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to patch"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to find and replace (must be exact match)"
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace with"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let path: String = args.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' argument")?
            .to_string();

        let old_string: String = args.get("old_string")
            .and_then(|v| v.as_str())
            .context("Missing 'old_string' argument")?
            .to_string();

        let new_string: String = args.get("new_string")
            .and_then(|v| v.as_str())
            .context("Missing 'new_string' argument")?
            .to_string();

        // Resolve path
        let full_path = if path.starts_with('/') || path.starts_with('~') {
            PathBuf::from(&path)
        } else {
            ctx.working_dir.join(&path)
        };

        // Read current content
        let content = fs::read_to_string(&full_path)
            .await
            .with_context(|| format!("Failed to read file: {}", full_path.display()))?;

        // Check if old_string exists
        if !content.contains(&old_string) {
            return Ok(ToolResult::error(format!(
                "Could not find '{}' in file {}",
                old_string,
                full_path.display()
            )));
        }

        // Apply patch - replace all occurrences (could add count/first option)
        let new_content = content.replace(&old_string, &new_string);

        // Write back
        fs::write(&full_path, &new_content)
            .await
            .with_context(|| format!("Failed to write patched file: {}", full_path.display()))?;

        let replacements = content.matches(&old_string).count();
        Ok(ToolResult::success(format!(
            "Successfully applied {} replacement(s) to {}",
            replacements,
            full_path.display()
        )))
    }
}

// ============================================================================
// Search Files Tool
// ============================================================================

/// Tool for searching files by pattern or content.
pub struct SearchFilesTool;

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Search for files by name pattern or search file contents for a pattern. \
         Returns list of matching files and optionally matched content."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "File name pattern (glob) or content search pattern"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (default: working directory)"
                },
                "search_type": {
                    "type": "string",
                    "enum": ["filename", "content"],
                    "description": "Type of search: 'filename' for glob matching, 'content' for text search"
                }
            },
            "required": ["pattern"]
        })
    }

    fn max_result_size_chars(&self) -> Option<usize> {
        Some(20_000) // Limit search results
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let pattern: String = args.get("pattern")
            .and_then(|v| v.as_str())
            .context("Missing 'pattern' argument")?
            .to_string();

        let path: Option<String> = args.get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let search_type: String = args.get("search_type")
            .and_then(|v| v.as_str())
            .unwrap_or("filename")
            .to_string();

        // Resolve search directory
        let search_dir = match path {
            Some(p) if p.starts_with('/') || p.starts_with('~') => PathBuf::from(p),
            Some(p) => ctx.working_dir.join(p),
            None => ctx.working_dir.clone(),
        };

        // Execute search based on type
        match search_type.as_str() {
            "filename" => search_by_filename(&search_dir, &pattern).await,
            "content" => search_by_content(&search_dir, &pattern).await,
            _ => Ok(ToolResult::error(format!(
                "Unknown search_type: '{}'. Use 'filename' or 'content'",
                search_type
            ))),
        }
    }
}

/// Search files by filename pattern (glob).
async fn search_by_filename(dir: &PathBuf, pattern: &str) -> Result<ToolResult> {
    use walkdir::WalkDir;

    let mut matches: Vec<String> = Vec::new();

    // Use walkdir for recursive search
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .take(100) // Limit results
    {
        let file_name = entry.file_name().to_string_lossy();
        if glob_match(&file_name, pattern) {
            matches.push(entry.path().display().to_string());
        }
    }

    if matches.is_empty() {
        Ok(ToolResult::success("No files found matching pattern"))
    } else {
        Ok(ToolResult::success(format!(
            "Found {} files:\n{}",
            matches.len(),
            matches.join("\n")
        )))
    }
}

/// Search file contents for a pattern.
async fn search_by_content(dir: &PathBuf, pattern: &str) -> Result<ToolResult> {
    use walkdir::WalkDir;

    let mut results: Vec<String> = Vec::new();

    // Walk through all files
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .take(50) // Limit files to search
    {
        let path = entry.path();

        // Skip binary files and very large files
        if path.extension().map(|e| e == "bin" || e == "exe").unwrap_or(false) {
            continue;
        }

        // Try to read as text
        if let Ok(content) = std::fs::read_to_string(path) {
            // Search for pattern
            for (line_num, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    results.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        line_num + 1,
                        line.trim()
                    ));

                    // Limit results per file
                    if results.len() > 100 {
                        break;
                    }
                }
            }
        }

        if results.len() > 100 {
            break;
        }
    }

    if results.is_empty() {
        Ok(ToolResult::success(format!(
            "No content matches found for '{}'",
            pattern
        )))
    } else {
        Ok(ToolResult::success(format!(
            "Found {} matches:\n{}",
            results.len(),
            results.join("\n")
        )))
    }
}

/// Simple glob pattern matching.
fn glob_match(name: &str, pattern: &str) -> bool {
    // Simple implementation: handle * wildcard
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            let prefix = parts[0];
            let suffix = parts[1];
            return name.starts_with(prefix) && name.ends_with(suffix);
        }
        // Handle single * anywhere
        return name.contains(pattern.replace('*', "").as_str());
    }
    // Exact match
    name == pattern
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use h_core::HermesConfig;

    fn make_context() -> ToolContext {
        ToolContext::new(
            "test-session",
            "test-task",
            Arc::new(HermesConfig::default()),
            std::env::temp_dir(),
        )
    }

    #[test]
    fn test_tool_metadata() {
        let read_tool = ReadFileTool;
        assert_eq!(read_tool.name(), "read_file");
        assert_eq!(read_tool.toolset(), "file");

        let write_tool = WriteFileTool;
        assert_eq!(write_tool.name(), "write_file");

        let patch_tool = PatchTool;
        assert_eq!(patch_tool.name(), "patch");

        let search_tool = SearchFilesTool;
        assert_eq!(search_tool.name(), "search_files");
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("test.rs", "*.rs"));
        assert!(glob_match("test.rs", "test*"));
        assert!(glob_match("test_file.rs", "test*.rs"));
        assert!(!glob_match("test.py", "*.rs"));
        assert!(glob_match("exact_match", "exact_match"));
    }

    #[tokio::test]
    async fn test_write_and_read_file() {
        let ctx = make_context();
        let test_path = format!("test_{}.txt", uuid::Uuid::new_v4());

        // Write
        let write_result = WriteFileTool.execute(
            json!({"path": test_path.clone(), "content": "Hello, Hermes!"}),
            &ctx,
        ).await.unwrap();
        assert!(!write_result.is_error);

        // Read
        let read_result = ReadFileTool.execute(
            json!({"path": test_path.clone()}),
            &ctx,
        ).await.unwrap();
        assert!(!read_result.is_error);
        assert!(read_result.content.contains("Hello, Hermes!"));

        // Cleanup
        let _ = tokio::fs::remove_file(ctx.working_dir.join(&test_path)).await;
    }

    #[tokio::test]
    async fn test_patch_file() {
        let ctx = make_context();
        let test_path = format!("patch_test_{}.txt", uuid::Uuid::new_v4());

        // Write initial content
        WriteFileTool.execute(
            json!({"path": test_path.clone(), "content": "old text here"}),
            &ctx,
        ).await.unwrap();

        // Patch
        let patch_result = PatchTool.execute(
            json!({
                "path": test_path.clone(),
                "old_string": "old text",
                "new_string": "new text"
            }),
            &ctx,
        ).await.unwrap();
        assert!(!patch_result.is_error);

        // Verify
        let read_result = ReadFileTool.execute(
            json!({"path": test_path.clone()}),
            &ctx,
        ).await.unwrap();
        assert!(read_result.content.contains("new text here"));

        // Cleanup
        let _ = tokio::fs::remove_file(ctx.working_dir.join(&test_path)).await;
    }

    #[tokio::test]
    async fn test_patch_not_found() {
        let ctx = make_context();
        let test_path = format!("patch_fail_{}.txt", uuid::Uuid::new_v4());

        // Write content
        WriteFileTool.execute(
            json!({"path": test_path.clone(), "content": "some content"}),
            &ctx,
        ).await.unwrap();

        // Try to patch non-existent text
        let patch_result = PatchTool.execute(
            json!({
                "path": test_path.clone(),
                "old_string": "nonexistent",
                "new_string": "replacement"
            }),
            &ctx,
        ).await.unwrap();
        assert!(patch_result.is_error);

        // Cleanup
        let _ = tokio::fs::remove_file(ctx.working_dir.join(&test_path)).await;
    }

    #[test]
    fn test_schemas_valid_json() {
        let read_schema = ReadFileTool.schema();
        assert!(read_schema.is_object());

        let write_schema = WriteFileTool.schema();
        assert!(write_schema.is_object());

        let patch_schema = PatchTool.schema();
        assert!(patch_schema.is_object());

        let search_schema = SearchFilesTool.schema();
        assert!(search_schema.is_object());
    }
}