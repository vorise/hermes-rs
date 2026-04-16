// Hermes plugin discovery and hook system

mod plugin;
mod hooks;
mod discover;

pub use plugin::{Plugin, PluginRegistry, PluginInfo};
pub use hooks::{Hook, HookContext, HookResult, HookDispatcher};
pub use discover::{discover_plugins, load_plugin_from_dir};
