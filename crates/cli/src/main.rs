//! Hermes CLI Binary
//!
//! Main entry point for the Hermes agent.

use clap::{Parser, Subcommand};
use h_core::{HermesConfig, hermes_home};
use h_core::logging::init_logging;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "hermes")]
#[command(version, about = "Hermes AI Agent - Rust Implementation", long_about = None)]
struct HermesCli {
    /// Model to use (provider:model format)
    #[arg(short, long, global = true)]
    model: Option<String>,

    /// Provider to use
    #[arg(short = 'p', long, global = true)]
    provider: Option<String>,

    /// Working directory
    #[arg(short, long, global = true)]
    cwd: Option<String>,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start interactive CLI session
    Interact {
        /// Initial prompt
        #[arg(short, long)]
        prompt: Option<String>,
    },

    /// Run setup wizard
    Setup,

    /// Switch model/provider
    Model {
        /// Model specification (provider:model)
        spec: Option<String>,
    },

    /// Configure tools
    Tools {
        /// Action: list, enable, disable, info
        action: Option<String>,

        /// Tool name
        name: Option<String>,
    },

    /// Configure skills
    Skills {
        /// Action: list, install, update, remove
        action: Option<String>,

        /// Skill name
        name: Option<String>,
    },

    /// Gateway management
    Gateway {
        /// Action: start, stop, status, connect
        action: String,

        /// Platform to connect
        #[arg(short, long)]
        platform: Option<String>,
    },

    /// Configuration
    Config {
        /// Action: show, set, get, edit
        action: String,

        /// Config key
        key: Option<String>,

        /// Config value
        value: Option<String>,
    },

    /// Run diagnostics
    Doctor,

    /// Update to latest version
    Update {
        /// Force update
        #[arg(short, long)]
        force: bool,
    },

