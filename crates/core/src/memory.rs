//! Memory System
//!
//! Persistent memory storage for Hermes agent context.
//! Memories are stored as markdown files in ~/.hermes/memory/

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::fs;
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::home::memory_dir;

/// Memory entry representing a single memory file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Memory name (filename without .md).
    pub name: String,

    /// Memory description (from frontmatter).
    pub description: String,

    /// Memory type (from frontmatter: user, feedback, project, reference).
    #[serde(rename = "type")]
    pub memory_type: String,

    /// Memory content (markdown body).
    pub content: String,

    /// File path.
    pub path: PathBuf,

    /// Last modified timestamp.
    pub modified: Option<u64>,
}

impl MemoryEntry {
    /// Load memory from file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .context("Failed to read memory file")?;

        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .replace(".md", "");

        // Parse frontmatter
        let (frontmatter, body) = parse_memory_frontmatter(&content)?;

        let description = frontmatter.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or(&name)
            .to_string();

        let memory_type = frontmatter.get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("user")
            .to_string();

        let modified = fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());

        Ok(Self {
            name,
            description,
            memory_type,
            content: body,
            path: path.to_path_buf(),
            modified,
        })
    }

    /// Save memory to file.
    pub fn save(&self) -> Result<()> {
        let dir = memory_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }

        let path = dir.join(format!("{}.md", self.name));

        // Build content with frontmatter
        let content = format!(
            "---\nname: {}\ndescription: {}\ntype: {}\n---\n\n{}",
            self.name, self.description, self.memory_type, self.content
        );

        fs::write(&path, content)
            .context("Failed to write memory file")?;

        info!("Saved memory: {}", self.name);
        Ok(())
    }

    /// Delete memory file.
    pub fn delete(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)
                .context("Failed to delete memory file")?;
            info!("Deleted memory: {}", self.name);
        }
        Ok(())
    }
}

/// Parse memory file frontmatter.
fn parse_memory_frontmatter(content: &str) -> Result<(HashMap<String, serde_json::Value>, String)> {
    // Check for YAML frontmatter
    if !content.starts_with("---") {
        return Ok((HashMap::new(), content.to_string()));
    }

    // Find the closing ---
    let end_marker = content[3..].find("---");
    if end_marker.is_none() {
        return Ok((HashMap::new(), content.to_string()));
    }

    let frontmatter_str = &content[3..end_marker.unwrap() + 3];
    let body = &content[end_marker.unwrap() + 6..];

    // Parse YAML frontmatter (simple key: value parsing)
    let mut frontmatter = HashMap::new();

    for line in frontmatter_str.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            // Try to parse as JSON value
            let json_value = if value.starts_with('"') && value.ends_with('"') {
                serde_json::Value::String(value[1..value.len()-1].to_string())
            } else if value == "true" {
                serde_json::Value::Bool(true)
            } else if value == "false" {
                serde_json::Value::Bool(false)
            } else if let Ok(n) = value.parse::<i64>() {
                serde_json::Value::Number(n.into())
            } else if let Ok(n) = value.parse::<f64>() {
                serde_json::Number::from_f64(n)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::String(value.to_string()))
            } else {
                serde_json::Value::String(value.to_string())
            };

            frontmatter.insert(key.to_string(), json_value);
        }
    }

    Ok((frontmatter, body.trim().to_string()))
}

/// Memory index from MEMORY.md.
#[derive(Debug, Clone, Default)]
pub struct MemoryIndex {
    /// Memory entries by name.
    entries: HashMap<String, MemoryEntry>,
}

