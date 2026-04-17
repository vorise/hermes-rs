use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /memory — View or manage persistent memory.
pub struct MemoryCommand;

#[async_trait]
impl SlashCommand for MemoryCommand {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "View or manage persistent memory"
    }

    fn category(&self) -> &str {
        "memory"
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        let memory_dir = h_core::home::memory_dir();
        match args {
            "" | "view" => Self::view_memories(&memory_dir),
            "clear" => Self::clear_memories(&memory_dir),
            "export" => Self::export_memories(&memory_dir),
            other => {
                Ok(CommandResult::Message(format!(
                    "Unknown subcommand: {other}. Use /memory [view|clear|export]"
                )))
            }
        }
    }
}

impl MemoryCommand {
    fn view_memories(dir: &std::path::Path) -> Result<CommandResult> {
        let mut lines = vec!["Memory Entries:".to_string(), "".to_string()];

        if !dir.exists() {
            lines.push("  No memory directory found. Memory system is not yet initialized.".to_string());
            return Ok(CommandResult::Message(lines.join("\n")));
        }

        let mut entries: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path = e.path();
                path.is_file() && path.extension().map_or(false, |ext| ext == "md")
            })
            .filter(|e| e.file_name() != "MEMORY.md")
            .collect();
        entries.sort_by_key(|e| e.file_name());

        // Also check MEMORY.md index
        let index_path = dir.join("MEMORY.md");
        if index_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&index_path) {
                lines.push(format!("  Index (MEMORY.md):\n{}", content.lines().take(20).collect::<Vec<_>>().join("\n")));
                if content.lines().count() > 20 {
                    lines.push(format!("    ... ({} total lines)", content.lines().count()));
                }
                lines.push(String::new());
            }
        }

        if entries.is_empty() {
            lines.push("  No individual memory files found.".to_string());
        } else {
            lines.push(format!("  {} memory files:\n", entries.len()));
            for entry in &entries {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    let first_line = content.lines().next().unwrap_or("").trim_start_matches('#').trim();
                    lines.push(format!("  - {name}: {first_line}"));
                } else {
                    lines.push(format!("  - {name}"));
                }
            }
        }

        Ok(CommandResult::Message(lines.join("\n")))
    }

    fn clear_memories(dir: &std::path::Path) -> Result<CommandResult> {
        if !dir.exists() {
            return Ok(CommandResult::Message("No memory directory found. Nothing to clear.".to_string()));
        }

        let mut count = 0;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "md") {
                std::fs::remove_file(&path)?;
                count += 1;
            }
        }

        Ok(CommandResult::Message(format!("Cleared {count} memory file(s).")))
    }

    fn export_memories(dir: &std::path::Path) -> Result<CommandResult> {
        if !dir.exists() {
            return Ok(CommandResult::Message("No memory directory found. Nothing to export.".to_string()));
        }

        let mut combined = String::from("# Hermes Memory Export\n\n");
        let mut count = 0;

        // Start with MEMORY.md if it exists
        let index_path = dir.join("MEMORY.md");
        if index_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&index_path) {
                combined.push_str(&content);
                combined.push_str("\n\n---\n\n");
                count += 1;
            }
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "md") && path.file_name() != Some("MEMORY.md".as_ref()) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    combined.push_str(&format!("## {}\n\n{}\n\n---\n\n",
                        entry.file_name().to_string_lossy(), content));
                    count += 1;
                }
            }
        }

        Ok(CommandResult::Message(format!("Exported {count} memory file(s). Combined output:\n\n{combined}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    fn make_ctx() -> CommandContext {
        CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            "test".to_string(),
            vec![],
            h_core::ModelRef::new(
                h_core::ProviderId::new("test"),
                h_core::ModelId::new("test"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_memory_view() {
        let result = MemoryCommand.execute("", &make_ctx()).await.unwrap();
        match result {
            CommandResult::Message(msg) => assert!(msg.contains("Memory Entries") || msg.contains("No memory")),
            _ => panic!("Expected Message"),
        }
    }
}
