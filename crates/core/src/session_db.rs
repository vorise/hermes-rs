//! SQLite Session Store with FTS5 Search
//!
//! Persistent session storage with full-text search capabilities.

use anyhow::{Result, Context};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use tracing::{debug, info};

use crate::session::{Session, SessionSummary};
use crate::{Message, CostTracker, ModelRef};

// ============================================================================
// Schema Version
// ============================================================================

/// Current schema version for migrations.
const SCHEMA_VERSION: i32 = 1;

// ============================================================================
// SessionDB
// ============================================================================

/// SQLite database for session storage with FTS5 search.
pub struct SessionDB {
    /// Database connection (protected by mutex for thread safety).
    conn: Arc<Mutex<Connection>>,
}

impl SessionDB {
    /// Open or create the session database.
    ///
    /// Creates the database file if it doesn't exist, sets up schema,
    /// enables WAL mode, and creates FTS5 tables.
    pub fn open(path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        // Open connection
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database: {}", path.display()))?;

        // Enable WAL mode for better concurrency
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;"
        ).context("Failed to set pragmas")?;

        // Check schema version and migrate if needed
        let current_version = get_schema_version(&conn)?;
        if current_version < SCHEMA_VERSION {
            migrate_schema(&conn, current_version)?;
        } else if current_version == 0 {
            // New database, create schema
            create_schema(&conn)?;
        }

        info!("SessionDB opened at {}", path.display());

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .context("Failed to open in-memory database")?;

        // Set pragmas
        conn.execute_batch(
            "PRAGMA journal_mode = MEMORY;
             PRAGMA synchronous = OFF;
             PRAGMA foreign_keys = ON;"
        ).context("Failed to set pragmas")?;

        // Create schema
        create_schema(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // ========================================================================
    // Session CRUD
    // ========================================================================

    /// Create a new session record.
    pub fn create_session(&self, session: &Session) -> Result<()> {
        let conn = self.conn.lock();

        let model_str = session.model.as_ref().map(|m| m.to_string());
        let started_ts = session.started_at.timestamp_millis() as f64 / 1000.0;

        conn.execute(
            "INSERT INTO sessions (
                id, source, user_id, model, model_config, system_prompt,
                parent_session_id, started_at, ended_at, end_reason,
                message_count, tool_call_count, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, reasoning_tokens,
                estimated_cost_usd, title
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                session.id,
                session.source,
                session.user_id,
                model_str,
                session.model_config,
                session.system_prompt,
                session.parent_session_id,
                started_ts,
                session.ended_at.map(|t| t.timestamp_millis() as f64 / 1000.0),
                session.end_reason,
                session.message_count as i64,
                session.tool_call_count as i64,
                session.cost.input_tokens as i64,
                session.cost.output_tokens as i64,
                session.cost.cache_read_tokens as i64,
                session.cost.cache_write_tokens as i64,
                session.cost.reasoning_tokens as i64,
                session.cost.estimated_cost_usd,
                session.title,
            ],
        ).context("Failed to create session")?;

        debug!("Created session {}", session.id);
        Ok(())
    }

    /// Get a session by ID.
    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let conn = self.conn.lock();

        let mut stmt = conn.prepare(
            "SELECT id, source, user_id, model, model_config, system_prompt,
                    parent_session_id, started_at, ended_at, end_reason,
                    message_count, tool_call_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    estimated_cost_usd, api_call_count, title
             FROM sessions WHERE id = ?1"
        ).context("Failed to prepare session query")?;

