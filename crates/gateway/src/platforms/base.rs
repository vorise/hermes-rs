//! Base Platform Adapter placeholder

use async_trait::async_trait;
use anyhow::Result;

#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    fn name(&self) -> &str;
    async fn connect(&self) -> Result<()>;
    async fn disconnect(&self) -> Result<()>;
}