impl MemoryIndex {
    /// Create empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load all memories from directory.
    pub fn load_all(&mut self) -> Result<()> {
        let dir = memory_dir();
        if !dir.exists() {
            debug!("Memory directory does not exist: {}", dir.display());
            return Ok(());
        }

        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                // Skip MEMORY.md index file
                if path.file_name().and_then(|n| n.to_str()) == Some("MEMORY.md") {
                    continue;
                }

                if let Ok(memory) = MemoryEntry::from_file(&path) {
                    let name = memory.name.clone();
                    self.entries.insert(name.clone(), memory);
                    debug!("Loaded memory: {}", name);
                }
            }
        }

        info!("Loaded {} memories", self.entries.len());
        Ok(())
    }

    /// Get all memories.
    pub fn get_all(&self) -> Vec<&MemoryEntry> {
        self.entries.values().collect()
    }

    /// Get memory by name.
    pub fn get(&self, name: &str) -> Option<&MemoryEntry> {
        self.entries.get(name)
    }

    /// Get memory mutably by name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut MemoryEntry> {
        self.entries.get_mut(name)
    }

    /// Add or update memory.
    pub fn insert(&mut self, memory: MemoryEntry) {
        self.entries.insert(memory.name.clone(), memory);
    }

    /// Remove memory.
    pub fn remove(&mut self, name: &str) -> Option<MemoryEntry> {
        self.entries.remove(name)
    }

    /// Count memories.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Clear all memories.
    pub fn clear(&mut self) -> Result<()> {
        for memory in self.entries.values() {
            memory.delete()?;
        }
        self.entries.clear();
        info!("Cleared all memories");
        Ok(())
    }
}

/// Memory manager for loading, saving, and querying memories.
#[derive(Debug)]
pub struct MemoryManager {
    /// Memory directory.
    memory_dir: PathBuf,

    /// Memory index.
    index: MemoryIndex,
}

impl MemoryManager {
    /// Create new memory manager.
    pub fn new() -> Self {
        Self {
            memory_dir: memory_dir(),
            index: MemoryIndex::new(),
        }
    }

    /// Create with custom directory.
    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            memory_dir: dir,
            index: MemoryIndex::new(),
        }
    }

    /// Load all memories.
    pub fn load(&mut self) -> Result<()> {
        self.index.load_all()
    }

    /// Prefetch memories relevant to a query.
    ///
    /// Returns concatenated content of relevant memories.
    pub fn prefetch_all(&self, query: &str) -> Result<String> {
        let memories = self.index.get_all();

        // Simple relevance scoring based on keyword matching
        let scored: Vec<(f64, &MemoryEntry)> = memories
            .iter()
            .map(|m| (relevance_score(query, m), *m))
            .filter(|(score, _)| *score > 0.0)
            .collect();

        // Sort by relevance (descending)
        let mut sorted = scored;
        sorted.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Limit to top N memories (prevent context overflow)
        let top_memories: Vec<&MemoryEntry> = sorted
            .iter()
            .take(10)
            .map(|(_, m)| *m)
            .collect();

        if top_memories.is_empty() {
            return Ok(String::new());
        }

        // Build context string
        let mut context = String::from("# Relevant Memories\n\n");

        let count = top_memories.len();
        for memory in top_memories {
            context.push_str(&format!("## {}\n{}\n\n", memory.name, memory.content));
        }

        debug!("Prefetched {} relevant memories for query", count);
        Ok(context)
    }

    /// Save a new memory.
    pub fn save_memory(&mut self, name: &str, content: &str) -> Result<()> {
        let memory = MemoryEntry {
            name: name.to_string(),
            description: name.to_string(),
            memory_type: "user".to_string(),
            content: content.to_string(),
            path: self.memory_dir.join(format!("{}.md", name)),
            modified: None,
        };

        memory.save()?;
        self.index.insert(memory);
        Ok(())
    }

    /// Save memory with full metadata.
    pub fn save_memory_with_meta(
        &mut self,
        name: &str,
        description: &str,
        memory_type: &str,
        content: &str,
    ) -> Result<()> {
        let memory = MemoryEntry {
            name: name.to_string(),
            description: description.to_string(),
            memory_type: memory_type.to_string(),
            content: content.to_string(),
            path: self.memory_dir.join(format!("{}.md", name)),
            modified: None,
        };

        memory.save()?;
        self.index.insert(memory);
        Ok(())
    }

    /// Get all memories.
    pub fn get_memories(&self) -> Vec<&MemoryEntry> {
        self.index.get_all()
    }

    /// Get memory by name.
    pub fn get_memory(&self, name: &str) -> Option<&MemoryEntry> {
        self.index.get(name)
    }

    /// Update existing memory.
    pub fn update_memory(&mut self, name: &str, content: &str) -> Result<()> {
        let memory = self.index.get_mut(name)
            .context("Memory not found")?;

        memory.content = content.to_string();
        memory.save()?;
        Ok(())
    }

    /// Delete memory.
    pub fn delete_memory(&mut self, name: &str) -> Result<()> {
        let memory = self.index.remove(name)
            .context("Memory not found")?;

        memory.delete()?;
        Ok(())
    }

    /// Clear all memories.
    pub fn clear_memories(&mut self) -> Result<()> {
        self.index.clear()
    }

    /// Count memories.
    pub fn count(&self) -> usize {
        self.index.count()
    }
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate relevance score between query and memory.
fn relevance_score(query: &str, memory: &MemoryEntry) -> f64 {
    let query_lower = query.to_lowercase();
    let content_lower = memory.content.to_lowercase();
    let name_lower = memory.name.to_lowercase();
    let desc_lower = memory.description.to_lowercase();

    let mut score = 0.0;

    // Split query into words
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    let query_len = query_words.len().max(1);

    for word in &query_words {
        // Name match (highest weight)
        if name_lower.contains(word) {
            score += 3.0;
        }

        // Description match
        if desc_lower.contains(word) {
            score += 2.0;
        }

        // Content match
        if content_lower.contains(word) {
            score += 1.0;
        }
    }

    // Normalize by query length
    score / query_len as f64
}

