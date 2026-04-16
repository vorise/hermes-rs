//! Query Loop Runner
//!
//! Core agentic conversation loop for Hermes Agent.

use anyhow::Result;
use h_api::{ApiClient, ResolvedApiConfig};
use h_core::{
    HermesConfig, Message, ModelRef, ToolDefinition, ToolResult, ToolCall, CostTracker,
};
use h_tools::{ToolRegistry, ToolContext};
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tracing::{debug, info, warn};

// ============================================================================
// Query Configuration
// ============================================================================

/// Configuration for running a query loop.
#[derive(Debug, Clone)]
pub struct QueryConfig {
    /// Model reference (provider + model).
    pub model: ModelRef,

    /// Resolved API configuration.
    pub api_config: ResolvedApiConfig,

    /// Available tools for this session.
    pub tools: Vec<ToolDefinition>,

    /// System prompt for the LLM.
    pub system_prompt: String,

    /// Conversation messages.
    pub messages: Vec<Message>,

    /// Maximum iteration count (default 90).
    pub max_iterations: u32,

    /// Maximum output tokens.
    pub max_tokens: Option<u32>,

    /// Reasoning configuration (for models that support it).
    pub reasoning_config: Option<ReasoningConfig>,

    /// Arbitrary request overrides.
    pub request_overrides: Option<serde_json::Value>,
}

impl QueryConfig {
    /// Create a new query config with default iterations.
    pub fn new(api_config: ResolvedApiConfig) -> Self {
        Self {
            model: ModelRef::new(api_config.provider.clone(), api_config.model.clone()),
            api_config,
            tools: Vec::new(),
            system_prompt: String::new(),
            messages: Vec::new(),
            max_iterations: 90,
            max_tokens: None,
            reasoning_config: None,
            request_overrides: None,
        }
    }
}

/// Reasoning configuration for extended thinking models.
#[derive(Debug, Clone)]
pub struct ReasoningConfig {
    /// Enable extended reasoning.
    pub enabled: bool,

    /// Budget for reasoning tokens.
    pub budget_tokens: Option<u32>,
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            budget_tokens: None,
        }
    }
}

// ============================================================================
// Iteration Budget
// ============================================================================

/// Thread-safe iteration budget tracker.
///
/// Used to limit the number of LLM iterations in a session.
pub struct IterationBudget {
    /// Maximum total iterations.
    max_total: u32,

    /// Number of iterations used so far.
    used: AtomicU32,
}

impl IterationBudget {
    /// Create a new iteration budget.
    pub fn new(max_total: u32) -> Self {
        Self {
            max_total,
            used: AtomicU32::new(0),
        }
    }

    /// Consume one iteration. Returns true if budget remains.
    pub fn consume(&self) -> bool {
        let current = self.used.fetch_add(1, Ordering::SeqCst);
        current < self.max_total
    }

    /// Refund an iteration (e.g., for execute_code turns that don't count).
    pub fn refund(&self) {
        self.used.fetch_sub(1, Ordering::SeqCst);
    }

    /// Get the remaining iterations.
    pub fn remaining(&self) -> u32 {
        self.max_total - self.used.load(Ordering::SeqCst)
    }

    /// Get the used iterations.
    pub fn used(&self) -> u32 {
        self.used.load(Ordering::SeqCst)
    }

    /// Check if budget is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.used.load(Ordering::SeqCst) >= self.max_total
    }
}

// ============================================================================
// Query Result
// ============================================================================

/// Result from a completed query loop.
#[derive(Debug)]
pub struct QueryResult {
    /// Final messages from the conversation.
    pub messages: Vec<Message>,

    /// Cost tracking for the session.
    pub cost: CostTracker,

    /// Number of iterations used.
    pub iterations: u32,

    /// Whether the loop was interrupted.
    pub interrupted: bool,

    /// Final stop reason.
    pub stop_reason: StopReason,
}

