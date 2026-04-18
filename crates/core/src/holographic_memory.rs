use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::memory_provider::{
    Entity, Fact, MemoryProvider, RetrievalResult,
};

/// HRR (Holographic Reduced Representation) dimension.
const HRR_DIM: usize = 1024;

/// Scoring weights for hybrid retrieval.
const FTS_WEIGHT: f64 = 0.4;
const JACCARD_WEIGHT: f64 = 0.3;
const HRR_WEIGHT: f64 = 0.3;

/// Default trust score for new facts.
const DEFAULT_TRUST: f64 = 0.5;

/// Minimum trust threshold for retrieval.
const MIN_TRUST: f64 = 0.3;

/// Trust delta for helpful feedback.
const TRUST_HELPFUL: f64 = 0.05;

/// Trust delta for unhelpful feedback.
const TRUST_UNHELPFUL: f64 = -0.10;

/// Holographic Memory — SQLite fact store with FTS5, trust scoring, and temporal decay.
pub struct HolographicMemory {
    db_path: PathBuf,
    conn: parking_lot::Mutex<Connection>,
    default_trust: f64,
    min_trust_threshold: f64,
    temporal_decay_half_life: f64, // days, 0 = disabled
}

impl HolographicMemory {
    /// Create a new HolographicMemory with the given database path.
    pub fn new(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open holographic DB: {}", db_path.display()))?;

        Self::init_schema(&conn)?;

        Ok(Self {
            db_path,
            conn: parking_lot::Mutex::new(conn),
            default_trust: DEFAULT_TRUST,
            min_trust_threshold: MIN_TRUST,
            temporal_decay_half_life: 0.0, // disabled by default
        })
    }

