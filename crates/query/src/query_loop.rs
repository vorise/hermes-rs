use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use h_api::ApiClient;
use h_core::{CostTracker, Message, SessionDB, StreamConsumer, ToolCall, ToolCallFunction};
use h_tools::{Tool, ToolContext, ToolResult as HToolResult};
use h_plugins::{PluginRegistry, HookContext, HookType};
use parking_lot::Mutex;
use tokio::sync::Notify;
use tokio::time::{timeout, Duration};
use tracing::{debug, info, warn};

use crate::config::{QueryConfig, QueryResult, StopReason};
use crate::context_compressor::{CompressorConfig, ContextCompressor, PreflightResult};
use crate::model_routing::{estimate_complexity, ModelTier, resolve_model_for_tier};

/// Parallel safety classification for tool calls.
#[derive(Debug, PartialEq, Eq)]
pub enum ParallelSafety {
    NeverParallel,
    ParallelSafe,
    PathScoped,
}

/// Classify whether a batch of tool calls can be executed in parallel.
pub fn should_parallelize(tool_calls: &[ToolCall]) -> bool {
    if tool_calls.len() <= 1 {
        return true;
    }

    let safety: Vec<ParallelSafety> = tool_calls
        .iter()
        .map(classify_tool_call_safety)
        .collect();

    // If any tool is never parallel, run sequentially
    if safety.iter().any(|s| *s == ParallelSafety::NeverParallel) {
        return false;
    }

    // All are parallel-safe
    if safety.iter().all(|s| *s == ParallelSafety::ParallelSafe) {
        return true;
    }

    // Path-scoped tools: check for path overlap
    let paths: Vec<_> = tool_calls
        .iter()
        .filter_map(|tc| extract_path_from_args(&tc.function))
        .collect();

    // No path overlap = safe to parallelize
    paths.len() == tool_calls.len()
}

fn classify_tool_call_safety(tc: &ToolCall) -> ParallelSafety {
    match tc.function.name.as_str() {
        "clarify" => ParallelSafety::NeverParallel,
        "read_file" | "search_files" | "web_search" | "web_extract"
        | "memory" | "session_search" | "skill_view" | "skills_list" => {
            ParallelSafety::ParallelSafe
        }
        "write_file" | "patch" => ParallelSafety::PathScoped,
        "terminal" => ParallelSafety::NeverParallel,
        _ => ParallelSafety::NeverParallel,
    }
}

