//! Hermes ACP Crate
//!
//! Agent Communication Protocol server for IDE integration.

pub mod server;
pub mod protocol;

pub use server::AcpServer;