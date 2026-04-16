use h_core::{ModelId, ModelRef, ProviderId};

/// Model tier for routing decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelTier {
    /// Fast, cheap models for simple tasks (e.g., Haiku, GPT-4o-mini)
    Fast,
    /// Balanced cost/quality for general tasks (e.g., Claude Sonnet, GPT-4o)
    Balanced,
    /// Most capable models for complex reasoning (e.g., Claude Opus, O1, O3)
    Powerful,
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelTier::Fast => write!(f, "fast"),
            ModelTier::Balanced => write!(f, "balanced"),
            ModelTier::Powerful => write!(f, "powerful"),
        }
    }
}

/// Configuration for model routing.
#[derive(Debug, Clone)]
pub struct RoutingConfig {
    /// Whether smart routing is enabled.
    pub enabled: bool,
    /// Maximum budget per query in USD. If estimated cost exceeds this, route to cheaper model.
    pub max_budget_usd: f64,
    /// Preferred model tier for complex tasks.
    pub preferred_complex_tier: ModelTier,
    /// Fallback model when the primary choice fails.
    pub fallback: Option<ModelRef>,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_budget_usd: 1.0,
            preferred_complex_tier: ModelTier::Powerful,
            fallback: None,
        }
    }
}

/// Complexity score from 0.0 to 1.0.
/// Higher scores indicate more complex tasks.
#[derive(Debug, Clone, Copy)]
pub struct ComplexityScore(pub f64);

impl ComplexityScore {
    pub fn tier(&self) -> ModelTier {
        if self.0 < 0.3 {
            ModelTier::Fast
        } else if self.0 < 0.7 {
            ModelTier::Balanced
        } else {
            ModelTier::Powerful
        }
    }
}

/// Estimate task complexity from the user's message.
///
/// Heuristics:
/// - Message length (longer = more complex)
/// - Presence of complexity indicators (words like "analyze", "refactor", "debug")
/// - Number of distinct requests/tasks
/// - Code content presence
pub fn estimate_complexity(user_message: &str) -> ComplexityScore {
    let mut score = 0.0;

    // Length factor: 0-0.25
    let len = user_message.len();
    if len > 500 {
        score += 0.25;
    } else if len > 200 {
        score += 0.15;
    } else if len > 100 {
        score += 0.1;
    } else if len > 50 {
        score += 0.05;
    }

    // Code presence: 0-0.2
    if has_code_block(user_message) {
        score += 0.2;
    } else if has_inline_code(user_message) {
        score += 0.1;
    }

    // Complexity keywords: 0-0.25
    let lower = user_message.to_lowercase();
    let complex_keywords = [
        "analyze", "refactor", "debug", "optimize", "architect", "design",
        "implement", "benchmark", "profile", "migrate", "transform",
        "investigate", "troubleshoot", "reverse", "parse", "compile",
        "recursive", "concurrent", "async", "parallel", "distributed",
        "why does", "how does", "explain", "compare", "evaluate",
    ];
    let keyword_count = complex_keywords
        .iter()
        .filter(|&&kw| lower.contains(kw))
        .count();
    score += (keyword_count as f64 * 0.05).min(0.25);

    // Multi-task detection (number of question marks or imperative sentences): 0-0.15
    let question_count = user_message.matches('?').count();
    let imperative_count = ["please", "can you", "could you", "i need", "i want", "do this"]
        .iter()
        .filter(|&&phrase| lower.contains(phrase))
        .count();
    let task_count = question_count + imperative_count;
    score += (task_count as f64 * 0.05).min(0.15);

    // Multi-file/path references: 0-0.15
    let path_count = user_message
        .lines()
        .filter(|line| {
            line.contains('/') || line.contains('\\') || line.contains("file:") || line.contains("src/")
        })
        .count();
    score += (path_count as f64 * 0.05).min(0.15);

    ComplexityScore(score.clamp(0.0, 1.0))
}

fn has_code_block(text: &str) -> bool {
    text.contains("```")
}

fn has_inline_code(text: &str) -> bool {
    text.contains('`')
}

/// Resolve a model tier to a concrete model.
/// Uses the user's current model as a base and maps it to the appropriate tier.
pub fn resolve_model_for_tier(current: &ModelRef, tier: ModelTier) -> ModelRef {
    let provider = current.provider.as_str();
    let model = current.model.as_str();

    // Determine the provider family
    let is_anthropic = provider == "anthropic" || model.contains("claude");
    let is_openai = provider == "openai" || model.starts_with("gpt-") || model.starts_with("o");

    if is_anthropic {
        match tier {
            ModelTier::Fast => ModelRef::new(
                ProviderId::new("anthropic"),
                ModelId::new("claude-haiku-4-20250414"),
            ),
            ModelTier::Balanced => ModelRef::new(
                ProviderId::new("anthropic"),
                ModelId::new("claude-sonnet-4-6-20250514"),
            ),
            ModelTier::Powerful => ModelRef::new(
                ProviderId::new("anthropic"),
                ModelId::new("claude-opus-4-6-20250514"),
            ),
        }
    } else if is_openai {
        match tier {
            ModelTier::Fast => ModelRef::new(
                ProviderId::new("openai"),
                ModelId::new("gpt-4o-mini"),
            ),
            ModelTier::Balanced => ModelRef::new(
                ProviderId::new("openai"),
                ModelId::new("gpt-4o"),
            ),
            ModelTier::Powerful => ModelRef::new(
                ProviderId::new("openai"),
                ModelId::new("o3"),
            ),
        }
    } else {
        // Unknown provider: return current model
        current.clone()
    }
}