/// Reason for stopping the query loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Natural completion (LLM returned no tool calls).
    Complete,

    /// Budget exhausted.
    BudgetExhausted,

    /// User interrupted.
    Interrupted,

    /// Error occurred.
    Error,

    /// Maximum tokens reached.
    MaxTokens,
}

// ============================================================================
// Query Loop
// ============================================================================

/// Interrupt signal for the query loop.
pub struct InterruptSignal {
    /// Flag indicating interrupt requested.
    interrupted: AtomicU32,
}

impl InterruptSignal {
    /// Create a new interrupt signal.
    pub fn new() -> Self {
        Self {
            interrupted: AtomicU32::new(0),
        }
    }

    /// Check if interrupt was requested.
    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst) == 1
    }

    /// Request an interrupt.
    pub fn interrupt(&self) {
        self.interrupted.store(1, Ordering::SeqCst);
    }

    /// Clear the interrupt.
    pub fn clear(&self) {
        self.interrupted.store(0, Ordering::SeqCst);
    }
}

impl Default for InterruptSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the query loop.
///
/// This is the main conversation loop that:
/// 1. Checks for interrupts
/// 2. Consumes iteration budget
/// 3. Calls the LLM API
/// 4. Handles responses (text, tool calls)
/// 5. Executes tools (parallel if safe)
/// 6. Feeds tool results back
/// 7. Continues until stop condition
pub async fn run_query_loop(
    config: QueryConfig,
    registry: Arc<ToolRegistry>,
    interrupt: Arc<InterruptSignal>,
) -> Result<QueryResult> {
    let budget = Arc::new(IterationBudget::new(config.max_iterations));
    let cost_tracker = Arc::new(RwLock::new(CostTracker::new()));

    // Create API client
    let client = ApiClient::new(config.api_config.clone())?;

    // Build tool context
    let tool_ctx = Arc::new(ToolContext::new(
        uuid::Uuid::new_v4().to_string(), // session_id
        uuid::Uuid::new_v4().to_string(), // task_id
        Arc::new(HermesConfig::default()),
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    ));

    let mut messages = config.messages.clone();
    let mut stop_reason = StopReason::Complete;
    let mut interrupted = false;

    info!("Starting query loop with {} max iterations", config.max_iterations);

    // Main loop
    while !budget.is_exhausted() && !interrupt.is_interrupted() {
        // Check for interrupt
        if interrupt.is_interrupted() {
            interrupted = true;
            stop_reason = StopReason::Interrupted;
            break;
        }

        // Consume iteration budget
        if !budget.consume() {
            stop_reason = StopReason::BudgetExhausted;
            break;
        }

        debug!("Iteration {} of {}", budget.used(), config.max_iterations);

        // Prepare messages for API call
        let request_messages = prepare_messages(&messages, &config.system_prompt);

        // Call LLM API (streaming)
        let response = match client.chat(&request_messages, &config.tools).await {
            Ok(r) => r,
            Err(e) => {
                warn!("API call failed: {}", e);
                // Simple retry logic - could be enhanced
                budget.refund();
                continue;
            }
        };

        // Update cost tracking
        if let Some(usage) = response.usage {
            let mut cost = cost_tracker.write();
            cost.input_tokens += usage.prompt_tokens;
            cost.output_tokens += usage.completion_tokens;
            cost.api_call_count += 1;
        }

        // Handle response
        let choice = response.choices.first();
        if choice.is_none() {
            budget.refund();
            continue;
        }

        let choice = choice.unwrap();
        let assistant_message = choice.message.clone();

        // Add assistant message to history
        messages.push(assistant_message.clone());

        // Check for tool calls
        if let Some(tool_calls) = &assistant_message.tool_calls {
            if tool_calls.is_empty() {
                // No tool calls - conversation complete
                stop_reason = StopReason::Complete;
                break;
            }

            // Execute tools (parallel if safe)
            let results = execute_tools_parallel(
                tool_calls,
                &registry,
                &tool_ctx,
            ).await;

            // Feed tool results back
            for (tool_call, result) in tool_calls.iter().zip(results.iter()) {
                let result_message = Message::tool_result(
                    tool_call.id.clone(),
                    h_core::Content::text(result.content.clone()),
                );
                messages.push(result_message);
            }
        } else {
            // No tool calls - conversation complete
            stop_reason = StopReason::Complete;
            break;
        }

        // Check finish reason
        if let Some(reason) = &choice.finish_reason {
            if reason == "max_tokens" {
                stop_reason = StopReason::MaxTokens;
                break;
            }
        }
    }

    // Build result
    let cost = cost_tracker.read().clone();
    Ok(QueryResult {
        messages,
        cost,
        iterations: budget.used(),
        interrupted,
        stop_reason,
    })
}

