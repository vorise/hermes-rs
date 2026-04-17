use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::context_references::{ContextReference, discover_context_files};
use crate::memory::MemoryManager;
use crate::session::SearchResult;
use crate::session_db::SessionDB;

/// The context engine coordinates all context-related operations:
/// - Context file discovery and caching
/// - Memory integration
/// - Session history
/// - Context window management
/// - Change detection for cached files
///
/// This is the high-level coordinator that the prompt builder,
/// context compressor, and session routing all interact with.
pub struct ContextEngine {
    /// Working directory context references.
    working_refs: Mutex<HashMap<String, ContextReference>>,
    /// Global (home directory) context references.
    global_refs: Mutex<HashMap<String, ContextReference>>,
    /// Memory manager instance.
    memory: Arc<Mutex<Option<MemoryManager>>>,
    /// Session database handle.
    session_db: Arc<Mutex<Option<SessionDB>>>,
    /// Working directory being tracked.
    working_dir: PathBuf,
    /// Whether context files have changed since last build.
    dirty: Mutex<bool>,
}

impl ContextEngine {
    pub fn new(working_dir: &Path) -> Self {
        Self {
            working_refs: Mutex::new(HashMap::new()),
            global_refs: Mutex::new(HashMap::new()),
            memory: Arc::new(Mutex::new(None)),
            session_db: Arc::new(Mutex::new(None)),
            working_dir: working_dir.to_path_buf(),
            dirty: Mutex::new(false),
        }
    }

    /// Initialize the context engine: discover files, set up caches.
    pub fn init(&self) -> Vec<String> {
        let mut discovered = Vec::new();

        // Discover working directory context files
        let working = discover_context_files(&self.working_dir);
        {
            let mut refs = self.working_refs.lock();
            for ref_ in working {
                let name = ref_.name.clone();
                refs.insert(name.clone(), ref_);
                discovered.push(format!("working: {name}"));
            }
        }

        // Discover global context files (from ~/.hermes)
        if let Ok(home) = std::env::var("HOME") {
            let hermes_dir = PathBuf::from(home).join(".hermes");
            if hermes_dir.exists() {
                let global = discover_context_files(&hermes_dir);
                let mut refs = self.global_refs.lock();
                for ref_ in global {
                    let name = ref_.name.clone();
                    refs.insert(name.clone(), ref_);
                    discovered.push(format!("global: {name}"));
                }
            }
        }

        discovered
    }

    /// Check if any context files have been modified since last check.
    pub fn check_changes(&self) -> Vec<String> {
        let mut changed = Vec::new();

        {
            let mut refs = self.working_refs.lock();
            for (name, ref_) in refs.iter_mut() {
                if ref_.is_modified() {
                    ref_.refresh();
                    changed.push(name.clone());
                }
            }
        }

        {
            let mut refs = self.global_refs.lock();
            for (name, ref_) in refs.iter_mut() {
                if ref_.is_modified() {
                    ref_.refresh();
                    changed.push(format!("global:{name}"));
                }
            }
        }

        if !changed.is_empty() {
            *self.dirty.lock() = true;
        }

        changed
    }

    /// Get all context file contents as a formatted prompt section.
    pub fn context_files_prompt(&self) -> String {
        let mut sections = Vec::new();

        {
            let refs = self.working_refs.lock();
            for (name, ref_) in refs.iter() {
                if let Some(ref content) = ref_.content {
                    if !content.is_empty() {
                        sections.push(format!("### {name}\n```\n{content}\n```"));
                    }
                }
            }
        }

        {
            let refs = self.global_refs.lock();
            for (name, ref_) in refs.iter() {
                if let Some(ref content) = ref_.content {
                    if !content.is_empty() {
                        sections.push(format!("### {name} (global)\n```\n{content}\n```"));
                    }
                }
            }
        }

        sections.join("\n\n---\n\n")
    }

    /// Get context file content by name.
    pub fn get_context_file(&self, name: &str) -> Option<String> {
        // Check working refs first
        if let Some(ref_) = self.working_refs.lock().get(name) {
            return ref_.content.clone();
        }
        // Then global refs
        if let Some(ref_) = self.global_refs.lock().get(name) {
            return ref_.content.clone();
        }
        None
    }

    /// List all discovered context files.
    pub fn list_context_files(&self) -> Vec<String> {
        let mut files = Vec::new();

        let refs = self.working_refs.lock();
        for name in refs.keys() {
            files.push(format!("[working] {name}"));
        }

        let refs = self.global_refs.lock();
        for name in refs.keys() {
            files.push(format!("[global] {name}"));
        }

        files
    }

    /// Manually add a context file by path.
    pub fn add_context_file(&self, path: &Path) -> bool {
        if !path.exists() {
            return false;
        }

        let ref_ = ContextReference::new(path.to_path_buf());
        let name = ref_.name.clone();
        self.working_refs.lock().insert(name.clone(), ref_);
        *self.dirty.lock() = true;
        true
    }

