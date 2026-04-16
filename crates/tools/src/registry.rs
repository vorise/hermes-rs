//! Tool Registry with inventory-based auto-discovery.
//!
//! This module provides the central registry for all Hermes tools,
//! with compile-time registration via the `inventory` crate.

use anyhow::Result;
use async_trait::async_trait;
use h_core::{HermesConfig, ToolDefinition, ToolResult};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// ============================================================================
// Tool Trait
// ============================================================================

/// Core trait for all Hermes tools.
///
/// Tools must implement this trait and register themselves via
/// `inventory::submit!` for automatic discovery.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g., "read_file", "terminal").
    fn name(&self) -> &str;

    /// Toolset this tool belongs to (e.g., "file", "terminal").
    fn toolset(&self) -> &str;

    /// Human-readable description for the LLM.
    fn description(&self) -> &str;

    /// JSON Schema for tool parameters.
    fn schema(&self) -> Value;

    /// Optional check function to determine if tool is available.
    /// Returns false if dependencies are missing.
    fn check_fn(&self) -> Option<fn() -> bool> {
        None
    }

    /// Environment variables required for this tool to work.
    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    /// Maximum result size in characters. Results larger than this
    /// will be persisted to a file and the path returned instead.
    fn max_result_size_chars(&self) -> Option<usize> {
        None
    }

    /// Execute the tool with given arguments and context.
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult>;
}

// ============================================================================
// Tool Context
// ============================================================================

/// Context provided to tool executions.
///
/// Contains session information, configuration, and shared resources.
#[derive(Clone)]
pub struct ToolContext {
    /// Unique session identifier.
    pub session_id: String,

    /// Unique task identifier within the session.
    pub task_id: String,

    /// Hermes configuration.
    pub config: Arc<HermesConfig>,

    /// Process registry for managing spawned processes.
    pub process_registry: Arc<RwLock<ProcessRegistry>>,

    /// Working directory for file operations.
    pub working_dir: PathBuf,

    /// Whether the tool requires user approval for destructive operations.
    pub require_approval: bool,
}

impl ToolContext {
    /// Create a new tool context.
    pub fn new(
        session_id: impl Into<String>,
        task_id: impl Into<String>,
        config: Arc<HermesConfig>,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            task_id: task_id.into(),
            config,
            process_registry: Arc::new(RwLock::new(ProcessRegistry::new())),
            working_dir,
            require_approval: false,
        }
    }

    /// Set the approval requirement.
    pub fn with_approval(mut self, require: bool) -> Self {
        self.require_approval = require;
        self
    }
}

// ============================================================================
// Tool Entry (for inventory)
// ============================================================================

/// Entry submitted to inventory for tool auto-discovery.
///
/// This type can be submitted via `inventory::submit!` macro.
pub struct ToolEntry {
    /// Tool name.
    pub name: &'static str,

    /// Toolset name.
    pub toolset: &'static str,

    /// Constructor function that creates the tool instance.
    pub constructor: fn() -> Box<dyn Tool>,
}

// Note: inventory::Collect implementation is handled by the inventory crate internals
// For now, we use manual registration. Inventory support can be added later.

// ============================================================================
// Tool Registry
// ============================================================================

/// Registry of all discovered tools.
///
/// Tools are automatically registered via `inventory::submit!` in each
/// tool implementation file. Call `discover_all()` to collect them.
pub struct ToolRegistry {
    /// Map of tool name -> tool instance.
    tools: HashMap<String, Box<dyn Tool>>,

    /// Map of toolset name -> tool names.
    toolsets: HashMap<String, Vec<String>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            toolsets: HashMap::new(),
        }
    }

    /// Discover all tools registered via `inventory::submit!`.
    ///
    /// For now, this uses manual registration of built-in tools.
    /// Inventory-based auto-discovery will be added once tool implementations
    /// are complete.
    pub fn discover_all() -> Self {
        let mut registry = Self::new();

        // Register built-in tools (manual registration for now)
        // When tools are implemented, they can be auto-discovered via inventory
        register_builtin_tools(&mut registry);

        registry
    }

    /// Register a single tool.
    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        let toolset = tool.toolset().to_string();

        // Add to tools map
        self.tools.insert(name.clone(), tool);

        // Add to toolsets map
        self.toolsets
            .entry(toolset)
            .or_default()
            .push(name);
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|b| b.as_ref())
    }

    /// Get all tools in a toolset.
    pub fn get_toolset(&self, toolset: &str) -> Vec<&dyn Tool> {
        self.toolsets
            .get(toolset)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|n| self.get(n))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all registered tool names.
    pub fn list_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Get all tool definitions for LLM tool calling.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| {
                ToolDefinition::new(
                    tool.name(),
                    tool.description(),
                    tool.schema(),
                )
            })
            .collect()
    }

    /// Check if a tool is available (passes check_fn and has required env vars).
    pub fn is_available(&self, name: &str) -> bool {
        self.tools.get(name).map(|tool| {
            // Check the optional check_fn
            if let Some(check) = tool.check_fn() {
                if !check() {
                    return false;
                }
            }

            // Check required environment variables
            for env_var in tool.requires_env() {
                if std::env::var(env_var).is_err() {
                    return false;
                }
            }

            true
        }).unwrap_or(false)
    }

    /// Get the number of registered tools.
    pub fn count(&self) -> usize {
        self.tools.len()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::discover_all()
    }
}

