use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use h_acp::AcpServer;
use tracing_subscriber::EnvFilter;

/// Hermes ACP Server — IDE integration via Agent Communication Protocol.
#[derive(Parser, Debug)]
#[command(name = "hermes-acp", version, about)]
struct Args {
    /// Workspace root path.
    #[arg(short, long, default_value = ".")]
    workspace: String,
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

    // Resolve workspace to absolute path
    let workspace_path = if PathBuf::from(&args.workspace).is_absolute() {
        args.workspace.clone()
    } else {
        std::env::current_dir()
            .map(|p| p.join(&args.workspace))
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(args.workspace.clone())
    };

    let server = AcpServer::new(workspace_path.clone());
    tracing::info!("Hermes ACP server starting — workspace: {workspace_path}");

    server.run_stdio().await?;

    Ok(())
}
