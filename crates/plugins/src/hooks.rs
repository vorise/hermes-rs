//! Hook System
//!
//! Plugin hooks for extending Hermes behavior.

use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Hook types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookType {
    /// New session created.
    OnSessionStart,

    /// Before LLM API call.
    PreLlmCall,

    /// After LLM response.
    PostLlmResponse,

    /// Before tool execution.
    OnToolCall,

    /// After tool execution.
    OnToolResult,

    /// On message received.
    OnMessage,

    /// Before message sent.
    PreSendMessage,

    /// After message sent.
    PostSendMessage,

    /// On error.
    OnError,

    /// On session end.
    OnSessionEnd,

    /// Custom hook.
    Custom,
}

impl HookType {
    /// Get hook name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::OnSessionStart => "on_session_start",
            Self::PreLlmCall => "pre_llm_call",
            Self::PostLlmResponse => "post_llm_response",
            Self::OnToolCall => "on_tool_call",
            Self::OnToolResult => "on_tool_result",
            Self::OnMessage => "on_message",
            Self::PreSendMessage => "pre_send_message",
            Self::PostSendMessage => "post_send_message",
            Self::OnError => "on_error",
            Self::OnSessionEnd => "on_session_end",
            Self::Custom => "custom",
        }
    }

    /// Parse from string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "on_session_start" => Some(Self::OnSessionStart),
            "pre_llm_call" => Some(Self::PreLlmCall),
            "post_llm_response" => Some(Self::PostLlmResponse),
            "on_tool_call" => Some(Self::OnToolCall),
            "on_tool_result" => Some(Self::OnToolResult),
            "on_message" => Some(Self::OnMessage),
            "pre_send_message" => Some(Self::PreSendMessage),
            "post_send_message" => Some(Self::PostSendMessage),
            "on_error" => Some(Self::OnError),
            "on_session_end" => Some(Self::OnSessionEnd),
            _ => None,
        }
    }
}

/// Hook arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookArgs {
    /// Session ID.
    pub session_id: Option<String>,

    /// Platform.
    pub platform: Option<String>,

    /// Chat ID.
    pub chat_id: Option<String>,

    /// Message content.
    pub message: Option<String>,

    /// Tool name.
    pub tool_name: Option<String>,

    /// Tool arguments.
    pub tool_args: Option<serde_json::Value>,

    /// Tool result.
    pub tool_result: Option<serde_json::Value>,

    /// Error message.
    pub error: Option<String>,

    /// Additional context.
    pub context: HashMap<String, serde_json::Value>,
}

impl HookArgs {
    /// Create empty args.
    pub fn new() -> Self {
        Self {
            session_id: None,
            platform: None,
            chat_id: None,
            message: None,
            tool_name: None,
            tool_args: None,
            tool_result: None,
            error: None,
            context: HashMap::new(),
        }
    }

    /// Create with session.
    pub fn with_session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Some(session_id.into()),
            ..Self::new()
        }
    }

    /// Create with message.
    pub fn with_message(session_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            session_id: Some(session_id.into()),
            message: Some(message.into()),
            ..Self::new()
        }
    }

    /// Create with tool.
    pub fn with_tool(session_id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        Self {
            session_id: Some(session_id.into()),
            tool_name: Some(tool_name.into()),
            ..Self::new()
        }
    }

    /// Create with error.
    pub fn with_error(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::new()
        }
    }

    /// Add context value.
    pub fn add_context(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.context.insert(key.into(), value);
    }
}

impl Default for HookArgs {
    fn default() -> Self {
        Self::new()
    }
}

/// Hook result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResult {
    /// Plugin that produced the result.
    pub plugin: String,

    /// Hook that was invoked.
    pub hook: String,

    /// Result data.
    pub data: serde_json::Value,

    /// Whether to continue processing.
    pub continue_processing: bool,

    /// Modified args (if any).
    pub modified_args: Option<HookArgs>,
}

