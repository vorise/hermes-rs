use std::sync::Arc;

use anyhow::{Result, anyhow};
use tracing::{info, warn};

use crate::base::{IncomingMessage, PlatformAdapter};
use crate::config::GatewayConfig;
use crate::session::GatewaySessionStore;

use h_api::client::{ApiClient, ApiConfig};
use h_api::provider::ApiMode;
use h_core::{HermesConfig, Message, ModelId, ModelRef, ProviderId};
use h_mcp::McpState;
use h_query::{PromptBuilder, QueryConfig, QueryLoop};
use h_tools::Tool;

/// Dispatches an incoming message to the query loop and streams
/// the response back to the platform.
///
/// This is the core of the gateway's message handling — it bridges
/// the platform adapter with the query loop.
pub async fn dispatch_message(
    msg: &IncomingMessage,
    adapter: &dyn PlatformAdapter,
    session_store: &GatewaySessionStore,
    _config: &GatewayConfig,
    hermes_config: &HermesConfig,
    all_tools: &[Arc<dyn Tool>],
    mcp_state: Option<&Arc<McpState>>,
) -> Result<()> {
    let session = session_store
        .get_or_create(adapter.name(), &msg.user_id, &msg.chat_id)
        .await?;

    info!(
        session_id = %session.session_id,
        platform = adapter.name(),
        user = %msg.user_id,
        "Dispatching to query loop"
    );

    // Create stream consumer for real-time delivery
    let consumer: Arc<dyn h_core::StreamConsumer> = Arc::from(adapter.create_consumer(&msg.chat_id, msg.message_id.clone()));

    // Resolve model
    let model_ref = resolve_model(hermes_config);

    // Build API client
    let api_config = build_api_config(hermes_config, &model_ref)?;
    let api_client = ApiClient::new(api_config)?;

    // Build tool registry and tool definitions
    let tool_registry = h_query::ToolRegistry::new();
    let enabled_tools = filter_tools(all_tools, hermes_config);
    for tool in &enabled_tools {
        tool_registry.register(tool.clone());
    }

    // Wire in MCP tools from connected servers
    let mcp_tools = if let Some(ref state) = mcp_state {
        state.tools().await
    } else {
        Vec::new()
    };
    for tool in &mcp_tools {
        tool_registry.register(tool.clone());
    }

    // Build tool definitions (built-in + MCP)
    let all_tool_defs: Vec<_> = enabled_tools.iter().chain(&mcp_tools).map(|t| t.to_definition()).collect();

    // Load conversation history from session DB
    let stored_messages = session_store
        .db()
        .get_messages(&session.session_id)?;

    let messages: Vec<Message> = stored_messages
        .into_iter()
        .filter_map(|m| stored_message_to_core(&m))
        .collect();

    // Build system prompt
    let system_prompt = build_system_prompt(hermes_config);

    // Build query config
    let mut query_config = QueryConfig::new(model_ref)
        .with_system_prompt(system_prompt)
        .with_max_iterations(90); // default matches QueryConfig default

    for m in &messages {
        query_config = query_config.with_message(m.clone());
    }

    // Build query config with tools
    query_config = query_config.with_tools(all_tool_defs);

    // Run query loop with stream consumer for real-time delivery
    let query_loop = QueryLoop::new(&query_config)
        .with_consumer(Arc::from(consumer));
    let interrupt = Arc::new(tokio::sync::Notify::new());

    let result = query_loop
        .run(&api_client, &tool_registry, interrupt)
        .await?;

    // Save assistant response to session DB
    for m in &result.messages {
        if let Err(e) = session_store.db().add_message(&session.session_id, m) {
            warn!("Failed to save message to session DB: {e}");
        }
    }

    // Update session stats
    if let Err(e) = session_store.db().update_session_stats(&session.session_id, &result.cost) {
        warn!("Failed to update session stats: {e}");
    }

    // Send final response back to platform (may already have been streamed)
    if !result.final_text.is_empty() {
        adapter.send_message(&msg.chat_id, &result.final_text).await?;
    }

    Ok(())
}

/// Resolve the current model from config.
fn resolve_model(config: &HermesConfig) -> ModelRef {
    let provider = config.provider.as_deref().unwrap_or("anthropic");
    let model = config.model.as_deref().unwrap_or("claude-sonnet-4-6");
    ModelRef::new(ProviderId::new(provider), ModelId::new(model))
}

