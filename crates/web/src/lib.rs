//! Hermes Web Crate
//!
//! Web UI server.

pub mod server;
pub mod routes;
pub mod sse;

pub use server::WebServer;