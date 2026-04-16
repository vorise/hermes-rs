//! Hermes Envs Crate
//!
//! Terminal backend implementations.

pub mod local;
pub mod docker;
pub mod ssh;
pub mod modal;
pub mod daytona;
pub mod singularity;
pub mod file_sync;

pub use local::LocalEnv;