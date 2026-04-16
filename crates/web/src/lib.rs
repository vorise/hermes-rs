//! Hermes Web Crate
//!
//! Web UI server with Axum framework.

pub mod server;
pub mod routes;
pub mod sse;

pub use server::{WebServer, WebConfig, WebState, SessionInfo};
pub use routes::{
    ChatRequest, ChatResponse, ModelRequest, ModelInfo, SessionDetail, MessageInfo,
};
pub use sse::{SseBroadcaster, SseEvent, StreamMessage, StreamMessageType};