use std::path::{Path, PathBuf};

use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use crate::{Message, Role};

/// A checkpoint (snapshot) of session state at a given turn.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    /// Unique checkpoint ID.
    pub id: i64,
    /// Associated session ID.
    pub session_id: String,
    /// Turn number when this checkpoint was created.
    pub turn: u32,
    /// Messages up to this point.
    pub messages: Vec<Message>,
    /// System prompt at this point.
    pub system_prompt: String,
    /// Timestamp (Unix epoch seconds).
    pub created_at: f64,
}

/// Manages session checkpoints for save/restore.
///
/// Stores per-turn snapshots of conversation state in SQLite,
/// allowing users to restore from any previous checkpoint.
pub struct CheckpointManager {
    conn: Mutex<Connection>,
    max_checkpoints: u32,
}

impl CheckpointManager {
    /// Open or create checkpoint store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS checkpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                turn INTEGER NOT NULL,
                system_prompt TEXT NOT NULL DEFAULT '',
                messages_json TEXT NOT NULL,
                created_at REAL NOT NULL,
                UNIQUE(session_id, turn)
            );
            CREATE INDEX IF NOT EXISTS idx_cp_session ON checkpoints(session_id);
            CREATE INDEX IF NOT EXISTS idx_cp_turn ON checkpoints(session_id, turn);
            ",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            max_checkpoints: 50,
        }
    }

    /// In-memory store for testing.
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS checkpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                turn INTEGER NOT NULL,
                system_prompt TEXT NOT NULL DEFAULT '',
                messages_json TEXT NOT NULL,
                created_at REAL NOT NULL,
                UNIQUE(session_id, turn)
            );
            CREATE INDEX IF NOT EXISTS idx_cp_session ON checkpoints(session_id);
            CREATE INDEX IF NOT EXISTS idx_cp_turn ON checkpoints(session_id, turn);
            ",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            max_checkpoints: 50,
        }
    }

    /// Set the maximum number of checkpoints per session.
    pub fn set_max_checkpoints(&mut self, n: u32) {
        self.max_checkpoints = n;
    }

    /// Save a checkpoint for the given session.
    pub fn save(&self, session_id: &str, turn: u32, messages: &[Message], system_prompt: &str) -> Result<i64> {
        let conn = self.conn.lock();
        let messages_json = serde_json::to_string(messages)?;
        let created_at = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;

        // Insert or replace existing checkpoint for this turn
        let id = conn.execute(
            "INSERT INTO checkpoints (session_id, turn, system_prompt, messages_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(session_id, turn) DO UPDATE SET
                 system_prompt = excluded.system_prompt,
                 messages_json = excluded.messages_json,
                 created_at = excluded.created_at",
            params![session_id, turn, system_prompt, messages_json, created_at],
        )?;

        // Enforce max checkpoints: delete oldest beyond limit
        conn.execute(
            "DELETE FROM checkpoints WHERE session_id = ?1
             AND id NOT IN (
                 SELECT id FROM checkpoints WHERE session_id = ?1
                 ORDER BY turn DESC LIMIT ?2
             )",
            params![session_id, self.max_checkpoints],
        )?;

        Ok(id as i64)
    }

    /// Restore from a specific checkpoint by ID.
    pub fn restore_by_id(&self, checkpoint_id: i64) -> Result<Option<Checkpoint>> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT id, session_id, turn, system_prompt, messages_json, created_at
                 FROM checkpoints WHERE id = ?1",
                params![checkpoint_id],
                |row| {
                    let messages_json: String = row.get(4)?;
                    let messages: Vec<Message> = serde_json::from_str(&messages_json)
                        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
                    Ok(Checkpoint {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        turn: row.get(2)?,
                        system_prompt: row.get(3)?,
                        messages,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Restore from the latest checkpoint for a session.
    pub fn restore_latest(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT id, session_id, turn, system_prompt, messages_json, created_at
                 FROM checkpoints WHERE session_id = ?1
                 ORDER BY turn DESC LIMIT 1",
                params![session_id],
                |row| {
                    let messages_json: String = row.get(4)?;
                    let messages: Vec<Message> = serde_json::from_str(&messages_json)
                        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
                    Ok(Checkpoint {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        turn: row.get(2)?,
                        system_prompt: row.get(3)?,
                        messages,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Restore from a specific turn number.
    pub fn restore_at_turn(&self, session_id: &str, turn: u32) -> Result<Option<Checkpoint>> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT id, session_id, turn, system_prompt, messages_json, created_at
                 FROM checkpoints WHERE session_id = ?1 AND turn = ?2",
                params![session_id, turn],
                |row| {
                    let messages_json: String = row.get(4)?;
                    let messages: Vec<Message> = serde_json::from_str(&messages_json)
                        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
                    Ok(Checkpoint {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        turn: row.get(2)?,
                        system_prompt: row.get(3)?,
                        messages,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// List all checkpoints for a session.
    pub fn list_for_session(&self, session_id: &str) -> Result<Vec<(i64, u32, f64)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, turn, created_at FROM checkpoints
             WHERE session_id = ?1 ORDER BY turn",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, u32>(1)?, row.get::<_, f64>(2)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// Delete all checkpoints for a session.
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM checkpoints WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Delete a specific checkpoint.
    pub fn delete_checkpoint(&self, checkpoint_id: i64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM checkpoints WHERE id = ?1",
            params![checkpoint_id],
        )?;
        Ok(())
    }

    /// Get the storage path (for CLI display).
    pub fn storage_path(&self) -> Option<PathBuf> {
        let conn = self.conn.lock();
        conn.path().map(|p| p.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_restore_latest() {
        let mgr = CheckpointManager::new_in_memory().unwrap();
        let messages = vec![
            Message::user("hello"),
            Message::assistant("hi"),
        ];

        let id = mgr.save("session-1", 0, &messages, "system").unwrap();
        assert!(id > 0);

        let cp = mgr.restore_latest("session-1").unwrap().unwrap();
        assert_eq!(cp.session_id, "session-1");
        assert_eq!(cp.turn, 0);
        assert_eq!(cp.messages.len(), 2);
        assert_eq!(cp.system_prompt, "system");
    }

    #[test]
    fn test_restore_by_id() {
        let mgr = CheckpointManager::new_in_memory().unwrap();
        let messages = vec![Message::user("test")];

        let id = mgr.save("session-1", 0, &messages, "").unwrap();
        let cp = mgr.restore_by_id(id).unwrap().unwrap();
        assert_eq!(cp.id, id);
    }

    #[test]
    fn test_restore_at_turn() {
        let mgr = CheckpointManager::new_in_memory().unwrap();

        mgr.save("session-1", 0, &[Message::user("first")], "").unwrap();
        mgr.save("session-1", 1, &[Message::user("second")], "").unwrap();
        mgr.save("session-1", 2, &[Message::user("third")], "").unwrap();

        let cp = mgr.restore_at_turn("session-1", 1).unwrap().unwrap();
        assert_eq!(cp.turn, 1);
        assert_eq!(cp.messages[0].role, Role::User);

        // Non-existent turn
        let cp = mgr.restore_at_turn("session-1", 99).unwrap();
        assert!(cp.is_none());
    }

    #[test]
    fn test_list_checkpoints() {
        let mgr = CheckpointManager::new_in_memory().unwrap();

        mgr.save("session-1", 0, &[Message::user("a")], "").unwrap();
        mgr.save("session-1", 1, &[Message::user("b")], "").unwrap();
        mgr.save("session-1", 2, &[Message::user("c")], "").unwrap();

        let list = mgr.list_for_session("session-1").unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].1, 0);
        assert_eq!(list[1].1, 1);
        assert_eq!(list[2].1, 2);
    }

    #[test]
    fn test_max_checkpoints_enforcement() {
        let mut mgr = CheckpointManager::new_in_memory().unwrap();
        mgr.set_max_checkpoints(3);

        // Save 5 checkpoints
        for i in 0..5 {
            mgr.save("session-1", i, &[Message::user(&format!("msg {i}"))], "").unwrap();
        }

        let list = mgr.list_for_session("session-1").unwrap();
        assert_eq!(list.len(), 3);
        // Should keep the 3 most recent (turns 2, 3, 4)
        assert_eq!(list[0].1, 2);
        assert_eq!(list[2].1, 4);
    }

    #[test]
    fn test_delete_session() {
        let mgr = CheckpointManager::new_in_memory().unwrap();

        mgr.save("session-1", 0, &[Message::user("a")], "").unwrap();
        mgr.save("session-1", 1, &[Message::user("b")], "").unwrap();
        mgr.save("session-2", 0, &[Message::user("c")], "").unwrap();

        mgr.delete_session("session-1").unwrap();

        assert!(mgr.list_for_session("session-1").unwrap().is_empty());
        assert_eq!(mgr.list_for_session("session-2").unwrap().len(), 1);
    }

    #[test]
    fn test_delete_checkpoint() {
        let mgr = CheckpointManager::new_in_memory().unwrap();

        let id = mgr.save("session-1", 0, &[Message::user("a")], "").unwrap();
        mgr.delete_checkpoint(id).unwrap();

        assert!(mgr.restore_by_id(id).unwrap().is_none());
    }

    #[test]
    fn test_upsert_same_turn() {
        let mgr = CheckpointManager::new_in_memory().unwrap();

        mgr.save("session-1", 0, &[Message::user("first")], "sys1").unwrap();
        mgr.save("session-1", 0, &[Message::user("updated")], "sys2").unwrap();

        let cp = mgr.restore_at_turn("session-1", 0).unwrap().unwrap();
        assert_eq!(cp.system_prompt, "sys2");
        assert_eq!(cp.messages[0].as_text(), Some("updated"));

        // Should still only have 1 checkpoint
        let list = mgr.list_for_session("session-1").unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_restore_nonexistent_session() {
        let mgr = CheckpointManager::new_in_memory().unwrap();

        assert!(mgr.restore_latest("nonexistent").unwrap().is_none());
        assert!(mgr.restore_at_turn("nonexistent", 0).unwrap().is_none());
    }
}
