use std::collections::HashMap;

use h_core::ProviderId;

use crate::provider::{ApiMode, ProviderInfo};

/// Registry of model providers.
#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<ProviderId, ProviderInfo>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            providers: HashMap::new(),
        };
        registry.register_builtin();
        registry
    }

    fn register_builtin(&mut self) {
        let builtins = [
            ProviderInfo::anthropic(),
            ProviderInfo::openai(),
            ProviderInfo::openrouter(),
            ProviderInfo::nous(),
            ProviderInfo::xiaomi_mimo(),
            ProviderInfo::z_ai(),
            ProviderInfo::kimi(),
            ProviderInfo::minimax(),
            ProviderInfo::huggingface(),
            ProviderInfo::ollama(),
            ProviderInfo::mistral(),
            ProviderInfo::gemini(),
            ProviderInfo::groq(),
        ];
        for info in builtins {
            self.providers.insert(info.id.clone(), info);
        }
    }

    pub fn register(&mut self, info: ProviderInfo) {
        self.providers.insert(info.id.clone(), info);
    }

    pub fn get(&self, id: &ProviderId) -> Option<&ProviderInfo> {
        self.providers.get(id)
    }

    pub fn list(&self) -> Vec<&ProviderInfo> {
        self.providers.values().collect()
    }

    /// Resolve the API mode for a provider.
    pub fn api_mode_for(&self, provider: &ProviderId) -> ApiMode {
        self.get(provider)
            .map(|p| p.api_mode.clone())
            .unwrap_or(ApiMode::ChatCompletions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_providers() {
        let registry = ProviderRegistry::new();
        assert!(registry.get(&ProviderId::new("anthropic")).is_some());
        assert!(registry.get(&ProviderId::new("openai")).is_some());
        assert!(registry.get(&ProviderId::new("openrouter")).is_some());
        assert!(registry.get(&ProviderId::new("ollama")).is_some());
        assert!(registry.get(&ProviderId::new("gemini")).is_some());
        assert!(registry.get(&ProviderId::new("groq")).is_some());
    }

    #[test]
    fn test_provider_count() {
        let registry = ProviderRegistry::new();
        assert_eq!(registry.list().len(), 13);
    }

    #[test]
    fn test_custom_provider() {
        let mut registry = ProviderRegistry::new();
        registry.register(ProviderInfo::generic("http://localhost:8080/v1"));
        assert!(registry.get(&ProviderId::new("generic")).is_some());
    }

    #[test]
    fn test_api_mode() {
        let registry = ProviderRegistry::new();
        assert_eq!(
            registry.api_mode_for(&ProviderId::new("anthropic")),
            ApiMode::AnthropicMessages
        );
        assert_eq!(
            registry.api_mode_for(&ProviderId::new("openai")),
            ApiMode::ChatCompletions
        );
    }
}