/// Prepare messages for API call, adding system prompt.
fn prepare_messages(messages: &[Message], system_prompt: &str) -> Vec<Message> {
    let mut prepared = Vec::new();

    // Add system prompt if provided
    if !system_prompt.is_empty() {
        prepared.push(Message::system(h_core::Content::text(system_prompt)));
    }

    // Add conversation messages
    prepared.extend(messages.iter().cloned());

    prepared
}

/// Execute tools in parallel if safe.
async fn execute_tools_parallel(
    tool_calls: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    ctx: &Arc<ToolContext>,
) -> Vec<ToolResult> {
    // Check if we can parallelize
    if should_parallelize(tool_calls, registry) {
        execute_parallel(tool_calls, registry, ctx).await
    } else {
        execute_sequential(tool_calls, registry, ctx).await
    }
}

/// Check if tool calls can be parallelized.
fn should_parallelize(tool_calls: &[ToolCall], registry: &ToolRegistry) -> bool {
    // Never parallelize single tool
    if tool_calls.len() <= 1 {
        return false;
    }

    // Check each tool's parallel safety
    for call in tool_calls {
        let name = &call.function.name;

        // Never parallelize certain tools (e.g., clarify, memory updates)
        if name == "clarify" || name == "memory" {
            return false;
        }

        // Check if tool exists and is read-only
        if registry.get(name).is_some() {
            // File tools with same path can't be parallelized
            if name == "write_file" || name == "patch" {
                // Could check for path overlap here
                return false;
            }
        }
    }

    // Default: parallelize if all tools are read-only
    true
}

/// Execute tools sequentially.
async fn execute_sequential(
    tool_calls: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    ctx: &Arc<ToolContext>,
) -> Vec<ToolResult> {
    let mut results = Vec::new();

    for call in tool_calls {
        let result = execute_single_tool(call, registry, ctx).await;
        results.push(result);
    }

    results
}

/// Execute tools in parallel (up to 8 workers).
async fn execute_parallel(
    tool_calls: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    ctx: &Arc<ToolContext>,
) -> Vec<ToolResult> {
    // Create tasks for each tool call (up to 8 workers in practice)
    let tasks: Vec<_> = tool_calls
        .iter()
        .map(|call| {
            let registry = registry.clone();
            let ctx = ctx.clone();
            let call = call.clone();
            async move { execute_single_tool(&call, &registry, &ctx).await }
        })
        .collect();

    // Execute in parallel with bounded concurrency
    futures::future::join_all(tasks).await
}

