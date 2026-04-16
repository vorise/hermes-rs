use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use crate::session::{SearchResult, Session, SessionSummary, StoredMessage};
use crate::{CostTracker, Message};

fn row_to_search_result(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchResult> {
    Ok(SearchResult {
        session_id: row.get(0)?,
        message_id: row.get(1)?,
        content: row.get(2).unwrap_or_default(),
        score: 0.0,
        timestamp: chrono::DateTime::from_timestamp_millis(
            (row.get::<_, f64>(3)? * 1000.0) as i64,
        )
        .unwrap_or_default(),
    })
}

/// SQLite session store with FTS5 full-text search.
pub struct SessionDB {
    conn: Arc<Mutex<Connection>>,
}

impl SessionDB {
    /// Open or create the session database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        // Create schema
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                user_id TEXT,
                model TEXT,
                model_config TEXT,
                system_prompt TEXT,
                parent_session_id TEXT REFERENCES sessions(id),
                started_at REAL NOT NULL,
                ended_at REAL,
                end_reason TEXT,
                message_count INTEGER DEFAULT 0,
                tool_call_count INTEGER DEFAULT 0,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                cache_read_tokens INTEGER DEFAULT 0,
                cache_write_tokens INTEGER DEFAULT 0,
                reasoning_tokens INTEGER DEFAULT 0,
                billing_provider TEXT,
                billing_base_url TEXT,
                estimated_cost_usd REAL,
                title TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                role TEXT NOT NULL,
                content TEXT,
                tool_call_id TEXT,
                tool_calls TEXT,
                tool_name TEXT,
                timestamp REAL NOT NULL,
                token_count INTEGER,
                finish_reason TEXT,
                reasoning TEXT,
                reasoning_details TEXT
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                content,
                content=messages,
                content_rowid=id
            );

            CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
            END;

            CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
            END;

            CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
                INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
            END;
            ",
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create an in-memory database (useful for tests).
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;

        // Create schema
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                user_id TEXT,
                model TEXT,
                model_config TEXT,
                system_prompt TEXT,
                parent_session_id TEXT REFERENCES sessions(id),
                started_at REAL NOT NULL,
                ended_at REAL,
                end_reason TEXT,
                message_count INTEGER DEFAULT 0,
                tool_call_count INTEGER DEFAULT 0,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                cache_read_tokens INTEGER DEFAULT 0,
                cache_write_tokens INTEGER DEFAULT 0,
                reasoning_tokens INTEGER DEFAULT 0,
                billing_provider TEXT,
                billing_base_url TEXT,
                estimated_cost_usd REAL,
                title TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                role TEXT NOT NULL,
                content TEXT,
                tool_call_id TEXT,
                tool_calls TEXT,
                tool_name TEXT,
                timestamp REAL NOT NULL,
                token_count INTEGER,
                finish_reason TEXT,
                reasoning TEXT,
                reasoning_details TEXT
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                content,
                content=messages,
                content_rowid=id
            );

            CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
            END;

            CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
            END;

            CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
                INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
            END;
            ",
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create a new session.
    pub fn create_session(&self, session: &Session) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO sessions (id, source, user_id, model, model_config, system_prompt,
             parent_session_id, started_at, ended_at, end_reason, message_count, tool_call_count,
             input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
             billing_provider, billing_base_url, estimated_cost_usd, title)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            params![
                session.id, session.source, session.user_id, session.model, session.model_config,
                session.system_prompt, session.parent_session_id, session.started_at,
                session.ended_at, session.end_reason, session.message_count, session.tool_call_count,
                session.input_tokens, session.output_tokens, session.cache_read_tokens,
                session.cache_write_tokens, session.reasoning_tokens, session.billing_provider,
                session.billing_base_url, session.estimated_cost_usd, session.title,
            ],
        )?;
        Ok(())
    }

    /// Add a message to a session.
    pub fn add_message(&self, session_id: &str, message: &Message) -> Result<i64> {
        let conn = self.conn.lock();
        let content = message.content.as_ref().and_then(|c| c.as_text()).map(|s| s.to_string());
        let tool_calls = message.tool_calls.as_ref().map(|tc| {
            serde_json::to_string(tc).unwrap_or_default()
        });
        let tool_name = message.name.clone();
        let tool_call_id = message.tool_call_id.clone();
        let reasoning = message.reasoning.clone();
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;

        let id = conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_call_id, tool_calls,
             tool_name, timestamp, reasoning)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session_id, message.role.to_string(), content, tool_call_id, tool_calls,
                tool_name, now, reasoning,
            ],
        )?;

        // Update session message count
        conn.execute(
            "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
            params![session_id],
        )?;

        Ok(id as i64)
    }

    /// Get a session by ID.
    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let conn = self.conn.lock();
        let session = conn
            .query_row(
                "SELECT * FROM sessions WHERE id = ?1",
                params![session_id],
                |row| {
                    Ok(Session {
                        id: row.get(0)?,
                        source: row.get(1)?,
                        user_id: row.get(2)?,
                        model: row.get(3)?,
                        model_config: row.get(4)?,
                        system_prompt: row.get(5)?,
                        parent_session_id: row.get(6)?,
                        started_at: row.get(7)?,
                        ended_at: row.get(8)?,
                        end_reason: row.get(9)?,
                        message_count: row.get(10)?,
                        tool_call_count: row.get(11)?,
                        input_tokens: row.get(12)?,
                        output_tokens: row.get(13)?,
                        cache_read_tokens: row.get(14)?,
                        cache_write_tokens: row.get(15)?,
                        reasoning_tokens: row.get(16)?,
                        billing_provider: row.get(17)?,
                        billing_base_url: row.get(18)?,
                        estimated_cost_usd: row.get(19)?,
                        title: row.get(20)?,
                    })
                },
            )
            .optional()?;
        Ok(session)
    }

    /// Get all messages for a session.
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, tool_call_id, tool_calls,
             tool_name, timestamp, token_count, finish_reason, reasoning, reasoning_details
             FROM messages WHERE session_id = ?1 ORDER BY id",
        )?;
        let messages = stmt.query_map(params![session_id], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                tool_call_id: row.get(4)?,
                tool_calls: row.get(5)?,
                tool_name: row.get(6)?,
                timestamp: row.get(7)?,
                token_count: row.get(8)?,
                finish_reason: row.get(9)?,
                reasoning: row.get(10)?,
                reasoning_details: row.get(11)?,
            })
        })?;
        messages.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// Update system prompt for a session.
    pub fn update_system_prompt(&self, session_id: &str, prompt: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET system_prompt = ?1 WHERE id = ?2",
            params![prompt, session_id],
        )?;
        Ok(())
    }

    /// Update session stats (cost, tokens).
    pub fn update_session_stats(&self, session_id: &str, cost: &CostTracker) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET input_tokens = ?1, output_tokens = ?2,
             cache_read_tokens = ?3, cache_write_tokens = ?4, reasoning_tokens = ?5,
             estimated_cost_usd = ?6 WHERE id = ?7",
            params![
                cost.input_tokens, cost.output_tokens, cost.cache_read_tokens,
                cost.cache_write_tokens, cost.reasoning_tokens,
                cost.estimated_cost_usd, session_id,
            ],
        )?;
        Ok(())
    }

    /// Search sessions using FTS5.
    pub fn search_sessions(
        &self,
        query: &str,
        source: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let conn = self.conn.lock();

        let results = match source {
            Some(src) => Self::search_with_source(&conn, query, src)?,
            None => Self::search_no_source(&conn, query)?,
        };

        Ok(results)
    }

    fn search_with_source(
        conn: &Connection,
        query: &str,
        source: &str,
    ) -> Result<Vec<SearchResult>> {
        let mut stmt = conn.prepare(
            "SELECT m.session_id, m.id as message_id, m.content, m.timestamp
             FROM messages_fts f
             JOIN messages m ON m.id = f.rowid
             JOIN sessions s ON s.id = m.session_id
             WHERE f.content MATCH ?1 AND s.source = ?2
             ORDER BY rank
             LIMIT 50",
        )?;
        let rows: std::result::Result<Vec<_>, _> =
            stmt.query_map(params![query, source], row_to_search_result)?.collect();
        rows.map_err(|e| e.into())
    }

    fn search_no_source(conn: &Connection, query: &str) -> Result<Vec<SearchResult>> {
        let mut stmt = conn.prepare(
            "SELECT m.session_id, m.id as message_id, m.content, m.timestamp
             FROM messages_fts f
             JOIN messages m ON m.id = f.rowid
             WHERE f.content MATCH ?1
             ORDER BY rank
             LIMIT 50",
        )?;
        let rows: std::result::Result<Vec<_>, _> =
            stmt.query_map(params![query], row_to_search_result)?.collect();
        rows.map_err(|e| e.into())
    }

    /// Get session summaries for display.
    pub fn get_session_summaries(&self, limit: usize) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, title, model, message_count, started_at, estimated_cost_usd, source
             FROM sessions ORDER BY started_at DESC LIMIT ?1",
        )?;
        let summaries = stmt.query_map(params![limit], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                model: row.get(2)?,
                message_count: row.get(3)?,
                started_at: chrono::DateTime::from_timestamp_millis(
                    (row.get::<_, f64>(4)? * 1000.0) as i64,
                )
                .unwrap_or_default(),
                estimated_cost_usd: row.get(5)?,
                source: row.get(6)?,
            })
        })?;
        summaries.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// End a session.
    pub fn end_session(&self, session_id: &str, reason: &str) -> Result<()> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
        conn.execute(
            "UPDATE sessions SET ended_at = ?1, end_reason = ?2 WHERE id = ?3",
            params![now, reason, session_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> SessionDB {
        let path = std::env::temp_dir().join(format!("hermes_test_{}.db", uuid::Uuid::new_v4()));
        SessionDB::open(&path).unwrap()
    }

    #[test]
    fn test_create_and_get_session() {
        let db = temp_db();
        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        let retrieved = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(retrieved.id, session.id);
        assert_eq!(retrieved.source, "cli");
    }

    #[test]
    fn test_add_and_get_messages() {
        let db = temp_db();
        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        let msg = Message::user("hello");
        db.add_message(&session.id, &msg).unwrap();

        let msg2 = Message::assistant("hi there");
        db.add_message(&session.id, &msg2).unwrap();

        let messages = db.get_messages(&session.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn test_update_system_prompt() {
        let db = temp_db();
        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        db.update_system_prompt(&session.id, "You are helpful").unwrap();
        let retrieved = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(retrieved.system_prompt, Some("You are helpful".to_string()));
    }

    #[test]
    fn test_update_session_stats() {
        let db = temp_db();
        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        let cost = CostTracker {
            input_tokens: 100,
            output_tokens: 50,
            estimated_cost_usd: 0.001,
            api_call_count: 1,
            ..Default::default()
        };
        db.update_session_stats(&session.id, &cost).unwrap();

        let retrieved = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(retrieved.input_tokens, 100);
        assert_eq!(retrieved.output_tokens, 50);
    }

    #[test]
    fn test_end_session() {
        let db = temp_db();
        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        db.end_session(&session.id, "completed").unwrap();
        let retrieved = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(retrieved.end_reason, Some("completed".to_string()));
        assert!(retrieved.ended_at.is_some());
    }

    #[test]
    fn test_get_session_summaries() {
        let db = temp_db();
        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        let summaries = db.get_session_summaries(10).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, session.id);
    }
}