fn extract_path_from_args(func: &ToolCallFunction) -> Option<String> {
    let args: serde_json::Value = serde_json::from_str(&func.arguments).ok()?;
    args.get("path").and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Shared state for the query loop.
pub struct QueryState {
    pub messages: Vec<Message>,
    pub cost: CostTracker,
    pub tool_calls_made: u32,
    pub interrupted: bool,
}

impl QueryState {
    pub fn new(initial_messages: Vec<Message>) -> Self {
        Self {
            messages: initial_messages,
            cost: CostTracker::default(),
            tool_calls_made: 0,
            interrupted: false,
        }
    }
}

/// Enhanced query loop with session persistence, context compression, and model routing.
pub struct QueryLoop {
    config: QueryConfig,
    compressor: Option<ContextCompressor>,
    enable_model_routing: bool,
    /// If true, save messages to session DB after each turn.
    persist_to_session: bool,
    /// Session DB and ID for persistence.
    session_db: Option<Arc<SessionDB>>,
    session_id: Option<String>,
    /// Plugin registry for hook invocations.
    plugins: Option<PluginRegistry>,
    /// Optional stream consumer for real-time response delivery.
    stream_consumer: Option<Arc<dyn StreamConsumer>>,
}

impl QueryLoop {
    pub fn new(config: &QueryConfig) -> Self {
        Self {
            config: config.clone(),
            compressor: None,
            enable_model_routing: false,
            persist_to_session: false,
            session_db: None,
            session_id: None,
            plugins: None,
            stream_consumer: None,
        }
    }

    /// Enable context compression.
    pub fn with_compression(mut self, config: CompressorConfig) -> Self {
        self.compressor = Some(ContextCompressor::new(config));
        self
    }

    /// Enable automatic model tier routing based on query complexity.
    pub fn with_model_routing(mut self) -> Self {
        self.enable_model_routing = true;
        self
    }

    /// Enable persistence of messages and cost to session DB.
    pub fn with_persistence(mut self) -> Self {
        self.persist_to_session = true;
        self
    }

    /// Wire up session DB for message persistence.
    pub fn with_session(mut self, session_db: Arc<SessionDB>, session_id: String) -> Self {
        self.session_db = Some(session_db);
        self.session_id = Some(session_id);
        self.persist_to_session = true;
        self
    }

    /// Wire up plugin registry for hook invocations.
    pub fn with_plugins(mut self, plugins: PluginRegistry) -> Self {
        self.plugins = Some(plugins);
        self
    }

    /// Wire up a stream consumer for real-time response delivery.
    pub fn with_consumer(mut self, consumer: Arc<dyn StreamConsumer>) -> Self {
        self.stream_consumer = Some(consumer);
        self
    }

    /// Send a text delta to the stream consumer, if configured.
    async fn emit_delta(&self, delta: &str) {
        if let Some(ref consumer) = self.stream_consumer {
            if let Err(e) = consumer.on_text_delta(delta).await {
                warn!("Stream consumer on_text_delta failed: {e}");
            }
        }
    }

    /// Send a tool start event to the stream consumer, if configured.
    async fn emit_tool_start(&self, tool_name: &str, args_preview: &str) {
        if let Some(ref consumer) = self.stream_consumer {
            if let Err(e) = consumer.on_tool_start(tool_name, args_preview).await {
                warn!("Stream consumer on_tool_start failed: {e}");
            }
        }
    }

    /// Send a tool complete event to the stream consumer, if configured.
    async fn emit_tool_complete(&self, tool_name: &str, result_preview: &str) {
        if let Some(ref consumer) = self.stream_consumer {
            if let Err(e) = consumer.on_tool_complete(tool_name, result_preview).await {
                warn!("Stream consumer on_tool_complete failed: {e}");
            }
        }
    }

    /// Flush the stream consumer, if configured.
    async fn emit_flush(&self) {
        if let Some(ref consumer) = self.stream_consumer {
            if let Err(e) = consumer.flush().await {
                warn!("Stream consumer flush failed: {e}");
            }
        }
    }

    /// Invoke a hook across all registered plugins, if any are configured.
    fn invoke_hook(&self, hook_type: HookType, ctx: &HookContext) -> Vec<serde_json::Value> {
        match &self.plugins {
            Some(registry) => registry.invoke_hook(hook_type.as_str(), ctx),
            None => Vec::new(),
        }
    }

    /// Persist a single message to the session DB. Logs errors but does not fail the query.
    fn persist_message(&self, msg: &Message) {
        if let (Some(ref db), Some(ref session_id)) = (&self.session_db, &self.session_id) {
            if let Err(e) = db.add_message(session_id, msg) {
                warn!("Failed to persist message to session DB: {e}");
            }
        }
    }

    /// Run the main query loop with retry, context compression, and optional model routing.
    pub async fn run(
        &self,
        api_client: &ApiClient,
        tool_registry: &ToolRegistry,
        interrupt_notify: Arc<Notify>,
    ) -> Result<QueryResult> {
        let budget = self.config.budget();
        let mut state = QueryState::new(self.config.messages.clone());

        info!(
            model = %self.config.model,
            max_iterations = self.config.max_iterations,
            tools = self.config.tools.len(),
            "Starting query loop"
        );

        // Fire on_session_start hook
        let session_id = self.session_id.clone();
        let ctx = HookContext::new("on_session_start".to_string(), session_id.clone());
        let _start_results = self.invoke_hook(HookType::OnSessionStart, &ctx);

        let mut current_model = self.config.model.clone();

        loop {
            // Check interrupt (non-blocking)
            if timeout(Duration::from_nanos(1), interrupt_notify.notified())
                .await
                .is_ok()
            {
                warn!("Query loop interrupted");
                state.interrupted = true;
                return Ok(build_result(state, StopReason::Interrupted));
            }

            // Check budget
            if !budget.consume() {
                info!("Query loop: max iterations reached");
                return Ok(build_result(state, StopReason::MaxIterationsReached));
            }

            // Context compression check
            if let Some(ref compressor) = self.compressor {
                let mut all_messages = vec![Message::system(&self.config.system_prompt)];
                all_messages.extend(state.messages.clone());

                match compressor.preflight_check(&all_messages, &self.config.system_prompt, &current_model) {
                    PreflightResult::Ok { .. } => {
                        // Context is fine
                    }
                    PreflightResult::Warning { .. } | PreflightResult::Critical { .. } => {
                        debug!("Context compression needed");
                        let compress_result = compressor.compress(&mut state.messages, Some(api_client)).await;
                        match compress_result {
                            Ok(res) => {
                                info!(
                                    original_tokens = res.original_tokens,
                                    compressed_tokens = res.compressed_tokens,
                                    messages_compressed = res.messages_compressed,
                                    "Context compressed"
                                );
                            }
                            Err(e) => {
                                warn!("Context compression failed: {e}");
                            }
                        }
                    }
                }
            }

            // Model routing: estimate complexity and potentially switch to a faster/slower model
            if self.enable_model_routing {
                let last_user_msg = state.messages.iter()
                    .rev()
                    .find(|m| m.role == h_core::Role::User)
                    .and_then(|m| m.content.as_ref().and_then(|c| c.as_text()));

                if let Some(text) = last_user_msg {
                    let score = estimate_complexity(text);
                    let tier = score.tier();
                    if tier != ModelTier::Balanced {
                        let routed = resolve_model_for_tier(&current_model, tier);
                        if routed != current_model {
                            debug!(
                                from = %current_model,
                                to = %routed,
                                complexity = score.0,
                                "Rerouting to model tier"
                            );
                            current_model = routed;
                        }
                    }
                }
            }

            // Fire pre_llm_call hook - plugins can inject context
            let ctx = HookContext::new("pre_llm_call".to_string(), session_id.clone());
            let _pre_results = self.invoke_hook(HookType::PreLlmCall, &ctx);

            // Prepare messages with system prompt
            let mut messages = vec![Message::system(&self.config.system_prompt)];
            messages.extend(state.messages.clone());

            debug!(
                message_count = messages.len(),
                model = %current_model,
                "Calling LLM API"
            );

            // Call LLM API (already has built-in retry via with_retry in ApiClient::chat)
            let response = match api_client.chat(&messages, &self.config.tools).await {
                Ok(resp) => resp,
                Err(e) => {
                    warn!("LLM API error after retries: {e}");
                    return Ok(build_result(state, StopReason::Error(e.to_string())));
                }
            };

            // Track cost
            let iteration_cost = response.to_cost_tracker();
            state.cost.add(&iteration_cost);

            // Extract response
            let text_content = response.text_content();
            let tool_calls = response.tool_calls();

            // Stream text content to consumer
            if let Some(ref text) = text_content {
                self.emit_delta(text).await;
            }

            // Fire post_llm_response hook - plugins can process the response
            let ctx = HookContext::new("post_llm_response".to_string(), session_id.clone());
            ctx.set("response_text", serde_json::json!(text_content));
            ctx.set("tool_call_count", serde_json::json!(tool_calls.len()));
            let _post_results = self.invoke_hook(HookType::PostLlmResponse, &ctx);

            // Add assistant response to state
            let assistant_msg = if !tool_calls.is_empty() {
                Message::assistant_tool_calls(
                    tool_calls.clone(),
                    text_content.clone(),
                )
            } else if let Some(text) = &text_content {
                Message::assistant(text.clone())
            } else {
                Message::assistant("")
            };

            // Persist to session DB before moving into state
            if self.persist_to_session {
                self.persist_message(&assistant_msg);
            }

            state.messages.push(assistant_msg);

            // If no tool calls, we're done
            if tool_calls.is_empty() {
                debug!("LLM response complete, no tool calls");
                return Ok(build_result(state, StopReason::Completed));
            }

            // Execute tools
            debug!(tool_calls = tool_calls.len(), "Executing tools");

            // Emit tool start for each tool call
            for tc in &tool_calls {
                let args_preview = if tc.function.arguments.len() > 100 {
                    format!("{}...", &tc.function.arguments[..100])
                } else {
                    tc.function.arguments.clone()
                };
                self.emit_tool_start(&tc.function.name, &args_preview).await;
            }

            // Fire on_tool_call hook before execution
            let tool_call_ids: Vec<String> = tool_calls.iter().map(|tc| tc.id.clone()).collect();
            let tool_call_names: Vec<String> = tool_calls.iter().map(|tc| tc.function.name.clone()).collect();
            let ctx = HookContext::new("on_tool_call".to_string(), session_id.clone());
            ctx.set("tool_call_ids", serde_json::json!(tool_call_ids));
            ctx.set("tool_call_names", serde_json::json!(tool_call_names));
            let _tool_pre_results = self.invoke_hook(HookType::OnToolCall, &ctx);

            let tool_results = if should_parallelize(&tool_calls) {
                execute_tools_parallel(&tool_calls, tool_registry).await
            } else {
                execute_tools_sequential(&tool_calls, tool_registry).await
            };

            // Add tool results to state
            for (tc, result) in tool_calls.iter().zip(tool_results.iter()) {
                state.tool_calls_made += 1;
                let result_msg = Message::tool_result(
                    String::new(),
                    if result.is_error {
                        format!("Error: {}", result.content)
                    } else {
                        result.content.clone()
                    },
                );

                // Emit tool complete to consumer
                let result_preview = if result.content.len() > 100 {
                    format!("{}...", &result.content[..100])
                } else {
                    result.content.clone()
                };
                self.emit_tool_complete(&tc.function.name, &result_preview).await;

                // Fire on_tool_result hook after each tool completes
                let ctx = HookContext::new("on_tool_result".to_string(), session_id.clone());
                ctx.set("tool_name", serde_json::json!(tc.function.name));
                ctx.set("tool_call_id", serde_json::json!(tc.id));
                ctx.set("result_is_error", serde_json::json!(result.is_error));
                let _result_results = self.invoke_hook(HookType::OnToolResult, &ctx);

                // Persist tool result to session DB
                if self.persist_to_session {
                    self.persist_message(&result_msg);
                }

                state.messages.push(result_msg);
            }

            // Flush stream consumer after each tool cycle
            self.emit_flush().await;
        }
    }
}

/// Run the main query loop (backwards-compatible entry point).
pub async fn run_query_loop(
    config: &QueryConfig,
    api_client: &ApiClient,
    tool_registry: &ToolRegistry,
    interrupt_notify: Arc<Notify>,
) -> Result<QueryResult> {
    let query_loop = QueryLoop::new(config);
    query_loop.run(api_client, tool_registry, interrupt_notify).await
}

/// Execute tools in parallel (up to 8 workers).
async fn execute_tools_parallel(
    tool_calls: &[ToolCall],
    registry: &ToolRegistry,
) -> Vec<HToolResult> {
    const MAX_WORKERS: usize = 8;

    let mut results = Vec::with_capacity(tool_calls.len());
    let mut tasks = Vec::new();

    for tc in tool_calls {
        let tc = tc.clone();
        let registry = registry.clone();
        tasks.push(tokio::spawn(async move {
            let result = execute_single_tool(&tc, &registry).await;
            (tc.id.clone(), result)
        }));

        // Respect worker limit
        if tasks.len() >= MAX_WORKERS {
            let (_id, result) = tasks.remove(0).await.unwrap_or_else(|e| {
                (String::new(), Err(anyhow!("Task join error: {e}")))
            });
            results.push(tool_result_or_error(&result));
        }
    }

    // Wait for remaining tasks
    for task in tasks {
        let (_id, result) = task.await.unwrap_or_else(|e| {
            (String::new(), Err(anyhow!("Task join error: {e}")))
        });
        results.push(tool_result_or_error(&result));
    }

    results
}

fn tool_result_or_error(result: &Result<HToolResult>) -> HToolResult {
    match result {
        Ok(r) => r.clone(),
        Err(e) => HToolResult::err(format!("Execution error: {e}")),
    }
}

/// Execute tools sequentially.
async fn execute_tools_sequential(
    tool_calls: &[ToolCall],
    registry: &ToolRegistry,
) -> Vec<HToolResult> {
    let mut results = Vec::new();
    for tc in tool_calls {
        let result = match execute_single_tool(tc, registry).await {
            Ok(r) => r,
            Err(e) => HToolResult::err(format!("Execution error: {e}")),
        };
        results.push(result);
    }
    results
}

/// Execute a single tool call.
async fn execute_single_tool(
    tc: &ToolCall,
    registry: &ToolRegistry,
) -> Result<HToolResult> {
    let tool = registry
        .get(&tc.function.name)
        .ok_or_else(|| anyhow!("unknown tool: {}", tc.function.name))?;

    let args = tc.function.parse_args()?;
    let ctx = ToolContext {
        session_id: String::new(),
        task_id: String::new(),
        config: Arc::new(h_core::HermesConfig::default()),
        working_dir: std::env::current_dir().unwrap_or_default(),
        clarify: None,
    };

    tool.execute(args, &ctx).await
}

fn build_result(state: QueryState, stopped_reason: StopReason) -> QueryResult {
    let final_text = state
        .messages
        .iter()
        .rev()
        .find_map(|m| m.content.as_ref().and_then(|c| c.as_text()))
        .unwrap_or("")
        .to_string();

    QueryResult {
        final_text,
        messages: state.messages,
        iterations_used: state.tool_calls_made,
        tool_calls_made: state.tool_calls_made,
        cost: state.cost,
        stopped_reason,
    }
}

/// Tool registry for the query loop.
#[derive(Clone)]
pub struct ToolRegistry {
    tools: Arc<Mutex<HashMap<String, Arc<dyn Tool>>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.tools
            .lock()
            .insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.lock().get(name).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        self.tools.lock().keys().cloned().collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_parallelize_empty() {
        assert!(should_parallelize(&[]));
    }

    #[test]
    fn test_should_parallelize_single() {
        let tc = ToolCall {
            id: "1".to_string(),
            function: ToolCallFunction {
                name: "read_file".to_string(),
                arguments: r#"{"path": "/test"}"#.to_string(),
            },
        };
        assert!(should_parallelize(&[tc]));
    }

    #[test]
    fn test_should_not_parallelize_clarify() {
        let tc1 = ToolCall {
            id: "1".to_string(),
            function: ToolCallFunction {
                name: "clarify".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let tc2 = ToolCall {
            id: "2".to_string(),
            function: ToolCallFunction {
                name: "read_file".to_string(),
                arguments: r#"{"path": "/test"}"#.to_string(),
            },
        };
        assert!(!should_parallelize(&[tc1, tc2]));
    }

    #[test]
    fn test_should_parallelize_safe_tools() {
        let tc1 = ToolCall {
            id: "1".to_string(),
            function: ToolCallFunction {
                name: "read_file".to_string(),
                arguments: r#"{"path": "/a"}"#.to_string(),
            },
        };
        let tc2 = ToolCall {
            id: "2".to_string(),
            function: ToolCallFunction {
                name: "web_search".to_string(),
                arguments: r#"{"query": "test"}"#.to_string(),
            },
        };
        assert!(should_parallelize(&[tc1, tc2]));
    }

    #[test]
    fn test_classify_tool_safety() {
        let tc = ToolCall {
            id: "1".to_string(),
            function: ToolCallFunction {
                name: "terminal".to_string(),
                arguments: "ls".to_string(),
            },
        };
        assert_eq!(
            classify_tool_call_safety(&tc),
            ParallelSafety::NeverParallel
        );
    }

    #[test]
    fn test_query_loop_builder() {
        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let query_loop = QueryLoop::new(&config);
        assert!(!query_loop.enable_model_routing);
        assert!(query_loop.compressor.is_none());
    }

    #[test]
    fn test_query_loop_with_compression() {
        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let query_loop = QueryLoop::new(&config)
            .with_compression(CompressorConfig::default());
        assert!(query_loop.compressor.is_some());
    }

    #[test]
    fn test_query_loop_with_model_routing() {
        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let query_loop = QueryLoop::new(&config)
            .with_model_routing();
        assert!(query_loop.enable_model_routing);
    }

    #[test]
    fn test_query_loop_with_session() {
        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let db = Arc::new(h_core::SessionDB::new_in_memory().unwrap());
        let query_loop = QueryLoop::new(&config)
            .with_session(db.clone(), "test-session".to_string());
        assert!(query_loop.persist_to_session);
        assert!(query_loop.session_db.is_some());
        assert!(query_loop.session_id.is_some());
    }

    #[test]
    fn test_query_loop_combined() {
        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let query_loop = QueryLoop::new(&config)
            .with_compression(CompressorConfig::default())
            .with_model_routing()
            .with_persistence();
        assert!(query_loop.compressor.is_some());
        assert!(query_loop.enable_model_routing);
        assert!(query_loop.persist_to_session);
    }

    #[test]
    fn test_query_loop_with_plugins() {
        use std::sync::Arc;
        use h_plugins::{Plugin, HookResult};

        struct TestPlugin;
        impl Plugin for TestPlugin {
            fn name(&self) -> &str { "test-plugin" }
            fn description(&self) -> &str { "Test plugin" }
            fn hook_names(&self) -> Vec<&str> { vec!["pre_llm_call", "on_tool_result"] }
            fn invoke_hook(&self, _hook_name: &str, _ctx: &HookContext) -> HookResult {
                Ok(vec![serde_json::json!({"injected": true})])
            }
        }

        let mut registry = PluginRegistry::new();
        registry.register(Arc::new(TestPlugin)).unwrap();

        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let query_loop = QueryLoop::new(&config)
            .with_plugins(registry);
        assert!(query_loop.plugins.is_some());
        assert_eq!(query_loop.plugins.as_ref().unwrap().count(), 1);
    }

    #[test]
    fn test_query_loop_invoke_hook_with_plugins() {
        use std::sync::Arc;
        use h_plugins::{Plugin, HookResult};

        struct CounterPlugin;
        impl Plugin for CounterPlugin {
            fn name(&self) -> &str { "counter" }
            fn description(&self) -> &str { "Counts hook invocations" }
            fn hook_names(&self) -> Vec<&str> { vec!["on_session_start"] }
            fn invoke_hook(&self, _hook_name: &str, _ctx: &HookContext) -> HookResult {
                Ok(vec![serde_json::json!({"hook_called": true})])
            }
        }

        let mut registry = PluginRegistry::new();
        registry.register(Arc::new(CounterPlugin)).unwrap();

        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let query_loop = QueryLoop::new(&config)
            .with_plugins(registry);

        let ctx = HookContext::new("on_session_start".to_string(), Some("test".to_string()));
        let results = query_loop.invoke_hook(HookType::OnSessionStart, &ctx);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["hook_called"], true);

        // No plugin registered for this hook type
        let ctx2 = HookContext::new("pre_llm_call".to_string(), Some("test".to_string()));
        let results2 = query_loop.invoke_hook(HookType::PreLlmCall, &ctx2);
        assert!(results2.is_empty());
    }

    #[test]
    fn test_query_loop_without_plugins_returns_empty() {
        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let query_loop = QueryLoop::new(&config);
        assert!(query_loop.plugins.is_none());

        // Invoking hooks without plugins should return empty
        let ctx = HookContext::new("pre_llm_call".to_string(), None);
        let results = query_loop.invoke_hook(HookType::PreLlmCall, &ctx);
        assert!(results.is_empty());
    }

    #[test]
    fn test_query_loop_with_stream_consumer() {
        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let consumer = Arc::new(h_core::NoOpConsumer);
        let query_loop = QueryLoop::new(&config)
            .with_consumer(consumer);
        assert!(query_loop.stream_consumer.is_some());
    }

    #[test]
    fn test_query_loop_combined_with_consumer() {
        let model = h_core::ModelRef::new(
            h_core::ProviderId::new("anthropic"),
            h_core::ModelId::new("claude-sonnet-4-6"),
        );
        let config = QueryConfig::new(model);
        let consumer = Arc::new(h_core::NoOpConsumer);
        let query_loop = QueryLoop::new(&config)
            .with_compression(CompressorConfig::default())
            .with_model_routing()
            .with_persistence()
            .with_consumer(consumer);
        assert!(query_loop.compressor.is_some());
        assert!(query_loop.enable_model_routing);
        assert!(query_loop.persist_to_session);
        assert!(query_loop.stream_consumer.is_some());
    }
}
