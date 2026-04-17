use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

/// A reference to a context file that should be included in the system prompt.
///
/// These are automatically discovered files like AGENTS.md, .cursorrules,
/// SOUL.md, CLAUDE.md, etc. that influence the agent's behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextReference {
    /// Absolute path to the file.
    pub path: PathBuf,
    /// File name (for display and matching).
    pub name: String,
    /// Whether this file currently exists on disk.
    pub exists: bool,
    /// SHA-256 hash of the file contents (for change detection).
    pub hash: String,
    /// Last modified timestamp (UNIX epoch seconds).
    pub mtime: u64,
    /// Whether this reference is currently cached.
    pub cached: bool,
    /// The cached content (if cached and valid).
    pub content: Option<String>,
}

impl ContextReference {
    pub fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let (exists, hash, mtime, content) = Self::read_file(&path);

        Self {
            path,
            name,
            exists,
            hash,
            mtime,
            cached: content.is_some(),
            content,
        }
    }

    fn read_file(path: &Path) -> (bool, String, u64, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let hash = compute_hash(&content);
                let mtime = Self::file_mtime(path);
                (true, hash, mtime, Some(content))
            }
            Err(_) => (false, String::new(), 0, None),
        }
    }

    fn file_mtime(path: &Path) -> u64 {
        match std::fs::metadata(path) {
            Ok(meta) => {
                match meta.modified() {
                    Ok(time) => time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                    Err(_) => 0,
                }
            }
            Err(_) => 0,
        }
    }

    /// Check if the file has changed since last cached read.
    pub fn is_modified(&self) -> bool {
        let current_mtime = Self::file_mtime(&self.path);
        current_mtime != self.mtime
    }

    /// Refresh the content from disk.
    pub fn refresh(&mut self) {
        let (exists, hash, mtime, content) = Self::read_file(&self.path);
        self.exists = exists;
        self.hash = hash;
        self.mtime = mtime;
        self.cached = content.is_some();
        self.content = content;
    }

    /// Get the content, refreshing if necessary.
    pub fn get_content(&mut self) -> Option<&str> {
        if self.is_modified() {
            self.refresh();
        }
        self.content.as_deref()
    }
}

/// Known context file names that influence agent behavior.
pub const KNOWN_CONTEXT_FILES: &[&str] = &[
    "AGENTS.md",
    ".cursorrules",
    "CLAUDE.md",
    "SOUL.md",
    "GEMINI.md",
    ".github/copilot-instructions.md",
];

/// Discover context files in a directory.
pub fn discover_context_files(dir: &Path) -> Vec<ContextReference> {
    let mut refs = Vec::new();

    for name in KNOWN_CONTEXT_FILES {
        let path = dir.join(name);
        if path.exists() {
            refs.push(ContextReference::new(path));
        }
    }

    refs
}

/// Compute a simple hash of content.
fn compute_hash(content: &str) -> String {
    // Simple hash — not cryptographic, just for change detection
    let mut hash: u64 = 0;
    for (i, b) in content.bytes().enumerate() {
        hash = hash.wrapping_add((b as u64).wrapping_mul((i as u64).wrapping_add(1)));
        hash = hash.rotate_left(7);
    }
    format!("{hash:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_reference_nonexistent() {
        let ref_ = ContextReference::new(PathBuf::from("/nonexistent/path"));
        assert!(!ref_.exists);
        assert!(ref_.content.is_none());
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let h1 = compute_hash("hello");
        let h2 = compute_hash("hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_hash_different() {
        let h1 = compute_hash("hello");
        let h2 = compute_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_discover_context_files_empty() {
        let dir = PathBuf::from("/tmp/hermes-test-no-context");
        // Ensure it's an empty dir (or non-existent)
        let refs = discover_context_files(&dir);
        assert!(refs.is_empty());
    }
}
