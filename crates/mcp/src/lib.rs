//! Hermes MCP Crate
//!
//! Model Context Protocol client implementation.

pub mod client;
pub mod transport;
pub mod discovery;

pub use client::McpClient;