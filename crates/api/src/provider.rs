use h_core::ProviderId;

/// Information about a model provider.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub id: ProviderId,
    pub display_name: &'static str,
    pub default_base_url: &'static str,
    pub api_key_env: &'static str,
    pub default_model: &'static str,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_reasoning: bool,
    pub api_mode: ApiMode,
}

/// API protocol mode for making requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiMode {
    /// OpenAI Chat Completions API
    ChatCompletions,
    /// OpenAI Codex Responses API
    CodexResponses,
    /// Anthropic Messages API
    AnthropicMessages,
}

impl ProviderInfo {
    pub fn anthropic() -> Self {
        Self {
            id: ProviderId::new("anthropic"),
            display_name: "Anthropic",
            default_base_url: "https://api.anthropic.com",
            api_key_env: "ANTHROPIC_API_KEY",
            default_model: "claude-sonnet-4-6-20250514",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            api_mode: ApiMode::AnthropicMessages,
        }
    }

    pub fn openai() -> Self {
        Self {
            id: ProviderId::new("openai"),
            display_name: "OpenAI",
            default_base_url: "https://api.openai.com/v1",
            api_key_env: "OPENAI_API_KEY",
            default_model: "gpt-4o",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn openrouter() -> Self {
        Self {
            id: ProviderId::new("openrouter"),
            display_name: "OpenRouter",
            default_base_url: "https://openrouter.ai/api/v1",
            api_key_env: "OPENROUTER_API_KEY",
            default_model: "anthropic/claude-sonnet-4-6",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn nous() -> Self {
        Self {
            id: ProviderId::new("nous"),
            display_name: "Nous Research",
            default_base_url: "https://api.nousresearch.com/v1",
            api_key_env: "NOUS_API_KEY",
            default_model: "hermes-3-llama-3.1-405b",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn xiaomi_mimo() -> Self {
        Self {
            id: ProviderId::new("xiaomi_mimo"),
            display_name: "Xiaomi MiMo",
            default_base_url: "https://api.xiaomimimo.com/v1",
            api_key_env: "XIAOMI_MIMO_API_KEY",
            default_model: "mimo-v2",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn z_ai() -> Self {
        Self {
            id: ProviderId::new("z_ai"),
            display_name: "Z.AI / GLM",
            default_base_url: "https://open.bigmodel.cn/api/paas/v4",
            api_key_env: "Z_API_KEY",
            default_model: "glm-4-plus",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn kimi() -> Self {
        Self {
            id: ProviderId::new("kimi"),
            display_name: "Kimi / Moonshot",
            default_base_url: "https://api.moonshot.cn/v1",
            api_key_env: "KIMI_API_KEY",
            default_model: "moonshot-v1-8k",
            supports_tools: true,
            supports_vision: false,
            supports_reasoning: false,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn minimax() -> Self {
        Self {
            id: ProviderId::new("minimax"),
            display_name: "MiniMax",
            default_base_url: "https://api.minimax.chat/v1",
            api_key_env: "MINIMAX_API_KEY",
            default_model: "MiniMax-Text-01",
            supports_tools: true,
            supports_vision: false,
            supports_reasoning: false,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn huggingface() -> Self {
        Self {
            id: ProviderId::new("huggingface"),
            display_name: "HuggingFace",
            default_base_url: "https://api-inference.huggingface.co/v1",
            api_key_env: "HUGGINGFACE_API_KEY",
            default_model: "meta-llama/Meta-Llama-3-70B-Instruct",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn ollama() -> Self {
        Self {
            id: ProviderId::new("ollama"),
            display_name: "Ollama",
            default_base_url: "http://localhost:11434/v1",
            api_key_env: "",
            default_model: "llama3",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn mistral() -> Self {
        Self {
            id: ProviderId::new("mistral"),
            display_name: "Mistral",
            default_base_url: "https://api.mistral.ai/v1",
            api_key_env: "MISTRAL_API_KEY",
            default_model: "mistral-large-latest",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn gemini() -> Self {
        Self {
            id: ProviderId::new("gemini"),
            display_name: "Google Gemini",
            default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
            api_key_env: "GEMINI_API_KEY",
            default_model: "gemini-2.5-pro",
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn groq() -> Self {
        Self {
            id: ProviderId::new("groq"),
            display_name: "Groq",
            default_base_url: "https://api.groq.com/openai/v1",
            api_key_env: "GROQ_API_KEY",
            default_model: "llama-3.3-70b-versatile",
            supports_tools: true,
            supports_vision: false,
            supports_reasoning: false,
            api_mode: ApiMode::ChatCompletions,
        }
    }

    pub fn generic(base_url: &str) -> Self {
        Self {
            id: ProviderId::new("generic"),
            display_name: "Generic OpenAI-compatible",
            default_base_url: Box::leak(base_url.to_string().into_boxed_str()),
            api_key_env: "API_KEY",
            default_model: "",
            supports_tools: true,
            supports_vision: false,
            supports_reasoning: false,
            api_mode: ApiMode::ChatCompletions,
        }
    }
}