/// Decide which model to use based on complexity and budget.
///
/// Returns the recommended model and the complexity score.
pub fn select_model(
    current: &ModelRef,
    user_message: &str,
    config: &RoutingConfig,
) -> (ModelRef, ComplexityScore) {
    if !config.enabled {
        return (current.clone(), estimate_complexity(user_message));
    }

    let complexity = estimate_complexity(user_message);
    let recommended_tier = complexity.tier();

    // Resolve the recommended model
    let recommended = resolve_model_for_tier(current, recommended_tier);

    // Check budget constraint: if the recommended model is too expensive,
    // fall back to a cheaper tier
    let estimated = estimate_query_cost(user_message, &recommended);
    if estimated > config.max_budget_usd {
        let cheaper_tier = match recommended_tier {
            ModelTier::Powerful => ModelTier::Balanced,
            ModelTier::Balanced => ModelTier::Fast,
            ModelTier::Fast => ModelTier::Fast, // Already cheapest
        };
        return (
            resolve_model_for_tier(current, cheaper_tier),
            complexity,
        );
    }

    (recommended, complexity)
}

/// Rough estimate of query cost based on message length and model pricing.
pub fn estimate_query_cost(user_message: &str, model: &ModelRef) -> f64 {
    use h_core::pricing::estimate_cost;

    let input_tokens = estimate_tokens(user_message);
    // Estimate output as 2x input for complex tasks, 1x for simple
    let complexity = estimate_complexity(user_message);
    let output_multiplier = if complexity.0 > 0.7 { 2.0 } else { 1.0 };
    let output_tokens = (input_tokens as f64 * output_multiplier) as u64;

    let tracker = h_core::CostTracker {
        input_tokens,
        output_tokens,
        ..Default::default()
    };

    estimate_cost(&model.model.0, &tracker)
}

/// Rough token count estimation: ~4 chars per token for English text.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as f64 / 4.0).ceil() as u64
}

