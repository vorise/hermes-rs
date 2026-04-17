use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// A subdirectory-specific context hint.
#[derive(Debug, Clone)]
pub struct DirHint {
    /// The directory this hint applies to.
    pub directory: String,
    /// The hint content.
    pub content: String,
    /// Whether this hint was auto-discovered (from context files) vs manually added.
    pub auto_discovered: bool,
}

/// Manages subdirectory-specific context hints.
///
/// When working in a multi-module project, different directories may have
/// their own conventions, build systems, or context files. This module
/// discovers and injects those hints into the system prompt.
pub struct SubdirectoryHints {
    /// Hints keyed by normalized directory path.
    hints: HashMap<String, DirHint>,
    /// Directories to scan for context files.
    search_roots: Vec<PathBuf>,
}

/// Context file names to look for in each directory.
const CONTEXT_FILES: &[&str] = &["HERMES.md", ".hermes.md", "AGENTS.md", "CLAUDE.md", ".cursorrules"];

impl SubdirectoryHints {
    pub fn new() -> Self {
        Self {
            hints: HashMap::new(),
            search_roots: vec![std::env::current_dir().unwrap_or_default()],
        }
    }

    /// Add a root directory to scan for context files.
    pub fn add_search_root(&mut self, path: &Path) {
        self.search_roots.push(path.to_path_buf());
    }

    /// Add a manual hint for a directory.
    pub fn add_hint(&mut self, directory: &str, content: &str) {
        let key = normalize_dir(directory);
        self.hints.insert(
            key,
            DirHint {
                directory: directory.to_string(),
                content: content.to_string(),
                auto_discovered: false,
            },
        );
    }

    /// Discover context files in the search roots and subdirectories.
    pub fn discover(&mut self) -> Result<()> {
        let roots: Vec<PathBuf> = self.search_roots.clone();
        for root in &roots {
            self.discover_in_dir(root, 3)?; // Max depth of 3
        }
        Ok(())
    }

    fn discover_in_dir(&mut self, dir: &Path, max_depth: u32) -> Result<()> {
        if max_depth == 0 {
            return Ok(());
        }

        // Check for context files in this directory
        for &filename in CONTEXT_FILES {
            let context_path = dir.join(filename);
            if context_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&context_path) {
                    let rel = dir
                        .to_string_lossy()
                        .to_string();
                    let key = normalize_dir(&rel);
                    if !self.hints.contains_key(&key) {
                        self.hints.insert(
                            key,
                            DirHint {
                                directory: rel,
                                content: format!("## {filename}\n\n{content}"),
                                auto_discovered: true,
                            },
                        );
                    }
                }
            }
        }

        // Recurse into subdirectories
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() {
                    // Skip hidden and common non-project dirs
                    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                    if name.starts_with('.') || name == "target" || name == "node_modules" || name == ".git" {
                        continue;
                    }
                    self.discover_in_dir(&path, max_depth - 1)?;
                }
            }
        }

        Ok(())
    }

    /// Get hints relevant to the current working directory and its ancestors.
    pub fn get_hints_for_dir(&self, current_dir: &Path) -> Vec<&DirHint> {
        let normalized = current_dir.to_string_lossy();
        let mut relevant = Vec::new();

        for hint in self.hints.values() {
            let hint_dir = &hint.directory;
            // Check if the current dir equals, ends with, or contains the hint dir as a path segment
            let is_match = normalized.as_ref() == hint_dir.as_str()
                || normalized.ends_with(format!("/{hint_dir}").as_str())
                || normalized.contains(format!("/{hint_dir}/").as_str())
                || normalized.contains(format!("\\{hint_dir}\\").as_str());
            if is_match {
                relevant.push(hint);
            }
        }

        relevant
    }

    /// Build a combined hint string for injection into the system prompt.
    pub fn build_hint_for_dir(&self, current_dir: &Path) -> String {
        let hints = self.get_hints_for_dir(current_dir);
        if hints.is_empty() {
            return String::new();
        }

        let mut result = String::from("## Directory Context\n\n");
        for hint in hints {
            result.push_str(&format!(
                "### {}\n\n{}\n\n",
                hint.directory, hint.content
            ));
        }
        result
    }

    /// Remove a hint for a directory.
    pub fn remove_hint(&mut self, directory: &str) {
        let key = normalize_dir(directory);
        self.hints.remove(&key);
    }

    /// Get all hints.
    pub fn all_hints(&self) -> Vec<&DirHint> {
        self.hints.values().collect()
    }

    /// Clear all hints.
    pub fn clear(&mut self) {
        self.hints.clear();
    }

    /// Get the count of discovered hints.
    pub fn len(&self) -> usize {
        self.hints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hints.is_empty()
    }
}

