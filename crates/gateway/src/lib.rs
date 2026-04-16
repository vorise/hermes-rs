//! Hermes Gateway Crate
//!
//! Messaging platform gateway and adapters.

pub mod runner;
pub mod session;
pub mod stream_consumer;
pub mod platforms;

pub use runner::GatewayRunner;