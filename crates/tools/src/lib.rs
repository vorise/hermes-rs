//! Hermes Tools Crate
//!
//! Tool registry and implementations for Hermes Agent.

pub mod registry;
pub mod approval;
pub mod process_registry;
pub mod file_tools;

// Main exports
pub use registry::{
    Tool, ToolContext, ToolEntry, ToolRegistry,
    ProcessRegistry, ProcessInfo, ProcessStatus, ProcessHandle,
};
pub use approval::{
    is_destructive_command, needs_warning, assess_risk, RiskLevel,
    is_destructive_file_op,
};
pub use file_tools::{
    ReadFileTool, WriteFileTool, PatchTool, SearchFilesTool,
};