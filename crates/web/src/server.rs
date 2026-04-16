//! Web Server placeholder

pub struct WebServer;

impl WebServer {
    pub fn new() -> Self { Self }
    pub async fn run(&mut self) -> anyhow::Result<()> { Ok(()) }
}