/// Memory nudge configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NudgeConfig {
    /// Interval between nudges (in turns).
    pub interval_turns: u32,

    /// Message template.
    pub message_template: String,

    /// Maximum memories to suggest.
    pub max_suggestions: usize,
}

impl Default for NudgeConfig {
    fn default() -> Self {
        Self {
            interval_turns: 50,
            message_template: "Consider saving important context as memory. What have you learned that might be useful later?".to_string(),
            max_suggestions: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let content = "---\nname: test\ndescription: Test memory\ntype: user\n---\n\nTest content";
        let (fm, body) = parse_memory_frontmatter(content).unwrap();
        assert_eq!(fm.get("name").unwrap().as_str().unwrap(), "test");
        assert_eq!(body, "Test content");
    }

    #[test]
    fn test_parse_frontmatter_empty() {
        let content = "No frontmatter here";
        let (fm, body) = parse_memory_frontmatter(content).unwrap();
        assert!(fm.is_empty());
        assert_eq!(body, content);
    }

    #[test]
    fn test_relevance_score() {
        let memory = MemoryEntry {
            name: "python_patterns".to_string(),
            description: "Python coding patterns".to_string(),
            memory_type: "user".to_string(),
            content: "Common Python patterns for async programming".to_string(),
            path: PathBuf::from("test.md"),
            modified: None,
        };

        let score = relevance_score("python async", &memory);
        assert!(score > 0.0);
    }

    #[test]
    fn test_memory_entry_creation() {
        let entry = MemoryEntry {
            name: "test".to_string(),
            description: "Test".to_string(),
            memory_type: "user".to_string(),
            content: "Content".to_string(),
            path: PathBuf::from("test.md"),
            modified: None,
        };

        assert_eq!(entry.name, "test");
        assert_eq!(entry.memory_type, "user");
    }

    #[test]
    fn test_memory_manager_new() {
        let manager = MemoryManager::new();
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_nudge_config_default() {
        let config = NudgeConfig::default();
        assert_eq!(config.interval_turns, 50);
        assert!(!config.message_template.is_empty());
    }
}