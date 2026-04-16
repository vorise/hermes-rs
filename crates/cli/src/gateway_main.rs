use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use h_core::SessionDB;
use h_gateway::{GatewayConfig, GatewayRunner};
use tracing_subscriber::EnvFilter;

/// Hermes Gateway — Messaging gateway daemon for Slack, Discord, Telegram, etc.
#[derive(Parser, Debug)]
#[command(name = "hermes-gateway", version, about)]
struct Args {
    /// Path to gateway config file.
    #[arg(short, long)]
    config: Option<String>,
    /// Log level (e.g. "info", "debug", "trace").
    #[arg(short, long, default_value = "info")]
    log: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let env_filter = EnvFilter::try_new(&args.log).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();

    // Load config
    let config = if let Some(path) = &args.config {
        let path_buf = std::path::PathBuf::from(path);
        GatewayConfig::load(&path_buf)?
    } else {
        GatewayConfig::default()
    };

    // Initialize session database
    let db = Arc::new(SessionDB::open(std::path::Path::new("hermes.db"))?);

    tracing::info!("Hermes Gateway starting");

    let runner = GatewayRunner::new(config, db);
    runner.start().await?;

    Ok(())
}