    /// Create with default path (~/.hermes/memory_store.db).
    pub fn new_default() -> Result<Self> {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let db_path = home.join(".hermes").join("memory_store.db");
        Self::new(db_path)
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS facts (
                fact_id         INTEGER PRIMARY KEY AUTOINCREMENT,
                content         TEXT NOT NULL UNIQUE,
                category        TEXT DEFAULT 'general',
                tags            TEXT DEFAULT '',
                trust_score     REAL DEFAULT 0.5,
                retrieval_count INTEGER DEFAULT 0,
                helpful_count   INTEGER DEFAULT 0,
                created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS entities (
                entity_id   INTEGER PRIMARY KEY AUTOINCREMENT,
                name        TEXT NOT NULL UNIQUE,
                entity_type TEXT DEFAULT 'unknown',
                aliases     TEXT DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS fact_entities (
                fact_id   INTEGER REFERENCES facts(fact_id),
                entity_id INTEGER REFERENCES entities(entity_id),
                PRIMARY KEY (fact_id, entity_id)
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts
                USING fts5(content, tags, content=facts, content_rowid=fact_id);

            -- FTS5 triggers
            CREATE TRIGGER IF NOT EXISTS facts_fts_insert AFTER INSERT ON facts BEGIN
                INSERT INTO facts_fts(rowid, content, tags)
                VALUES (new.fact_id, new.content, new.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS facts_fts_delete AFTER DELETE ON facts BEGIN
                INSERT INTO facts_fts(facts_fts, rowid, content, tags)
                VALUES ('delete', old.fact_id, old.content, old.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS facts_fts_update AFTER UPDATE ON facts BEGIN
                INSERT INTO facts_fts(facts_fts, rowid, content, tags)
                VALUES ('delete', old.fact_id, old.content, old.tags);
                INSERT INTO facts_fts(rowid, content, tags)
                VALUES (new.fact_id, new.content, new.tags);
            END;
            ",
        )?;

        Ok(())
    }

    /// Add a fact with entities.
    pub fn add_fact(
        &self,
        content: &str,
        category: &str,
        tags: &[&str],
        entities: &[&str],
    ) -> Result<i64> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        let tags_str = tags.join(",");

        tx.execute(
            "INSERT OR IGNORE INTO facts (content, category, tags, trust_score)
             VALUES (?1, ?2, ?3, ?4)",
            (content, category, tags_str, self.default_trust),
        )?;

        let fact_id = tx.last_insert_rowid();

        // Associate entities
        for entity_name in entities {
            // Insert or get entity
            let entity_str = entity_name.to_string();
            tx.execute(
                "INSERT OR IGNORE INTO entities (name, entity_type) VALUES (?1, ?2)",
                (&entity_str, "auto"),
            )?;

            let entity_id: i64 = tx.query_row(
                "SELECT entity_id FROM entities WHERE name = ?1",
                [&entity_str],
                |row| row.get(0),
            )?;

            tx.execute(
                "INSERT OR IGNORE INTO fact_entities (fact_id, entity_id) VALUES (?1, ?2)",
                (fact_id, entity_id),
            )?;
        }

        tx.commit()?;
        Ok(fact_id)
    }

    /// Search facts using FTS5 + Jaccard hybrid retrieval.
    fn _search(&self, query: &str, limit: usize) -> Result<Vec<RetrievalResult>> {
        let conn = self.conn.lock();

        // Stage 1: FTS5 search — get limit * 3 candidates
        let candidates = self.fts5_search(&conn, query, limit * 3)?;

        // Stage 2: Jaccard rerank + trust weighting
        let mut scored: Vec<(Fact, Vec<Entity>, f64)> = Vec::new();
        let query_lower = query.to_lowercase();
        let query_tokens: Vec<&str> = query_lower.split_whitespace().collect();

        for (fact, entities) in candidates {
            if fact.trust_score < self.min_trust_threshold {
                continue;
            }

            // Jaccard similarity
            let fact_content_lower = fact.content.to_lowercase();
            let fact_tokens: Vec<&str> = fact_content_lower
                .split_whitespace()
                .collect();
            let jaccard = jaccard_similarity(&query_tokens, &fact_tokens);

            // HRR similarity (simplified — full implementation uses vector math)
            let hrr_score = 0.0;

            // Combined score: FTS * 0.4 + Jaccard * 0.3 + HRR * 0.3
            let fts_score = 1.0; // Already from FTS5 ranking
            let combined = fts_score * FTS_WEIGHT
                + jaccard * JACCARD_WEIGHT
                + hrr_score * HRR_WEIGHT;

            // Apply trust weighting
            let final_score = combined * fact.trust_score;

            scored.push((fact, entities, final_score));
        }

        // Sort by score descending
        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        // Apply temporal decay
        if self.temporal_decay_half_life > 0.0 {
            for (fact, _entities, score) in &mut scored {
                let age_days = fact_age_days(&fact.created_at);
                let decay = 0.5f64.powf(age_days / self.temporal_decay_half_life);
                *score *= decay;
            }
        }

        let results: Vec<RetrievalResult> = scored
            .into_iter()
            .take(limit)
            .map(|(fact, entities, score)| RetrievalResult {
                fact,
                score,
                entities,
            })
            .collect();

        Ok(results)
    }

    fn fts5_search(
        &self,
        conn: &Connection,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(Fact, Vec<Entity>)>> {
        let mut stmt = conn.prepare(
            "SELECT f.fact_id, f.content, f.category, f.tags,
                    f.trust_score, f.retrieval_count, f.helpful_count,
                    f.created_at, f.updated_at
             FROM facts f
             JOIN facts_fts fts ON f.fact_id = fts.rowid
             WHERE facts_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let rows = stmt.query_map([query, &limit.to_string()], |row| {
            Ok(Fact {
                id: Some(row.get(0)?),
                content: row.get(1)?,
                category: row.get(2)?,
                tags: row.get(3)?,
                trust_score: row.get(4)?,
                retrieval_count: row.get(5)?,
                helpful_count: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            let fact = row?;
            let entities = self.get_entities_for_fact(conn, fact.id.unwrap_or(0))?;
            results.push((fact, entities));
        }

        Ok(results)
    }

    fn get_entities_for_fact(&self, conn: &Connection, fact_id: i64) -> Result<Vec<Entity>> {
        let mut stmt = conn.prepare(
            "SELECT e.entity_id, e.name, e.entity_type, e.aliases
             FROM entities e
             JOIN fact_entities fe ON e.entity_id = fe.entity_id
             WHERE fe.fact_id = ?1",
        )?;

        let rows = stmt.query_map([fact_id], |row| {
            Ok(Entity {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                entity_type: row.get(2)?,
                aliases: row.get(3)?,
            })
        })?;

        let mut entities = Vec::new();
        for row in rows {
            entities.push(row?);
        }

        Ok(entities)
    }

    /// Probe: get all facts about a named entity.
    pub fn probe_entity(&self, entity_name: &str, limit: usize) -> Result<Vec<RetrievalResult>> {
        let conn = self.conn.lock();

        let mut stmt = conn.prepare(
            "SELECT f.fact_id, f.content, f.category, f.tags,
                    f.trust_score, f.retrieval_count, f.helpful_count,
                    f.created_at, f.updated_at
             FROM facts f
             JOIN fact_entities fe ON f.fact_id = fe.fact_id
             JOIN entities e ON fe.entity_id = e.entity_id
             WHERE e.name = ?1 AND f.trust_score >= ?2
             LIMIT ?3",
        )?;

        let rows = stmt.query_map(
            [entity_name, &self.min_trust_threshold.to_string(), &limit.to_string()],
            |row| {
                Ok((
                    Fact {
                        id: Some(row.get(0)?),
                        content: row.get(1)?,
                        category: row.get(2)?,
                        tags: row.get(3)?,
                        trust_score: row.get(4)?,
                        retrieval_count: row.get(5)?,
                        helpful_count: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                    },
                    row.get::<_, i64>(0)?,
                ))
            },
        )?;

        let mut results = Vec::new();
        for row in rows {
            let (fact, fact_id) = row?;
            let trust = fact.trust_score;
            let entities = self.get_entities_for_fact(&conn, fact_id)?;
            results.push(RetrievalResult {
                fact,
                score: trust,
                entities,
            });
        }

        Ok(results)
    }

    /// Reason: find facts connected to multiple entities (compositional query).
    pub fn _reason(&self, entity_names: &[&str], limit: usize) -> Result<Vec<RetrievalResult>> {
        if entity_names.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock();

        // Build query to find facts associated with ALL given entities
        let placeholders = entity_names
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");

        let query = format!(
            "SELECT f.fact_id, f.content, f.category, f.tags,
                    f.trust_score, f.retrieval_count, f.helpful_count,
                    f.created_at, f.updated_at,
                    COUNT(DISTINCT e.name) as match_count
             FROM facts f
             JOIN fact_entities fe ON f.fact_id = fe.fact_id
             JOIN entities e ON fe.entity_id = e.entity_id
             WHERE e.name IN ({placeholders})
             GROUP BY f.fact_id
             HAVING match_count = ?
             ORDER BY f.trust_score DESC
             LIMIT ?",
        );

        let mut stmt = conn.prepare(&query)?;
        // Build params: entity names + match_count + limit
        let mut params: Vec<&(dyn rusqlite::types::ToSql)> = Vec::new();
        for name in entity_names {
            params.push(name);
        }
        let match_count = entity_names.len().to_string();
        let limit_str = limit.to_string();
        params.push(&match_count);
        params.push(&limit_str);

        let rows = stmt.query_map(params.as_slice(), |row| {
                Ok((
                    Fact {
                        id: Some(row.get(0)?),
                        content: row.get(1)?,
                        category: row.get(2)?,
                        tags: row.get(3)?,
                        trust_score: row.get(4)?,
                        retrieval_count: row.get(5)?,
                        helpful_count: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                    },
                    row.get::<_, i64>(9)?,
                ))
            },
        )?;

        let mut results = Vec::new();
        for row in rows {
            let (fact, fact_id) = row?;
            let trust = fact.trust_score;
            let entities = self.get_entities_for_fact(&conn, fact_id)?;
            results.push(RetrievalResult {
                fact,
                score: trust,
                entities,
            });
        }

        Ok(results)
    }

    /// Find facts that may contradict a query.
    pub fn _contradict(&self, query: &str, limit: usize) -> Result<Vec<RetrievalResult>> {
        // Search for facts containing negation markers or conflicting terms
        let negation_terms = ["not", "no", "never", "wrong", "false", "incorrect", "but", "however"];

        // First do a normal search, then filter for negation patterns
        let results = self._search(query, limit * 3)?;

        let filtered: Vec<RetrievalResult> = results
            .into_iter()
            .filter(|r| {
                let content_lower = r.fact.content.to_lowercase();
                negation_terms.iter().any(|t| content_lower.contains(t))
            })
            .take(limit)
            .collect();

        Ok(filtered)
    }

    /// Mark a fact as helpful (trust +0.05).
    pub fn mark_helpful(&self, fact_id: i64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE facts SET trust_score = MIN(1.0, trust_score + ?1),
                    helpful_count = helpful_count + 1,
                    updated_at = CURRENT_TIMESTAMP
             WHERE fact_id = ?2",
            (TRUST_HELPFUL, fact_id),
        )?;
        Ok(())
    }

    /// Mark a fact as unhelpful (trust -0.10).
    pub fn mark_unhelpful(&self, fact_id: i64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE facts SET trust_score = MAX(0.0, trust_score + ?1),
                    updated_at = CURRENT_TIMESTAMP
             WHERE fact_id = ?2",
            (TRUST_UNHELPFUL, fact_id),
        )?;
        Ok(())
    }

    /// Update a fact's trust score or content.
    pub fn update_fact(
        &self,
        fact_id: i64,
        trust_score: Option<f64>,
        content: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock();
        if let Some(ts) = trust_score {
            conn.execute(
                "UPDATE facts SET trust_score = ?1, updated_at = CURRENT_TIMESTAMP WHERE fact_id = ?2",
                (ts, fact_id),
            )?;
        }
        if let Some(c) = content {
            conn.execute(
                "UPDATE facts SET content = ?1, updated_at = CURRENT_TIMESTAMP WHERE fact_id = ?2",
                (c, fact_id),
            )?;
        }
        Ok(())
    }

    /// Remove a fact by ID.
    pub fn remove_fact(&self, fact_id: i64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM facts WHERE fact_id = ?1", [fact_id])?;
        Ok(())
    }

    /// List all facts.
    pub fn list_facts(&self, limit: usize) -> Result<Vec<Fact>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT fact_id, content, category, tags, trust_score,
                    retrieval_count, helpful_count, created_at, updated_at
             FROM facts ORDER BY created_at DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map([&limit.to_string()], |row| {
            Ok(Fact {
                id: Some(row.get(0)?),
                content: row.get(1)?,
                category: row.get(2)?,
                tags: row.get(3)?,
                trust_score: row.get(4)?,
                retrieval_count: row.get(5)?,
                helpful_count: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        let mut facts = Vec::new();
        for row in rows {
            facts.push(row?);
        }

        Ok(facts)
    }
}

#[async_trait::async_trait]
impl MemoryProvider for HolographicMemory {
    fn name(&self) -> &str {
        "holographic"
    }

    fn description(&self) -> &str {
        "SQLite fact store with FTS5, entity resolution, trust scoring, and temporal decay"
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn save(&self, content: &str, category: &str, tags: &[&str]) -> Result<()> {
        // Auto-extract entities from content (simple NER via keyword detection)
        let entities = extract_entities(content);
        let entity_refs: Vec<&str> = entities.iter().map(|s| s.as_str()).collect();
        self.add_fact(content, category, tags, &entity_refs)?;
        Ok(())
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<RetrievalResult>> {
        self._search(query, limit)
    }

    async fn probe(&self, entity: &str, limit: usize) -> Result<Vec<RetrievalResult>> {
        self.probe_entity(entity, limit)
    }

    async fn reason(&self, entities: &[&str], limit: usize) -> Result<Vec<RetrievalResult>> {
        self._reason(entities, limit)
    }

    async fn contradict(&self, query: &str, limit: usize) -> Result<Vec<RetrievalResult>> {
        self._contradict(query, limit)
    }

    async fn remove(&self, fact_id: i64) -> Result<()> {
        self.remove_fact(fact_id)
    }

    async fn list(&self, limit: usize) -> Result<Vec<Fact>> {
        self.list_facts(limit)
    }

    async fn mark_helpful(&self, fact_id: i64) -> Result<()> {
        self.mark_helpful(fact_id)
    }

    async fn mark_unhelpful(&self, fact_id: i64) -> Result<()> {
        self.mark_unhelpful(fact_id)
    }

    async fn update_fact(
        &self,
        fact_id: i64,
        trust_score: Option<f64>,
        content: Option<&str>,
    ) -> Result<()> {
        self.update_fact(fact_id, trust_score, content)
    }

    async fn prefetch(&self, query: &str) -> Result<String> {
        let results = self.search(query, 10).await?;
        if results.is_empty() {
            return Ok(String::new());
        }

        let mut output = String::from("## Relevant Memories\n\n");
        for r in &results {
            output.push_str(&format!(
                "- [{:.2}] {} (trust: {:.2})\n",
                r.score, r.fact.content, r.fact.trust_score
            ));
        }
        output.push('\n');
        Ok(output)
    }
}

/// Simple named entity extraction (keyword-based for now).
fn extract_entities(content: &str) -> Vec<String> {
    // Simple heuristic: extract capitalized words that appear at word boundaries
    // This is a simplified version — real NER would use an ML model
    let mut entities = Vec::new();
    for word in content.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| !c.is_alphabetic());
        if cleaned.len() > 2
            && cleaned.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            && cleaned.chars().all(|c| c.is_alphabetic() || c == '-' || c == '\'')
        {
            let entity = cleaned.to_string();
            if !entities.contains(&entity) {
                entities.push(entity);
            }
        }
    }
    entities
}

/// Jaccard similarity between two token sets.
fn jaccard_similarity(a: &[&str], b: &[&str]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let set_a: std::collections::HashSet<&str> = a.iter().copied().collect();
    let set_b: std::collections::HashSet<&str> = b.iter().copied().collect();

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Calculate age of a fact in days.
fn fact_age_days(created_at: &str) -> f64 {
    // Parse ISO-like timestamp
    let created = chrono::DateTime::parse_from_rfc3339(created_at)
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S%.f")
            .map(|nd| nd.and_utc().fixed_offset()))
        .unwrap_or_else(|_| chrono::Utc::now().fixed_offset());

    let now = chrono::Utc::now().fixed_offset();
    let duration = now.signed_duration_since(created);
    duration.num_seconds() as f64 / 86400.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> PathBuf {
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        std::env::temp_dir().join(format!("hermes_holo_{id}.db"))
    }

    #[test]
    fn test_create_and_add_fact() {
        let db = temp_db();
        let memory = HolographicMemory::new(db.clone()).unwrap();
        let id = memory
            .add_fact("Rust is a systems programming language", "tech", &["rust", "programming"], &["Rust"])
            .unwrap();
        assert!(id > 0);

        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn test_search_returns_results() {
        let db = temp_db();
        let memory = HolographicMemory::new(db.clone()).unwrap();
        memory
            .add_fact("Rust is a systems programming language", "tech", &["rust"], &["Rust"])
            .unwrap();
        memory
            .add_fact("Python is great for data science", "tech", &["python"], &["Python"])
            .unwrap();

        let results = memory._search("Rust programming", 5).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].fact.content.contains("Rust"));

        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn test_trust_helpful_unhelpful() {
        let db = temp_db();
        let memory = HolographicMemory::new(db.clone()).unwrap();
        let id = memory
            .add_fact("Test fact", "test", &[], &[])
            .unwrap();

        memory.mark_helpful(id).unwrap();
        let facts = memory.list_facts(10).unwrap();
        assert!(facts[0].trust_score > DEFAULT_TRUST);

        memory.mark_unhelpful(id).unwrap();
        let facts = memory.list_facts(10).unwrap();
        // helpful +0.05, unhelpful -0.10 => net -0.05
        assert!(facts[0].trust_score < DEFAULT_TRUST);

        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn test_jaccard_similarity() {
        let a = vec!["hello", "world"];
        let b = vec!["hello", "rust"];
        let sim = jaccard_similarity(&a, &b);
        assert!((sim - 0.3333).abs() < 0.01);
    }

    #[test]
    fn test_extract_entities() {
        let entities = extract_entities("Alice lives in Paris and works at Google");
        assert!(entities.contains(&"Alice".to_string()));
        assert!(entities.contains(&"Paris".to_string()));
        assert!(entities.contains(&"Google".to_string()));
    }
}
