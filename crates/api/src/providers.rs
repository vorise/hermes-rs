//! Built-in provider configurations.
//!
//! Static configurations for all supported LLM providers.

use h_core::ProviderId;

/// Get the list of all built-in provider IDs.
pub fn builtin_provider_ids() -> Vec<ProviderId> {
    vec![
        ProviderId::new("nous"),
        ProviderId::new("openrouter"),
        ProviderId::new("anthropic"),
        ProviderId::new("openai"),
        ProviderId::new("mimo"),
        ProviderId::new("glm"),
        ProviderId::new("kimi"),
        ProviderId::new("minimax"),
        ProviderId::new("huggingface"),
        ProviderId::new("ollama"),
        ProviderId::new("mistral"),
        ProviderId::new("deepseek"),
        ProviderId::new("groq"),
        ProviderId::new("cerebras"),
        ProviderId::new("custom"),
    ]
}

/// Provider display names for UI.
pub const PROVIDER_DISPLAY_NAMES: &[(&str, &str)] = &[
    ("nous", "Nous Portal"),
    ("openrouter", "OpenRouter"),
    ("anthropic", "Anthropic"),
    ("openai", "OpenAI"),
    ("mimo", "Xiaomi MiMo"),
    ("glm", "z.ai/GLM"),
    ("kimi", "Kimi/Moonshot"),
    ("minimax", "MiniMax"),
    ("huggingface", "HuggingFace"),
    ("ollama", "Ollama (Local)"),
    ("mistral", "Mistral"),
    ("deepseek", "DeepSeek"),
    ("groq", "Groq"),
    ("cerebras", "Cerebras"),
    ("custom", "Custom Endpoint"),
];

/// Get display name for a provider.
pub fn get_display_name(provider_id: &str) -> Option<&'static str> {
    PROVIDER_DISPLAY_NAMES
        .iter()
        .find(|(id, _)| *id == provider_id)
        .map(|(_, name)| name)
        .map(|v| &**v)
}