impl Default for SubdirectoryHints {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize a directory path for consistent keying.
fn normalize_dir(path: &str) -> String {
    let normalized = path.trim_matches('/');
    if normalized.is_empty() {
        ".".to_string()
    } else {
        normalized.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_hints_empty() {
        let hints = SubdirectoryHints::new();
        assert!(hints.is_empty());
        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_add_hint() {
        let mut hints = SubdirectoryHints::new();
        hints.add_hint("src/api", "Use REST conventions here");
        assert_eq!(hints.len(), 1);
        assert!(!hints.is_empty());
    }

    #[test]
    fn test_get_hints_for_dir() {
        let mut hints = SubdirectoryHints::new();
        hints.add_hint("src", "Root src hint");
        hints.add_hint("src/api", "API-specific hint");
        hints.add_hint("tests", "Test directory hint");

        let relevant = hints.get_hints_for_dir(Path::new("/some/path/src/api"));
        // Should match both "src" (parent) and "src/api" (exact)
        assert_eq!(relevant.len(), 2);

        let relevant2 = hints.get_hints_for_dir(Path::new("/some/path/src"));
        assert_eq!(relevant2.len(), 1);

        let relevant3 = hints.get_hints_for_dir(Path::new("/some/path/tests"));
        assert_eq!(relevant3.len(), 1);
    }

    #[test]
    fn test_build_hint_for_dir() {
        let mut hints = SubdirectoryHints::new();
        hints.add_hint("src", "Root src hint");

        let built = hints.build_hint_for_dir(Path::new("/path/src"));
        assert!(built.contains("Directory Context"));
        assert!(built.contains("Root src hint"));

        let empty = hints.build_hint_for_dir(Path::new("/path/other"));
        assert!(empty.is_empty());
    }

    #[test]
    fn test_remove_hint() {
        let mut hints = SubdirectoryHints::new();
        hints.add_hint("src", "test");
        assert_eq!(hints.len(), 1);

        hints.remove_hint("src");
        assert!(hints.is_empty());
    }

    #[test]
    fn test_normalize_dir() {
        assert_eq!(normalize_dir("/src/api/"), "src/api");
        assert_eq!(normalize_dir("src/api"), "src/api");
        assert_eq!(normalize_dir(""), ".");
        assert_eq!(normalize_dir("/"), ".");
    }

    #[test]
    fn test_clear() {
        let mut hints = SubdirectoryHints::new();
        hints.add_hint("a", "x");
        hints.add_hint("b", "y");
        hints.clear();
        assert!(hints.is_empty());
    }

    #[test]
    fn test_all_hints() {
        let mut hints = SubdirectoryHints::new();
        hints.add_hint("a", "x");
        hints.add_hint("b", "y");
        let all = hints.all_hints();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_hint_auto_discovered_flag() {
        let mut hints = SubdirectoryHints::new();
        hints.add_hint("src", "manual hint");
        let h = hints.get_hints_for_dir(Path::new("/src")).pop().unwrap();
        assert!(!h.auto_discovered);
    }
}
