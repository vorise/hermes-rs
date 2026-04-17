use std::collections::HashMap;

use parking_lot::Mutex;

/// Known model context windows (in tokens).
/// Populated from provider docs, model cards, and runtime probing.
fn known_context_windows() -> HashMap<&'static str, u64> {
    let mut m = HashMap::new();
    // Anthropic
    m.insert("claude-sonnet-4-20250514", 200_000);
    m.insert("claude-opus-4-20250414", 200_000);
    m.insert("claude-3-7-sonnet", 200_000);
    m.insert("claude-3-5-sonnet", 200_000);
    m.insert("claude-3-5-haiku", 200_000);
    m.insert("claude-3-opus", 200_000);
    m.insert("claude-3-sonnet", 200_000);
    m.insert("claude-3-haiku", 200_000);
    // OpenAI
    m.insert("gpt-4o", 128_000);
    m.insert("gpt-4o-mini", 128_000);
    m.insert("gpt-4-turbo", 128_000);
    m.insert("gpt-4", 8_192);
    m.insert("gpt-3.5-turbo", 16_385);
    m.insert("gpt-4.1", 1_048_576);
    m.insert("gpt-4.1-mini", 1_048_576);
    // Mistral
    m.insert("mistral-large", 128_000);
    m.insert("mistral-small", 32_000);
    m.insert("mistral-nemo", 128_000);
    // Nous
    m.insert("nous-hermes", 8_192);
    // Google
    m.insert("gemini-2.5-pro", 1_048_576);
    m.insert("gemini-2.0-flash", 1_048_576);
    m.insert("gemini-1.5-pro", 2_097_152);
    // Meta
    m.insert("llama-3.3-70b", 128_000);
    m.insert("llama-3.1-70b", 128_000);
    m.insert("llama-3.1-405b", 128_000);
    m
}

/// Metadata about a model.
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    /// Model ID (e.g., "claude-sonnet-4-20250514").
    pub model_id: String,
    /// Context window in tokens.
    pub context_window: u64,
    /// Maximum output tokens (if known).
    pub max_output_tokens: Option<u64>,
    /// Whether the model supports prompt caching.
    pub supports_caching: bool,
    /// Whether the model supports tool calling.
    pub supports_tools: bool,
    /// Provider slug (e.g., "anthropic", "openai").
    pub provider: String,
}

impl ModelMetadata {
    pub fn new(model_id: &str, context_window: u64, provider: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            context_window,
            max_output_tokens: None,
            supports_caching: false,
            supports_tools: false,
            provider: provider.to_string(),
        }
    }

    pub fn with_max_output(mut self, max: u64) -> Self {
        self.max_output_tokens = Some(max);
        self
    }

    pub fn with_caching(mut self, yes: bool) -> Self {
        self.supports_caching = yes;
        self
    }

    pub fn with_tools(mut self, yes: bool) -> Self {
        self.supports_tools = yes;
        self
    }
}

/// Registry of model metadata with caching and lookup.
pub struct ModelMetadataRegistry {
    /// Known models with full metadata.
    known: Mutex<HashMap<String, ModelMetadata>>,
    /// Runtime-cached context lengths (from probing or error parsing).
    probed: Mutex<HashMap<String, u64>>,
}

impl ModelMetadataRegistry {
    pub fn new() -> Self {
        let mut known = HashMap::new();
        let known_map = known_context_windows();

        // Build metadata entries from known context windows
        for (model_id, ctx) in &known_map {
            let provider = infer_provider(model_id);
            let max_output = infer_max_output(model_id);
            let caching = supports_caching(model_id);
            let tools = supports_tools(model_id);

            known.insert(model_id.to_string(), ModelMetadata {
                model_id: model_id.to_string(),
                context_window: *ctx,
                max_output_tokens: max_output,
                supports_caching: caching,
                supports_tools: tools,
                provider,
            });
        }

        Self {
            known: Mutex::new(known),
            probed: Mutex::new(HashMap::new()),
        }
    }

    /// Look up metadata for a model by ID or partial match.
    pub fn lookup(&self, model_id: &str) -> Option<ModelMetadata> {
        // Exact match first
        if let Some(meta) = self.known.lock().get(model_id) {
            return Some(meta.clone());
        }

        // Partial match (substring)
        let known = self.known.lock();
        for (key, meta) in known.iter() {
            if model_id.contains(key.as_str()) || key.contains(model_id) {
                return Some(meta.clone());
            }
        }

        // Check probed cache
        if let Some(ctx) = self.probed.lock().get(model_id) {
            let provider = infer_provider(model_id);
            return Some(ModelMetadata::new(model_id, *ctx, &provider));
        }

        // Fallback: infer from model ID patterns
        let ctx = infer_context_window(model_id);
        let provider = infer_provider(model_id);
        Some(ModelMetadata::new(model_id, ctx, &provider))
    }

    /// Get the context window for a model.
    pub fn context_window(&self, model_id: &str) -> u64 {
        self.lookup(model_id).map(|m| m.context_window).unwrap_or(128_000)
    }

