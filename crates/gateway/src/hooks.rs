use std::collections::HashMap;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// A gateway hook result containing optional values to inject or modify.
pub type HookResult = Vec<serde_json::Value>;

/// Pre-defined hook types used by the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GatewayHookType {
    /// Fires before a message is processed by the agent.
    /// Can modify, reject, or enrich the incoming message.
    PreMessage,
    /// Fires after the agent produces a response, before it is sent to the platform.
    /// Can modify or suppress the response.
    PostMessage,
    /// Fires before a tool is executed.
    /// Can intercept, modify arguments, or block the tool call.
    PreToolCall,
    /// Fires after a tool completes execution.
    /// Can modify the result or log/track the outcome.
    PostToolResult,
    /// Fires when a new session is created.
    /// Can initialize session-scoped state or reject the session.
    SessionStart,
    /// Fires when a session is cleared (e.g., /new command).
    SessionEnd,
    /// Fires when a platform connects or disconnects.
    PlatformChange,
    /// Fires on a gateway error.
    OnError,
}

impl GatewayHookType {
    /// Get the string identifier for this hook type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreMessage => "pre_message",
            Self::PostMessage => "post_message",
            Self::PreToolCall => "pre_tool_call",
            Self::PostToolResult => "post_tool_result",
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::PlatformChange => "platform_change",
            Self::OnError => "on_error",
        }
    }
}

/// Context passed to hook invocations.
///
/// Hooks can read from and mutate the values in this context
/// to influence gateway behavior.
#[derive(Debug, Clone)]
pub struct HookContext {
    /// Key-value pairs that hooks can read and modify.
    pub values: HashMap<String, serde_json::Value>,
}

impl HookContext {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Get a value from the context.
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.values.get(key)
    }

    /// Set a value in the context.
    pub fn set(&mut self, key: &str, value: serde_json::Value) {
        self.values.insert(key.to_string(), value);
    }

    /// Set multiple values at once.
    pub fn set_many(&mut self, pairs: Vec<(&str, serde_json::Value)>) {
        for (k, v) in pairs {
            self.values.insert(k.to_string(), v);
        }
    }
}

impl Default for HookContext {
    fn default() -> Self {
        Self::new()
    }
}

/// A gateway hook function that processes a context and returns results.
pub type HookFn = Box<dyn Fn(&HookContext) -> HookResult + Send + Sync>;

/// Registered gateway hook with metadata.
pub struct RegisteredHook {
    /// Human-readable name of the hook.
    pub name: String,
    /// Description of what the hook does.
    pub description: String,
    /// The hook function.
    pub handler: HookFn,
}

