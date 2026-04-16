use std::time::Duration;

use parking_lot::Mutex;
use std::collections::HashMap;

/// A stored tool result that persists across turns.
#[derive(Debug, Clone)]
pub struct StoredToolResult {
    /// Tool call ID.
    pub tool_call_id: String,
    /// Tool name.
    pub tool_name: String,
    /// The result content.
    pub content: String,
    /// Whether the result was an error.
    pub is_error: bool,
    /// Turn number when this was stored.
    pub turn: u32,
    /// Timestamp (seconds since epoch).
    pub stored_at: f64,
}

/// Stores and retrieves tool results across turns.
///
/// When a tool result is too long to include inline, it's stored here
/// and referenced by ID in the conversation. Results are pruned based
/// on a turn budget.
pub struct ToolResultStorage {
    store: Mutex<HashMap<String, StoredToolResult>>,
    /// Maximum number of turns to retain results.
    max_turns_retention: u32,
    /// Maximum total stored results.
    max_results: usize,
}

impl ToolResultStorage {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
            max_turns_retention: 20,
            max_results: 100,
        }
    }

    /// Set the maximum number of turns to retain results.
    pub fn set_max_turns_retention(&mut self, n: u32) {
        self.max_turns_retention = n;
    }

    /// Set the maximum total stored results.
    pub fn set_max_results(&mut self, n: usize) {
        self.max_results = n;
    }

    /// Store a tool result for later retrieval.
    pub fn store(&self, tool_call_id: String, tool_name: String, content: String, is_error: bool, turn: u32) {
        let mut store = self.store.lock();

        // Enforce max results: evict oldest if at capacity
        if store.len() >= self.max_results {
            // Remove the oldest entry (lowest turn)
            if let Some(oldest_key) = store
                .iter()
                .min_by_key(|(_, v)| v.turn)
                .map(|(k, _)| k.clone())
            {
                store.remove(&oldest_key);
            }
        }

        let stored_at = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
        store.insert(
            tool_call_id.clone(),
            StoredToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                turn,
                stored_at,
            },
        );
    }

    /// Retrieve a stored tool result by ID.
    pub fn get(&self, tool_call_id: &str) -> Option<StoredToolResult> {
        self.store.lock().get(tool_call_id).cloned()
    }

    /// Get a reference string for a stored result (for inline insertion).
    pub fn get_reference(&self, tool_call_id: &str) -> Option<String> {
        self.get(tool_call_id).map(|r| {
            let preview = if r.content.len() > 100 {
                format!("{}...", &r.content[..100])
            } else {
                r.content.clone()
            };
            format!("[Tool result from {} (turn {}): {}]", r.tool_name, r.turn, preview)
        })
    }

    /// Clean up results older than the retention window.
    pub fn cleanup(&self, current_turn: u32) {
        let mut store = self.store.lock();
        let cutoff = current_turn.saturating_sub(self.max_turns_retention);
        store.retain(|_, v| v.turn >= cutoff);
    }

    /// Get the number of stored results.
    pub fn len(&self) -> usize {
        self.store.lock().len()
    }

    /// Check if storage is empty.
    pub fn is_empty(&self) -> bool {
        self.store.lock().is_empty()
    }

    /// Get all tool call IDs stored.
    pub fn all_ids(&self) -> Vec<String> {
        self.store.lock().keys().cloned().collect()
    }

    /// Get results for a specific turn.
    pub fn get_by_turn(&self, turn: u32) -> Vec<StoredToolResult> {
        self.store
            .lock()
            .values()
            .filter(|v| v.turn == turn)
            .cloned()
            .collect()
    }

    /// Truncate content to a max length for display.
    pub fn truncate_content(content: &str, max_length: usize) -> String {
        if content.len() <= max_length {
            content.to_string()
        } else {
            format!("{}... [truncated, {} chars total]", &content[..max_length], content.len())
        }
    }
}

impl Default for ToolResultStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_get() {
        let storage = ToolResultStorage::new();
        storage.store(
            "call-1".to_string(),
            "read_file".to_string(),
            "file contents here".to_string(),
            false,
            0,
        );

        let result = storage.get("call-1").unwrap();
        assert_eq!(result.tool_call_id, "call-1");
        assert_eq!(result.tool_name, "read_file");
        assert_eq!(result.content, "file contents here");
        assert!(!result.is_error);
        assert_eq!(result.turn, 0);
    }

    #[test]
    fn test_get_reference() {
        let storage = ToolResultStorage::new();
        let long_content = "a".repeat(200);
        storage.store(
            "call-1".to_string(),
            "read_file".to_string(),
            long_content,
            false,
            5,
        );

        let reference = storage.get_reference("call-1").unwrap();
        assert!(reference.contains("read_file"));
        assert!(reference.contains("turn 5"));
        assert!(reference.contains("truncated"));
    }

    #[test]
    fn test_cleanup_removes_old() {
        let storage = ToolResultStorage::new();
        storage.set_max_turns_retention(5);

        storage.store("old-1".to_string(), "tool".to_string(), "old".to_string(), false, 0);
        storage.store("old-2".to_string(), "tool".to_string(), "old".to_string(), false, 1);
        storage.store("new-1".to_string(), "tool".to_string(), "new".to_string(), false, 10);

        storage.cleanup(12); // cutoff = 7

        assert!(storage.get("old-1").is_none());
        assert!(storage.get("old-2").is_none());
        assert!(storage.get("new-1").is_some());
    }

    #[test]
    fn test_max_results_enforcement() {
        let mut storage = ToolResultStorage::new();
        storage.set_max_results(3);

        storage.store("a".to_string(), "t".to_string(), "a".to_string(), false, 0);
        storage.store("b".to_string(), "t".to_string(), "b".to_string(), false, 1);
        storage.store("c".to_string(), "t".to_string(), "c".to_string(), false, 2);
        storage.store("d".to_string(), "t".to_string(), "d".to_string(), false, 3);

        assert_eq!(storage.len(), 3);
        // Oldest (turn 0) should have been evicted
        assert!(storage.get("a").is_none());
        assert!(storage.get("d").is_some());
    }

    #[test]
    fn test_truncate_content() {
        let short = "hello";
        assert_eq!(ToolResultStorage::truncate_content(short, 10), "hello");

        let long = "a".repeat(50);
        let truncated = ToolResultStorage::truncate_content(&long, 10);
        assert!(truncated.contains("truncated"));
        assert!(truncated.contains("50 chars total"));
    }

    #[test]
    fn test_get_by_turn() {
        let storage = ToolResultStorage::new();
        storage.store("a".to_string(), "t".to_string(), "a".to_string(), false, 0);
        storage.store("b".to_string(), "t".to_string(), "b".to_string(), false, 0);
        storage.store("c".to_string(), "t".to_string(), "c".to_string(), false, 1);

        let turn0 = storage.get_by_turn(0);
        assert_eq!(turn0.len(), 2);

        let turn1 = storage.get_by_turn(1);
        assert_eq!(turn1.len(), 1);
    }

    #[test]
    fn test_error_result() {
        let storage = ToolResultStorage::new();
        storage.store(
            "call-err".to_string(),
            "terminal".to_string(),
            "command not found".to_string(),
            true,
            0,
        );

        let result = storage.get("call-err").unwrap();
        assert!(result.is_error);
    }

    #[test]
    fn test_default() {
        let storage = ToolResultStorage::default();
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }
}
