// Hermes plugin discovery and hook system

pub mod plugin;
pub mod hooks;
pub mod discover;
pub mod providers;

pub use plugin::{Plugin, PluginRegistry, PluginInfo};
pub use hooks::{Hook, HookContext, HookResult, HookDispatcher, HookType};
pub use discover::{discover_plugins, load_plugin_from_dir};
pub use providers::list_memory_plugins;
