use std::collections::HashMap;
use std::time::Duration;

/// Pricing data for a single model (per 1M tokens, in USD).
#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
    pub cache_read_per_1m: f64,
    pub cache_write_per_1m: f64,
}

impl ModelPricing {
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input_per_1m: input,
            output_per_1m: output,
            cache_read_per_1m: input * 0.1,  // 10% of input by default
            cache_write_per_1m: input * 0.2, // 20% of input by default
        }
    }

    pub fn with_cache(mut self, cache_read: f64, cache_write: f64) -> Self {
        self.cache_read_per_1m = cache_read;
        self.cache_write_per_1m = cache_write;
        self
    }
}

/// Token usage from an API response (for pricing calculations).
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens
    }
}

/// Normalized usage with cost breakdown.
#[derive(Debug, Clone)]
pub struct NormalizedUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub input_cost_usd: f64,
    pub output_cost_usd: f64,
    pub cache_read_cost_usd: f64,
    pub cache_write_cost_usd: f64,
    pub total_cost_usd: f64,
}

impl NormalizedUsage {
    pub fn format_compact(&self) -> String {
        format!(
            "{} tok · ${:.4}",
            format_token_count_compact(self.total_tokens),
            self.total_cost_usd
        )
    }
}

/// Per-provider pricing tables.
pub struct PricingCatalog {
    models: HashMap<String, ModelPricing>,
}

impl PricingCatalog {
    pub fn new() -> Self {
        let mut catalog = Self {
            models: HashMap::new(),
        };
        catalog.register_builtins();
        catalog
    }

    fn register_builtins(&mut self) {
        // Anthropic (per 1M tokens, USD)
        self.add("anthropic", "claude-sonnet-4-20250514", ModelPricing::new(3.0, 15.0).with_cache(0.30, 3.75));
        self.add("anthropic", "claude-opus-4-20250416", ModelPricing::new(15.0, 75.0).with_cache(1.50, 18.75));
        self.add("anthropic", "claude-3-5-haiku-20241022", ModelPricing::new(0.80, 4.0).with_cache(0.08, 1.0));
        self.add("anthropic", "claude-3-7-sonnet-20250219", ModelPricing::new(3.0, 15.0).with_cache(0.30, 3.75));

        // OpenAI (per 1M tokens, USD)
        self.add("openai", "gpt-4o", ModelPricing::new(2.5, 10.0).with_cache(1.25, 2.50));
        self.add("openai", "gpt-4o-mini", ModelPricing::new(0.15, 0.60).with_cache(0.075, 0.15));
        self.add("openai", "o3", ModelPricing::new(10.0, 40.0));
        self.add("openai", "o3-mini", ModelPricing::new(1.10, 4.40));
        self.add("openai", "gpt-4.1", ModelPricing::new(2.0, 8.0).with_cache(0.50, 2.0));
        self.add("openai", "gpt-4.1-mini", ModelPricing::new(0.40, 1.60).with_cache(0.10, 0.40));
        self.add("openai", "gpt-4.1-nano", ModelPricing::new(0.10, 0.40).with_cache(0.025, 0.10));

        // Google Gemini (per 1M tokens, USD)
        self.add("gemini", "gemini-2.5-pro", ModelPricing::new(1.25, 10.0));
        self.add("gemini", "gemini-2.5-flash", ModelPricing::new(0.15, 0.60));
        self.add("gemini", "gemini-2.0-flash", ModelPricing::new(0.10, 0.40));

        // Mistral (per 1M tokens, USD)
        self.add("mistral", "mistral-large-latest", ModelPricing::new(2.0, 6.0));
        self.add("mistral", "mistral-small-latest", ModelPricing::new(0.20, 0.60));
        self.add("mistral", "codestral-latest", ModelPricing::new(0.30, 0.90));

        // Groq (per 1M tokens, USD)
        self.add("groq", "llama-3.1-70b", ModelPricing::new(0.59, 0.79));
        self.add("groq", "llama-3.1-8b", ModelPricing::new(0.05, 0.08));
        self.add("groq", "mixtral-8x7b", ModelPricing::new(0.24, 0.24));

        // OpenRouter — no fixed pricing (passthrough), use defaults
        self.add("openrouter", "*", ModelPricing::new(0.0, 0.0));

        // Ollama — local, no cost
        self.add("ollama", "*", ModelPricing::new(0.0, 0.0));

        // HuggingFace — use defaults
        self.add("huggingface", "*", ModelPricing::new(0.0, 0.0));

        // Nous Research
        self.add("nous", "*", ModelPricing::new(0.0, 0.0));

        // Xiaomi MIMO
        self.add("xiaomi_mimo", "*", ModelPricing::new(0.0, 0.0));

        // Z.AI
        self.add("z_ai", "*", ModelPricing::new(0.0, 0.0));

        // Kimi
        self.add("kimi", "*", ModelPricing::new(0.0, 0.0));

        // MiniMax
        self.add("minimax", "*", ModelPricing::new(0.0, 0.0));
    }

