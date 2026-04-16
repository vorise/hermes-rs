use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result};

/// A single memory entry stored as a markdown file.
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    /// Memory name (filename without .md).
    pub name: String,
    /// Memory content.
    pub content: String,
    /// Path to the memory file.
    pub path: PathBuf,
}

/// Type of memory as classified in MEMORY.md index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
}

impl std::str::FromStr for MemoryType {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "user" => Ok(MemoryType::User),
            "feedback" => Ok(MemoryType::Feedback),
            "project" => Ok(MemoryType::Project),
            "reference" => Ok(MemoryType::Reference),
            _ => Err(format!("Unknown memory type: {s}")),
        }
    }
}

/// A parsed entry from MEMORY.md index.
#[derive(Debug, Clone)]
pub struct MemoryIndexEntry {
    pub name: String,
    pub description: String,
    pub mem_type: MemoryType,
    pub file_path: String,
}

/// Manages persistent memories in ~/.hermes/memory/.
///
/// Memories are stored as individual markdown files and indexed by MEMORY.md.
pub struct MemoryManager {
    memory_dir: PathBuf,
}

impl MemoryManager {
    /// Create a new MemoryManager with the given memory directory.
    pub fn new(memory_dir: PathBuf) -> Self {
        Self { memory_dir }
    }

    /// Create a MemoryManager with the default path (~/.hermes/memory/).
    pub fn default_path() -> Result<Self> {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let memory_dir = home.join(".hermes").join("memory");
        Ok(Self::new(memory_dir))
    }

    /// Ensure the memory directory exists.
    pub fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.memory_dir)
            .with_context(|| format!("Failed to create memory dir: {:?}", self.memory_dir))?;
        Ok(())
    }

    /// Fetch all relevant memories for a query.
    ///
    /// Returns a concatenated string of all memory contents,
    /// suitable for injection into a system prompt.
    pub fn prefetch_all(&self, query: &str) -> Result<String> {
        if !self.memory_dir.exists() {
            return Ok(String::new());
        }

        let mut memories = self.get_memories()?;
        let query_lower = query.to_lowercase();

        // Score memories by relevance to the query
        memories.retain(|m| {
            let content_lower = m.content.to_lowercase();
            let name_lower = m.name.to_lowercase();
            name_lower.contains(&query_lower) || content_lower.contains(&query_lower)
        });

        if memories.is_empty() {
            // Return all memories if none match the query
            memories = self.get_memories()?;
        }

        let mut output = String::new();
        for memory in &memories {
            output.push_str(&format!("## {}\n\n{}\n\n", memory.name, memory.content));
        }
        Ok(output)
    }

    /// Save a new memory entry.
    pub fn save_memory(&self, name: &str, content: &str) -> Result<()> {
        self.ensure_dir()?;
        let sanitized = sanitize_filename(name);
        let path = self.memory_dir.join(format!("{sanitized}.md"));
        fs::write(&path, content)
            .with_context(|| format!("Failed to write memory: {}", path.display()))?;
        self.update_index()?;
        Ok(())
    }

    /// Get all memory entries.
    pub fn get_memories(&self) -> Result<Vec<MemoryEntry>> {
        if !self.memory_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.memory_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "md")
                && path.file_stem().map_or(true, |stem| stem != "MEMORY")
            {
                let content = fs::read_to_string(&path)?;
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                entries.push(MemoryEntry {
                    name,
                    content,
                    path,
                });
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// Delete a memory by name.
    pub fn delete_memory(&self, name: &str) -> Result<()> {
        let sanitized = sanitize_filename(name);
        let path = self.memory_dir.join(format!("{sanitized}.md"));
        if path.exists() {
            fs::remove_file(&path)?;
            self.update_index()?;
        }
        Ok(())
    }

    /// Clear all memories.
    pub fn clear_memories(&self) -> Result<()> {
        if !self.memory_dir.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&self.memory_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "md") {
                let _ = fs::remove_file(&path);
            }
        }
        Ok(())
    }

    /// Get a single memory by name.
    pub fn get_memory(&self, name: &str) -> Result<Option<MemoryEntry>> {
        let sanitized = sanitize_filename(name);
        let path = self.memory_dir.join(format!("{sanitized}.md"));
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            Ok(Some(MemoryEntry {
                name: name.to_string(),
                content,
                path,
            }))
        } else {
            Ok(None)
        }
    }

    /// Update the MEMORY.md index file.
    pub fn update_index(&self) -> Result<()> {
        let memories = self.get_memories()?;
        if memories.is_empty() {
            return Ok(());
        }

        let mut index = String::from("# Memory Index\n\n");
        index.push_str("<!-- Auto-generated by MemoryManager -->\n\n");
        for mem in &memories {
            // Try to extract type from frontmatter
            let mem_type = extract_memory_type(&mem.content);
            let type_label = match mem_type {
                Some(t) => format!("{t:?}"),
                None => "Unknown".to_string(),
            };
            let first_line = mem
                .content
                .lines()
                .find(|l| !l.starts_with("---") && !l.is_empty())
                .unwrap_or("No content")
                .to_string();
            index.push_str(&format!(
                "- [{}]({}.md) — [{type_label}] {first_line}\n",
                mem.name, mem.name
            ));
        }

        let index_path = self.memory_dir.join("MEMORY.md");
        fs::write(&index_path, index)?;
        Ok(())
    }

    /// Parse the MEMORY.md index file.
    pub fn parse_index(&self) -> Result<Vec<MemoryIndexEntry>> {
        let index_path = self.memory_dir.join("MEMORY.md");
        if !index_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&index_path)?;
        let mut entries = Vec::new();

        for line in content.lines() {
            if line.starts_with("- [") {
                if let Some(entry) = parse_memory_index_line(line) {
                    entries.push(entry);
                }
            }
        }

        Ok(entries)
    }

    /// Check if memory nudge should be triggered.
    ///
    /// Returns true if the number of turns since last memory review
    /// exceeds the nudge interval.
    pub fn should_nudge(&self, turns_since_review: u32, interval: u32) -> bool {
        interval > 0 && turns_since_review >= interval
    }

    /// Generate a nudge prompt for memory review.
    pub fn nudge_prompt(&self) -> String {
        let count = self.get_memories().map(|m| m.len()).unwrap_or(0);
        format!(
            "You have {count} memory entries. Consider reviewing and consolidating them."
        )
    }
}

