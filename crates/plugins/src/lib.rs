//! Hermes Plugins Crate
//!
//! Plugin discovery and hook system.

pub mod registry;
pub mod discovery;
pub mod hooks;

pub use registry::{PluginRegistry, Plugin, PluginMetadata, PluginState, PluginEntry, PluginError};
pub use discovery::{DiscoveryConfig, DiscoveredPlugin, PluginManifest, discover_plugins};
pub use hooks::{
    HookType, HookArgs, HookResult, HookRegistry,
    invoke_hook, invoke_hook_by_name, register_hook,
};