    fn add(&mut self, provider: &str, model: &str, pricing: ModelPricing) {
        let key = format!("{provider}/{model}");
        self.models.insert(key, pricing);
    }

    /// Get pricing for a provider/model combination.
    pub fn get(&self, provider: &str, model: &str) -> Option<&ModelPricing> {
        // Try exact match first
        let key = format!("{provider}/{model}");
        if let Some(p) = self.models.get(&key) {
            return Some(p);
        }
        // Fall back to wildcard
        let wildcard = format!("{provider}/*");
        self.models.get(&wildcard)
    }
}

impl Default for PricingCatalog {
    fn default() -> Self {
        Self::new()
    }
}

/// Estimate the cost of an API call given provider, model, and usage.
pub fn estimate_cost(
    catalog: &PricingCatalog,
    provider: &str,
    model: &str,
    usage: &TokenUsage,
) -> f64 {
    let pricing = match catalog.get(provider, model) {
        Some(p) => p,
        None => return 0.0,
    };

    let input_cost = (usage.input_tokens as f64 / 1_000_000.0) * pricing.input_per_1m;
    let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * pricing.output_per_1m;
    let cache_read_cost = (usage.cache_read_tokens as f64 / 1_000_000.0) * pricing.cache_read_per_1m;
    let cache_write_cost = (usage.cache_write_tokens as f64 / 1_000_000.0) * pricing.cache_write_per_1m;

    input_cost + output_cost + cache_read_cost + cache_write_cost
}

/// Normalize usage into a detailed cost breakdown.
pub fn normalize_usage(
    catalog: &PricingCatalog,
    provider: &str,
    model: &str,
    usage: &TokenUsage,
) -> NormalizedUsage {
    let pricing = match catalog.get(provider, model) {
        Some(p) => p,
        None => {
            return NormalizedUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cache_read_tokens: usage.cache_read_tokens,
                cache_write_tokens: usage.cache_write_tokens,
                total_tokens: usage.total(),
                input_cost_usd: 0.0,
                output_cost_usd: 0.0,
                cache_read_cost_usd: 0.0,
                cache_write_cost_usd: 0.0,
                total_cost_usd: 0.0,
            };
        }
    };

    let input_cost_usd = (usage.input_tokens as f64 / 1_000_000.0) * pricing.input_per_1m;
    let output_cost_usd = (usage.output_tokens as f64 / 1_000_000.0) * pricing.output_per_1m;
    let cache_read_cost_usd = (usage.cache_read_tokens as f64 / 1_000_000.0) * pricing.cache_read_per_1m;
    let cache_write_cost_usd = (usage.cache_write_tokens as f64 / 1_000_000.0) * pricing.cache_write_per_1m;
    let total_cost_usd = input_cost_usd + output_cost_usd + cache_read_cost_usd + cache_write_cost_usd;

    NormalizedUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        total_tokens: usage.total(),
        input_cost_usd,
        output_cost_usd,
        cache_read_cost_usd,
        cache_write_cost_usd,
        total_cost_usd,
    }
}

/// Format a token count in compact form (e.g., "1.2M", "45.6K", "123").
pub fn format_token_count_compact(n: u64) -> String {
    if n >= 1_000_000 {
        let v = format!("{:.1}", n as f64 / 1_000_000.0)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
        format!("{v}M")
    } else if n >= 1_000 {
        let v = format!("{:.1}", n as f64 / 1_000.0)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
        format!("{v}K")
    } else {
        n.to_string()
    }
}

