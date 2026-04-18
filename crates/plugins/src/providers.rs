//! Memory plugin provider stubs.
//!
//! These providers are described in the Hermes spec but require external
//! services/APIs to function. Each stub implements the MemoryProvider trait
//! with `is_available() -> false` unless the required dependencies are present.

use anyhow::Result;
use h_core::memory_provider::{Fact, MemoryProvider, RetrievalResult};

/// Common stub implementation for external memory providers.
macro_rules! stub_provider {
    ($name:ident, $display_name:expr, $desc:expr, $env_var:expr, $provider_name:expr) => {
        #[doc = $display_name]
        #[doc = " memory provider."]
        pub struct $name;

        impl $name {
            pub fn new() -> Self {
                Self
            }
        }

        #[async_trait::async_trait]
        impl MemoryProvider for $name {
            fn name(&self) -> &str {
                $provider_name
            }

            fn description(&self) -> &str {
                $desc
            }

            fn is_available(&self) -> bool {
                std::env::var($env_var).is_ok()
            }

            async fn save(&self, _content: &str, _category: &str, _tags: &[&str]) -> Result<()> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn search(&self, _query: &str, _limit: usize) -> Result<Vec<RetrievalResult>> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn probe(&self, _entity: &str, _limit: usize) -> Result<Vec<RetrievalResult>> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn reason(&self, _entities: &[&str], _limit: usize) -> Result<Vec<RetrievalResult>> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn contradict(&self, _query: &str, _limit: usize) -> Result<Vec<RetrievalResult>> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn remove(&self, _fact_id: i64) -> Result<()> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn list(&self, _limit: usize) -> Result<Vec<Fact>> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn mark_helpful(&self, _fact_id: i64) -> Result<()> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn mark_unhelpful(&self, _fact_id: i64) -> Result<()> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn update_fact(&self, _fact_id: i64, _trust_score: Option<f64>, _content: Option<&str>) -> Result<()> {
                Err(anyhow::anyhow!("{} not configured. Set {}", $display_name, $env_var))
            }

            async fn prefetch(&self, _query: &str) -> Result<String> {
                Ok(String::new())
            }
        }
    };
}

stub_provider!(
    HonchoMemory,
    "Honcho",
    "Cross-session user modeling with dialectic Q&A, semantic search, and peer cards",
    "HONCHO_API_KEY",
    "honcho"
);

stub_provider!(
    OpenVikingMemory,
    "OpenViking",
    "Vector-based semantic memory via OpenViking REST API",
    "OPENVIKING_API_KEY",
    "openviking"
);

stub_provider!(
    RetainDBMemory,
    "RetainDB",
    "Persistent memory database with structured storage and configurable TTL",
    "RETAINDB_URL",
    "retaindb"
);

stub_provider!(
    SupermemoryMemory,
    "Supermemory",
    "AI-powered memory with automatic organization and retrieval via Supermemory API",
    "SUPERMEMORY_API_KEY",
    "supermemory"
);

stub_provider!(
    HindsightMemory,
    "Hindsight",
    "Retrospective memory system that learns from past conversations",
    "HINDSIGHT_API_KEY",
    "hindsight"
);

stub_provider!(
    ByteroverMemory,
    "Byterover",
    "Lightweight persistent memory with fast retrieval via Byterover service",
    "BYTEROVER_API_KEY",
    "byterover"
);

/// List all available memory plugin providers.
pub fn list_memory_plugins() -> Vec<(&'static str, Box<dyn MemoryProvider>)> {
    vec![
        ("honcho", Box::new(HonchoMemory::new())),
        ("openviking", Box::new(OpenVikingMemory::new())),
        ("retaindb", Box::new(RetainDBMemory::new())),
        ("supermemory", Box::new(SupermemoryMemory::new())),
        ("hindsight", Box::new(HindsightMemory::new())),
        ("byterover", Box::new(ByteroverMemory::new())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_honcho_not_available_without_env() {
        let provider = HonchoMemory::new();
        // HONCHO_API_KEY is not set in test env
        assert!(!provider.is_available());
        assert_eq!(provider.name(), "honcho");
    }

    #[test]
    fn test_openviking_not_available_without_env() {
        let provider = OpenVikingMemory::new();
        assert!(!provider.is_available());
        assert_eq!(provider.name(), "openviking");
    }

    #[test]
    fn test_retaindb_not_available_without_env() {
        let provider = RetainDBMemory::new();
        assert!(!provider.is_available());
        assert_eq!(provider.name(), "retaindb");
    }

    #[test]
    fn test_supermemory_not_available_without_env() {
        let provider = SupermemoryMemory::new();
        assert!(!provider.is_available());
        assert_eq!(provider.name(), "supermemory");
    }

    #[test]
    fn test_hindsight_not_available_without_env() {
        let provider = HindsightMemory::new();
        assert!(!provider.is_available());
        assert_eq!(provider.name(), "hindsight");
    }

    #[test]
    fn test_byterover_not_available_without_env() {
        let provider = ByteroverMemory::new();
        assert!(!provider.is_available());
        assert_eq!(provider.name(), "byterover");
    }

    #[test]
    fn test_list_all_plugins() {
        let plugins = list_memory_plugins();
        assert_eq!(plugins.len(), 6);
    }
}
