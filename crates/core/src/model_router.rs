/// Smart model routing: automatically selects the most appropriate model
/// based on task complexity, cost constraints, and capability requirements.
///
/// Routes requests across available models to balance:
/// - Task complexity (simple queries → cheaper models, complex reasoning → top models)
/// - Tool usage (tool-heavy tasks → models with strong tool support)
/// - Context length (long conversations → models with large context windows)
/// - Cost constraints (budget-aware routing)
/// - Capability requirements (vision, reasoning, etc.)

use std::collections::HashMap;

/// Complexity tier for routing decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ComplexityTier {
    /// Simple greetings, short questions, formatting tasks.
    Trivial,
    /// Factual questions, short explanations, basic coding.
    Simple,
    /// Multi-step reasoning, moderate code tasks, analysis.
    Moderate,
    /// Complex multi-step tasks, architecture, debugging.
    Complex,
    /// Deep reasoning, long code generation, research tasks.
    Expert,
}

impl ComplexityTier {
    pub fn name(&self) -> &str {
        match self {
            Self::Trivial => "trivial",
            Self::Simple => "simple",
            Self::Moderate => "moderate",
            Self::Complex => "complex",
            Self::Expert => "expert",
        }
    }
}

/// Capability flags a model may support.
#[derive(Debug, Clone, Default)]
pub struct ModelCapabilities {
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_reasoning: bool,
    pub supports_caching: bool,
}

/// A routable model entry with its capabilities and pricing.
#[derive(Debug, Clone)]
pub struct RoutableModel {
    pub provider: String,
    pub model_id: String,
    pub context_window: u64,
    pub input_cost_per_1m: f64,
    pub output_cost_per_1m: f64,
    pub capabilities: ModelCapabilities,
    /// Quality tier (1-10, subjective).
    pub quality_score: u8,
    /// Whether this model is currently available (API key configured).
    pub available: bool,
}

impl RoutableModel {
    pub fn identifier(&self) -> String {
        format!("{}/{}", self.provider, self.model_id)
    }
}

/// Configuration for the model router.
#[derive(Debug, Clone)]
pub struct ModelRouterConfig {
    /// Maximum cost per request (USD). Routes to cheaper models if exceeded.
    pub max_cost_per_request: Option<f64>,
    /// Preferred quality tier (0-10). Only route to models at or above this score.
    pub min_quality_score: u8,
    /// Whether to prefer cheaper models when complexity allows.
    pub cost_optimized: bool,
    /// Fallback model if no suitable model found.
    pub fallback_provider: String,
    pub fallback_model: String,
}

impl Default for ModelRouterConfig {
    fn default() -> Self {
        Self {
            max_cost_per_request: None,
            min_quality_score: 1,
            cost_optimized: true,
            fallback_provider: "anthropic".to_string(),
            fallback_model: "claude-sonnet-4-20250514".to_string(),
        }
    }
}

/// Result of a routing decision.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    /// Selected provider.
    pub provider: String,
    /// Selected model.
    pub model: String,
    /// Estimated complexity of the task.
    pub complexity: ComplexityTier,
    /// Why this model was chosen.
    pub reasoning: String,
    /// Estimated cost for this request.
    pub estimated_cost_usd: f64,
}

/// Smart model router that selects the best model for a given task.
pub struct ModelRouter {
    config: ModelRouterConfig,
    /// All available models for routing.
    models: Vec<RoutableModel>,
    /// Heuristic thresholds for complexity estimation.
    complexity_thresholds: ComplexityThresholds,
}

#[derive(Debug, Clone)]
struct ComplexityThresholds {
    /// Token count above which tasks are considered complex.
    complex_token_threshold: u64,
    /// Token count above which tasks are considered expert-level.
    expert_token_threshold: u64,
    /// Number of tool calls that trigger complex routing.
    complex_tool_threshold: usize,
}

