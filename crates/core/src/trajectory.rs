use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{CostTracker, Message};

/// A single trajectory entry (one turn in the conversation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryEntry {
    /// System prompt for this turn.
    pub system_prompt: String,
    /// Messages exchanged in this turn.
    pub messages: Vec<Message>,
    /// Model used.
    pub model: String,
    /// Token usage.
    pub cost: CostTracker,
    /// Timestamp.
    pub timestamp: String,
    /// Whether the assistant response is complete.
    pub is_complete: bool,
}

/// A complete trajectory (full conversation session).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    /// Session ID.
    pub session_id: String,
    /// Source platform (cli, telegram, etc.).
    pub source: String,
    /// Model used.
    pub model: String,
    /// System prompt.
    pub system_prompt: String,
    /// All entries in the trajectory.
    pub entries: Vec<TrajectoryEntry>,
    /// Total cost tracking.
    pub total_cost: CostTracker,
    /// Created at timestamp.
    pub created_at: String,
}

impl Trajectory {
    pub fn new(session_id: &str, source: &str, model: &str, system_prompt: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            source: source.to_string(),
            model: model.to_string(),
            system_prompt: system_prompt.to_string(),
            entries: Vec::new(),
            total_cost: CostTracker::default(),
            created_at: Utc::now().to_rfc3339(),
        }
    }

    /// Add a turn to the trajectory.
    pub fn add_turn(
        &mut self,
        messages: &[Message],
        cost: &CostTracker,
        is_complete: bool,
    ) {
        let entry = TrajectoryEntry {
            system_prompt: self.system_prompt.clone(),
            messages: messages.to_vec(),
            model: self.model.clone(),
            cost: cost.clone(),
            timestamp: Utc::now().to_rfc3339(),
            is_complete,
        };
        self.entries.push(entry);
        self.total_cost.add(cost);
    }

    /// Get the number of turns in the trajectory.
    pub fn turn_count(&self) -> usize {
        self.entries.len()
    }

    /// Get total tokens used.
    pub fn total_tokens(&self) -> u64 {
        self.total_cost.total_tokens()
    }
}

/// Manages trajectory saving and loading for training data.
pub struct TrajectoryManager {
    /// Directory to store trajectory files.
    pub storage_dir: PathBuf,
}

impl TrajectoryManager {
    pub fn new(storage_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(storage_dir)?;
        Ok(Self {
            storage_dir: storage_dir.to_path_buf(),
        })
    }