        let result = stmt.query_row(params![session_id], |row| {
            Ok(Session {
                id: row.get(0)?,
                source: row.get(1)?,
                user_id: row.get(2)?,
                model: row.get::<_, Option<String>>(3)?.and_then(|s| ModelRef::parse(&s)),
                model_config: row.get(4)?,
                system_prompt: row.get(5)?,
                parent_session_id: row.get(6)?,
                started_at: timestamp_to_datetime(row.get::<_, f64>(7)?),
                ended_at: row.get::<_, Option<f64>>(8)?.map(|ts| timestamp_to_datetime(ts)),
                end_reason: row.get(9)?,
                message_count: row.get::<_, i64>(10)? as u64,
                tool_call_count: row.get::<_, i64>(11)? as u64,
                cost: CostTracker {
                    input_tokens: row.get::<_, i64>(12)? as u64,
                    output_tokens: row.get::<_, i64>(13)? as u64,
                    cache_read_tokens: row.get::<_, i64>(14)? as u64,
                    cache_write_tokens: row.get::<_, i64>(15)? as u64,
                    reasoning_tokens: row.get::<_, i64>(16)? as u64,
                    estimated_cost_usd: row.get(17)?,
                    api_call_count: row.get::<_, i64>(18)? as u64,
                },
                title: row.get(19)?,
            })
        });

        match result {
            Ok(session) => Ok(Some(session)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("Failed to get session"),
        }
    }

    /// Update session system prompt.
    pub fn update_system_prompt(&self, session_id: &str, prompt: &str) -> Result<()> {
        let conn = self.conn.lock();

        conn.execute(
            "UPDATE sessions SET system_prompt = ?1 WHERE id = ?2",
            params![prompt, session_id],
        ).context("Failed to update system prompt")?;

        Ok(())
    }

    /// Update session stats (cost tracking).
    pub fn update_session_stats(&self, session_id: &str, stats: &CostTracker) -> Result<()> {
        let conn = self.conn.lock();

        conn.execute(
            "UPDATE sessions SET
                input_tokens = ?1,
                output_tokens = ?2,
                cache_read_tokens = ?3,
                cache_write_tokens = ?4,
                reasoning_tokens = ?5,
                estimated_cost_usd = ?6,
                api_call_count = ?7
             WHERE id = ?8",
            params![
                stats.input_tokens as i64,
                stats.output_tokens as i64,
                stats.cache_read_tokens as i64,
                stats.cache_write_tokens as i64,
                stats.reasoning_tokens as i64,
                stats.estimated_cost_usd,
                stats.api_call_count as i64,
                session_id,
            ],
        ).context("Failed to update session stats")?;

        Ok(())
    }

    /// End a session.
    pub fn end_session(&self, session_id: &str, reason: &str) -> Result<()> {
        let conn = self.conn.lock();
        let ended_ts = Utc::now().timestamp_millis() as f64 / 1000.0;

        conn.execute(
            "UPDATE sessions SET ended_at = ?1, end_reason = ?2 WHERE id = ?3",
            params![ended_ts, reason, session_id],
        ).context("Failed to end session")?;

        Ok(())
    }

    /// Get session summaries (for listing).
    pub fn get_session_summaries(&self, limit: usize) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock();

        let mut stmt = conn.prepare(
            "SELECT id, source, title, started_at, message_count, estimated_cost_usd
             FROM sessions
             ORDER BY started_at DESC
             LIMIT ?1"
        ).context("Failed to prepare summaries query")?;

