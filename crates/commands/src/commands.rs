//! Commands placeholder

use async_trait::async_trait;
use anyhow::Result;

#[async_trait]
pub trait SlashCommand: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, args: &str) -> Result<String>;
}