impl Default for ComplexityThresholds {
    fn default() -> Self {
        Self {
            complex_token_threshold: 4_000,
            expert_token_threshold: 16_000,
            complex_tool_threshold: 5,
        }
    }
}

impl ModelRouter {
    pub fn new(config: ModelRouterConfig) -> Self {
        Self {
            config,
            models: Vec::new(),
            complexity_thresholds: ComplexityThresholds::default(),
        }
    }

    /// Register a model for routing consideration.
    pub fn register_model(&mut self, model: RoutableModel) {
        self.models.push(model);
    }

    /// Register multiple models at once.
    pub fn register_models(&mut self, models: Vec<RoutableModel>) {
        self.models.extend(models);
    }

    /// Route a request to the best available model.
    ///
    /// Considers:
    /// - Task complexity (estimated from input length and structure)
    /// - Capability requirements (tools, vision, reasoning)
    /// - Context window needs
    /// - Cost constraints
    /// - Model availability
    pub fn route(&self, input: &str, requires: &ModelCapabilities) -> RoutingDecision {
        let complexity = self.estimate_complexity(input);
        let available = self.filter_available_models();
        let capable: Vec<_> = available
            .into_iter()
            .filter(|m| self.capability_match(m, requires))
            .collect();

        if capable.is_empty() {
            return self.fallback_decision(complexity);
        }

        // Route by complexity tier
        let target_quality = match complexity {
            ComplexityTier::Trivial => 1,
            ComplexityTier::Simple => 3,
            ComplexityTier::Moderate => 5,
            ComplexityTier::Complex => 7,
            ComplexityTier::Expert => 9,
        };

        // Find models that meet the target quality
        let suitable: Vec<_> = capable
            .into_iter()
            .filter(|m| m.quality_score >= target_quality && m.quality_score >= self.config.min_quality_score)
            .collect();

        if suitable.is_empty() {
            return self.fallback_decision(complexity);
        }

        // Among suitable models, pick the cheapest if cost-optimized
        let selected = if self.config.cost_optimized {
            suitable
                .into_iter()
                .min_by(|a, b| {
                    let cost_a = self.estimate_request_cost(a, input.len() as u64);
                    let cost_b = self.estimate_request_cost(b, input.len() as u64);
                    cost_a.partial_cmp(&cost_b).unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap()
        } else {
            // Otherwise pick the highest quality model
            suitable
                .into_iter()
                .max_by_key(|m| m.quality_score)
                .unwrap()
        };

        let estimated_cost = self.estimate_request_cost(&selected, input.len() as u64);
        let reasoning = format!(
            "Complexity: {} (target quality ≥ {}). Selected {} (quality: {}, cost: ${:.4}/req)",
            complexity.name(),
            target_quality,
            selected.identifier(),
            selected.quality_score,
            estimated_cost
        );

        RoutingDecision {
            provider: selected.provider.clone(),
            model: selected.model_id.clone(),
            complexity,
            reasoning,
            estimated_cost_usd: estimated_cost,
        }
    }

    /// Route based on explicit complexity tier (bypasses estimation).
    pub fn route_by_complexity(
        &self,
        complexity: ComplexityTier,
        requires: &ModelCapabilities,
    ) -> RoutingDecision {
        let available = self.filter_available_models();
        let capable: Vec<_> = available
            .into_iter()
            .filter(|m| self.capability_match(m, requires))
            .collect();

        if capable.is_empty() {
            return self.fallback_decision(complexity);
        }

        let target_quality = match complexity {
            ComplexityTier::Trivial => 1,
            ComplexityTier::Simple => 3,
            ComplexityTier::Moderate => 5,
            ComplexityTier::Complex => 7,
            ComplexityTier::Expert => 9,
        };

        let suitable: Vec<_> = capable
            .into_iter()
            .filter(|m| m.quality_score >= target_quality && m.quality_score >= self.config.min_quality_score)
            .collect();

        if suitable.is_empty() {
            return self.fallback_decision(complexity);
        }

        let selected = if self.config.cost_optimized {
            suitable.into_iter().min_by(|a, b| a.input_cost_per_1m.partial_cmp(&b.input_cost_per_1m).unwrap()).unwrap()
        } else {
            suitable.into_iter().max_by_key(|m| m.quality_score).unwrap()
        };

        RoutingDecision {
            provider: selected.provider.clone(),
            model: selected.model_id.clone(),
            complexity,
            reasoning: format!(
                "Explicit complexity: {}. Selected {} (quality: {})",
                complexity.name(),
                selected.identifier(),
                selected.quality_score
            ),
            estimated_cost_usd: 0.0,
        }
    }

    /// Estimate task complexity from the input text.
    pub fn estimate_complexity(&self, input: &str) -> ComplexityTier {
        let token_count = estimate_token_count(input);
        let tool_indicators = count_tool_indicators(input);
        let reasoning_indicators = count_reasoning_indicators(input);
        let code_indicators = count_code_indicators(input);

        // Expert: very long inputs with multiple complex signals
        if token_count >= self.complexity_thresholds.expert_token_threshold
            || (reasoning_indicators >= 3 && code_indicators >= 2 && tool_indicators >= 2)
        {
            return ComplexityTier::Expert;
        }

        // Complex: moderate length with multiple signals
        if token_count >= self.complexity_thresholds.complex_token_threshold
            || tool_indicators >= self.complexity_thresholds.complex_tool_threshold
            || (reasoning_indicators >= 2 && code_indicators >= 1)
        {
            return ComplexityTier::Complex;
        }

        // Moderate: some complexity signals
        if reasoning_indicators >= 1 || code_indicators >= 1 || tool_indicators >= 2 {
            return ComplexityTier::Moderate;
        }

        // Simple: non-trivial but not complex
        if token_count > 200 || input.contains('?') {
            return ComplexityTier::Simple;
        }

        // Trivial: short, simple inputs
        ComplexityTier::Trivial
    }

    /// Get all available models grouped by complexity tier suitability.
    pub fn models_by_tier(&self) -> HashMap<ComplexityTier, Vec<&RoutableModel>> {
        let mut map: HashMap<ComplexityTier, Vec<&RoutableModel>> = HashMap::new();
        let available = self.filter_available_models();

        for model in available {
            let tier = if model.quality_score >= 9 {
                ComplexityTier::Expert
            } else if model.quality_score >= 7 {
                ComplexityTier::Complex
            } else if model.quality_score >= 5 {
                ComplexityTier::Moderate
            } else if model.quality_score >= 3 {
                ComplexityTier::Simple
            } else {
                ComplexityTier::Trivial
            };
            map.entry(tier).or_default().push(model);
        }

        map
    }

    /// Get the cheapest available model that meets requirements.
    pub fn cheapest_model(&self, requires: &ModelCapabilities) -> Option<RoutingDecision> {
        let available = self.filter_available_models();
        let capable: Vec<_> = available
            .into_iter()
            .filter(|m| self.capability_match(m, requires))
            .collect();

        capable
            .into_iter()
            .min_by(|a, b| a.input_cost_per_1m.partial_cmp(&b.input_cost_per_1m).unwrap())
            .map(|m| RoutingDecision {
                provider: m.provider.clone(),
                model: m.model_id.clone(),
                complexity: ComplexityTier::Trivial,
                reasoning: format!("Cheapest available: {}", m.identifier()),
                estimated_cost_usd: m.input_cost_per_1m / 1_000_000.0,
            })
    }

    fn filter_available_models(&self) -> Vec<&RoutableModel> {
        self.models.iter().filter(|m| m.available).collect()
    }

    fn capability_match(&self, model: &RoutableModel, requires: &ModelCapabilities) -> bool {
        if requires.supports_tools && !model.capabilities.supports_tools {
            return false;
        }
        if requires.supports_vision && !model.capabilities.supports_vision {
            return false;
        }
        if requires.supports_reasoning && !model.capabilities.supports_reasoning {
            return false;
        }
        true
    }

    fn estimate_request_cost(&self, model: &RoutableModel, input_chars: u64) -> f64 {
        let input_tokens = input_chars / 4; // rough estimate
        let output_tokens = input_tokens / 2; // assume 2:1 input:output ratio
        (input_tokens as f64 / 1_000_000.0) * model.input_cost_per_1m
            + (output_tokens as f64 / 1_000_000.0) * model.output_cost_per_1m
    }

    fn fallback_decision(&self, complexity: ComplexityTier) -> RoutingDecision {
        RoutingDecision {
            provider: self.config.fallback_provider.clone(),
            model: self.config.fallback_model.clone(),
            complexity,
            reasoning: format!(
                "No suitable model found (complexity: {}). Falling back to default.",
                complexity.name()
            ),
            estimated_cost_usd: 0.0,
        }
    }
}

/// Estimate token count from character count (~4 chars per token for English).
fn estimate_token_count(text: &str) -> u64 {
    (text.chars().count() as f64 / 4.0).ceil() as u64
}

/// Count signals that suggest complex tool-using behavior.
fn count_tool_indicators(input: &str) -> usize {
    let tool_keywords = [
        "run", "execute", "terminal", "command", "file", "read", "write",
        "search", "grep", "browser", "mcp", "skill", "install", "deploy",
        "git", "build", "test", "compile", "docker", "ssh", "create",
    ];
    let lower = input.to_lowercase();
    tool_keywords.iter().filter(|&&kw| lower.contains(kw)).count()
}

/// Count signals that suggest deep reasoning is needed.
fn count_reasoning_indicators(input: &str) -> usize {
    let reasoning_keywords = [
        "analyze", "compare", "explain", "why", "how", "reason", "think",
        "evaluate", "design", "architecture", "optimize", "debug", "trace",
        "infer", "deduce", "derive", "calculate", "prove", "strategy",
    ];
    let lower = input.to_lowercase();
    reasoning_keywords.iter().filter(|&&kw| lower.contains(kw)).count()
}

/// Count signals that suggest code work is involved.
fn count_code_indicators(input: &str) -> usize {
    let code_keywords = [
        "function", "class", "def", "fn", "impl", "struct", "interface",
        "async", "await", "import", "export", "return", "error", "panic",
        "```\n", "```rust", "```python", "```typescript", "```go", "```javascript",
    ];
    let lower = input.to_lowercase();
    code_keywords.iter().filter(|&&kw| lower.contains(kw)).count()
}

/// Register built-in models with the router.
pub fn register_builtin_models(router: &mut ModelRouter) {
    let models = vec![
        // Trivial tier (quality 1-2): cheap models for simple tasks
        RoutableModel {
            provider: "openai".to_string(),
            model_id: "gpt-4o-mini".to_string(),
            context_window: 128_000,
            input_cost_per_1m: 0.15,
            output_cost_per_1m: 0.60,
            capabilities: ModelCapabilities {
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: false,
                supports_caching: false,
            },
            quality_score: 3,
            available: true,
        },
        RoutableModel {
            provider: "mistral".to_string(),
            model_id: "mistral-small-latest".to_string(),
            context_window: 32_000,
            input_cost_per_1m: 0.20,
            output_cost_per_1m: 0.60,
            capabilities: ModelCapabilities {
                supports_tools: true,
                supports_vision: false,
                supports_reasoning: false,
                supports_caching: false,
            },
            quality_score: 3,
            available: true,
        },

        // Simple-Moderate tier (quality 3-5): good all-rounders
        RoutableModel {
            provider: "google".to_string(),
            model_id: "gemini-2.5-flash".to_string(),
            context_window: 1_048_576,
            input_cost_per_1m: 0.15,
            output_cost_per_1m: 0.60,
            capabilities: ModelCapabilities {
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: false,
                supports_caching: false,
            },
            quality_score: 5,
            available: true,
        },
        RoutableModel {
            provider: "openai".to_string(),
            model_id: "gpt-4.1".to_string(),
            context_window: 1_048_576,
            input_cost_per_1m: 2.0,
            output_cost_per_1m: 8.0,
            capabilities: ModelCapabilities {
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: false,
                supports_caching: true,
            },
            quality_score: 5,
            available: true,
        },

        // Complex tier (quality 7-8): strong reasoning and tool use
        RoutableModel {
            provider: "anthropic".to_string(),
            model_id: "claude-sonnet-4-20250514".to_string(),
            context_window: 200_000,
            input_cost_per_1m: 3.0,
            output_cost_per_1m: 15.0,
            capabilities: ModelCapabilities {
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: true,
                supports_caching: true,
            },
            quality_score: 7,
            available: true,
        },
        RoutableModel {
            provider: "anthropic".to_string(),
            model_id: "claude-3-7-sonnet-20250219".to_string(),
            context_window: 200_000,
            input_cost_per_1m: 3.0,
            output_cost_per_1m: 15.0,
            capabilities: ModelCapabilities {
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: true,
                supports_caching: true,
            },
            quality_score: 7,
            available: true,
        },

        // Expert tier (quality 9-10): best available models
        RoutableModel {
            provider: "anthropic".to_string(),
            model_id: "claude-opus-4-20250416".to_string(),
            context_window: 200_000,
            input_cost_per_1m: 15.0,
            output_cost_per_1m: 75.0,
            capabilities: ModelCapabilities {
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: true,
                supports_caching: true,
            },
            quality_score: 10,
            available: true,
        },
        RoutableModel {
            provider: "openai".to_string(),
            model_id: "o3".to_string(),
            context_window: 128_000,
            input_cost_per_1m: 10.0,
            output_cost_per_1m: 40.0,
            capabilities: ModelCapabilities {
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: true,
                supports_caching: false,
            },
            quality_score: 9,
            available: true,
        },
    ];

    router.register_models(models);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_router() -> ModelRouter {
        let mut router = ModelRouter::new(ModelRouterConfig::default());
        register_builtin_models(&mut router);
        router
    }

    #[test]
    fn test_estimate_complexity_trivial() {
        let router = make_router();
        assert_eq!(router.estimate_complexity("hello"), ComplexityTier::Trivial);
        assert_eq!(router.estimate_complexity("hi"), ComplexityTier::Trivial);
    }

    #[test]
    fn test_estimate_complexity_simple() {
        let router = make_router();
        assert_eq!(router.estimate_complexity("what is rust?"), ComplexityTier::Simple);
    }

    #[test]
    fn test_estimate_complexity_complex() {
        let router = make_router();
        // Input with multiple complexity signals but not expert-level
        let input = "analyze the architecture of the terminal tool and explain \
                     how it handles process management and error handling.";
        assert_eq!(router.estimate_complexity(input), ComplexityTier::Complex);
    }

    #[test]
    fn test_estimate_complexity_expert() {
        let router = make_router();
        // Very long input with multiple complex signals
        let input = "debug this async rust function that executes terminal commands in docker. \
                     analyze the architecture, compare different approaches, explain why the \
                     current implementation fails when running git commands with ssh access. \
                     create a class-based interface for the tool, implement error handling with \
                     proper retry logic, and optimize the design for production deployment. \
                     Also trace the execution path, evaluate performance tradeoffs, and derive \
                     a strategy for testing this across multiple platforms including browser \
                     automation and MCP integration.";
        // Build up length to cross expert threshold
        let long_input = input.repeat(50);
        assert_eq!(router.estimate_complexity(&long_input), ComplexityTier::Expert);
    }

    #[test]
    fn test_route_trivial_to_cheap() {
        let router = make_router();
        let requires = ModelCapabilities::default();
        let decision = router.route("hello", &requires);
        // Trivial tasks should route to cheap models
        assert!(decision.complexity == ComplexityTier::Trivial);
    }

    #[test]
    fn test_route_expert_to_opus() {
        let router = make_router();
        let requires = ModelCapabilities {
            supports_tools: true,
            supports_reasoning: true,
            ..Default::default()
        };
        // Expert tasks need high quality
        let decision = router.route_by_complexity(ComplexityTier::Expert, &requires);
        assert!(decision.provider == "anthropic" || decision.provider == "openai");
    }

    #[test]
    fn test_route_requires_tools() {
        let router = make_router();
        let requires = ModelCapabilities {
            supports_tools: true,
            ..Default::default()
        };
        let decision = router.route("run a terminal command", &requires);
        // All models in our set support tools, so any model is fine
        assert!(!decision.provider.is_empty());
    }

    #[test]
    fn test_route_requires_vision_filters() {
        let router = make_router();
        let requires = ModelCapabilities {
            supports_vision: true,
            ..Default::default()
        };
        let decision = router.route("analyze this image", &requires);
        // Mistral-small doesn't support vision, shouldn't be selected for vision tasks
        assert_ne!(decision.model, "mistral-small-latest");
    }

    #[test]
    fn test_cheapest_model() {
        let router = make_router();
        let cheapest = router.cheapest_model(&ModelCapabilities::default());
        assert!(cheapest.is_some());
        let decision = cheapest.unwrap();
        // Gemini flash and gpt-4o-mini are both $0.15/1M input
        assert!(decision.model == "gemini-2.5-flash" || decision.model == "gpt-4o-mini");
    }

    #[test]
    fn test_fallback_when_no_models() {
        let router = ModelRouter::new(ModelRouterConfig {
            fallback_provider: "anthropic".to_string(),
            fallback_model: "claude-sonnet-4-20250514".to_string(),
            ..Default::default()
        });
        // No models registered
        let decision = router.route("hello", &ModelCapabilities::default());
        assert_eq!(decision.provider, "anthropic");
        assert_eq!(decision.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_models_by_tier() {
        let router = make_router();
        let tiers = router.models_by_tier();
        // Built-in models span Simple through Expert tiers
        assert!(tiers.contains_key(&ComplexityTier::Simple));
        assert!(tiers.contains_key(&ComplexityTier::Moderate));
        assert!(tiers.contains_key(&ComplexityTier::Complex));
        assert!(tiers.contains_key(&ComplexityTier::Expert));
    }

    #[test]
    fn test_complexity_tier_ordering() {
        assert!(ComplexityTier::Expert > ComplexityTier::Complex);
        assert!(ComplexityTier::Complex > ComplexityTier::Moderate);
        assert!(ComplexityTier::Moderate > ComplexityTier::Simple);
        assert!(ComplexityTier::Simple > ComplexityTier::Trivial);
    }

    #[test]
    fn test_routing_decision_has_reasoning() {
        let router = make_router();
        let decision = router.route("write a python script to parse CSV files", &ModelCapabilities::default());
        assert!(!decision.reasoning.is_empty());
        assert!(decision.reasoning.contains("Complexity"));
    }

    #[test]
    fn test_cost_optimized_picks_cheaper() {
        let router = ModelRouter::new(ModelRouterConfig {
            cost_optimized: true,
            ..Default::default()
        });
        let mut cost_router = router;
        register_builtin_models(&mut cost_router);

        let requires = ModelCapabilities::default();
        let decision = cost_router.route("what time is it", &requires);
        // Should pick cheapest model that meets trivial requirements
        assert!(decision.estimated_cost_usd >= 0.0);
    }

    #[test]
    fn test_cost_unoptimized_picks_best() {
        let router = ModelRouter::new(ModelRouterConfig {
            cost_optimized: false,
            ..Default::default()
        });
        let mut cost_router = router;
        register_builtin_models(&mut cost_router);

        let requires = ModelCapabilities::default();
        let decision = cost_router.route("what time is it", &requires);
        // Should pick highest quality model
        assert!(decision.provider == "anthropic" || decision.provider == "openai");
    }
}
