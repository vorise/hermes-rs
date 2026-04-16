//! Provider Registry for Hermes Agent.
//!
//! Manages registration and lookup of LLM providers with their configurations.

use h_core::ProviderId;
use std::collections::HashMap;

/// Information about a model provider.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    /// Provider identifier
    pub id: ProviderId,
    /// Human-readable name
    pub display_name: &'static str,
    /// Default API base URL
    pub default_base_url: &'static str,
    /// Environment variable for API key
    pub api_key_env: &'static str,
    /// Default model for this provider
    pub default_model: &'static str,
    /// Whether provider supports tool calling
    pub supports_tools: bool,
    /// Whether provider supports vision/image input
    pub supports_vision: bool,
    /// Whether provider supports extended reasoning
    pub supports_reasoning: bool,
}

/// Registry of all known providers.
#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<ProviderId, ProviderInfo>,
}

impl ProviderRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Create registry with all built-in providers registered.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        register_builtin_providers(&mut registry);
        registry
    }

    /// Register a provider.
    pub fn register(&mut self, info: ProviderInfo) {
        self.providers.insert(info.id.clone(), info);
    }

    /// Get provider information by ID.
    pub fn get(&self, id: &ProviderId) -> Option<&ProviderInfo> {
        self.providers.get(id)
    }

    /// List all registered providers.
    pub fn list(&self) -> Vec<&ProviderInfo> {
        self.providers.values().collect()
    }

    /// Check if a provider is registered.
    pub fn contains(&self, id: &ProviderId) -> bool {
        self.providers.contains_key(id)
    }

    /// Get the number of registered providers.
    pub fn count(&self) -> usize {
        self.providers.len()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

/// Register all built-in providers.
fn register_builtin_providers(registry: &mut ProviderRegistry) {
    // Nous Portal - Nous Research's own API
    registry.register(ProviderInfo {
        id: ProviderId::new("nous"),
        display_name: "Nous Portal",
        default_base_url: "https://api.nousresearch.com/v1",
        api_key_env: "NOUS_API_KEY",
        default_model: "hermes-3-llama-3.1-405b",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: true,
    });

    // OpenRouter - Aggregator with 200+ models
    registry.register(ProviderInfo {
        id: ProviderId::new("openrouter"),
        display_name: "OpenRouter",
        default_base_url: "https://openrouter.ai/api/v1",
        api_key_env: "OPENROUTER_API_KEY",
        default_model: "anthropic/claude-sonnet-4",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: true,
    });

    // Anthropic - Claude models
    registry.register(ProviderInfo {
        id: ProviderId::new("anthropic"),
        display_name: "Anthropic",
        default_base_url: "https://api.anthropic.com/v1",
        api_key_env: "ANTHROPIC_API_KEY",
        default_model: "claude-sonnet-4-20250514",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: true,
    });

    // OpenAI - GPT models
    registry.register(ProviderInfo {
        id: ProviderId::new("openai"),
        display_name: "OpenAI",
        default_base_url: "https://api.openai.com/v1",
        api_key_env: "OPENAI_API_KEY",
        default_model: "gpt-4o",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: false, // o-series uses different endpoint
    });

    // Xiaomi MiMo
    registry.register(ProviderInfo {
        id: ProviderId::new("mimo"),
        display_name: "Xiaomi MiMo",
        default_base_url: "https://platform.xiaomimimo.com/v1",
        api_key_env: "MIMO_API_KEY",
        default_model: "mimo-7b",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: false,
    });

    // z.ai/GLM (Zhipu AI)
    registry.register(ProviderInfo {
        id: ProviderId::new("glm"),
        display_name: "z.ai/GLM",
        default_base_url: "https://open.bigmodel.cn/api/paas/v4",
        api_key_env: "GLM_API_KEY",
        default_model: "glm-4-plus",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: true,
    });

    // Kimi/Moonshot
    registry.register(ProviderInfo {
        id: ProviderId::new("kimi"),
        display_name: "Kimi/Moonshot",
        default_base_url: "https://api.moonshot.cn/v1",
        api_key_env: "KIMI_API_KEY",
        default_model: "moonshot-v1-8k",
        supports_tools: true,
        supports_vision: false,
        supports_reasoning: false,
    });

    // MiniMax
    registry.register(ProviderInfo {
        id: ProviderId::new("minimax"),
        display_name: "MiniMax",
        default_base_url: "https://api.minimax.chat/v1",
        api_key_env: "MINIMAX_API_KEY",
        default_model: "abab6.5-chat",
        supports_tools: true,
        supports_vision: false,
        supports_reasoning: false,
    });

    // HuggingFace
    registry.register(ProviderInfo {
        id: ProviderId::new("huggingface"),
        display_name: "HuggingFace",
        default_base_url: "https://api-inference.huggingface.co/models",
        api_key_env: "HF_TOKEN",
        default_model: "meta-llama/Llama-3.3-70B-Instruct",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: false,
    });

    // Ollama - Local models
    registry.register(ProviderInfo {
        id: ProviderId::new("ollama"),
        display_name: "Ollama",
        default_base_url: "http://localhost:11434/v1",
        api_key_env: "OLLAMA_API_KEY", // Usually empty for local
        default_model: "llama3.3",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: false,
    });

    // Mistral
    registry.register(ProviderInfo {
        id: ProviderId::new("mistral"),
        display_name: "Mistral",
        default_base_url: "https://api.mistral.ai/v1",
        api_key_env: "MISTRAL_API_KEY",
        default_model: "mistral-large-latest",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: false,
    });

    // DeepSeek
    registry.register(ProviderInfo {
        id: ProviderId::new("deepseek"),
        display_name: "DeepSeek",
        default_base_url: "https://api.deepseek.com/v1",
        api_key_env: "DEEPSEEK_API_KEY",
        default_model: "deepseek-chat",
        supports_tools: true,
        supports_vision: false,
        supports_reasoning: true,
    });

    // Groq - Fast inference
    registry.register(ProviderInfo {
        id: ProviderId::new("groq"),
        display_name: "Groq",
        default_base_url: "https://api.groq.com/openai/v1",
        api_key_env: "GROQ_API_KEY",
        default_model: "llama-3.3-70b-versatile",
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: false,
    });

    // Cerebras - Ultra-fast inference
    registry.register(ProviderInfo {
        id: ProviderId::new("cerebras"),
        display_name: "Cerebras",
        default_base_url: "https://api.cerebras.ai/v1",
        api_key_env: "CEREBRAS_API_KEY",
        default_model: "llama-3.3-70b",
        supports_tools: true,
        supports_vision: false,
        supports_reasoning: false,
    });

    // Generic OpenAI-compatible endpoint
    registry.register(ProviderInfo {
        id: ProviderId::new("custom"),
        display_name: "Custom Endpoint",
        default_base_url: "", // Must be configured
        api_key_env: "CUSTOM_API_KEY",
        default_model: "", // Must be configured
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: false,
    });
}

/// Resolve API key for a provider from environment.
pub fn resolve_api_key(provider: &ProviderInfo) -> Option<String> {
    std::env::var(provider.api_key_env).ok()
}

/// Resolve base URL for a provider (env override or default).
pub fn resolve_base_url(provider: &ProviderInfo) -> String {
    // Check for environment override (e.g., OPENAI_BASE_URL)
    let env_override = format!("{}_BASE_URL", provider.id.as_str().to_uppercase());
    std::env::var(&env_override)
        .ok()
        .unwrap_or_else(|| provider.default_base_url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_with_builtins() {
        let registry = ProviderRegistry::with_builtins();
        assert!(registry.count() >= 15);
    }

    #[test]
    fn test_get_provider() {
        let registry = ProviderRegistry::with_builtins();
        let provider = registry.get(&ProviderId::new("openai")).unwrap();
        assert_eq!(provider.display_name, "OpenAI");
        assert!(provider.supports_tools);
    }

    #[test]
    fn test_provider_not_found() {
        let registry = ProviderRegistry::with_builtins();
        assert!(registry.get(&ProviderId::new("nonexistent")).is_none());
    }

    #[test]
    fn test_resolve_base_url_default() {
        // Clear any env override to test default (unsafe is required for remove_var)
        unsafe { std::env::remove_var("ANTHROPIC_BASE_URL") };
        let registry = ProviderRegistry::with_builtins();
        let provider = registry.get(&ProviderId::new("anthropic")).unwrap();
        let url = resolve_base_url(provider);
        assert!(url.contains("anthropic.com") || url == provider.default_base_url);
    }

    #[test]
    fn test_provider_info_fields() {
        let registry = ProviderRegistry::with_builtins();
        let openrouter = registry.get(&ProviderId::new("openrouter")).unwrap();
        assert_eq!(openrouter.api_key_env, "OPENROUTER_API_KEY");
        assert!(openrouter.supports_vision);
        assert!(openrouter.supports_reasoning);
    }
}