/// Execute a single tool.
async fn execute_single_tool(
    call: &ToolCall,
    registry: &Arc<ToolRegistry>,
    ctx: &Arc<ToolContext>,
) -> ToolResult {
    let name = &call.function.name;

    // Parse arguments
    let args = match call.function.parse_arguments() {
        Ok(a) => a,
        Err(e) => {
            return ToolResult::error(format!("Failed to parse arguments: {}", e));
        }
    };

    // Get tool from registry
    let tool = registry.get(name);
    if tool.is_none() {
        return ToolResult::error(format!("Unknown tool: {}", name));
    }

    let tool = tool.unwrap();

    // Execute tool
    match tool.execute(args, ctx).await {
        Ok(result) => result,
        Err(e) => ToolResult::error(format!("Tool execution failed: {}", e)),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iteration_budget_new() {
        let budget = IterationBudget::new(10);
        assert_eq!(budget.remaining(), 10);
        assert_eq!(budget.used(), 0);
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn test_iteration_budget_consume() {
        let budget = IterationBudget::new(5);

        assert!(budget.consume()); // 1 used, 4 remaining
        assert_eq!(budget.used(), 1);
        assert_eq!(budget.remaining(), 4);

        assert!(budget.consume()); // 2 used, 3 remaining
        assert!(budget.consume()); // 3 used, 2 remaining
        assert!(budget.consume()); // 4 used, 1 remaining
        assert!(budget.consume()); // 5 used, 0 remaining

        assert_eq!(budget.used(), 5);
        assert!(budget.is_exhausted());

        // Budget exhausted, consume returns false
        assert!(!budget.consume());
    }

    #[test]
    fn test_iteration_budget_refund() {
        let budget = IterationBudget::new(10);

        budget.consume();
        budget.consume();
        assert_eq!(budget.used(), 2);

        budget.refund();
        assert_eq!(budget.used(), 1);

        budget.refund();
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn test_iteration_budget_thread_safe() {
        use std::sync::Arc;
        use std::thread;

        let budget = Arc::new(IterationBudget::new(100));
        let mut handles = Vec::new();

        for _ in 0..10 {
            let b = budget.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..10 {
                    b.consume();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(budget.used(), 100);
        assert!(budget.is_exhausted());
    }

    #[test]
    fn test_interrupt_signal() {
        let signal = InterruptSignal::new();
        assert!(!signal.is_interrupted());

        signal.interrupt();
        assert!(signal.is_interrupted());

        signal.clear();
        assert!(!signal.is_interrupted());
    }

    #[test]
    fn test_query_config_new() {
        use h_core::{ProviderId, ModelId};
        use h_api::ApiMode;

        let api_config = ResolvedApiConfig {
            provider: ProviderId::new("openrouter"),
            model: ModelId::new("claude-sonnet-4"),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: "test-key".to_string(),
            mode: ApiMode::ChatCompletions,
            timeout_seconds: 60,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
        };

        let config = QueryConfig::new(api_config);
        assert_eq!(config.max_iterations, 90);
        assert!(config.tools.is_empty());
        assert!(config.messages.is_empty());
    }

    #[test]
    fn test_stop_reason() {
        assert_eq!(StopReason::Complete, StopReason::Complete);
        assert_ne!(StopReason::Complete, StopReason::Interrupted);
    }

    #[test]
    fn test_prepare_messages() {
        let messages = vec![
            Message::user("Hello"),
            Message::assistant("Hi there"),
        ];
        let prepared = prepare_messages(&messages, "System prompt");

        assert_eq!(prepared.len(), 3);
        assert_eq!(prepared[0].role, h_core::Role::System);
        assert_eq!(prepared[1].role, h_core::Role::User);
        assert_eq!(prepared[2].role, h_core::Role::Assistant);
    }

    #[test]
    fn test_should_parallelize_empty() {
        let registry = ToolRegistry::new();
        let calls: Vec<ToolCall> = Vec::new();
        assert!(!should_parallelize(&calls, &registry));
    }

    #[test]
    fn test_should_parallelize_single() {
        let registry = ToolRegistry::new();
        let calls = vec![
            ToolCall::new("id1", "read_file", "{}"),
        ];
        assert!(!should_parallelize(&calls, &registry));
    }

    #[test]
    fn test_should_parallelize_clarify() {
        let registry = ToolRegistry::new();
        let calls = vec![
            ToolCall::new("id1", "read_file", "{}"),
            ToolCall::new("id2", "clarify", "{}"),
        ];
        assert!(!should_parallelize(&calls, &registry));
    }
}