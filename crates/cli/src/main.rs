//! Hermes CLI Binary
//!
//! Main entry point for the Hermes agent.

use clap::Parser;
use h_core::{HermesConfig, hermes_home};
use h_core::logging::init_logging;

#[derive(Parser)]
#[command(name = "hermes")]
#[command(version, about = "Hermes AI Agent - Rust Implementation")]
struct HermesCli {
    /// Model to use (provider:model format)
    #[arg(short, long)]
    model: Option<String>,

    /// Run gateway mode
    #[arg(short, long)]
    gateway: bool,

    /// Run ACP server mode
    #[arg(short, long)]
    acp: bool,

    /// Run web UI server
    #[arg(short, long)]
    web: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();

    let args = HermesCli::parse();

    // Ensure home directory exists
    h_core::home::ensure_all_dirs()?;

    // Load configuration
    let config = HermesConfig::load()?;

    tracing::info!("Hermes starting with model: {:?}", config.model_ref());

    if args.gateway {
        // Gateway mode - TODO: implement in Phase 11
        tracing::info!("Gateway mode not yet implemented");
        return Ok(());
    }

    if args.acp {
        // ACP server mode - TODO: implement in Phase 14
        tracing::info!("ACP server mode not yet implemented");
        return Ok(());
    }

    if args.web {
        // Web UI mode - TODO: implement in Phase 13
        tracing::info!("Web UI mode not yet implemented");
        return Ok(());
    }

    // Default: Interactive CLI TUI
    // TODO: implement in Phase 6
    tracing::info!("Interactive TUI not yet implemented");
    println!("Hermes Agent (Rust) - Phase 1 skeleton complete");
    println!("Config path: {}", hermes_home().display());

    Ok(())
}