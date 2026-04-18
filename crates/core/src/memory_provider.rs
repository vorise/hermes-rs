use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A single fact stored in the Holographic memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub id: Option<i64>,
    pub content: String,
    pub category: String,
    pub tags: String,
    pub trust_score: f64,
    pub retrieval_count: i64,
    pub helpful_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// An entity associated with a fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: Option<i64>,
    pub name: String,
    pub entity_type: String,
    pub aliases: String,
}

/// Retrieval result from the Holographic memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub fact: Fact,
    pub score: f64,
    pub entities: Vec<Entity>,
}

/// Action for the fact_store tool.
#[derive(Debug, Clone)]
pub enum FactAction {
    Add {
        content: String,
        category: String,
        tags: String,
        entities: Vec<String>,
    },
    Search {
        query: String,
        limit: usize,
    },
    Probe {
        entity: String,
        limit: usize,
    },
    Related {
        entity: String,
        limit: usize,
    },
    Reason {
        entities: Vec<String>,
        limit: usize,
    },
    Contradict {
        query: String,
        limit: usize,
    },
    Update {
        fact_id: i64,
        trust_score: Option<f64>,
        content: Option<String>,
    },
    Remove {
        fact_id: i64,
    },
    List {
        limit: usize,
    },
}

/// Feedback action for the fact_feedback tool.
#[derive(Debug, Clone)]
pub enum FeedbackAction {
    Helpful { fact_id: i64 },
    Unhelpful { fact_id: i64 },
}

/// MemoryProvider ABC — standard interface that all memory backends implement.
///
/// Only ONE provider can be active at a time, selected via `memory.provider`
/// in `~/.hermes/config.yaml`.
#[async_trait::async_trait]
pub trait MemoryProvider: Send + Sync {
    /// Returns the provider's unique name (e.g., "holographic", "honcho").
    fn name(&self) -> &str;

    /// Returns a human-readable description of the provider.
    fn description(&self) -> &str;

    /// Check if this provider is available (dependencies met, connectivity OK).
    fn is_available(&self) -> bool;

    /// Save a memory fact.
    async fn save(&self, content: &str, category: &str, tags: &[&str]) -> Result<()>;

    /// Search memories by query.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<RetrievalResult>>;

    /// Retrieve all facts about a named entity (entity recall).
    async fn probe(&self, entity: &str, limit: usize) -> Result<Vec<RetrievalResult>>;

    /// Find facts connected to multiple entities (compositional query).
    async fn reason(&self, entities: &[&str], limit: usize) -> Result<Vec<RetrievalResult>>;

    /// Find facts that may contradict the given query.
    async fn contradict(&self, query: &str, limit: usize) -> Result<Vec<RetrievalResult>>;

    /// Delete a fact by ID.
    async fn remove(&self, fact_id: i64) -> Result<()>;

    /// List all facts.
    async fn list(&self, limit: usize) -> Result<Vec<Fact>>;

    /// Mark a fact as helpful (trust +0.05).
    async fn mark_helpful(&self, fact_id: i64) -> Result<()>;

    /// Mark a fact as unhelpful (trust -0.10).
    async fn mark_unhelpful(&self, fact_id: i64) -> Result<()>;

    /// Update a fact's trust score or content.
    async fn update_fact(
        &self,
        fact_id: i64,
        trust_score: Option<f64>,
        content: Option<&str>,
    ) -> Result<()>;

    /// Prefetch relevant memories for the current query (for prompt injection).
    async fn prefetch(&self, query: &str) -> Result<String>;
}

/// Metadata about a discovered memory provider.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub name: String,
    pub description: String,
    pub is_available: bool,
}

/// Discover available memory providers.
///
/// Scans `~/.hermes/plugins/memory/` directories for provider plugins.
/// Each plugin directory should contain a `plugin.yaml` with metadata.
pub fn discover_memory_providers() -> Vec<ProviderInfo> {
    let mut providers = Vec::new();

    // Add built-in providers
    providers.push(ProviderInfo {
        name: "holographic".to_string(),
        description: "SQLite fact store with FTS5, trust scoring, and temporal decay".to_string(),
        is_available: true,
    });

    // Scan plugin directory
    let plugin_dir = std::env::var("HERMES_HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join("plugins/memory"))
        .or_else(|| {
            std::env::var("HOME").ok().map(|h| {
                std::path::PathBuf::from(h)
                    .join(".hermes/plugins/memory")
            })
        });

    if let Some(dir) = plugin_dir {
        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let plugin_yaml = path.join("plugin.yaml");
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();

                        let description = if plugin_yaml.exists() {
                            std::fs::read_to_string(&plugin_yaml)
                                .ok()
                                .and_then(|content| {
                                    content
                                        .lines()
                                        .find(|l| l.starts_with("description:"))
                                        .map(|l| l.strip_prefix("description:").unwrap_or("").trim().to_string())
                                })
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };

                        providers.push(ProviderInfo {
                            name,
                            description,
                            is_available: false, // Plugin not loaded yet
                        });
                    }
                }
            }
        }
    }

    providers
}

/// Load a memory provider by name.
///
/// Returns None if the provider is not found or cannot be loaded.
pub fn load_memory_provider(name: &str) -> Option<Box<dyn MemoryProvider>> {
    match name {
        "holographic" => {
            // Built-in provider
            if let Ok(provider) = crate::holographic_memory::HolographicMemory::new_default() {
                Some(Box::new(provider))
            } else {
                None
            }
        }
        _ => {
            // Try to load from plugin directory
            let plugin_dir = std::env::var("HERMES_HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join("plugins/memory"))
                .or_else(|| {
                    std::env::var("HOME").ok().map(|h| {
                        std::path::PathBuf::from(h)
                            .join(".hermes/plugins/memory")
                    })
                });

            if let Some(dir) = plugin_dir {
                let plugin_path = dir.join(name);
                if plugin_path.exists() {
                    // Plugin exists but dynamic loading requires special handling
                    // For now, log that it was found
                    tracing::warn!("Plugin '{name}' found but dynamic loading not implemented");
                }
                None
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_providers_has_holographic() {
        let providers = discover_memory_providers();
        let holographic = providers.iter().find(|p| p.name == "holographic");
        assert!(holographic.is_some());
        assert!(holographic.unwrap().is_available);
    }

    #[test]
    fn test_load_holographic_provider() {
        // Should return Some when SQLite is available
        let provider = load_memory_provider("holographic");
        assert!(provider.is_some());
        assert_eq!(provider.as_ref().unwrap().name(), "holographic");
    }

    #[test]
    fn test_load_unknown_provider() {
        let provider = load_memory_provider("nonexistent");
        assert!(provider.is_none());
    }
}