    /// Remove a context file from tracking.
    pub fn remove_context_file(&self, name: &str) -> bool {
        let removed = self.working_refs.lock().remove(name).is_some()
            || self.global_refs.lock().remove(name).is_some();
        if removed {
            *self.dirty.lock() = true;
        }
        removed
    }

    /// Set the memory manager.
    pub fn set_memory(&self, memory: MemoryManager) {
        *self.memory.lock() = Some(memory);
    }

    /// Set the session database.
    pub fn set_session_db(&self, db: SessionDB) {
        *self.session_db.lock() = Some(db);
    }

    /// Get memory guidance prompt (if memory is configured).
    pub fn memory_prompt(&self) -> Option<String> {
        let memory = self.memory.lock();
        memory.as_ref().and_then(|m| {
            let memories = m.get_memories().ok()?;
            if memories.is_empty() {
                return None;
            }
            let lines: Vec<String> = memories.iter().take(10).map(|mem| {
                format!("- {}: {}", mem.name, mem.content)
            }).collect();
            Some(format!("### Memories\n{}\n", lines.join("\n")))
        })
    }

    /// Search sessions for relevant context (if DB is configured).
    pub fn search_sessions(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let db = self.session_db.lock();
        match db.as_ref() {
            Some(db) => {
                let _ = limit; // FTS5 has its own LIMIT
                db.search_sessions(query, None).unwrap_or_default()
            }
            None => Vec::new(),
        }
    }

    /// Check if the prompt needs rebuilding (context changed).
    pub fn is_dirty(&self) -> bool {
        *self.dirty.lock()
    }

    /// Mark the context as clean (after rebuilding the prompt).
    pub fn mark_clean(&self) {
        *self.dirty.lock() = false;
    }

    /// Get the number of discovered context files.
    pub fn context_file_count(&self) -> usize {
        self.working_refs.lock().len() + self.global_refs.lock().len()
    }
}

/// Builder for the context engine.
pub struct ContextEngineBuilder {
    working_dir: PathBuf,
    memory: Option<MemoryManager>,
    session_db: Option<SessionDB>,
}

impl ContextEngineBuilder {
    pub fn new() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_default(),
            memory: None,
            session_db: None,
        }
    }

    pub fn with_working_dir(mut self, dir: &Path) -> Self {
        self.working_dir = dir.to_path_buf();
        self
    }

    pub fn with_memory(mut self, memory: MemoryManager) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_session_db(mut self, db: SessionDB) -> Self {
        self.session_db = Some(db);
        self
    }

    pub fn build(self) -> Arc<ContextEngine> {
        let engine = Arc::new(ContextEngine::new(&self.working_dir));

        if let Some(memory) = self.memory {
            engine.set_memory(memory);
        }

        if let Some(db) = self.session_db {
            engine.set_session_db(db);
        }

        engine.init();
        engine
    }
}

impl Default for ContextEngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_engine_new() {
        let engine = ContextEngine::new(Path::new("/tmp"));
        assert_eq!(engine.working_dir, PathBuf::from("/tmp"));
        assert!(!engine.is_dirty());
    }

    #[test]
    fn test_context_engine_init_empty() {
        let engine = ContextEngine::new(Path::new("/tmp/hermes-test-no-context"));
        let discovered = engine.init();
        // Should be empty since no context files exist there
        assert!(discovered.is_empty());
        assert_eq!(engine.context_file_count(), 0);
    }

    #[test]
    fn test_context_engine_is_clean() {
        let engine = ContextEngine::new(Path::new("/tmp"));
        assert!(!engine.is_dirty());
        engine.mark_clean();
        assert!(!engine.is_dirty());
    }

    #[test]
    fn test_context_engine_list_empty() {
        let engine = ContextEngine::new(Path::new("/tmp"));
        engine.init();
        let files = engine.list_context_files();
        assert!(files.is_empty());
    }

    #[test]
    fn test_context_engine_get_nonexistent() {
        let engine = ContextEngine::new(Path::new("/tmp"));
        engine.init();
        assert!(engine.get_context_file("NONEXISTENT.md").is_none());
    }

    #[test]
    fn test_context_engine_context_files_prompt_empty() {
        let engine = ContextEngine::new(Path::new("/tmp"));
        engine.init();
        let prompt = engine.context_files_prompt();
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_context_engine_remove_nonexistent() {
        let engine = ContextEngine::new(Path::new("/tmp"));
        engine.init();
        assert!(!engine.remove_context_file("NONEXISTENT.md"));
    }

    #[test]
    fn test_context_engine_builder_default() {
        let engine = ContextEngineBuilder::new().build();
        assert!(engine.context_file_count() == 0);
    }

    #[test]
    fn test_context_engine_builder_with_dir() {
        let engine = ContextEngineBuilder::new()
            .with_working_dir(Path::new("/tmp"))
            .build();
        assert_eq!(engine.working_dir, PathBuf::from("/tmp"));
    }

    #[test]
    fn test_context_engine_default_builder() {
        let _engine = ContextEngineBuilder::default().build();
    }
}