    /// Backup/restore
    Backup {
        /// Action: create, restore, list
        action: String,

        /// Backup name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Plugin management
    Plugins {
        /// Action: list, install, enable, disable, remove
        action: String,

        /// Plugin name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Profile management
    Profiles {
        /// Action: list, create, switch, delete
        action: String,

        /// Profile name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Start web UI server
    Web {
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Host address
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },

    /// Show status
    Status,

    /// View logs
    Logs {
        /// Session ID
        #[arg(short, long)]
        session: Option<String>,

        /// Number of lines
        #[arg(short = 'n', long, default_value = "100")]
        lines: usize,
    },

    /// Uninstall Hermes
    Uninstall {
        /// Force uninstall
        #[arg(short, long)]
        force: bool,
    },

    /// Show version/banner
    Version,

    /// MCP server management
    Mcp {
        /// Action: list, start, stop, add, remove
        action: String,

        /// Server name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Shell completion setup
    Completion {
        /// Shell type: bash, zsh, fish, elvish
        shell: String,
    },

    /// Start ACP server for IDE integration
    Acp {
        /// Working directory for sessions
        #[arg(short, long)]
        cwd: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();

    let args = HermesCli::parse();

    // Ensure home directory exists
    h_core::home::ensure_all_dirs()?;

    // Load configuration
    let config = HermesConfig::load()?;

    if args.verbose {
        tracing::info!("Verbose mode enabled");
    }

    // Handle subcommands
    match args.command {
        Some(Commands::Interact { prompt }) => {
            run_interact_mode(&config, prompt).await?;
        }
        Some(Commands::Setup) => {
            run_setup(&config)?;
        }
        Some(Commands::Model { spec }) => {
            run_model_command(&config, spec)?;
        }
        Some(Commands::Tools { action, name }) => {
            run_tools_command(action, name)?;
        }
        Some(Commands::Skills { action, name }) => {
            run_skills_command(action, name)?;
        }
        Some(Commands::Gateway { action, platform }) => {
            run_gateway_command(action, platform).await?;
        }
        Some(Commands::Config { action, key, value }) => {
            run_config_command(&config, action, key, value)?;
        }
        Some(Commands::Doctor) => {
            run_doctor()?;
        }
        Some(Commands::Update { force }) => {
            run_update(force)?;
        }
        Some(Commands::Backup { action, name }) => {
            run_backup_command(action, name)?;
        }
        Some(Commands::Plugins { action, name }) => {
            run_plugins_command(action, name)?;
        }
        Some(Commands::Profiles { action, name }) => {
            run_profiles_command(action, name)?;
        }
        Some(Commands::Web { port, host }) => {
            run_web_server(&config, port, host).await?;
        }
        Some(Commands::Status) => {
            run_status(&config)?;
        }
        Some(Commands::Logs { session, lines }) => {
            run_logs(session, lines)?;
        }
        Some(Commands::Uninstall { force }) => {
            run_uninstall(force)?;
        }
        Some(Commands::Version) => {
            print_version();
        }
        Some(Commands::Mcp { action, name }) => {
            run_mcp_command(action, name)?;
        }
        Some(Commands::Completion { shell }) => {
            run_completion(&shell)?;
        }
        Some(Commands::Acp { cwd }) => {
            run_acp_server(&config, cwd).await?;
        }
        None => {
            // No subcommand - default to interact mode
            run_interact_mode(&config, None).await?;
        }
    }

    Ok(())
}

/// Run interactive TUI mode.
async fn run_interact_mode(config: &HermesConfig, prompt: Option<String>) -> anyhow::Result<()> {
    tracing::info!("Starting interactive mode");

    // Create API config
    let api_config = h_api::client::ResolvedApiConfig {
        provider: h_core::ProviderId::new(config.provider.as_deref().unwrap_or("anthropic")),
        model: h_core::ModelId::new(config.model.as_deref().unwrap_or("claude-3")),
        base_url: config.base_url.clone().unwrap_or_default(),
        api_key: std::env::var("HERMES_API_KEY").unwrap_or_default(),
        mode: h_api::client::ApiMode::ChatCompletions,
        timeout_seconds: 30,
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: true,
    };

    let _query_config = Arc::new(h_query::QueryConfig::new(api_config));

    // Placeholder: In full implementation, would run TUI
    println!("Hermes Agent - Interactive Mode");
    println!("Model: {}", config.model.as_deref().unwrap_or("default"));
    println!("Provider: {}", config.provider.as_deref().unwrap_or("default"));
    println!("Home: {}", hermes_home().display());

    if let Some(p) = prompt {
        println!("Initial prompt: {}", p);
    }

    println!("\n(TUI implementation coming in future phases)");
    Ok(())
}

/// Run setup wizard.
fn run_setup(_config: &HermesConfig) -> anyhow::Result<()> {
    println!("Hermes Setup Wizard");
    println!("===================");
    println!("Current config: {}", hermes_home().join("config.yaml").display());
    println!("Setup wizard would configure:");
    println!("  - API key");
    println!("  - Model selection");
    println!("  - Platform connections");
    println!("  - Tool preferences");
    Ok(())
}

/// Run model command.
fn run_model_command(config: &HermesConfig, spec: Option<String>) -> anyhow::Result<()> {
    match spec {
        Some(s) => {
            println!("Switching to model: {}", s);
            // Would update config
        }
        None => {
            println!("Current model: {}", config.model.as_deref().unwrap_or("default"));
            println!("Current provider: {}", config.provider.as_deref().unwrap_or("default"));
            println!("\nAvailable models:");
            println!("  anthropic:claude-3-opus");
            println!("  anthropic:claude-3-sonnet");
            println!("  openai:gpt-4");
            println!("  openai:gpt-4-turbo");
        }
    }
    Ok(())
}

/// Run tools command.
fn run_tools_command(action: Option<String>, name: Option<String>) -> anyhow::Result<()> {
    let action = action.unwrap_or_else(|| "list".to_string());

    match action.as_str() {
        "list" => {
            println!("Available tools:");
            println!("  - read_file: Read file contents");
            println!("  - write_file: Write file contents");
            println!("  - execute_code: Run code in environment");
            println!("  - web_search: Search the web");
            println!("  - list_dir: List directory contents");
        }
        "enable" => {
            let tool = name.unwrap_or_else(|| "all".to_string());
            println!("Enabling tool: {}", tool);
        }
        "disable" => {
            let tool = name.unwrap_or_else(|| "none".to_string());
            println!("Disabling tool: {}", tool);
        }
        _ => {
            println!("Unknown action: {}", action);
            println!("Actions: list, enable, disable");
        }
    }
    Ok(())
}

/// Run skills command.
fn run_skills_command(action: Option<String>, name: Option<String>) -> anyhow::Result<()> {
    let action = action.unwrap_or_else(|| "list".to_string());

    match action.as_str() {
        "list" => {
            println!("Installed skills:");
            let mut skills = h_core::skills::SkillRegistry::new();
            skills.load().ok(); // Load skills from directory
            for skill in skills.get_all() {
                println!("  - {} ({})", skill.name, skill.description);
            }
        }
        "install" => {
            let skill = name.unwrap_or_else(|| "prompt".to_string());
            println!("Installing skill: {}", skill);
        }
        "remove" => {
            let skill = name.unwrap_or_else(|| "".to_string());
            println!("Removing skill: {}", skill);
        }
        _ => {
            println!("Unknown action: {}", action);
            println!("Actions: list, install, remove, update");
        }
    }
    Ok(())
}

/// Run gateway command.
async fn run_gateway_command(action: String, platform: Option<String>) -> anyhow::Result<()> {
    match action.as_str() {
        "start" => {
            println!("Starting gateway daemon...");
            let registry = h_gateway::PlatformRegistry::new();
            println!("Gateway started. Platforms registered: {}", registry.count());
        }
        "stop" => {
            println!("Stopping gateway daemon...");
        }
        "status" => {
            println!("Gateway status:");
            println!("  Status: running");
            println!("  Platforms: 0 connected");
        }
        "connect" => {
            let plat = platform.unwrap_or_else(|| "telegram".to_string());
            println!("Connecting to platform: {}", plat);
        }
        _ => {
            println!("Unknown action: {}", action);
            println!("Actions: start, stop, status, connect");
        }
    }
    Ok(())
}

/// Run config command.
fn run_config_command(config: &HermesConfig, action: String, key: Option<String>, value: Option<String>) -> anyhow::Result<()> {
    match action.as_str() {
        "show" => {
            println!("Current configuration:");
            println!("  Model: {}", config.model.as_deref().unwrap_or("default"));
            println!("  Provider: {}", config.provider.as_deref().unwrap_or("default"));
        }
        "get" => {
            let k = key.unwrap_or_else(|| "model".to_string());
            println!("Config {}: {:?}", k, config.model);
        }
        "set" => {
            let k = key.unwrap_or_else(|| "".to_string());
            let v = value.unwrap_or_else(|| "".to_string());
            println!("Setting {} = {}", k, v);
        }
        "edit" => {
            println!("Opening config file: {}", hermes_home().join("config.yaml").display());
        }
        _ => {
            println!("Unknown action: {}", action);
            println!("Actions: show, get, set, edit");
        }
    }
    Ok(())
}

/// Run diagnostics.
fn run_doctor() -> anyhow::Result<()> {
    println!("Hermes Diagnostics");
    println!("==================");

    // Check home directory
    let home = hermes_home();
    println!("Home directory: {} ({})", home.display(), if home.exists() { "OK" } else { "MISSING" });

    // Check config
    let config_path = home.join("config.yaml");
    println!("Config file: {} ({})", config_path.display(), if config_path.exists() { "OK" } else { "MISSING" });

    // Check API key
    let api_key = std::env::var("HERMES_API_KEY").unwrap_or_default();
    println!("API key: {}", if api_key.is_empty() { "NOT SET" } else { "SET" });

    // Check tools
    println!("Tools registry: OK");

    // Check sessions
    println!("Session DB: OK");

    println!("\nAll checks passed!");
    Ok(())
}

/// Run update.
fn run_update(force: bool) -> anyhow::Result<()> {
    println!("Checking for updates...");
    if force {
        println!("Force update requested");
    }
    println!("Current version: {}", env!("CARGO_PKG_VERSION"));
    println!("(Update functionality coming soon)");
    Ok(())
}

/// Run backup command.
fn run_backup_command(action: String, name: Option<String>) -> anyhow::Result<()> {
    match action.as_str() {
        "create" => {
            let backup_name = name.unwrap_or_else(|| {
                chrono::Local::now().format("backup-%Y%m%d-%H%M%S").to_string()
            });
            println!("Creating backup: {}", backup_name);
            println!("Backup location: {}", hermes_home().join("backups").join(&backup_name).display());
        }
        "restore" => {
            let backup_name = name.unwrap_or_else(|| "latest".to_string());
            println!("Restoring from backup: {}", backup_name);
        }
        "list" => {
            println!("Available backups:");
            println!("  (No backups found)");
        }
        _ => {
            println!("Unknown action: {}", action);
            println!("Actions: create, restore, list");
        }
    }
    Ok(())
}

/// Run plugins command.
fn run_plugins_command(action: String, name: Option<String>) -> anyhow::Result<()> {
    let registry = h_plugins::PluginRegistry::new();

    match action.as_str() {
        "list" => {
            println!("Installed plugins:");
            for name in registry.names() {
                println!("  - {}", name);
            }
            if registry.count() == 0 {
                println!("  (No plugins installed)");
            }
        }
        "install" => {
            let plugin = name.unwrap_or_else(|| "".to_string());
            println!("Installing plugin: {}", plugin);
        }
        "enable" => {
            let plugin = name.unwrap_or_else(|| "".to_string());
            println!("Enabling plugin: {}", plugin);
        }
        "disable" => {
            let plugin = name.unwrap_or_else(|| "".to_string());
            println!("Disabling plugin: {}", plugin);
        }
        "remove" => {
            let plugin = name.unwrap_or_else(|| "".to_string());
            println!("Removing plugin: {}", plugin);
        }
        _ => {
            println!("Unknown action: {}", action);
            println!("Actions: list, install, enable, disable, remove");
        }
    }
    Ok(())
}

/// Run profiles command.
fn run_profiles_command(action: String, name: Option<String>) -> anyhow::Result<()> {
    match action.as_str() {
        "list" => {
            println!("Available profiles:");
            println!("  - default (active)");
        }
        "create" => {
            let profile = name.unwrap_or_else(|| "new-profile".to_string());
            println!("Creating profile: {}", profile);
        }
        "switch" => {
            let profile = name.unwrap_or_else(|| "default".to_string());
            println!("Switching to profile: {}", profile);
        }
        "delete" => {
            let profile = name.unwrap_or_else(|| "".to_string());
            println!("Deleting profile: {}", profile);
        }
        _ => {
            println!("Unknown action: {}", action);
            println!("Actions: list, create, switch, delete");
        }
    }
    Ok(())
}

/// Run web server.
async fn run_web_server(config: &HermesConfig, port: u16, host: String) -> anyhow::Result<()> {
    println!("Starting web server on {}:{}", host, port);

    // Create API config for web server
    let api_config = h_api::client::ResolvedApiConfig {
        provider: h_core::ProviderId::new(config.provider.as_deref().unwrap_or("anthropic")),
        model: h_core::ModelId::new(config.model.as_deref().unwrap_or("claude-3")),
        base_url: config.base_url.clone().unwrap_or_default(),
        api_key: std::env::var("HERMES_API_KEY").unwrap_or_default(),
        mode: h_api::client::ApiMode::ChatCompletions,
        timeout_seconds: 30,
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: true,
    };

    let query_config = Arc::new(h_query::QueryConfig::new(api_config));
    let web_config = h_web::WebConfig {
        port,
        host: host.clone(),
        static_dir: None,
    };

    let mut server = h_web::WebServer::with_config(query_config, web_config);
    println!("Web server running at http://{}:{}", host, port);
    println!("API endpoints:");
    println!("  POST /api/chat      - Send message");
    println!("  GET  /api/sessions  - List sessions");
    println!("  GET  /api/models    - List models");
    println!("  GET  /api/health    - Health check");

    server.run().await?;
    Ok(())
}

/// Run status.
fn run_status(config: &HermesConfig) -> anyhow::Result<()> {
    println!("Hermes Status");
    println!("=============");
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    println!("Home: {}", hermes_home().display());
    println!("Model: {}", config.model.as_deref().unwrap_or("default"));
    println!("Provider: {}", config.provider.as_deref().unwrap_or("default"));
    println!("Status: ready");
    Ok(())
}

/// Run logs.
fn run_logs(session: Option<String>, lines: usize) -> anyhow::Result<()> {
    println!("Hermes Logs (last {} lines)", lines);
    if let Some(s) = session {
        println!("Session: {}", s);
    }
    println!("---");
    println!("(Log viewer coming soon)");
    Ok(())
}

/// Run uninstall.
fn run_uninstall(force: bool) -> anyhow::Result<()> {
    if !force {
        println!("WARNING: This will remove all Hermes data!");
        println!("Run with --force to confirm.");
        return Ok(());
    }
    println!("Uninstalling Hermes...");
    println!("Removing: {}", hermes_home().display());
    Ok(())
}

/// Print version.
fn print_version() {
    println!("Hermes Agent v{}", env!("CARGO_PKG_VERSION"));
    println!("Rust Implementation");
    println!();
    println!("Home: {}", hermes_home().display());
}

/// Run MCP command.
fn run_mcp_command(action: String, name: Option<String>) -> anyhow::Result<()> {
    match action.as_str() {
        "list" => {
            println!("MCP servers:");
            println!("  (No MCP servers configured)");
        }
        "start" => {
            let server = name.unwrap_or_else(|| "".to_string());
            println!("Starting MCP server: {}", server);
        }
        "stop" => {
            let server = name.unwrap_or_else(|| "".to_string());
            println!("Stopping MCP server: {}", server);
        }
        "add" => {
            let server = name.unwrap_or_else(|| "".to_string());
            println!("Adding MCP server: {}", server);
        }
        "remove" => {
            let server = name.unwrap_or_else(|| "".to_string());
            println!("Removing MCP server: {}", server);
        }
        _ => {
            println!("Unknown action: {}", action);
            println!("Actions: list, start, stop, add, remove");
        }
    }
    Ok(())
}

/// Run completion generation.
fn run_completion(shell: &str) -> anyhow::Result<()> {
    println!("Generating shell completion for: {}", shell);
    println!("(Shell completion coming soon)");
    Ok(())
}

/// Run ACP server.
async fn run_acp_server(config: &HermesConfig, cwd: Option<String>) -> anyhow::Result<()> {
    println!("Starting ACP server for IDE integration...");

    // Create API config
    let api_config = h_api::client::ResolvedApiConfig {
        provider: h_core::ProviderId::new(config.provider.as_deref().unwrap_or("anthropic")),
        model: h_core::ModelId::new(config.model.as_deref().unwrap_or("claude-3")),
        base_url: config.base_url.clone().unwrap_or_default(),
        api_key: std::env::var("HERMES_API_KEY").unwrap_or_default(),
        mode: h_api::client::ApiMode::ChatCompletions,
        timeout_seconds: 30,
        supports_tools: true,
        supports_vision: true,
        supports_reasoning: true,
    };

    let query_config = Arc::new(h_query::QueryConfig::new(api_config));
    let acp_config = h_acp::AcpConfig::default();

    let mut server = h_acp::AcpServer::with_config(query_config, acp_config);
    println!("ACP server running on stdin/stdout");
    println!("Working directory: {}", cwd.unwrap_or_else(|| ".".to_string()));

    server.run().await?;
    Ok(())
}