/// Sanitize a filename for use as a memory name.
fn sanitize_filename(name: &str) -> String {
    name.replace('/', "_").replace('\\', "_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .collect::<String>()
        .to_lowercase()
        .replace(' ', "_")
}

/// Extract memory type from frontmatter.
fn extract_memory_type(content: &str) -> Option<MemoryType> {
    // Look for "type: XXX" in frontmatter
    if let Some(start) = content.find("type:") {
        let rest = &content[start + 5..];
        let end = rest.find('\n').unwrap_or(rest.len());
        let type_str = rest[..end].trim();
        MemoryType::from_str(type_str).ok()
    } else {
        None
    }
}

/// Parse a single line from MEMORY.md index.
fn parse_memory_index_line(line: &str) -> Option<MemoryIndexEntry> {
    // Format: - [Title](file.md) — [Type] description
    let line = line.strip_prefix("- [")?;
    let name_end = line.find("](")?;
    let name = &line[..name_end];

    let file_start = name_end + 2;
    let file_end = line[file_start..].find(")")?;
    let file_path = &line[file_start..file_start + file_end];

    let rest = &line[file_start + file_end..];
    let (description, mem_type) = if let Some(type_start) = rest.find("— [") {
        let type_content = &rest[type_start + 3..];
        let type_end = type_content.find(']')?;
        let type_str = type_content[..type_end].to_lowercase();
        let mem_type = MemoryType::from_str(&type_str).ok();
        let desc = type_content[type_end + 1..].trim().to_string();
        (desc, mem_type)
    } else {
        (rest.trim_start_matches('—').trim().to_string(), None)
    };

    Some(MemoryIndexEntry {
        name: name.to_string(),
        description,
        mem_type: mem_type.unwrap_or(MemoryType::User),
        file_path: file_path.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_memory_dir() -> PathBuf {
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let dir = std::env::temp_dir().join(format!("hermes_memory_{id}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn test_save_and_get_memory() {
        let dir = temp_memory_dir();
        let manager = MemoryManager::new(dir.clone());
        manager.save_memory("test", "This is a test memory").unwrap();

        let memories = manager.get_memories().unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].name, "test");
        assert!(memories[0].content.contains("test memory"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_delete_memory() {
        let dir = temp_memory_dir();
        let manager = MemoryManager::new(dir.clone());
        manager.save_memory("test", "content").unwrap();
        assert_eq!(manager.get_memories().unwrap().len(), 1);

        manager.delete_memory("test").unwrap();
        assert_eq!(manager.get_memories().unwrap().len(), 0);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_clear_memories() {
        let dir = temp_memory_dir();
        let manager = MemoryManager::new(dir.clone());
        manager.save_memory("one", "first").unwrap();
        manager.save_memory("two", "second").unwrap();

        manager.clear_memories().unwrap();
        assert_eq!(manager.get_memories().unwrap().len(), 0);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_get_memory_missing() {
        let dir = temp_memory_dir();
        let manager = MemoryManager::new(dir);
        let result = manager.get_memory("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_prefetch_all_empty() {
        let dir = temp_memory_dir();
        let manager = MemoryManager::new(dir);
        let result = manager.prefetch_all("anything").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_prefetch_all_with_content() {
        let dir = temp_memory_dir();
        let manager = MemoryManager::new(dir.clone());
        manager.save_memory("user_pref", "User prefers concise responses").unwrap();
        manager.save_memory("project_info", "This is a Rust project using Tokio").unwrap();

        let result = manager.prefetch_all("user").unwrap();
        assert!(result.contains("user_pref"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_should_nudge() {
        let dir = temp_memory_dir();
        let manager = MemoryManager::new(dir);
        assert!(manager.should_nudge(10, 5));
        assert!(!manager.should_nudge(3, 5));
        assert!(!manager.should_nudge(10, 0));
    }

    #[test]
    fn test_nudge_prompt() {
        let dir = temp_memory_dir();
        let manager = MemoryManager::new(dir.clone());
        manager.save_memory("test", "content").unwrap();
        let prompt = manager.nudge_prompt();
        assert!(prompt.contains("1 memory"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("hello world"), "hello_world");
        assert_eq!(sanitize_filename("Hello_World"), "hello_world");
        assert_eq!(sanitize_filename("test/unsafe"), "test_unsafe");
    }

    #[test]
    fn test_extract_memory_type() {
        let content = "---\nname: test\ntype: feedback\ndescription: test\n---\ncontent here";
        let result = extract_memory_type(content);
        assert_eq!(result, Some(MemoryType::Feedback));
    }

    #[test]
    fn test_parse_memory_index_line() {
        let line = "- [Title](file.md) — [user] This is the description";
        let entry = parse_memory_index_line(line).unwrap();
        assert_eq!(entry.name, "Title");
        assert_eq!(entry.file_path, "file.md");
        assert_eq!(entry.mem_type, MemoryType::User);
        assert!(entry.description.contains("This is the description"));
    }
}
