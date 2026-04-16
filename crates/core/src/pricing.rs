/// Model pricing data: prices per 1M tokens in USD.
#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
    pub cache_read_per_1m: f64,
    pub cache_write_per_1m: f64,
}

impl ModelPricing {
    /// Calculate the cost for a given usage.
    pub fn calculate_cost(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    ) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_per_1m;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_per_1m;
        let cache_read_cost = (cache_read_tokens as f64 / 1_000_000.0) * self.cache_read_per_1m;
        let cache_write_cost = (cache_write_tokens as f64 / 1_000_000.0) * self.cache_write_per_1m;
        input_cost + output_cost + cache_read_cost + cache_write_cost
    }
}

/// Get pricing for a known model. Returns None for unknown models.
pub fn get_model_pricing(model_id: &str) -> Option<ModelPricing> {
    let m = model_id.to_lowercase();
    match () {
        // Anthropic
        _ if m.contains("claude-sonnet-4") => Some(ModelPricing {
            input_per_1m: 3.0,
            output_per_1m: 15.0,
            cache_read_per_1m: 0.3,
            cache_write_per_1m: 3.75,
        }),
        _ if m.contains("claude-sonnet-3-5") => Some(ModelPricing {
            input_per_1m: 3.0,
            output_per_1m: 15.0,
            cache_read_per_1m: 0.3,
            cache_write_per_1m: 3.75,
        }),
        _ if m.contains("claude-opus") => Some(ModelPricing {
            input_per_1m: 15.0,
            output_per_1m: 75.0,
            cache_read_per_1m: 1.5,
            cache_write_per_1m: 18.75,
        }),
        _ if m.contains("claude-haiku") => Some(ModelPricing {
            input_per_1m: 0.25,
            output_per_1m: 1.25,
            cache_read_per_1m: 0.025,
            cache_write_per_1m: 0.3,
        }),
        _ if m.contains("claude") => Some(ModelPricing {
            // Default Claude pricing (fallback)
            input_per_1m: 3.0,
            output_per_1m: 15.0,
            cache_read_per_1m: 0.3,
            cache_write_per_1m: 3.75,
        }),
        // OpenAI
        _ if m.contains("gpt-4o") && !m.contains("mini") => Some(ModelPricing {
            input_per_1m: 2.5,
            output_per_1m: 10.0,
            cache_read_per_1m: 1.25,
            cache_write_per_1m: 2.5,
        }),
        _ if m.contains("gpt-4o-mini") => Some(ModelPricing {
            input_per_1m: 0.15,
            output_per_1m: 0.6,
            cache_read_per_1m: 0.075,
            cache_write_per_1m: 0.15,
        }),
        _ if m.contains("gpt-4-turbo") => Some(ModelPricing {
            input_per_1m: 10.0,
            output_per_1m: 30.0,
            cache_read_per_1m: 0.0,
            cache_write_per_1m: 0.0,
        }),
        _ if m.contains("gpt-4") => Some(ModelPricing {
            input_per_1m: 10.0,
            output_per_1m: 30.0,
            cache_read_per_1m: 0.0,
            cache_write_per_1m: 0.0,
        }),
        _ if m.contains("gpt-3.5-turbo") => Some(ModelPricing {
            input_per_1m: 0.5,
            output_per_1m: 1.5,
            cache_read_per_1m: 0.0,
            cache_write_per_1m: 0.0,
        }),
        _ if m.contains("o1") => Some(ModelPricing {
            input_per_1m: 15.0,
            output_per_1m: 60.0,
            cache_read_per_1m: 0.0,
            cache_write_per_1m: 0.0,
        }),
        _ if m.contains("o3") => Some(ModelPricing {
            input_per_1m: 10.0,
            output_per_1m: 40.0,
            cache_read_per_1m: 0.0,
            cache_write_per_1m: 0.0,
        }),
        // Mistral
        _ if m.contains("mistral-large") => Some(ModelPricing {
            input_per_1m: 2.0,
            output_per_1m: 6.0,
            cache_read_per_1m: 0.0,
            cache_write_per_1m: 0.0,
        }),
        _ if m.contains("mistral-small") => Some(ModelPricing {
            input_per_1m: 0.1,
            output_per_1m: 0.3,
            cache_read_per_1m: 0.0,
            cache_write_per_1m: 0.0,
        }),
        _ if m.contains("mistral") => Some(ModelPricing {
            input_per_1m: 0.25,
            output_per_1m: 1.0,
            cache_read_per_1m: 0.0,
            cache_write_per_1m: 0.0,
        }),
        // Google / Gemini
        _ if m.contains("gemini") => Some(ModelPricing {
            input_per_1m: 1.25,
            output_per_1m: 5.0,
            cache_read_per_1m: 0.0,
            cache_write_per_1m: 0.0,
        }),
        // DeepSeek
        _ if m.contains("deepseek") => Some(ModelPricing {
            input_per_1m: 0.27,
            output_per_1m: 1.1,
            cache_read_per_1m: 0.07,
            cache_write_per_1m: 0.0,
        }),
        // Default: conservative estimate
        _ => None,
    }
}