// ============================================================================
// Process Registry
// ============================================================================

/// Registry for managing spawned processes.
///
/// Tracks background and foreground processes, allowing cleanup
/// and output retrieval.
pub struct ProcessRegistry {
    /// Map of process handle ID -> process info.
    processes: HashMap<String, ProcessInfo>,
}

/// Information about a spawned process.
#[derive(Debug)]
pub struct ProcessInfo {
    /// Process handle ID.
    pub handle_id: String,

    /// Command that was executed.
    pub command: String,

    /// Whether the process is running in background.
    pub is_background: bool,

    /// Process status.
    pub status: ProcessStatus,

    /// Output collected so far (for background processes).
    pub output: String,
}

/// Status of a spawned process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Completed,
    Failed,
    Killed,
}

/// Handle to a spawned process for retrieval.
#[derive(Debug, Clone)]
pub struct ProcessHandle {
    /// Unique handle ID.
    pub id: String,
}

impl ProcessRegistry {
    /// Create an empty process registry.
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
        }
    }

    /// Spawn a new process.
    ///
    /// If `background` is true, the process runs asynchronously and
    /// output can be retrieved later. If false, it runs synchronously.
    pub fn spawn(&mut self, cmd: &str, background: bool) -> Result<ProcessHandle> {
        use uuid::Uuid;

        let handle_id = Uuid::new_v4().to_string();
        let handle = ProcessHandle { id: handle_id.clone() };

        // Store initial process info
        self.processes.insert(handle_id.clone(), ProcessInfo {
            handle_id,
            command: cmd.to_string(),
            is_background: background,
            status: ProcessStatus::Running,
            output: String::new(),
        });

        Ok(handle)
    }

    /// Get output from a process.
    pub fn get_output(&mut self, handle: &ProcessHandle) -> Result<String> {
        self.processes
            .get(&handle.id)
            .map(|info| info.output.clone())
            .ok_or_else(|| anyhow::anyhow!("Process not found: {}", handle.id))
    }

    /// Get process info.
    pub fn get_info(&self, handle: &ProcessHandle) -> Option<&ProcessInfo> {
        self.processes.get(&handle.id)
    }

    /// Update process output (for background processes).
    pub fn append_output(&mut self, handle: &ProcessHandle, output: &str) {
        if let Some(info) = self.processes.get_mut(&handle.id) {
            info.output.push_str(output);
        }
    }

    /// Update process status.
    pub fn update_status(&mut self, handle: &ProcessHandle, status: ProcessStatus) {
        if let Some(info) = self.processes.get_mut(&handle.id) {
            info.status = status;
        }
    }

    /// Kill a process.
    pub fn kill(&mut self, handle: &ProcessHandle) -> Result<()> {
        if let Some(info) = self.processes.get_mut(&handle.id) {
            info.status = ProcessStatus::Killed;
        }
        Ok(())
    }

    /// Cleanup all processes (kill any still running).
    pub fn cleanup_all(&mut self) {
        for (_, info) in self.processes.iter_mut() {
            if info.status == ProcessStatus::Running {
                info.status = ProcessStatus::Killed;
            }
        }
    }

    /// Get the number of tracked processes.
    pub fn count(&self) -> usize {
        self.processes.len()
    }

    /// List all process handles.
    pub fn list_handles(&self) -> Vec<ProcessHandle> {
        self.processes
            .keys()
            .map(|id| ProcessHandle { id: id.clone() })
            .collect()
    }
}

impl Default for ProcessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Built-in Tool Registration
// ============================================================================

/// Register all built-in tools.
///
/// This function is called by `ToolRegistry::discover_all()` to
/// register all standard Hermes tools.
fn register_builtin_tools(_registry: &mut ToolRegistry) {
    // For now, this is empty - tools will be implemented in subsequent phases.
    // When file tools, terminal tools, etc. are implemented, they will be
    // registered here.

    // Placeholder: will be populated as tools are implemented:
    // - read_file, write_file, patch, search_files (file tools)
    // - terminal (terminal tool)
    // - web_search, web_extract (web tools)
    // - delegate (delegation tool)
    // - memory (memory tool)
    // - etc.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new() {
        let registry = ToolRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_process_registry_new() {
        let registry = ProcessRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_process_spawn() {
        let mut registry = ProcessRegistry::new();
        let handle = registry.spawn("echo hello", false).unwrap();
        assert!(registry.get_info(&handle).is_some());
    }

    #[test]
    fn test_process_kill() {
        let mut registry = ProcessRegistry::new();
        let handle = registry.spawn("sleep 100", true).unwrap();
        registry.kill(&handle).unwrap();
        let info = registry.get_info(&handle).unwrap();
        assert_eq!(info.status, ProcessStatus::Killed);
    }

    #[test]
    fn test_process_cleanup_all() {
        let mut registry = ProcessRegistry::new();
        let _h1 = registry.spawn("cmd1", true).unwrap();
        let _h2 = registry.spawn("cmd2", true).unwrap();
        registry.cleanup_all();
        for info in registry.processes.values() {
            assert_eq!(info.status, ProcessStatus::Killed);
        }
    }

    #[test]
    fn test_tool_context_new() {
        let ctx = ToolContext::new(
            "session-1",
            "task-1",
            Arc::new(HermesConfig::default()),
            PathBuf::from("/tmp"),
        );
        assert_eq!(ctx.session_id, "session-1");
        assert_eq!(ctx.task_id, "task-1");
    }
}