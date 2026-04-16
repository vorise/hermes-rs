use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use h_api::ApiClient;
use h_core::{CostTracker, Message, ToolCall, ToolCallFunction};
use h_tools::{Tool, ToolContext, ToolResult as HToolResult};
use parking_lot::Mutex;
use tokio::sync::Notify;
use tokio::time::{timeout, Duration};
use tracing::{debug, info, warn};

use crate::config::{QueryConfig, QueryResult, StopReason};

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

/// Run the main query loop.
pub async fn run_query_loop(
    config: &QueryConfig,
    api_client: &ApiClient,
    tool_registry: &ToolRegistry,
    interrupt_notify: Arc<Notify>,
) -> Result<QueryResult> {
    let budget = config.budget();
    let mut state = QueryState::new(config.messages.clone());

    info!(
        model = %config.model,
        max_iterations = config.max_iterations,
        tools = config.tools.len(),
        "Starting query loop"
    );

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

        // Prepare messages with system prompt
        let mut messages = vec![Message::system(&config.system_prompt)];
        messages.extend(state.messages.clone());

        debug!(
            message_count = messages.len(),
            "Calling LLM API"
        );

        // Call LLM API
        let response = match api_client.chat(&messages, &config.tools).await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("LLM API error: {e}");
                return Ok(build_result(state, StopReason::Error(e.to_string())));
            }
        };

        // Track cost
        let iteration_cost = response.to_cost_tracker();
        state.cost.add(&iteration_cost);

        // Extract response
        let text_content = response.text_content();
        let tool_calls = response.tool_calls();

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
        state.messages.push(assistant_msg);

        // If no tool calls, we're done
        if tool_calls.is_empty() {
            debug!("LLM response complete, no tool calls");
            return Ok(build_result(state, StopReason::Completed));
        }

        // Execute tools
        debug!(tool_calls = tool_calls.len(), "Executing tools");
        let tool_results = if should_parallelize(&tool_calls) {
            execute_tools_parallel(&tool_calls, tool_registry).await
        } else {
            execute_tools_sequential(&tool_calls, tool_registry).await
        };

        // Add tool results to state
        for (tc, result) in tool_calls.iter().zip(tool_results.iter()) {
            state.tool_calls_made += 1;
            let result_msg = Message::tool_result(
                tc.id.clone(),
                if result.is_error {
                    format!("Error: {}", result.content)
                } else {
                    result.content.clone()
                },
            );
            state.messages.push(result_msg);
        }
    }
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
}