impl HookResult {
    /// Create a result.
    pub fn new(plugin: impl Into<String>, hook: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            plugin: plugin.into(),
            hook: hook.into(),
            data,
            continue_processing: true,
            modified_args: None,
        }
    }

    /// Create a result that stops processing.
    pub fn stop(plugin: impl Into<String>, hook: impl Into<String>) -> Self {
        Self {
            plugin: plugin.into(),
            hook: hook.into(),
            data: serde_json::Value::Null,
            continue_processing: false,
            modified_args: None,
        }
    }

    /// Create a result that modifies args.
    pub fn modify(plugin: impl Into<String>, hook: impl Into<String>, args: HookArgs) -> Self {
        Self {
            plugin: plugin.into(),
            hook: hook.into(),
            data: serde_json::Value::Null,
            continue_processing: true,
            modified_args: Some(args),
        }
    }
}

/// Hook handler function.
pub type HookHandler = fn(&HookArgs) -> HookResult;

/// Hook registry.
pub struct HookRegistry {
    /// Registered handlers by hook type.
    handlers: HashMap<HookType, Vec<(String, HookHandler)>>,
}

impl HookRegistry {
    /// Create new registry.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a hook handler.
    pub fn register(&mut self, hook: HookType, plugin: &str, handler: HookHandler) {
        self.handlers
            .entry(hook)
            .or_insert_with(Vec::new)
            .push((plugin.to_string(), handler));
        info!("Registered hook {} for plugin {}", hook.name(), plugin);
    }

    /// Unregister a hook handler.
    pub fn unregister(&mut self, hook: HookType, plugin: &str) {
        if let Some(handlers) = self.handlers.get_mut(&hook) {
            handlers.retain(|(p, _)| p != plugin);
        }
    }

    /// Invoke a hook.
    pub fn invoke(&self, hook: HookType, args: &HookArgs) -> Vec<HookResult> {
        let handlers = self.handlers.get(&hook);
        if handlers.is_none() || handlers.unwrap().is_empty() {
            return Vec::new();
        }

        let results: Vec<HookResult> = handlers.unwrap()
            .iter()
            .map(|(plugin, handler)| {
                let result = handler(args);
                if !result.continue_processing {
                    warn!("Hook {} from plugin {} stopped processing", hook.name(), plugin);
                }
                result
            })
            .collect();

        info!("Hook {} invoked, {} results", hook.name(), results.len());
        results
    }

    /// Invoke a hook by name.
    pub fn invoke_by_name(&self, name: &str, args: &HookArgs) -> Vec<HookResult> {
        let hook = HookType::from_name(name);
        match hook {
            Some(h) => self.invoke(h, args),
            None => {
                warn!("Unknown hook: {}", name);
                Vec::new()
            }
        }
    }

    /// Check if a hook has handlers.
    pub fn has_handlers(&self, hook: HookType) -> bool {
        self.handlers.get(&hook).map(|h| !h.is_empty()).unwrap_or(false)
    }

    /// Count handlers for a hook.
    pub fn handler_count(&self, hook: HookType) -> usize {
        self.handlers.get(&hook).map(|h| h.len()).unwrap_or(0)
    }