/// Gateway hook registry.
///
/// Allows registering custom hooks that fire at key points in the
/// gateway lifecycle: before/after messages, tool calls, session
/// creation, platform changes, and errors.
pub struct HookRegistry {
    hooks: Mutex<HashMap<String, Vec<RegisteredHook>>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            hooks: Mutex::new(HashMap::new()),
        }
    }

    /// Register a hook for the given hook type.
    pub fn register<F>(&self, hook_type: GatewayHookType, name: &str, description: &str, handler: F)
    where
        F: Fn(&HookContext) -> HookResult + Send + Sync + 'static,
    {
        let key = hook_type.as_str().to_string();
        self.hooks.lock().entry(key).or_default().push(RegisteredHook {
            name: name.to_string(),
            description: description.to_string(),
            handler: Box::new(handler),
        });
    }

    /// Register a hook using a string hook type (for dynamic/plugin hooks).
    pub fn register_by_name<F>(&self, hook_type: &str, name: &str, description: &str, handler: F)
    where
        F: Fn(&HookContext) -> HookResult + Send + Sync + 'static,
    {
        self.hooks.lock().entry(hook_type.to_string()).or_default().push(RegisteredHook {
            name: name.to_string(),
            description: description.to_string(),
            handler: Box::new(handler),
        });
    }

    /// Invoke all hooks of the given type and return their combined results.
    pub fn invoke(&self, hook_type: GatewayHookType, ctx: &HookContext) -> Vec<serde_json::Value> {
        let key = hook_type.as_str();
        let hooks = self.hooks.lock();
        hooks
            .get(key)
            .map(|registered| {
                registered
                    .iter()
                    .flat_map(|h| (h.handler)(ctx))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Invoke hooks by string hook type name.
    pub fn invoke_by_name(&self, hook_type: &str, ctx: &HookContext) -> Vec<serde_json::Value> {
        let hooks = self.hooks.lock();
        hooks
            .get(hook_type)
            .map(|registered| {
                registered
                    .iter()
                    .flat_map(|h| (h.handler)(ctx))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Remove all hooks of the given type.
    pub fn clear(&self, hook_type: GatewayHookType) {
        self.hooks.lock().remove(hook_type.as_str());
    }

    /// Remove a specific hook by type and name.
    pub fn remove(&self, hook_type: GatewayHookType, name: &str) -> bool {
        let key = hook_type.as_str();
        let mut hooks = self.hooks.lock();
        if let Some(list) = hooks.get_mut(key) {
            let before = list.len();
            list.retain(|h| h.name != name);
            list.len() < before
        } else {
            false
        }
    }

    /// Get the count of registered hooks for a hook type.
    pub fn count(&self, hook_type: GatewayHookType) -> usize {
        self.hooks
            .lock()
            .get(hook_type.as_str())
            .map(|h| h.len())
            .unwrap_or(0)
    }

    /// Get the total number of registered hooks across all types.
    pub fn total_count(&self) -> usize {
        self.hooks.lock().values().map(|h| h.len()).sum()
    }

    /// List all registered hook names for a hook type.
    pub fn list_hooks(&self, hook_type: GatewayHookType) -> Vec<String> {
        self.hooks
            .lock()
            .get(hook_type.as_str())
            .map(|h| h.iter().map(|r| r.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Check if any hooks are registered for a hook type.
    pub fn has_hooks(&self, hook_type: GatewayHookType) -> bool {
        self.hooks
            .lock()
            .get(hook_type.as_str())
            .map(|h| !h.is_empty())
            .unwrap_or(false)
    }

    /// Get all registered hook type names.
    pub fn hook_types(&self) -> Vec<String> {
        self.hooks.lock().keys().cloned().collect()
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to build a HookContext from key-value pairs.
pub fn ctx(pairs: Vec<(&str, serde_json::Value)>) -> HookContext {
    let mut ctx = HookContext::new();
    ctx.set_many(pairs);
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_type_strings() {
        assert_eq!(GatewayHookType::PreMessage.as_str(), "pre_message");
        assert_eq!(GatewayHookType::PostMessage.as_str(), "post_message");
        assert_eq!(GatewayHookType::PreToolCall.as_str(), "pre_tool_call");
        assert_eq!(GatewayHookType::PostToolResult.as_str(), "post_tool_result");
        assert_eq!(GatewayHookType::SessionStart.as_str(), "session_start");
        assert_eq!(GatewayHookType::SessionEnd.as_str(), "session_end");
        assert_eq!(GatewayHookType::PlatformChange.as_str(), "platform_change");
        assert_eq!(GatewayHookType::OnError.as_str(), "on_error");
    }

    #[test]
    fn test_hook_context_get_set() {
        let mut ctx = HookContext::new();
        ctx.set("platform", serde_json::json!("telegram"));
        ctx.set("user_id", serde_json::json!("user123"));
        assert_eq!(ctx.get("platform").unwrap().as_str().unwrap(), "telegram");
        assert_eq!(ctx.get("user_id").unwrap().as_str().unwrap(), "user123");
        assert!(ctx.get("missing").is_none());
    }

    #[test]
    fn test_hook_context_set_many() {
        let mut ctx = HookContext::new();
        ctx.set_many(vec![
            ("a", serde_json::json!(1)),
            ("b", serde_json::json!("two")),
        ]);
        assert_eq!(ctx.get("a").unwrap().as_i64().unwrap(), 1);
        assert_eq!(ctx.get("b").unwrap().as_str().unwrap(), "two");
    }

    #[test]
    fn test_register_and_invoke() {
        let registry = HookRegistry::new();
        registry.register(
            GatewayHookType::PreMessage,
            "test_hook",
            "A test hook",
            |_ctx| vec![serde_json::json!({"injected": true})],
        );

        assert!(registry.has_hooks(GatewayHookType::PreMessage));
        assert_eq!(registry.count(GatewayHookType::PreMessage), 1);

        let ctx = HookContext::new();
        let results = registry.invoke(GatewayHookType::PreMessage, &ctx);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["injected"], true);
    }

    #[test]
    fn test_invoke_no_hooks_returns_empty() {
        let registry = HookRegistry::new();
        let ctx = HookContext::new();
        let results = registry.invoke(GatewayHookType::PreMessage, &ctx);
        assert!(results.is_empty());
    }

    #[test]
    fn test_multiple_hooks_same_type() {
        let registry = HookRegistry::new();
        registry.register(
            GatewayHookType::PreMessage,
            "hook_a",
            "Hook A",
            |_| vec![serde_json::json!("a")],
        );
        registry.register(
            GatewayHookType::PreMessage,
            "hook_b",
            "Hook B",
            |_| vec![serde_json::json!("b")],
        );

        let ctx = HookContext::new();
        let results = registry.invoke(GatewayHookType::PreMessage, &ctx);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_hook_can_read_context() {
        let registry = HookRegistry::new();
        registry.register(
            GatewayHookType::PreMessage,
            "echo_platform",
            "Echoes the platform from context",
            |ctx| {
                let platform = ctx.get("platform").cloned().unwrap_or(serde_json::json!("unknown"));
                vec![serde_json::json!({"platform": platform})]
            },
        );

        let mut ctx = HookContext::new();
        ctx.set("platform", serde_json::json!("discord"));
        let results = registry.invoke(GatewayHookType::PreMessage, &ctx);
        assert_eq!(results[0]["platform"], "discord");
    }

    #[test]
    fn test_remove_hook() {
        let registry = HookRegistry::new();
        registry.register(GatewayHookType::PreMessage, "hook1", "H1", |_| vec![]);
        registry.register(GatewayHookType::PreMessage, "hook2", "H2", |_| vec![]);
        assert_eq!(registry.count(GatewayHookType::PreMessage), 2);

        assert!(registry.remove(GatewayHookType::PreMessage, "hook1"));
        assert_eq!(registry.count(GatewayHookType::PreMessage), 1);

        assert!(!registry.remove(GatewayHookType::PreMessage, "nonexistent"));
    }

    #[test]
    fn test_clear_hooks() {
        let registry = HookRegistry::new();
        registry.register(GatewayHookType::PreMessage, "h1", "H1", |_| vec![]);
        registry.register(GatewayHookType::PostMessage, "h2", "H2", |_| vec![]);

        registry.clear(GatewayHookType::PreMessage);
        assert_eq!(registry.count(GatewayHookType::PreMessage), 0);
        assert_eq!(registry.count(GatewayHookType::PostMessage), 1);
    }

    #[test]
    fn test_list_hooks() {
        let registry = HookRegistry::new();
        registry.register(GatewayHookType::PreMessage, "alpha", "A", |_| vec![]);
        registry.register(GatewayHookType::PreMessage, "beta", "B", |_| vec![]);

        let names = registry.list_hooks(GatewayHookType::PreMessage);
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
    }

    #[test]
    fn test_total_count() {
        let registry = HookRegistry::new();
        registry.register(GatewayHookType::PreMessage, "h1", "H1", |_| vec![]);
        registry.register(GatewayHookType::PostMessage, "h2", "H2", |_| vec![]);
        registry.register(GatewayHookType::SessionStart, "h3", "H3", |_| vec![]);
        assert_eq!(registry.total_count(), 3);
    }

    #[test]
    fn test_hook_types() {
        let registry = HookRegistry::new();
        registry.register(GatewayHookType::PreMessage, "h1", "H1", |_| vec![]);
        registry.register(GatewayHookType::PostMessage, "h2", "H2", |_| vec![]);

        let types = registry.hook_types();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&"pre_message".to_string()));
        assert!(types.contains(&"post_message".to_string()));
    }

    #[test]
    fn test_register_by_name() {
        let registry = HookRegistry::new();
        registry.register_by_name("custom_hook", "custom", "Custom hook", |_| {
            vec![serde_json::json!({"custom": true})]
        });

        let ctx = HookContext::new();
        let results = registry.invoke_by_name("custom_hook", &ctx);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["custom"], true);
    }

    #[test]
    fn test_ctx_helper() {
        let ctx = ctx(vec![
            ("platform", serde_json::json!("telegram")),
            ("turn", serde_json::json!(5)),
        ]);
        assert_eq!(ctx.get("platform").unwrap().as_str().unwrap(), "telegram");
        assert_eq!(ctx.get("turn").unwrap().as_i64().unwrap(), 5);
    }
}