    /// Save a trajectory as a JSONL file (one entry per line).
    pub fn save_trajectory(&self, trajectory: &Trajectory) -> Result<PathBuf> {
        let filename = format!("{}_{}.jsonl", trajectory.session_id, trajectory.source);
        let path = self.storage_dir.join(&filename);

        // Use atomic write: write to temp file, then rename
        let tmp_path = path.with_extension("jsonl.tmp");
        let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp_path)?);

        // Write header (trajectory metadata) as first line
        let header = serde_json::json!({
            "type": "header",
            "session_id": trajectory.session_id,
            "source": trajectory.source,
            "model": trajectory.model,
            "system_prompt": trajectory.system_prompt,
            "total_cost": trajectory.total_cost,
            "created_at": trajectory.created_at,
        });
        writeln!(file, "{}", serde_json::to_string(&header)?)?;

        // Write each entry
        for (i, entry) in trajectory.entries.iter().enumerate() {
            let line = serde_json::json!({
                "type": "entry",
                "turn": i,
                "system_prompt": entry.system_prompt,
                "messages": entry.messages,
                "model": entry.model,
                "cost": entry.cost,
                "timestamp": entry.timestamp,
                "is_complete": entry.is_complete,
            });
            writeln!(file, "{}", serde_json::to_string(&line)?)?;
        }

        file.flush()?;
        drop(file);

        // Atomic rename
        std::fs::rename(&tmp_path, &path)?;

        Ok(path)
    }

    /// Load a trajectory from a JSONL file.
    pub fn load_trajectory(&self, path: &Path) -> Result<Trajectory> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);

        let mut session_id = String::new();
        let mut source = String::new();
        let mut model = String::new();
        let mut system_prompt = String::new();
        let mut total_cost = CostTracker::default();
        let mut created_at = String::new();
        let mut entries = Vec::new();

        for line in std::io::BufRead::lines(reader) {
            let line = line?;
            let value: serde_json::Value = serde_json::from_str(&line)?;

            let entry_type = value["type"].as_str().unwrap_or("");
            match entry_type {
                "header" => {
                    session_id = value["session_id"].as_str().unwrap_or("").to_string();
                    source = value["source"].as_str().unwrap_or("").to_string();
                    model = value["model"].as_str().unwrap_or("").to_string();
                    system_prompt = value["system_prompt"].as_str().unwrap_or("").to_string();
                    created_at = value["created_at"].as_str().unwrap_or("").to_string();
                    if let Some(cost) = value.get("total_cost") {
                        total_cost = serde_json::from_value(cost.clone())
                            .unwrap_or_default();
                    }
                }
                "entry" => {
                    let messages: Vec<Message> = serde_json::from_value(value["messages"].clone())
                        .unwrap_or_default();
                    let cost: CostTracker = serde_json::from_value(value["cost"].clone())
                        .unwrap_or_default();
                    let is_complete = value["is_complete"].as_bool().unwrap_or(true);
                    let entry_system = value["system_prompt"].as_str().unwrap_or("").to_string();
                    let entry_model = value["model"].as_str().unwrap_or("").to_string();
                    let timestamp = value["timestamp"].as_str().unwrap_or("").to_string();

                    entries.push(TrajectoryEntry {
                        system_prompt: entry_system,
                        messages,
                        model: entry_model,
                        cost,
                        timestamp,
                        is_complete,
                    });
                }
                _ => {}
            }
        }

        Ok(Trajectory {
            session_id,
            source,
            model,
            system_prompt,
            entries,
            total_cost,
            created_at,
        })
    }

    /// List all trajectory files in the storage directory.
    pub fn list_trajectories(&self) -> Result<Vec<PathBuf>> {
        let mut trajectories = Vec::new();
        if self.storage_dir.exists() {
            for entry in std::fs::read_dir(&self.storage_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "jsonl") {
                    trajectories.push(path);
                }
            }
        }
        trajectories.sort();
        Ok(trajectories)
    }

    /// Delete a trajectory file.
    pub fn delete_trajectory(&self, path: &Path) -> Result<()> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// Convert scratchpad/thinking content to think tags.
///
/// Used for training data preparation to standardize thinking format.
pub fn convert_scratchpad_to_think(content: &str) -> String {
    // If content already has think tags, leave it alone
    if content.contains("<think>") {
        return content.to_string();
    }

    // If content looks like a scratchpad (starts with common scratchpad patterns),
    // wrap it in think tags
    let trimmed = content.trim();
    let scratchpad_indicators = [
        "let me think",
        "let me analyze",
        "thinking",
        "reasoning",
        "step 1",
        "step 2",
        "first,",
        "ok,",
        "okay,",
        "hmm",
    ];

    let lower = trimmed.to_lowercase();
    if scratchpad_indicators.iter().any(|&kw| lower.starts_with(kw)) {
        return format!("<think>\n{trimmed}\n</think>");
    }

    content.to_string()
}