        let summaries = stmt.query_map(params![limit as i64], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                source: row.get(1)?,
                title: row.get(2)?,
                started_at: timestamp_to_datetime(row.get::<_, f64>(3)?),
                message_count: row.get::<_, i64>(4)? as u64,
                estimated_cost_usd: row.get(5)?,
            })
        }).context("Failed to query summaries")?;

        summaries.collect::<Result<Vec<_>, _>>().context("Failed to collect summaries")
    }

    // ========================================================================
    // Message CRUD
    // ========================================================================

    /// Add a message to a session.
    pub fn add_message(&self, session_id: &str, message: &Message) -> Result<i64> {
        let conn = self.conn.lock();

        let content_str = message.content.as_ref().map(|c| c.to_string_repr());
        let tool_calls_str = message.tool_calls.as_ref()
            .map(|tc| serde_json::to_string(tc).unwrap_or_default());
        let timestamp = Utc::now().timestamp_millis() as f64 / 1000.0;

        // Insert message
        conn.execute(
            "INSERT INTO messages (
                session_id, role, content, tool_call_id, tool_calls, tool_name,
                timestamp, token_count, finish_reason, reasoning
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                session_id,
                message.role.to_string(),
                content_str,
                message.tool_call_id,
                tool_calls_str,
                message.name,
                timestamp,
                None::<i64>, // token_count (placeholder)
                None::<String>, // finish_reason
                message.reasoning,
            ],
        ).context("Failed to add message")?;

        let message_id = conn.last_insert_rowid();

        // Update session message count
        conn.execute(
            "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
            params![session_id],
        ).context("Failed to update message count")?;

        debug!("Added message {} to session {}", message_id, session_id);
        Ok(message_id)
    }

    /// Get all messages for a session.
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let conn = self.conn.lock();

        let mut stmt = conn.prepare(
            "SELECT role, content, tool_call_id, tool_calls, tool_name, reasoning
             FROM messages
             WHERE session_id = ?1
             ORDER BY timestamp ASC"
        ).context("Failed to prepare messages query")?;

        let messages = stmt.query_map(params![session_id], |row| {
            let role_str: String = row.get(0)?;
            let role = parse_role(&role_str);
            let content_str: Option<String> = row.get(1)?;
            let tool_call_id: Option<String> = row.get(2)?;
            let tool_calls_str: Option<String> = row.get(3)?;
            let name: Option<String> = row.get(4)?;
            let reasoning: Option<String> = row.get(5)?;

            let tool_calls = tool_calls_str.and_then(|s| {
                serde_json::from_str(&s).ok()
            });

            let content = content_str.map(|s| crate::Content::text(s));

            Ok(Message {
                role,
                content,
                tool_calls,
                tool_call_id,
                name,
                reasoning,
            })
        }).context("Failed to query messages")?;

        messages.collect::<Result<Vec<_>, _>>().context("Failed to collect messages")
    }

    // ========================================================================
    // FTS5 Search
    // ========================================================================

    /// Search sessions by message content.
    pub fn search_sessions(
        &self,
        query: &str,
        source: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let conn = self.conn.lock();

        // Build FTS5 query
        let fts_query = format!("\"{}\"", query); // Exact phrase match

        let results = if let Some(src) = source {
            let mut stmt = conn.prepare(
                "SELECT m.session_id, s.source, s.title, m.content, m.timestamp
                 FROM messages_fts mfts
                 JOIN messages m ON m.id = mfts.rowid
                 JOIN sessions s ON s.id = m.session_id
                 WHERE messages_fts MATCH ?1 AND s.source = ?2
                 ORDER BY m.timestamp DESC
                 LIMIT ?3"
            ).context("Failed to prepare search query")?;

            stmt.query_map(params![fts_query, src, limit as i64], |row| {
                Ok(SearchResult {
                    session_id: row.get(0)?,
                    source: row.get(1)?,
                    session_title: row.get(2)?,
                    matched_content: row.get(3)?,
                    timestamp: timestamp_to_datetime(row.get::<_, f64>(4)?),
                })
            }).context("Failed to search messages")?
                .collect::<Result<Vec<_>, _>>().context("Failed to collect search results")?
        } else {
            let mut stmt = conn.prepare(
                "SELECT m.session_id, s.source, s.title, m.content, m.timestamp
                 FROM messages_fts mfts
                 JOIN messages m ON m.id = mfts.rowid
                 JOIN sessions s ON s.id = m.session_id
                 WHERE messages_fts MATCH ?1
                 ORDER BY m.timestamp DESC
                 LIMIT ?2"
            ).context("Failed to prepare search query")?;

            stmt.query_map(params![fts_query, limit as i64], |row| {
                Ok(SearchResult {
                    session_id: row.get(0)?,
                    source: row.get(1)?,
                    session_title: row.get(2)?,
                    matched_content: row.get(3)?,
                    timestamp: timestamp_to_datetime(row.get::<_, f64>(4)?),
                })
            }).context("Failed to search messages")?
                .collect::<Result<Vec<_>, _>>().context("Failed to collect search results")?
        };

        Ok(results)
    }

    /// Count total sessions.
    pub fn count_sessions(&self) -> Result<i64> {
        let conn = self.conn.lock();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions",
            [],
            |row| row.get(0)
        ).context("Failed to count sessions")?;

        Ok(count)
    }

    /// Count total messages.
    pub fn count_messages(&self) -> Result<i64> {
        let conn = self.conn.lock();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages",
            [],
            |row| row.get(0)
        ).context("Failed to count messages")?;

        Ok(count)
    }
}