/// Calculate the estimated cost for a token usage given a model.
pub fn estimate_cost(model_id: &str, usage: &crate::CostTracker) -> f64 {
    match get_model_pricing(model_id) {
        Some(pricing) => pricing.calculate_cost(
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            usage.cache_write_tokens,
        ),
        None => {
            // Unknown model: use a default rate ($3/1M input, $15/1M output)
            let default_pricing = ModelPricing {
                input_per_1m: 3.0,
                output_per_1m: 15.0,
                cache_read_per_1m: 0.3,
                cache_write_per_1m: 3.75,
            };
            default_pricing.calculate_cost(
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_write_tokens,
            )
        }
    }
}

/// Format a token count in a human-readable way (e.g., "1.2M", "500K", "123").
pub fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

/// Format a cost in USD with appropriate precision.
pub fn format_cost(usd: f64) -> String {
    if usd < 0.001 {
        format!("<$0.001")
    } else if usd < 0.01 {
        format!("${:.4}", usd)
    } else if usd < 1.0 {
        format!("${:.3}", usd)
    } else {
        format!("${:.2}", usd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CostTracker;

    #[test]
    fn test_model_pricing_claude_sonnet() {
        let pricing = get_model_pricing("claude-sonnet-4-6").unwrap();
        let cost = pricing.calculate_cost(10_000, 5_000, 5_000, 0);
        // 10K input * $3/1M + 5K output * $15/1M + 5K cache_read * $0.3/1M
        let expected = (10_000.0 / 1_000_000.0) * 3.0
            + (5_000.0 / 1_000_000.0) * 15.0
            + (5_000.0 / 1_000_000.0) * 0.3;
        assert!((cost - expected).abs() < 0.00001);
    }

    #[test]
    fn test_model_pricing_gpt4o() {
        let pricing = get_model_pricing("gpt-4o").unwrap();
        assert!(pricing.input_per_1m > 0.0);
        assert!(pricing.output_per_1m > 0.0);
    }

    #[test]
    fn test_model_pricing_unknown() {
        assert!(get_model_pricing("some-unknown-model-xyz").is_none());
    }

    #[test]
    fn test_estimate_cost_known() {
        let usage = CostTracker {
            input_tokens: 100_000,
            output_tokens: 50_000,
            cache_read_tokens: 50_000,
            cache_write_tokens: 0,
            ..Default::default()
        };
        let cost = estimate_cost("claude-sonnet-4-6", &usage);
        assert!(cost > 0.0);
    }

    #[test]
    fn test_estimate_cost_unknown_uses_default() {
        let usage = CostTracker {
            input_tokens: 10_000,
            output_tokens: 5_000,
            ..Default::default()
        };
        let cost = estimate_cost("unknown-model", &usage);
        assert!(cost > 0.0);
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1_500), "1.5K");
        assert_eq!(format_tokens(1_200_000), "1.2M");
    }

    #[test]
    fn test_format_cost() {
        assert_eq!(format_cost(0.004), "$0.0040");
        assert_eq!(format_cost(0.05), "$0.050");
        assert_eq!(format_cost(1.50), "$1.50");
        assert_eq!(format_cost(12.34), "$12.34");
    }
}
