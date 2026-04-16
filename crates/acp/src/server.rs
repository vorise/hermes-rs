//! ACP Server placeholder

pub struct AcpServer;

impl AcpServer {
    pub fn new() -> Self { Self }
    pub async fn run(&mut self) -> anyhow::Result<()> { Ok(()) }
}