// ============================================================================
// SearchResult
// ============================================================================

/// Result from FTS5 search.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Session ID containing the match.
    pub session_id: String,

    /// Source of the session.
    pub source: String,

    /// Session title.
    pub session_title: Option<String>,

    /// Matched message content.
    pub matched_content: Option<String>,

    /// Timestamp of the matched message.
    pub timestamp: DateTime<Utc>,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert Unix timestamp to DateTime.
fn timestamp_to_datetime(ts: f64) -> DateTime<Utc> {
    let secs = ts as i64;
    let nanos = ((ts - secs as f64) * 1_000_000_000.0) as u32;
    DateTime::from_timestamp(secs, nanos).unwrap_or_else(|| Utc::now())
}

/// Parse role string to Role enum.
fn parse_role(s: &str) -> crate::Role {
    match s {
        "system" => crate::Role::System,
        "user" => crate::Role::User,
        "assistant" => crate::Role::Assistant,
        "tool" => crate::Role::Tool,
        _ => crate::Role::User, // Default
    }
}

/// Get current schema version.
fn get_schema_version(conn: &Connection) -> Result<i32> {
    let version: i32 = conn.query_row(
        "SELECT value FROM schema_version WHERE key = 'version'",
        [],
        |row| row.get(0)
    ).unwrap_or(0); // Table doesn't exist yet = version 0

    Ok(version)
}

/// Migrate schema from old version to current.
fn migrate_schema(conn: &Connection, from_version: i32) -> Result<()> {
    info!("Migrating schema from version {} to {}", from_version, SCHEMA_VERSION);

    // Run migrations in order
    for version in (from_version + 1)..=SCHEMA_VERSION {
        match version {
            1 => create_schema(conn)?,
            // Future migrations would go here:
            // 2 => migrate_v1_to_v2(conn)?,
            // etc.
            _ => {}
        }
    }

    // Update version
    conn.execute(
        "UPDATE schema_version SET value = ?1 WHERE key = 'version'",
        params![SCHEMA_VERSION],
    ).or_else(|_| {
        // Insert if update failed (table might not have row)
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (key, value) VALUES ('version', ?1)",
            params![SCHEMA_VERSION],
        )
    }).context("Failed to update schema version")?;

    Ok(())
}

