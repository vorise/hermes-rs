use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

/// Result of a hook invocation.
pub type HookResult = Result<Vec<Value>>;

/// Context passed to hook invocations.
#[derive(Debug, Clone)]
pub struct HookContext {
    /// The hook being invoked.
    pub hook_name: String,
    /// Current session ID, if any.
    pub session_id: Option<String>,
    /// Arbitrary data shared between hooks.
    pub data: Arc<std::sync::Mutex<std::collections::HashMap<String, Value>>>,
}

impl HookContext {
    pub fn new(hook_name: String, session_id: Option<String>) -> Self {
        Self {
            hook_name,
            session_id,
            data: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Get a value from the shared data store.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.lock().unwrap().get(key).cloned()
    }

    /// Set a value in the shared data store.
    pub fn set(&self, key: &str, value: Value) {
        self.data.lock().unwrap().insert(key.to_string(), value);
    }
}

/// All available hook types in the system.
#[derive(Debug, Clone, PartialEq)]
pub enum HookType {
    /// Fired when a new session is created.
    OnSessionStart,
    /// Fired before an LLM API call. Plugins can inject context.
    PreLlmCall,
    /// Fired after an LLM response. Plugins can process the response.
    PostLlmResponse,
    /// Fired before a tool call executes. Plugins can intercept/modify.
    OnToolCall,
    /// Fired after a tool call completes. Plugins can process results.
    OnToolResult,
}

impl HookType {
    /// All hook type names as strings.
    pub fn all_names() -> &'static [&'static str] {
        &[
            "on_session_start",
            "pre_llm_call",
            "post_llm_response",
            "on_tool_call",
            "on_tool_result",
        ]
    }

    /// Parse a hook name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "on_session_start" => Some(HookType::OnSessionStart),
            "pre_llm_call" => Some(HookType::PreLlmCall),
            "post_llm_response" => Some(HookType::PostLlmResponse),
            "on_tool_call" => Some(HookType::OnToolCall),
            "on_tool_result" => Some(HookType::OnToolResult),
            _ => None,
        }
    }

    /// Hook name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            HookType::OnSessionStart => "on_session_start",
            HookType::PreLlmCall => "pre_llm_call",
            HookType::PostLlmResponse => "post_llm_response",
            HookType::OnToolCall => "on_tool_call",
            HookType::OnToolResult => "on_tool_result",
        }
    }
}

/// Hook trait for plugins to implement.
/// A plugin can implement multiple hooks to react to lifecycle events.
pub trait Hook: Send + Sync {
    /// Name of the hook this implements.
    fn hook_type(&self) -> HookType;

    /// Execute the hook.
    fn execute(&self, ctx: &HookContext) -> HookResult;
}

/// Dispatcher that invokes hooks across registered plugins.
pub struct HookDispatcher {
    hooks: Vec<Box<dyn Hook>>,
}

impl HookDispatcher {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register a hook handler.
    pub fn register(&mut self, hook: Box<dyn Hook>) {
        self.hooks.push(hook);
    }

    /// Dispatch a hook event to all registered handlers of the matching type.
    pub fn dispatch(&self, hook_type: HookType, ctx: &HookContext) -> Vec<Value> {
        let hook_name = hook_type.as_str();
        let mut results = Vec::new();

        for hook in &self.hooks {
            if std::mem::discriminant(&hook.hook_type()) == std::mem::discriminant(&hook_type) {
                match hook.execute(ctx) {
                    Ok(values) => results.extend(values),
                    Err(e) => tracing::warn!("Hook {hook_name} failed: {e}"),
                }
            }
        }

        results
    }

    /// Count registered hook handlers.
    pub fn count(&self) -> usize {
        self.hooks.len()
    }
}

impl Default for HookDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHook;

    impl Hook for TestHook {
        fn hook_type(&self) -> HookType {
            HookType::PreLlmCall
        }

        fn execute(&self, _ctx: &HookContext) -> HookResult {
            Ok(vec![Value::String("injected context".to_string())])
        }
    }

    #[test]
    fn test_hook_type_names() {
        let names = HookType::all_names();
        assert_eq!(names.len(), 5);
        assert_eq!(names[0], "on_session_start");
        assert_eq!(names[1], "pre_llm_call");
    }

    #[test]
    fn test_hook_type_from_name() {
        assert!(HookType::from_name("pre_llm_call").is_some());
        assert!(HookType::from_name("unknown").is_none());
        assert_eq!(
            HookType::from_name("on_tool_call").unwrap(),
            HookType::OnToolCall
        );
    }

    #[test]
    fn test_hook_type_as_str() {
        assert_eq!(HookType::OnSessionStart.as_str(), "on_session_start");
        assert_eq!(HookType::PostLlmResponse.as_str(), "post_llm_response");
    }

    #[test]
    fn test_hook_context() {
        let ctx = HookContext::new("test".to_string(), Some("s1".to_string()));
        assert_eq!(ctx.session_id, Some("s1".to_string()));
        assert_eq!(ctx.hook_name, "test");

        ctx.set("key", Value::Number(42.into()));
        assert_eq!(ctx.get("key"), Some(Value::Number(42.into())));
    }

    #[test]
    fn test_hook_dispatcher() {
        let mut dispatcher = HookDispatcher::new();
        assert_eq!(dispatcher.count(), 0);

        dispatcher.register(Box::new(TestHook));
        assert_eq!(dispatcher.count(), 1);

        let ctx = HookContext::new("pre_llm_call".to_string(), None);
        let results = dispatcher.dispatch(HookType::PreLlmCall, &ctx);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], Value::String("injected context".to_string()));

        // Different hook type should return empty
        let results2 = dispatcher.dispatch(HookType::OnToolCall, &ctx);
        assert!(results2.is_empty());
    }
}
