//! Local environment placeholder

use async_trait::async_trait;
use anyhow::Result;

#[async_trait]
pub trait Environment: Send + Sync {
    async fn run_command(&self, cmd: &str) -> Result<String>;
}

pub struct LocalEnv;

impl LocalEnv {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Environment for LocalEnv {
    async fn run_command(&self, _cmd: &str) -> Result<String> {
        Ok("placeholder".to_string())
    }
}