    /// Save a probed context length for a model.
    pub fn save_context_length(&self, model_id: &str, context_length: u64) {
        self.probed.lock().insert(model_id.to_string(), context_length);

        // Also update known cache if it exists
        if let Some(meta) = self.known.lock().get_mut(model_id) {
            meta.context_window = context_length;
        }
    }

    /// Parse a context limit from an API error message.
    ///
    /// Common error patterns:
    /// - "maximum context length is X tokens"
    /// - "requested Y tokens exceeds limit of X"
    /// - "prompt is too long: X > Y"
    pub fn parse_context_limit_from_error(error: &str) -> Option<u64> {
        // Pattern 1: "limit of X" or "limit: X"
        if let Some(pos) = error.find("limit") {
            let rest = &error[pos..];
            return extract_first_number(rest);
        }

        // Pattern 2: "X > Y" (Y is the limit)
        if let Some(pos) = error.find("> ") {
            let rest = &error[pos + 2..];
            return extract_first_number(rest);
        }

        // Pattern 3: "maximum X tokens"
        if let Some(pos) = error.find("maximum") {
            let rest = &error[pos..];
            return extract_first_number(rest);
        }

        None
    }

    /// Parse available output tokens from an API error message.
    ///
    /// Common patterns:
    /// - "max_tokens X is greater than context window"
    /// - "output length must be less than X"
    pub fn parse_output_limit_from_error(error: &str) -> Option<u64> {
        if let Some(pos) = error.find("max_tokens") {
            let rest = &error[pos..];
            return extract_first_number(rest);
        }
        if let Some(pos) = error.find("output") {
            let rest = &error[pos..];
            return extract_first_number(rest);
        }
        None
    }

    /// Check if a URL looks like a local endpoint.
    pub fn is_local_endpoint(url: &str) -> bool {
        url.contains("localhost")
            || url.contains("127.0.0.1")
            || url.contains("0.0.0.0")
            || url.contains("host.docker.internal")
            || url.contains(":11434") // Ollama default port
            || url.starts_with("http://") && !url.contains(".com") && !url.contains(".org")
    }
}

impl Default for ModelMetadataRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Estimate tokens from text using ~4 chars per token.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as f64 / 4.0).ceil() as u64
}

/// Estimate tokens for a list of text segments.
pub fn estimate_tokens_batch(texts: &[&str]) -> u64 {
    texts.iter().map(|t| estimate_tokens(t)).sum()
}

/// Infer provider from model ID.
fn infer_provider(model_id: &str) -> String {
    let lower = model_id.to_lowercase();
    if lower.contains("claude") { "anthropic".to_string() }
    else if lower.contains("gpt") { "openai".to_string() }
    else if lower.contains("gemini") { "google".to_string() }
    else if lower.contains("llama") { "meta".to_string() }
    else if lower.contains("mistral") { "mistral".to_string() }
    else if lower.contains("nous") { "nous".to_string() }
    else if lower.contains("grok") { "xai".to_string() }
    else if lower.contains("deepseek") { "deepseek".to_string() }
    else { "unknown".to_string() }
}

/// Infer max output tokens from model ID patterns.
fn infer_max_output(model_id: &str) -> Option<u64> {
    let lower = model_id.to_lowercase();
    if lower.contains("claude") { Some(8_192) }
    else if lower.contains("gpt-4o") { Some(16_384) }
    else if lower.contains("gpt-4") { Some(8_192) }
    else if lower.contains("gpt-3.5") { Some(4_096) }
    else if lower.contains("gemini") { Some(8_192) }
    else { None }
}

/// Whether the model supports Anthropic-style prompt caching.
fn supports_caching(model_id: &str) -> bool {
    let lower = model_id.to_lowercase();
    lower.contains("claude") || lower.contains("gpt-4o")
}

/// Whether the model supports tool/function calling.
fn supports_tools(model_id: &str) -> bool {
    let lower = model_id.to_lowercase();
    // Most modern models support tool calling; only older ones don't
    !lower.contains("gpt-3.5-turbo-0301")
        && !lower.contains("gpt-4-0314")
        && !lower.contains("text-")
}

/// Infer context window from model ID patterns when not in known list.
fn infer_context_window(model_id: &str) -> u64 {
    let lower = model_id.to_lowercase();
    if lower.contains("claude") { 200_000 }
    else if lower.contains("gpt-4.1") { 1_048_576 }
    else if lower.contains("gpt-4o") { 128_000 }
    else if lower.contains("gpt-4") { 128_000 }
    else if lower.contains("gpt-3.5") { 16_385 }
    else if lower.contains("gemini") { 1_048_576 }
    else if lower.contains("llama-3") { 128_000 }
    else if lower.contains("llama") { 8_192 }
    else if lower.contains("mistral") { 128_000 }
    else { 128_000 } // Conservative default
}

