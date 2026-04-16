//! Hermes API Crate
//!
//! LLM API client with multi-provider support and streaming.

pub mod client;
pub mod registry;
pub mod providers;

pub use client::{ApiClient, ApiConfig, ApiMode, ResolvedApiConfig, StreamDelta, SseStream};
pub use registry::{ProviderInfo, ProviderRegistry};