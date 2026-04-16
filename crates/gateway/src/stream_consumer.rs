//! Stream Consumer placeholder

use async_trait::async_trait;
use anyhow::Result;

#[async_trait]
pub trait StreamConsumer: Send + Sync {
    async fn on_text_delta(&self, delta: &str) -> Result<()>;
    async fn flush(&self) -> Result<()>;
}