/// Check if an assistant response appear to be incomplete (truncated mid-thought).
pub fn is_incomplete_scratchpad(content: &str) -> bool {
    let trimmed = content.trim();

    // Empty
    if trimmed.is_empty() {
        return true;
    }

    // Has think tags but no closing tag
    if trimmed.contains("<think>") && !trimmed.contains("</think>") {
        return true;
    }

    // Ends mid-sentence (no terminal punctuation)
    let last_char = trimmed.chars().last().unwrap_or(' ');
    let terminal_punctuation = ['.', '!', '?', '\n'];
    if !terminal_punctuation.contains(&last_char) && trimmed.len() > 10 {
        // Ends with a comma, colon, or other non-terminal character
        let non_terminal = [',', ':', ';', '-', '(', '{', '['];
        if non_terminal.contains(&last_char) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trajectory_new() {
        let traj = Trajectory::new("session-1", "cli", "claude-sonnet", "system");
        assert_eq!(traj.session_id, "session-1");
        assert_eq!(traj.turn_count(), 0);
        assert_eq!(traj.total_tokens(), 0);
    }

    #[test]
    fn test_trajectory_add_turn() {
        let mut traj = Trajectory::new("session-1", "cli", "claude-sonnet", "system");

        let cost = CostTracker {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        traj.add_turn(&[Message::user("hello")], &cost, true);

        assert_eq!(traj.turn_count(), 1);
        assert_eq!(traj.total_tokens(), 150);
    }

    #[test]
    fn test_save_and_load_trajectory() {
        let tmp = std::env::temp_dir().join(format!("traj_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();

        let mgr = TrajectoryManager::new(&tmp).unwrap();

        let mut traj = Trajectory::new("test-session", "cli", "claude-sonnet", "You are helpful");
        let cost = CostTracker {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        traj.add_turn(&[Message::user("hello"), Message::assistant("hi")], &cost, true);

        let path = mgr.save_trajectory(&traj).unwrap();
        assert!(path.exists());

        let loaded = mgr.load_trajectory(&path).unwrap();
        assert_eq!(loaded.session_id, "test-session");
        assert_eq!(loaded.turn_count(), 1);
        assert_eq!(loaded.entries[0].messages.len(), 2);

        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn test_list_trajectories() {
        let tmp = std::env::temp_dir().join(format!("traj_list_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();

        let mgr = TrajectoryManager::new(&tmp).unwrap();

        let traj1 = Trajectory::new("s1", "cli", "m", "sys");
        let traj2 = Trajectory::new("s2", "cli", "m", "sys");
        mgr.save_trajectory(&traj1).unwrap();
        mgr.save_trajectory(&traj2).unwrap();

        let list = mgr.list_trajectories().unwrap();
        assert_eq!(list.len(), 2);

        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn test_delete_trajectory() {
        let tmp = std::env::temp_dir().join(format!("traj_del_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();

        let mgr = TrajectoryManager::new(&tmp).unwrap();
        let traj = Trajectory::new("s1", "cli", "m", "sys");
        let path = mgr.save_trajectory(&traj).unwrap();

        mgr.delete_trajectory(&path).unwrap();
        assert!(!path.exists());

        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn test_convert_scratchpad_to_think() {
        let content = "let me think about this step by step\nFirst, I need to analyze the code";
        let result = convert_scratchpad_to_think(content);
        assert!(result.starts_with("<think>"));
        assert!(result.ends_with("</think>"));
    }

    #[test]
    fn test_convert_scratchpad_already_has_tags() {
        let content = "<think>\nalready tagged\n</think>";
        let result = convert_scratchpad_to_think(content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_convert_scratchpad_not_scratchpad() {
        let content = "Hello, how can I help you?";
        let result = convert_scratchpad_to_think(content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_is_incomplete_scratchpad_empty() {
        assert!(is_incomplete_scratchpad(""));
    }

    #[test]
    fn test_is_incomplete_scratchpad_open_think() {
        assert!(is_incomplete_scratchpad("<think>I'm thinking about"));
    }

    #[test]
    fn test_is_incomplete_scratchpad_complete() {
        assert!(!is_incomplete_scratchpad("<think>complete thought</think>"));
        assert!(!is_incomplete_scratchpad("This is a complete sentence."));
    }

    #[test]
    fn test_is_incomplete_scratchpad_trailing_comma() {
        assert!(is_incomplete_scratchpad("Let me analyze the code,"));
    }
}
