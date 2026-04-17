use std::path::PathBuf;

use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use crate::Message;

/// A checkpoint (snapshot) of session state at a given turn.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub id: i64,
    pub session_id: String,
    pub turn: u32,
    pub messages: Vec<Message>,
    pub system_prompt: String,
    pub created_at: f64,
}

/// Manages session checkpoints for save/restore.
pub struct CheckpointManager {
    conn: Mutex<Connection>,
    max_checkpoints: u32,
}

impl CheckpointManager {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Self::init_schema(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            max_checkpoints: 50,
        })
    }

    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_schema(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            max_checkpoints: 50,
        })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
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
        Ok(())
    }

    pub fn set_max_checkpoints(&mut self, n: u32) {
        self.max_checkpoints = n;
    }

    pub fn save(&self, session_id: &str, turn: u32, messages: &[Message], system_prompt: &str) -> Result<i64> {
        let conn = self.conn.lock();
        let messages_json = serde_json::to_string(messages)?;
        let created_at = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;

        let id = conn.execute(
            "INSERT INTO checkpoints (session_id, turn, system_prompt, messages_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(session_id, turn) DO UPDATE SET
                 system_prompt = excluded.system_prompt,
                 messages_json = excluded.messages_json,
                 created_at = excluded.created_at",
            params![session_id, turn, system_prompt, messages_json, created_at],
        )?;

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

    fn parse_checkpoint_row(
        id: i64,
        session_id: String,
        turn: i64,
        system_prompt: String,
        messages_json: String,
        created_at: f64,
    ) -> Result<Checkpoint> {
        let messages: Vec<Message> = serde_json::from_str(&messages_json)?;
        Ok(Checkpoint {
            id,
            session_id,
            turn: turn as u32,
            system_prompt,
            messages,
            created_at,
        })
    }

    pub fn restore_by_id(&self, checkpoint_id: i64) -> Result<Option<Checkpoint>> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT id, session_id, turn, system_prompt, messages_json, created_at
                 FROM checkpoints WHERE id = ?1",
                params![checkpoint_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, f64>(5)?,
                    ))
                },
            )
            .optional()?;
        match row {
            Some((id, sid, turn, sp, mj, ca)) => Self::parse_checkpoint_row(id, sid, turn, sp, mj, ca).map(Some),
            None => Ok(None),
        }
    }

    pub fn restore_latest(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT id, session_id, turn, system_prompt, messages_json, created_at
                 FROM checkpoints WHERE session_id = ?1
                 ORDER BY turn DESC LIMIT 1",
                params![session_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, f64>(5)?,
                    ))
                },
            )
            .optional()?;
        match row {
            Some((id, sid, turn, sp, mj, ca)) => Self::parse_checkpoint_row(id, sid, turn, sp, mj, ca).map(Some),
            None => Ok(None),
        }
    }

    pub fn restore_at_turn(&self, session_id: &str, turn: u32) -> Result<Option<Checkpoint>> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT id, session_id, turn, system_prompt, messages_json, created_at
                 FROM checkpoints WHERE session_id = ?1 AND turn = ?2",
                params![session_id, turn],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, f64>(5)?,
                    ))
                },
            )
            .optional()?;
        match row {
            Some((id, sid, turn, sp, mj, ca)) => Self::parse_checkpoint_row(id, sid, turn, sp, mj, ca).map(Some),
            None => Ok(None),
        }
    }

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

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM checkpoints WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    pub fn delete_checkpoint(&self, checkpoint_id: i64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM checkpoints WHERE id = ?1",
            params![checkpoint_id],
        )?;
        Ok(())
    }

    pub fn storage_path(&self) -> Option<PathBuf> {
        let conn = self.conn.lock();
        conn.path().map(|p| std::path::PathBuf::from(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;

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

        for i in 0..5 {
            mgr.save("session-1", i, &[Message::user(&format!("msg {i}"))], "").unwrap();
        }

        let list = mgr.list_for_session("session-1").unwrap();
        assert_eq!(list.len(), 3);
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
        assert_eq!(cp.messages[0].content.as_ref().and_then(|c| c.as_text()), Some("updated"));

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
