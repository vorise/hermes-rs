use anyhow::Result;
use async_trait::async_trait;

use h_core::checkpoint::CheckpointManager;
use h_core::home::hermes_home;

use crate::{CommandContext, CommandResult, SlashCommand};

/// /checkpoint — Save, restore, list, or delete session checkpoints.
pub struct CheckpointCommand;

#[async_trait]
impl SlashCommand for CheckpointCommand {
    fn name(&self) -> &str {
        "checkpoint"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["cp"]
    }

    fn description(&self) -> &str {
        "Save, restore, list, or delete session checkpoints"
    }

    fn category(&self) -> &str {
        "session"
    }

    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
        let db_path = hermes_home().join("checkpoints.db");
        let manager = CheckpointManager::open(&db_path)?;

        let parts: Vec<&str> = args.split_whitespace().collect();
        let subcommand = parts.first().map(|s| *s).unwrap_or("list");

        match subcommand {
            "save" | "s" => {
                let turn = parts.get(1).and_then(|t| t.parse::<u32>().ok()).unwrap_or(0);
                // Use messages from context (excluding system message for the checkpoint)
                let id = manager.save(&ctx.session_id, turn, &ctx.messages, "")?;
                Ok(CommandResult::Message(format!(
                    "Checkpoint saved (id={id}, session={}, turn={turn})",
                    ctx.session_id
                )))
            }
            "restore" | "r" => {
                let id = parts
                    .get(1)
                    .and_then(|t| t.parse::<i64>().ok())
                    .ok_or_else(|| anyhow::anyhow!("Usage: /checkpoint restore <id>. Use /checkpoint list to see available checkpoints."))?;

                match manager.restore_by_id(id)? {
                    Some(cp) => {
                        let msg_count = cp.messages.len();
                        Ok(CommandResult::ConfigChange(crate::ConfigChange::RestoreCheckpoint {
                            checkpoint_id: id,
                            session_id: cp.session_id,
                            turn: cp.turn,
                            message_count: msg_count,
                        }))
                    }
                    None => Ok(CommandResult::Message(format!(
                        "Checkpoint {id} not found."
                    ))),
                }
            }
            "list" | "ls" | "l" => {
                let checkpoints = manager.list_for_session(&ctx.session_id)?;
                if checkpoints.is_empty() {
                    Ok(CommandResult::Message(
                        "No checkpoints for this session. Use /checkpoint save to create one.".to_string(),
                    ))
                } else {
                    let mut lines = vec![format!("Checkpoints for session {}:", ctx.session_id)];
                    for (id, turn, created_at) in &checkpoints {
                        let ts = chrono::DateTime::from_timestamp_millis((*created_at as i64) * 1000)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        lines.push(format!("  [{id}] turn {turn} — {ts}"));
                    }
                    lines.push(format!("  Total: {} checkpoints", checkpoints.len()));
                    Ok(CommandResult::Message(lines.join("\n")))
                }
            }
            "delete" | "del" | "rm" => {
                let id = parts
                    .get(1)
                    .and_then(|t| t.parse::<i64>().ok())
                    .ok_or_else(|| anyhow::anyhow!("Usage: /checkpoint delete <id>. Use /checkpoint list to see available checkpoints."))?;

                match manager.restore_by_id(id)? {
                    Some(cp) => {
                        manager.delete_checkpoint(id)?;
                        Ok(CommandResult::Message(format!(
                            "Deleted checkpoint [{}] (turn {}) for session {}",
                            id, cp.turn, cp.session_id
                        )))
                    }
                    None => Ok(CommandResult::Message(format!(
                        "Checkpoint {id} not found."
                    ))),
                }
            }
            "latest" => {
                match manager.restore_latest(&ctx.session_id)? {
                    Some(cp) => {
                        Ok(CommandResult::ConfigChange(crate::ConfigChange::RestoreCheckpoint {
                            checkpoint_id: cp.id,
                            session_id: cp.session_id,
                            turn: cp.turn,
                            message_count: cp.messages.len(),
                        }))
                    }
                    None => Ok(CommandResult::Message(
                        "No checkpoints found for this session.".to_string(),
                    )),
                }
            }
            _ => Ok(CommandResult::Message(
                "Usage: /checkpoint {save|restore <id>|list|delete <id>|latest}\n\
                 Aliases: /cp save, /cp restore <id>, /cp list, /cp delete <id>, /cp latest"
                    .to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use h_core::checkpoint::CheckpointManager;
    use std::sync::Arc;
    use tokio::sync::Notify;

    use crate::CommandContext;

    fn test_ctx() -> CommandContext {
        CommandContext::new(
            Arc::new(h_core::SessionDB::new_in_memory().unwrap()),
            format!("sess-cp-test-{}", std::process::id()),
            vec![
                h_core::Message::user("hello"),
                h_core::Message::assistant("hi there"),
            ],
            h_core::ModelRef::new(
                h_core::ProviderId::new("anthropic"),
                h_core::ModelId::new("claude-sonnet-4-6"),
            ),
            h_core::CostTracker::default(),
            Some(90),
            false,
            Arc::new(Notify::new()),
            h_core::HermesConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_checkpoint_save_and_list() {
        let ctx = test_ctx();
        let mgr = CheckpointManager::new_in_memory().unwrap();

        let turn = 0u32;
        let id = mgr.save(&ctx.session_id, turn, &ctx.messages, "").unwrap();
        assert!(id > 0);

        let list = mgr.list_for_session(&ctx.session_id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1, 0);
    }

    #[tokio::test]
    async fn test_checkpoint_restore_nonexistent() {
        let mgr = CheckpointManager::new_in_memory().unwrap();
        let cp = mgr.restore_by_id(99999).unwrap();
        assert!(cp.is_none());
    }

    #[tokio::test]
    async fn test_checkpoint_delete() {
        let mgr = CheckpointManager::new_in_memory().unwrap();
        let id = mgr.save("sess-del", 0, &[h_core::Message::user("a")], "").unwrap();
        mgr.delete_checkpoint(id).unwrap();
        let cp = mgr.restore_by_id(id).unwrap();
        assert!(cp.is_none());
    }
}