/// Build an APIConfig from HermesConfig and model ref.
fn build_api_config(hermes: &HermesConfig, model_ref: &ModelRef) -> Result<ApiConfig> {
    let provider = &model_ref.provider;
    let model = &model_ref.model;

    let env_var = match provider.as_str() {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" | "openrouter" => "OPENAI_API_KEY",
        "nous" => "NOUS_API_KEY",
        _ => "API_KEY",
    };

    let api_key = std::env::var(env_var)
        .map_err(|_| anyhow!("API key not found in environment: {env_var}"))?;

    let base_url = hermes.base_url.clone().unwrap_or_else(|| match provider.as_str() {
        "anthropic" => "https://api.anthropic.com".to_string(),
        "openai" => "https://api.openai.com/v1".to_string(),
        "openrouter" => "https://openrouter.ai/api/v1".to_string(),
        "nous" => "https://api.nousresearch.com/v1".to_string(),
        _ => "https://api.openai.com/v1".to_string(),
    });

    let api_mode = match provider.as_str() {
        "anthropic" => ApiMode::AnthropicMessages,
        _ => ApiMode::ChatCompletions,
    };

    Ok(ApiConfig {
        provider: provider.clone(),
        model: model.clone(),
        base_url,
        api_key,
        api_mode,
        max_tokens: Some(4096),
        temperature: Some(0.7),
        reasoning_effort: None,
    })
}

/// Filter tools based on enabled toolsets in config.
fn filter_tools(
    all_tools: &[Arc<dyn Tool>],
    config: &HermesConfig,
) -> Vec<Arc<dyn Tool>> {
    if let Some(ref disabled) = config.disabled_toolsets {
        all_tools
            .iter()
            .filter(|t| !disabled.contains(&t.toolset().to_string()))
            .cloned()
            .collect()
    } else if let Some(ref enabled) = config.enabled_toolsets {
        all_tools
            .iter()
            .filter(|t| enabled.contains(&t.toolset().to_string()))
            .cloned()
            .collect()
    } else {
        all_tools.to_vec()
    }
}

/// Build the system prompt.
fn build_system_prompt(config: &HermesConfig) -> String {
    let mut prompt = PromptBuilder::build_cli();

    // Add personality if configured
    if let Some(ref personality) = config.personality {
        prompt = format!("{personality}\n\n{prompt}");
    }

    prompt
}

/// Convert a stored message from the session DB to a core Message.
fn stored_message_to_core(stored: &h_core::session::StoredMessage) -> Option<Message> {
    stored.to_message().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_model_default() {
        let config = HermesConfig::default();
        let model = resolve_model(&config);
        assert_eq!(model.provider.as_str(), "anthropic");
        assert_eq!(model.model.as_str(), "claude-sonnet-4-6");
    }

    #[test]
    fn test_resolve_model_custom() {
        let config = HermesConfig {
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            ..Default::default()
        };
        let model = resolve_model(&config);
        assert_eq!(model.provider.as_str(), "openai");
        assert_eq!(model.model.as_str(), "gpt-4o");
    }

    #[test]
    fn test_build_system_prompt_default() {
        let config = HermesConfig::default();
        let prompt = build_system_prompt(&config);
        assert!(prompt.contains("Hermes"));
    }

    #[test]
    fn test_build_system_prompt_personality() {
        let config = HermesConfig {
            personality: Some("You are a terse robot".to_string()),
            ..Default::default()
        };
        let prompt = build_system_prompt(&config);
        assert!(prompt.contains("terse robot"));
    }

    #[test]
    fn test_filter_tools_disabled() {
        let config = HermesConfig {
            disabled_toolsets: Some(vec!["terminal".to_string()]),
            ..Default::default()
        };
        let all: Vec<Arc<dyn Tool>> = vec![
            Arc::new(h_tools::TerminalTool::new()),
            Arc::new(h_tools::ReadFileTool),
        ];
        let filtered = filter_tools(&all, &config);
        assert_eq!(filtered.len(), 1); // Only ReadFileTool
    }

    #[test]
    fn test_filter_tools_enabled() {
        let config = HermesConfig {
            enabled_toolsets: Some(vec!["file_io".to_string()]),
            ..Default::default()
        };
        let all: Vec<Arc<dyn Tool>> = vec![
            Arc::new(h_tools::TerminalTool::new()),
            Arc::new(h_tools::ReadFileTool),
        ];
        let filtered = filter_tools(&all, &config);
        assert_eq!(filtered.len(), 1); // Only ReadFileTool
    }
}
