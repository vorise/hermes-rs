//! Hermes Query Crate
//!
//! Core agentic conversation loop.

pub mod loop_runner;
pub mod prompt_builder;
pub mod context_compressor;

pub use loop_runner::{
    run_query_loop, QueryConfig, QueryResult, IterationBudget,
    InterruptSignal, StopReason, ReasoningConfig,
};
pub use prompt_builder::{build_system_prompt, build_toolset_prompt, PromptCacheKey};
pub use context_compressor::ContextCompressor;