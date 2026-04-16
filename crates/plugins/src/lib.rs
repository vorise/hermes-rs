//! Hermes Plugins Crate
//!
//! Plugin discovery and hook system.

pub mod registry;
pub mod discovery;
pub mod hooks;

pub use registry::PluginRegistry;