/// Format a duration in compact form (e.g., "2m 30s", "45s").
pub fn format_duration_compact(d: Duration) -> String {
    let total_secs = d.as_secs();
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    if mins > 0 {
        format!("{mins}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_catalog_builtins() {
        let catalog = PricingCatalog::new();
        assert!(catalog.get("anthropic", "claude-sonnet-4-20250514").is_some());
        assert!(catalog.get("openai", "gpt-4o").is_some());
        assert!(catalog.get("ollama", "any-model").is_some()); // wildcard
    }

    #[test]
    fn test_estimate_cost_sonnet() {
        let catalog = PricingCatalog::new();
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let cost = estimate_cost(&catalog, "anthropic", "claude-sonnet-4-20250514", &usage);
        // $3.00/1M input + $15.00/1M output
        let expected = (1000.0 / 1_000_000.0) * 3.0 + (500.0 / 1_000_000.0) * 15.0;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn test_estimate_cost_unknown_model() {
        let catalog = PricingCatalog::new();
        let usage = TokenUsage::default();
        let cost = estimate_cost(&catalog, "unknown", "unknown-model", &usage);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_estimate_cost_free_provider() {
        let catalog = PricingCatalog::new();
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let cost = estimate_cost(&catalog, "ollama", "llama3", &usage);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_normalize_usage() {
        let catalog = PricingCatalog::new();
        let usage = TokenUsage {
            input_tokens: 100_000,
            output_tokens: 50_000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let normalized = normalize_usage(&catalog, "openai", "gpt-4o", &usage);
        assert_eq!(normalized.total_tokens, 150_000);
        assert!(normalized.total_cost_usd > 0.0);
        // $2.50/1M input => 100K * 2.50 / 1M = $0.25
        assert!((normalized.input_cost_usd - 0.25).abs() < 1e-10);
        // $10.00/1M output => 50K * 10.00 / 1M = $0.50
        assert!((normalized.output_cost_usd - 0.50).abs() < 1e-10);
    }

    #[test]
    fn test_format_token_count_compact() {
        assert_eq!(format_token_count_compact(123), "123");
        assert_eq!(format_token_count_compact(1_000), "1K");
        assert_eq!(format_token_count_compact(1_500), "1.5K");
        assert_eq!(format_token_count_compact(45_600), "45.6K");
        assert_eq!(format_token_count_compact(1_000_000), "1M");
        assert_eq!(format_token_count_compact(1_200_000), "1.2M");
        assert_eq!(format_token_count_compact(12_345_678), "12.3M");
    }

    #[test]
    fn test_format_duration_compact() {
        assert_eq!(
            format_duration_compact(Duration::from_secs(45)),
            "45s"
        );
        assert_eq!(
            format_duration_compact(Duration::from_secs(150)),
            "2m 30s"
        );
        assert_eq!(
            format_duration_compact(Duration::from_secs(3661)),
            "61m 1s"
        );
    }

    #[test]
    fn test_usage_total() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 200,
            cache_write_tokens: 30,
        };
        assert_eq!(usage.total(), 380);
    }

    #[test]
    fn test_model_pricing_with_cache() {
        let pricing = ModelPricing::new(3.0, 15.0).with_cache(0.30, 3.75);
        assert_eq!(pricing.input_per_1m, 3.0);
        assert_eq!(pricing.output_per_1m, 15.0);
        assert_eq!(pricing.cache_read_per_1m, 0.30);
        assert_eq!(pricing.cache_write_per_1m, 3.75);
    }

    #[test]
    fn test_model_pricing_default_cache() {
        let pricing = ModelPricing::new(10.0, 50.0);
        assert_eq!(pricing.cache_read_per_1m, 1.0);  // 10% of input
        assert_eq!(pricing.cache_write_per_1m, 2.0); // 20% of input
    }

    #[test]
    fn test_normalized_usage_format() {
        let catalog = PricingCatalog::new();
        let usage = TokenUsage {
            input_tokens: 1_200_000,
            output_tokens: 600_000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let normalized = normalize_usage(&catalog, "anthropic", "claude-sonnet-4-20250514", &usage);
        let compact = normalized.format_compact();
        assert!(compact.contains("1.8M"));
        assert!(compact.contains("$"));
    }

    #[test]
    fn test_estimate_cost_with_cache() {
        let catalog = PricingCatalog::new();
        let usage = TokenUsage {
            input_tokens: 100_000,
            output_tokens: 50_000,
            cache_read_tokens: 80_000,
            cache_write_tokens: 20_000,
        };
        let cost = estimate_cost(&catalog, "anthropic", "claude-sonnet-4-20250514", &usage);
        // input: 100K * 3.0 / 1M = 0.30
        // output: 50K * 15.0 / 1M = 0.75
        // cache_read: 80K * 0.30 / 1M = 0.024
        // cache_write: 20K * 3.75 / 1M = 0.075
        let expected = 0.30 + 0.75 + 0.024 + 0.075;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn test_pricing_catalog_default() {
        let catalog = PricingCatalog::default();
        assert!(catalog.get("anthropic", "claude-sonnet-4-20250514").is_some());
    }
}