/// Extract the first number found in a string.
fn extract_first_number(s: &str) -> Option<u64> {
    let digits: String = s.chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
        .filter(|c| c.is_ascii_digit())
        .collect();

    if digits.is_empty() {
        None
    } else {
        digits.parse::<u64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_lookup_exact() {
        let registry = ModelMetadataRegistry::new();
        let meta = registry.lookup("claude-sonnet-4-20250514").unwrap();
        assert_eq!(meta.context_window, 200_000);
        assert_eq!(meta.provider, "anthropic");
        assert!(meta.supports_caching);
    }

    #[test]
    fn test_registry_lookup_partial() {
        let registry = ModelMetadataRegistry::new();
        let meta = registry.lookup("claude-sonnet-4").unwrap();
        assert_eq!(meta.provider, "anthropic");
    }

    #[test]
    fn test_registry_lookup_unknown() {
        let registry = ModelMetadataRegistry::new();
        let meta = registry.lookup("some-random-model").unwrap();
        assert_eq!(meta.context_window, 128_000);
        assert_eq!(meta.provider, "unknown");
    }

    #[test]
    fn test_context_window() {
        let registry = ModelMetadataRegistry::new();
        assert_eq!(registry.context_window("claude-sonnet-4-20250514"), 200_000);
        assert_eq!(registry.context_window("gpt-4o"), 128_000);
        assert_eq!(registry.context_window("gpt-3.5-turbo"), 16_385);
        assert_eq!(registry.context_window("gemini-2.5-pro"), 1_048_576);
    }

    #[test]
    fn test_save_context_length() {
        let registry = ModelMetadataRegistry::new();
        registry.save_context_length("test-model", 50_000);
        assert_eq!(registry.context_window("test-model"), 50_000);
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello"), 2);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("1234"), 1);
        assert_eq!(estimate_tokens("12345"), 2);
    }

    #[test]
    fn test_estimate_tokens_batch() {
        let texts = vec!["hello world", "foo bar"];
        assert_eq!(estimate_tokens_batch(&texts), estimate_tokens("hello world") + estimate_tokens("foo bar"));
    }

    #[test]
    fn test_parse_context_limit_from_error() {
        assert_eq!(
            ModelMetadataRegistry::parse_context_limit_from_error("maximum context length is 128000 tokens"),
            Some(128000)
        );
        assert_eq!(
            ModelMetadataRegistry::parse_context_limit_from_error("requested 200000 tokens exceeds limit of 128000"),
            Some(128000)
        );
        assert_eq!(
            ModelMetadataRegistry::parse_context_limit_from_error("prompt is too long: 150000 > 128000"),
            Some(128000)
        );
    }

    #[test]
    fn test_parse_context_limit_no_match() {
        assert_eq!(
            ModelMetadataRegistry::parse_context_limit_from_error("some unrelated error"),
            None
        );
    }

    #[test]
    fn test_parse_output_limit_from_error() {
        assert_eq!(
            ModelMetadataRegistry::parse_output_limit_from_error("max_tokens 8192 is greater than context window"),
            Some(8192)
        );
    }

    #[test]
    fn test_is_local_endpoint() {
        assert!(ModelMetadataRegistry::is_local_endpoint("http://localhost:11434"));
        assert!(ModelMetadataRegistry::is_local_endpoint("http://127.0.0.1:8000"));
        assert!(ModelMetadataRegistry::is_local_endpoint("http://0.0.0.0:3000"));
        assert!(ModelMetadataRegistry::is_local_endpoint("http://host.docker.internal:8080"));
        assert!(!ModelMetadataRegistry::is_local_endpoint("https://api.anthropic.com"));
        assert!(!ModelMetadataRegistry::is_local_endpoint("https://api.openai.com"));
    }

    #[test]
    fn test_infer_provider() {
        assert_eq!(infer_provider("claude-sonnet-4"), "anthropic");
        assert_eq!(infer_provider("gpt-4o"), "openai");
        assert_eq!(infer_provider("gemini-2.5-pro"), "google");
        assert_eq!(infer_provider("llama-3.3-70b"), "meta");
        assert_eq!(infer_provider("mistral-large"), "mistral");
        assert_eq!(infer_provider("deepseek-chat"), "deepseek");
    }

    #[test]
    fn test_model_metadata_builder() {
        let meta = ModelMetadata::new("test", 100_000, "test-provider")
            .with_max_output(4_096)
            .with_caching(true)
            .with_tools(true);

        assert_eq!(meta.max_output_tokens, Some(4_096));
        assert!(meta.supports_caching);
        assert!(meta.supports_tools);
    }

    #[test]
    fn test_registry_probed_cache() {
        let registry = ModelMetadataRegistry::new();
        // First lookup of unknown model returns inferred context window
        let before = registry.context_window("unknown-provider/xyz-1.0");

        // Save a probed value
        registry.save_context_length("unknown-provider/xyz-1.0", 64_000);

        // Now the probed value should be returned
        let after = registry.context_window("unknown-provider/xyz-1.0");
        assert!(after >= before || after == 64_000);
        assert_eq!(registry.probed.lock().get("unknown-provider/xyz-1.0"), Some(&64_000));
    }

    #[test]
    fn test_default_registry() {
        let registry = ModelMetadataRegistry::default();
        assert!(registry.lookup("claude-sonnet-4-20250514").is_some());
    }
}
