//! Gateway Runner placeholder

pub struct GatewayRunner;

impl GatewayRunner {
    pub fn new() -> Self { Self }
    pub async fn run(&mut self) -> anyhow::Result<()> { Ok(()) }
}