/// Fallback chain: given a primary model, return a sequence of fallback models.
pub fn fallback_chain(primary: &ModelRef) -> Vec<ModelRef> {
    let provider = primary.provider.as_str();
    let model = primary.model.as_str();

    let is_anthropic = provider == "anthropic" || model.contains("claude");
    let is_openai = provider == "openai" || model.starts_with("gpt-") || model.starts_with("o");

    if is_anthropic {
        vec![
            ModelRef::new(
                ProviderId::new("anthropic"),
                ModelId::new("claude-sonnet-4-6-20250514"),
            ),
            ModelRef::new(
                ProviderId::new("anthropic"),
                ModelId::new("claude-haiku-4-20250414"),
            ),
        ]
    } else if is_openai {
        vec![
            ModelRef::new(ProviderId::new("openai"), ModelId::new("gpt-4o")),
            ModelRef::new(ProviderId::new("openai"), ModelId::new("gpt-4o-mini")),
        ]
    } else {
        // Generic: try OpenAI as fallback
        vec![
            ModelRef::new(ProviderId::new("openai"), ModelId::new("gpt-4o")),
            ModelRef::new(ProviderId::new("openai"), ModelId::new("gpt-4o-mini")),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_complexity_empty() {
        let score = estimate_complexity("");
        assert_eq!(score.0, 0.0);
        assert_eq!(score.tier(), ModelTier::Fast);
    }

    #[test]
    fn test_estimate_complexity_short_message() {
        let score = estimate_complexity("hi");
        assert!(score.0 < 0.3);
        assert_eq!(score.tier(), ModelTier::Fast);
    }

    #[test]
    fn test_estimate_complexity_code_block() {
        let msg = "Can you refactor this code?\n```\nfn main() {\n    println!(\"hello\");\n}\n```";
        let score = estimate_complexity(msg);
        assert!(score.0 >= 0.2); // code block adds 0.2
        assert!(score.tier() == ModelTier::Balanced || score.tier() == ModelTier::Fast);
    }

    #[test]
    fn test_estimate_complexity_complex_keywords() {
        let msg = "Please analyze and refactor the authentication module. I need to debug the concurrent async token refresh mechanism.";
        let score = estimate_complexity(msg);
        assert!(score.0 > 0.3);
        assert_eq!(score.tier(), ModelTier::Balanced);
    }

    #[test]
    fn test_estimate_complexity_long_with_paths() {
        let msg = r#"I'm having issues with my project.
The file src/main.rs has a problem on line 42.
Also check src/utils/auth.rs and src/middleware/token.rs.
Could you please analyze the architecture and suggest improvements?
Why does the concurrent token refresh fail intermittently?"#;
        let score = estimate_complexity(msg);
        assert!(score.0 > 0.5);
        assert_eq!(score.tier(), ModelTier::Balanced);
    }

    #[test]
    fn test_estimate_complexity_very_long() {
        let msg = "a".repeat(600);
        let score = estimate_complexity(&msg);
        assert!(score.0 >= 0.25);
    }

    #[test]
    fn test_complexity_score_tier_boundaries() {
        assert_eq!(ComplexityScore(0.0).tier(), ModelTier::Fast);
        assert_eq!(ComplexityScore(0.29).tier(), ModelTier::Fast);
        assert_eq!(ComplexityScore(0.3).tier(), ModelTier::Balanced);
        assert_eq!(ComplexityScore(0.69).tier(), ModelTier::Balanced);
        assert_eq!(ComplexityScore(0.7).tier(), ModelTier::Powerful);
        assert_eq!(ComplexityScore(1.0).tier(), ModelTier::Powerful);
    }

    #[test]
    fn test_resolve_model_anthropic() {
        let current = ModelRef::new(
            ProviderId::new("anthropic"),
            ModelId::new("claude-sonnet-4-6"),
        );

        let fast = resolve_model_for_tier(&current, ModelTier::Fast);
        assert!(fast.model.as_str().contains("haiku"));

        let balanced = resolve_model_for_tier(&current, ModelTier::Balanced);
        assert!(balanced.model.as_str().contains("sonnet"));

        let powerful = resolve_model_for_tier(&current, ModelTier::Powerful);
        assert!(powerful.model.as_str().contains("opus"));
    }

    #[test]
    fn test_resolve_model_openai() {
        let current = ModelRef::new(
            ProviderId::new("openai"),
            ModelId::new("gpt-4o"),
        );

        let fast = resolve_model_for_tier(&current, ModelTier::Fast);
        assert!(fast.model.as_str().contains("mini"));

        let balanced = resolve_model_for_tier(&current, ModelTier::Balanced);
        assert!(balanced.model.as_str().contains("gpt-4o"));

        let powerful = resolve_model_for_tier(&current, ModelTier::Powerful);
        assert!(powerful.model.as_str().starts_with("o"));
    }

    #[test]
    fn test_resolve_model_unknown_provider() {
        let current = ModelRef::new(
            ProviderId::new("custom"),
            ModelId::new("custom-model"),
        );
        let result = resolve_model_for_tier(&current, ModelTier::Fast);
        assert_eq!(result.model.as_str(), "custom-model");
    }

    #[test]
    fn test_select_model_routing_disabled() {
        let current = ModelRef::new(
            ProviderId::new("anthropic"),
            ModelId::new("claude-sonnet-4-6"),
        );
        let config = RoutingConfig { enabled: false, ..Default::default() };
        let (model, _) = select_model(&current, "simple question", &config);
        assert_eq!(model.model.as_str(), "claude-sonnet-4-6");
    }

    #[test]
    fn test_select_model_routing_fast() {
        let current = ModelRef::new(
            ProviderId::new("anthropic"),
            ModelId::new("claude-sonnet-4-6"),
        );
        let config = RoutingConfig { enabled: true, ..Default::default() };
        let (model, score) = select_model(&current, "hi", &config);
        assert!(score.0 < 0.3);
        assert!(model.model.as_str().contains("haiku"));
    }

    #[test]
    fn test_fallback_chain_anthropic() {
        let primary = ModelRef::new(
            ProviderId::new("anthropic"),
            ModelId::new("claude-opus-4-6"),
        );
        let chain = fallback_chain(&primary);
        assert_eq!(chain.len(), 2);
        assert!(chain[0].model.as_str().contains("sonnet"));
        assert!(chain[1].model.as_str().contains("haiku"));
    }

    #[test]
    fn test_fallback_chain_openai() {
        let primary = ModelRef::new(
            ProviderId::new("openai"),
            ModelId::new("o3"),
        );
        let chain = fallback_chain(&primary);
        assert_eq!(chain.len(), 2);
        assert!(chain[0].model.as_str().contains("gpt-4o"));
        assert!(chain[1].model.as_str().contains("mini"));
    }

    #[test]
    fn test_estimate_tokens() {
        let tokens = estimate_tokens("Hello, world!");
        assert!(tokens > 0);
        // "Hello, world!" is 13 chars, ~3.25 tokens rounded up = 4
        assert_eq!(tokens, 4);
    }

    #[test]
    fn test_estimate_query_cost() {
        let model = ModelRef::new(
            ProviderId::new("anthropic"),
            ModelId::new("claude-sonnet-4-6"),
        );
        let cost = estimate_query_cost("Hello, world!", &model);
        assert!(cost > 0.0);
    }

    #[test]
    fn test_model_tier_display() {
        assert_eq!(ModelTier::Fast.to_string(), "fast");
        assert_eq!(ModelTier::Balanced.to_string(), "balanced");
        assert_eq!(ModelTier::Powerful.to_string(), "powerful");
    }

    #[test]
    fn test_routing_config_default() {
        let config = RoutingConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_budget_usd, 1.0);
        assert_eq!(config.preferred_complex_tier, ModelTier::Powerful);
    }
}