/// Create the initial schema.
fn create_schema(conn: &Connection) -> Result<()> {
    info!("Creating database schema");

    // Schema version table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            key TEXT PRIMARY KEY,
            value INTEGER NOT NULL
        )",
        [],
    ).context("Failed to create schema_version table")?;

    // Sessions table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
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
            api_call_count INTEGER DEFAULT 0,
            estimated_cost_usd REAL,
            title TEXT
        )",
        [],
    ).context("Failed to create sessions table")?;

    // Messages table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
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
        )",
        [],
    ).context("Failed to create messages table")?;

    // FTS5 virtual table
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
            content,
            content='messages',
            content_rowid='id'
        )",
        [],
    ).context("Failed to create FTS5 table")?;

    // FTS5 triggers for automatic sync
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
            INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
        END;

        CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
            INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
        END;

        CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
        END;"
    ).context("Failed to create FTS5 triggers")?;

    // Set initial schema version
    conn.execute(
        "INSERT INTO schema_version (key, value) VALUES ('version', 1)",
        [],
    ).context("Failed to set schema version")?;

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, Role};

    #[test]
    fn test_open_in_memory() {
        let db = SessionDB::open_in_memory().unwrap();
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn test_create_and_get_session() {
        let db = SessionDB::open_in_memory().unwrap();

        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        let retrieved = db.get_session(&session.id).unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, session.id);
        assert_eq!(retrieved.source, "cli");
    }

    #[test]
    fn test_session_not_found() {
        let db = SessionDB::open_in_memory().unwrap();

        let result = db.get_session("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_add_and_get_messages() {
        let db = SessionDB::open_in_memory().unwrap();

        // Create session
        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        // Add messages
        let msg1 = Message::user("Hello");
        let msg2 = Message::assistant("Hi there!");

        db.add_message(&session.id, &msg1).unwrap();
        db.add_message(&session.id, &msg2).unwrap();

        // Get messages
        let messages = db.get_messages(&session.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[1].role, Role::Assistant);
    }

    #[test]
    fn test_update_system_prompt() {
        let db = SessionDB::open_in_memory().unwrap();

        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        db.update_system_prompt(&session.id, "New prompt").unwrap();

        let retrieved = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(retrieved.system_prompt, Some("New prompt".to_string()));
    }

    #[test]
    fn test_update_session_stats() {
        let db = SessionDB::open_in_memory().unwrap();

        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        let stats = CostTracker {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 20,
            cache_write_tokens: 10,
            reasoning_tokens: 5,
            estimated_cost_usd: 0.01,
            api_call_count: 1,
        };

        db.update_session_stats(&session.id, &stats).unwrap();

        let retrieved = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(retrieved.cost.input_tokens, 100);
        assert_eq!(retrieved.cost.output_tokens, 50);
    }

    #[test]
    fn test_end_session() {
        let db = SessionDB::open_in_memory().unwrap();

        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        db.end_session(&session.id, "user_exit").unwrap();

        let retrieved = db.get_session(&session.id).unwrap().unwrap();
        assert!(retrieved.ended_at.is_some());
        assert_eq!(retrieved.end_reason, Some("user_exit".to_string()));
    }

    #[test]
    fn test_session_summaries() {
        let db = SessionDB::open_in_memory().unwrap();

        // Create multiple sessions
        for i in 0..5 {
            let session = Session::new("cli");
            db.create_session(&session).unwrap();

            // Add a message
            let msg = Message::user(format!("Message {}", i));
            db.add_message(&session.id, &msg).unwrap();
        }

        let summaries = db.get_session_summaries(10).unwrap();
        assert_eq!(summaries.len(), 5);
    }

    #[test]
    fn test_fts5_search() {
        let db = SessionDB::open_in_memory().unwrap();

        // Create sessions with messages
        let session1 = Session::new("cli");
        db.create_session(&session1).unwrap();
        db.add_message(&session1.id, &Message::user("The quick brown fox")).unwrap();

        let session2 = Session::new("cli");
        db.create_session(&session2).unwrap();
        db.add_message(&session2.id, &Message::user("The lazy dog")).unwrap();

        let session3 = Session::new("cli");
        db.create_session(&session3).unwrap();
        db.add_message(&session3.id, &Message::user("A quick brown bear")).unwrap();

        // Search for "quick brown"
        let results = db.search_sessions("quick brown", None, 10).unwrap();
        assert_eq!(results.len(), 2); // session1 and session3 match
    }

    #[test]
    fn test_fts5_search_by_source() {
        let db = SessionDB::open_in_memory().unwrap();

        // Create sessions from different sources
        let cli_session = Session::new("cli");
        db.create_session(&cli_session).unwrap();
        db.add_message(&cli_session.id, &Message::user("Test message from CLI")).unwrap();

        let telegram_session = Session::new("telegram");
        db.create_session(&telegram_session).unwrap();
        db.add_message(&telegram_session.id, &Message::user("Test message from Telegram")).unwrap();

        // Search only in CLI
        let results = db.search_sessions("Test", Some("cli"), 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source, "cli");
    }

    #[test]
    fn test_message_count_updates() {
        let db = SessionDB::open_in_memory().unwrap();

        let session = Session::new("cli");
        db.create_session(&session).unwrap();

        // Add 3 messages
        for i in 0..3 {
            db.add_message(&session.id, &Message::user(format!("Msg {}", i))).unwrap();
        }

        let retrieved = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(retrieved.message_count, 3);
    }
}