    /// Clear all handlers.
    pub fn clear(&mut self) {
        self.handlers.clear();
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global hook registry (simple singleton pattern).
static HOOK_REGISTRY: std::sync::OnceLock<Arc<std::sync::RwLock<HookRegistry>>> = std::sync::OnceLock::new();

/// Get the global hook registry.
pub fn global_registry() -> Arc<std::sync::RwLock<HookRegistry>> {
    HOOK_REGISTRY.get_or_init(|| Arc::new(std::sync::RwLock::new(HookRegistry::new()))).clone()
}

/// Invoke a hook using the global registry.
pub fn invoke_hook(hook: HookType, args: &HookArgs) -> Vec<HookResult> {
    let registry = global_registry();
    let guard = registry.read().unwrap();
    guard.invoke(hook, args)
}

/// Invoke a hook by name.
pub fn invoke_hook_by_name(name: &str, args: &HookArgs) -> Vec<HookResult> {
    let registry = global_registry();
    let guard = registry.read().unwrap();
    guard.invoke_by_name(name, args)
}

/// Register a hook in the global registry.
pub fn register_hook(hook: HookType, plugin: &str, handler: HookHandler) {
    let registry = global_registry();
    let mut guard = registry.write().unwrap();
    guard.register(hook, plugin, handler);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_handler(args: &HookArgs) -> HookResult {
        HookResult::new("test", "on_session_start", serde_json::json!({"processed": true}))
    }

    fn stop_handler(_args: &HookArgs) -> HookResult {
        HookResult::stop("test", "on_tool_call")
    }

    #[test]
    fn test_hook_type_name() {
        assert_eq!(HookType::OnSessionStart.name(), "on_session_start");
        assert_eq!(HookType::PreLlmCall.name(), "pre_llm_call");
    }

    #[test]
    fn test_hook_type_from_name() {
        assert_eq!(HookType::from_name("on_session_start"), Some(HookType::OnSessionStart));
        assert_eq!(HookType::from_name("invalid"), None);
    }

    #[test]
    fn test_hook_args_new() {
        let args = HookArgs::new();
        assert!(args.session_id.is_none());
        assert!(args.message.is_none());
    }

    #[test]
    fn test_hook_args_with_session() {
        let args = HookArgs::with_session("s1");
        assert_eq!(args.session_id, Some("s1".to_string()));
    }

    #[test]
    fn test_hook_args_with_message() {
        let args = HookArgs::with_message("s1", "hello");
        assert_eq!(args.session_id, Some("s1".to_string()));
        assert_eq!(args.message, Some("hello".to_string()));
    }

    #[test]
    fn test_hook_result_new() {
        let result = HookResult::new("plugin", "hook", serde_json::json!(true));
        assert!(result.continue_processing);
    }

    #[test]
    fn test_hook_result_stop() {
        let result = HookResult::stop("plugin", "hook");
        assert!(!result.continue_processing);
    }

    #[test]
    fn test_hook_registry_new() {
        let registry = HookRegistry::new();
        assert!(!registry.has_handlers(HookType::OnSessionStart));
    }

    #[test]
    fn test_hook_registry_register() {
        let mut registry = HookRegistry::new();
        registry.register(HookType::OnSessionStart, "test", test_handler);
        assert!(registry.has_handlers(HookType::OnSessionStart));
        assert_eq!(registry.handler_count(HookType::OnSessionStart), 1);
    }

    #[test]
    fn test_hook_registry_invoke() {
        let mut registry = HookRegistry::new();
        registry.register(HookType::OnSessionStart, "test", test_handler);
        let args = HookArgs::new();
        let results = registry.invoke(HookType::OnSessionStart, &args);
        assert_eq!(results.len(), 1);
        assert!(results[0].continue_processing);
    }

    #[test]
    fn test_hook_registry_invoke_stop() {
        let mut registry = HookRegistry::new();
        registry.register(HookType::OnToolCall, "test", stop_handler);
        let args = HookArgs::new();
        let results = registry.invoke(HookType::OnToolCall, &args);
        assert_eq!(results.len(), 1);
        assert!(!results[0].continue_processing);
    }

    #[test]
    fn test_hook_registry_unregister() {
        let mut registry = HookRegistry::new();
        registry.register(HookType::OnSessionStart, "test", test_handler);
        registry.unregister(HookType::OnSessionStart, "test");
        assert!(!registry.has_handlers(HookType::OnSessionStart));
    }

    #[test]
    fn test_hook_registry_invoke_by_name() {
        let mut registry = HookRegistry::new();
        registry.register(HookType::OnSessionStart, "test", test_handler);
        let args = HookArgs::new();
        let results = registry.invoke_by_name("on_session_start", &args);
        assert_eq!(results.len(), 1);
    }
}