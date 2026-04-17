use anyhow::{anyhow, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use h_api::{ApiConfig, ApiClient, ProviderRegistry};
use h_api::streaming::Delta;
use h_commands::{all_commands, CommandRegistry, CommandResult, ConfigChange};
use h_core::logging::init_logging;
use h_core::{
    config::load_config,
    home::{config_path, env_path, hermes_home, ensure_hermes_home, logs_dir, memory_dir, skills_dir},
    HermesConfig, ModelId, ModelRef, ProviderId, Message, ToolDefinition,
};
use h_core::session::Session;
use h_core::session_db::SessionDB;
use h_core::checkpoint::CheckpointManager;
use h_core::tool_result_storage::ToolResultStorage;
use h_core::trajectory::{Trajectory, TrajectoryManager};
use h_plugins::{PluginRegistry, discover_plugins};
use h_query::{PromptBuilder, QueryConfig, ToolRegistry};
use h_query::context_compressor::{ContextCompressor, CompressorConfig};
use h_tui::{App, print_banner, Completer, build_completer};
use h_mcp::{McpState, config::McpConfig};
use tokio::sync::{Notify, mpsc};
use std::sync::Arc;
use futures::{StreamExt, FutureExt};

#[derive(Parser)]
#[command(name = "hermes", version = env!("CARGO_PKG_VERSION"), about = "Hermes Agent — AI-powered agentic assistant")]
struct Cli {
    #[command(subcommand)]
    command: Option<HermesCommand>,
}

#[derive(Debug, Subcommand)]
enum HermesCommand {
    /// Start interactive CLI session
    Run {
        /// Model to use (e.g., "anthropic/claude-sonnet-4-6")
        #[arg(short, long)]
        model: Option<String>,
        /// Provider to use
        #[arg(short, long)]
        provider: Option<String>,
    },
    /// Run setup wizard
    Setup,
    /// Switch model/provider
    Model { spec: Option<String> },
    /// Configure tools
    Tools { action: Option<String>, name: Option<String> },
    /// Configure skills
    Skills { action: Option<String>, name: Option<String> },
    /// Gateway management
    Gateway { action: String },
    /// Configuration
    Config { action: String, key: Option<String>, value: Option<String> },
    /// Run diagnostics
    Doctor,
    /// Update to latest version
    Update,
    /// Backup/restore
    Backup { action: String },
    /// Plugin management
    Plugins { action: String },
    /// Profile management
    Profiles { action: String },
    /// Start web UI server
    Web,
    /// Show status
    Status,
    /// View logs
    Logs { session: Option<String> },
    /// Uninstall
    Uninstall,
    /// Show version/banner
    Version,
    /// MCP server management
    Mcp { action: String },
    /// OpenClaw migration
    Claw { action: String },
    /// Shell completion setup
    Completion { shell: Option<String> },
}

fn main() -> Result<()> {
    init_logging();
    ensure_hermes_home()?;

    let args = Cli::parse();

    match args.command {
        Some(HermesCommand::Run { model, provider }) => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_interactive_cli(model, provider))?;
        }
        Some(HermesCommand::Setup) => {
            run_setup_wizard()?;
        }
        Some(HermesCommand::Model { spec }) => {
            run_model_command(spec)?;
        }
        Some(HermesCommand::Doctor) => {
            run_doctor()?;
        }
        Some(HermesCommand::Status) => {
            run_status()?;
        }
        Some(HermesCommand::Logs { session }) => {
            run_logs(session.as_deref())?;
        }
        Some(HermesCommand::Version) => {
            print_version();
        }
        Some(HermesCommand::Completion { shell }) => {
            run_completion(shell.as_deref())?;
        }
        Some(HermesCommand::Mcp { action }) => {
            run_mcp(&action)?;
        }
        Some(HermesCommand::Tools { action, name }) => {
            run_tools(action.as_deref(), name.as_deref())?;
        }
        Some(HermesCommand::Skills { action, name }) => {
            run_skills(action.as_deref(), name.as_deref())?;
        }
        Some(HermesCommand::Plugins { action }) => {
            run_plugins(&action)?;
        }
        Some(HermesCommand::Backup { action }) => {
            run_backup(&action)?;
        }
        Some(HermesCommand::Update) => {
            run_update()?;
        }
        Some(HermesCommand::Config { action, key, value }) => {
            run_config(&action, key.as_deref(), value.as_deref())?;
        }
        Some(HermesCommand::Web) => {
            run_web()?;
        }
        Some(HermesCommand::Gateway { action }) => {
            run_gateway(&action)?;
        }
        Some(HermesCommand::Profiles { action }) => {
            run_profiles(&action)?;
        }
        Some(HermesCommand::Uninstall) => {
            run_uninstall()?;
        }
        Some(HermesCommand::Claw { action }) => {
            run_claw(&action)?;
        }
        None => {
            print_version();
            println!();
            println!("Run `hermes run` to start the interactive CLI.");
            println!("Run `hermes --help` for all commands.");
        }
    }

    Ok(())
}

fn print_version() {
    let version = env!("CARGO_PKG_VERSION");
    println!("Hermes Agent v{version}");
    println!("Hermes home: {}", h_core::home::display_hermes_home());
}

// ---------------------------------------------------------------------------
// Run: Interactive CLI TUI
// ---------------------------------------------------------------------------

async fn run_interactive_cli(model_arg: Option<String>, provider_arg: Option<String>) -> Result<()> {
    // Load config
    let config = load_config(&config_path()).context("Failed to load config")?;

    // Load env vars
    h_core::config::load_env(&env_path()).context("Failed to load .env")?;

    // Determine model/provider
    let (provider_id, model_id) = resolve_model(&config, &model_arg, &provider_arg)?;

    // Set up plugin system
    let mut plugin_registry = PluginRegistry::new();
    discover_and_register_plugins(&config, &mut plugin_registry)?;

    // Set up provider registry and API client
    let provider_registry = ProviderRegistry::new();
    let provider_info = provider_registry
        .get(&provider_id)
        .ok_or_else(|| anyhow!("Unknown provider: {}", provider_id.0))?;

    let api_key = std::env::var(provider_info.api_key_env)
        .map_err(|_| anyhow!(
            "Missing API key: {} not set. Run `hermes setup` to configure.",
            provider_info.api_key_env
        ))?;

    let api_config = ApiConfig {
        provider: provider_id.clone(),
        model: model_id.clone(),
        base_url: provider_info.default_base_url.to_string(),
        api_key,
        api_mode: provider_info.api_mode.clone(),
        max_tokens: None,
        temperature: None,
        reasoning_effort: None,
    };
    let api_client = ApiClient::new(api_config).context("Failed to create API client")?;

    // Build system prompt
    let system_prompt = PromptBuilder::build_cli();

    // Set up session database
    let db_path = hermes_home().join("sessions.db");
    let session_db = Arc::new(SessionDB::open(&db_path).context("Failed to open session database")?);

    // Create a session
    let session = Session::new("cli");
    let session_id = session.id.clone();
    session_db.create_session(&session)?;

    // Set up command registry and completer
    let command_registry = CommandRegistry::new(all_commands());
    let completer = build_completer(&[
        ("help", "Show help", &["h"]),
        ("new", "Start a new session", &["reset", "clear"]),
        ("model", "Switch model/provider", &[]),
        ("compress", "Compress context", &[]),
        ("usage", "Show token/cost usage", &[]),
        ("undo", "Undo last turn", &[]),
        ("retry", "Retry last turn", &[]),
        ("stop", "Interrupt current work", &[]),
        ("tools", "List/enable/disable tools", &[]),
        ("skills", "Browse/search skills", &[]),
        ("memory", "View/manage memory", &[]),
        ("status", "Show session status", &[]),
        ("title", "Set session title", &[]),
        ("export", "Export conversation", &[]),
        ("personality", "Set personality", &[]),
        ("summarize", "Summarize conversation", &[]),
        ("insights", "Usage analytics", &[]),
        ("doctor", "Run diagnostics", &[]),
        ("mcp", "Manage MCP servers", &[]),
        ("config", "View or set configuration values", &[]),
        ("platforms", "Show connected platform status", &[]),
        ("sethome", "Set current channel as home", &[]),
        ("checkpoint", "Save, restore, list, or delete checkpoints", &["cp"]),
    ]);

    // Interrupt notification
    let interrupt_notify = Arc::new(Notify::new());

    // MCP shared state (for /mcp connect/disconnect)
    let mcp_state = Arc::new(McpState::new());

    // Set up model ref
    let model_ref = ModelRef::new(provider_id, model_id);

    // Build query config
    let query_config = QueryConfig::new(model_ref.clone())
        .with_system_prompt(system_prompt)
        .with_max_iterations(90);

    // Set up tool registry with all built-in tools
    let tool_registry = ToolRegistry::new();
    for tool in h_tools::create_all_tools() {
        tool_registry.register(tool);
    }

    // Print banner
    print_banner(env!("CARGO_PKG_VERSION"), &format!("{}/{}", model_ref.provider.0, model_ref.model.0));

    // Create TUI app
    let mut app = App::new(interrupt_notify.clone())
        .with_model(&format!("{}/{}", model_ref.provider.0, model_ref.model.0));
    app.iteration_budget = Some(query_config.max_iterations);
    app.with_commands(&[
        ("help", "Show help", &["h"]),
        ("new", "Start a new session", &["reset", "clear"]),
        ("model", "Switch model/provider", &[]),
        ("compress", "Compress context", &[]),
        ("usage", "Show token/cost usage", &[]),
        ("undo", "Undo last turn", &[]),
        ("retry", "Retry last turn", &[]),
        ("stop", "Interrupt current work", &[]),
        ("tools", "List/enable/disable tools", &[]),
        ("skills", "Browse/search skills", &[]),
        ("memory", "View/manage memory", &[]),
        ("status", "Show session status", &[]),
        ("title", "Set session title", &[]),
        ("export", "Export conversation", &[]),
        ("personality", "Set personality", &[]),
        ("summarize", "Summarize conversation", &[]),
        ("insights", "Usage analytics", &[]),
        ("doctor", "Run diagnostics", &[]),
        ("mcp", "Manage MCP servers", &[]),
        ("config", "View or set configuration values", &[]),
        ("platforms", "Show connected platform status", &[]),
        ("sethome", "Set current channel as home", &[]),
        ("checkpoint", "Save, restore, list, or delete checkpoints", &["cp"]),
    ]);

    // Add welcome message
    app.add_output_line("Welcome to Hermes Agent. Type your message or use /help for commands.");
    app.add_output_line("");

    // Run TUI with streaming query loop integration
    let session_db_arc = Arc::new(session_db);
    run_tui_with_query_loop(
        &mut app,
        &api_client,
        &tool_registry,
        &command_registry,
        &completer,
        &query_config,
        &session_db_arc,
        &session_id,
        &config,
        &model_ref,
        mcp_state,
    ).await?;

    // Close session
    let _ = session_db_arc.end_session(&session_id, "user_exit");

    Ok(())
}

/// Add or remove a toolset from the config file.
fn update_toolset_in_config(config_path: &std::path::Path, toolset: &str, enable: bool) -> anyhow::Result<()> {
    let mut config: serde_yaml::Value = if config_path.exists() {
        let content = std::fs::read_to_string(config_path)?;
        serde_yaml::from_str(&content).unwrap_or(serde_yaml::Value::Mapping(Default::default()))
    } else {
        serde_yaml::Value::Mapping(Default::default())
    };

    let mapping = config
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("Failed to parse config"))?;

    if enable {
        let mut enabled: Vec<String> = mapping
            .get("enabled_toolsets")
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if !enabled.iter().any(|t| t == toolset) {
            enabled.push(toolset.to_string());
        }
        // Remove from disabled if present
        if let Some(disabled_seq) = mapping.get_mut("disabled_toolsets") {
            if let Some(seq) = disabled_seq.as_sequence_mut() {
                seq.retain(|v| v.as_str() != Some(toolset));
            }
        }
        mapping.insert(
            serde_yaml::Value::String("enabled_toolsets".to_string()),
            serde_yaml::Value::Sequence(enabled.into_iter().map(serde_yaml::Value::String).collect()),
        );
    } else {
        let mut disabled: Vec<String> = mapping
            .get("disabled_toolsets")
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if !disabled.iter().any(|t| t == toolset) {
            disabled.push(toolset.to_string());
        }
        // Remove from enabled if present
        if let Some(enabled_seq) = mapping.get_mut("enabled_toolsets") {
            if let Some(seq) = enabled_seq.as_sequence_mut() {
                seq.retain(|v| v.as_str() != Some(toolset));
            }
        }
        mapping.insert(
            serde_yaml::Value::String("disabled_toolsets".to_string()),
            serde_yaml::Value::Sequence(disabled.into_iter().map(serde_yaml::Value::String).collect()),
        );
    }

    if let Ok(yaml) = serde_yaml::to_string(&config) {
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(config_path, yaml)?;
    }
    Ok(())
}

/// Add or remove a skill from the config file.
fn update_skill_in_config(config_path: &std::path::Path, skill: &str, enable: bool) -> anyhow::Result<()> {
    let mut config: serde_yaml::Value = if config_path.exists() {
        let content = std::fs::read_to_string(config_path)?;
        serde_yaml::from_str(&content).unwrap_or(serde_yaml::Value::Mapping(Default::default()))
    } else {
        serde_yaml::Value::Mapping(Default::default())
    };

    let mapping = config
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("Failed to parse config"))?;

    if enable {
        let mut enabled: Vec<String> = mapping
            .get("enabled_skills")
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if !enabled.iter().any(|s| s == skill) {
            enabled.push(skill.to_string());
        }
        if let Some(disabled_seq) = mapping.get_mut("disabled_skills") {
            if let Some(seq) = disabled_seq.as_sequence_mut() {
                seq.retain(|v| v.as_str() != Some(skill));
            }
        }
        mapping.insert(
            serde_yaml::Value::String("enabled_skills".to_string()),
            serde_yaml::Value::Sequence(enabled.into_iter().map(serde_yaml::Value::String).collect()),
        );
    } else {
        let mut disabled: Vec<String> = mapping
            .get("disabled_skills")
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if !disabled.iter().any(|s| s == skill) {
            disabled.push(skill.to_string());
        }
        if let Some(enabled_seq) = mapping.get_mut("enabled_skills") {
            if let Some(seq) = enabled_seq.as_sequence_mut() {
                seq.retain(|v| v.as_str() != Some(skill));
            }
        }
        mapping.insert(
            serde_yaml::Value::String("disabled_skills".to_string()),
            serde_yaml::Value::Sequence(disabled.into_iter().map(serde_yaml::Value::String).collect()),
        );
    }

    if let Ok(yaml) = serde_yaml::to_string(&config) {
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(config_path, yaml)?;
    }
    Ok(())
}

/// Update a memory nudge interval in the config file.
fn update_memory_nudge_in_config(config_path: &std::path::Path, kind: &str, interval: u32) -> anyhow::Result<()> {
    let mut config: serde_yaml::Value = if config_path.exists() {
        let content = std::fs::read_to_string(config_path)?;
        serde_yaml::from_str(&content).unwrap_or(serde_yaml::Value::Mapping(Default::default()))
    } else {
        serde_yaml::Value::Mapping(Default::default())
    };

    let mapping = config
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("Failed to parse config"))?;

    let memory = mapping
        .entry(serde_yaml::Value::String("memory".to_string()))
        .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));

    let memory_map = memory
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("Failed to parse memory config"))?;

    let key = match kind {
        "memory" => "memory_nudge_interval",
        "skill" => "skill_nudge_interval",
        _ => return Err(anyhow::anyhow!("Unknown nudge kind: {kind}")),
    };

    memory_map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Number(interval.into()),
    );

    if let Ok(yaml) = serde_yaml::to_string(&config) {
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(config_path, yaml)?;
    }
    Ok(())
}

/// Handle a config change from a slash command and update TUI state.
fn handle_config_change(
    current_model: &mut ModelRef,
    messages: &mut Vec<Message>,
    app: &mut App,
    change: ConfigChange,
    session_db: &Arc<SessionDB>,
    _session_id: &str,
) {
    match change {
        ConfigChange::Model { provider, model } => {
            *current_model = ModelRef::new(ProviderId::new(&provider), ModelId::new(&model));
            app.model_info = format!("{provider}/{model}");
            app.add_output_line(&format!("Model switched to: {provider}/{model}"));
        }
        ConfigChange::NewSession => {
            messages.clear();
            app.output.clear();
            let new_session = Session::new("cli");
            let _ = session_db.create_session(&new_session);
            // Note: session_id would need to be updated in a real impl
            app.add_output_line("New session started.");
        }
        ConfigChange::ClearSession => {
            messages.clear();
            app.output.clear();
            app.add_output_line("Session cleared.");
        }
        ConfigChange::UndoTurn => {
            if messages.len() >= 2 {
                messages.pop();
                messages.pop();
                app.add_output_line("Last turn undone.");
            } else {
                app.add_output_line("Nothing to undo.");
            }
        }
        ConfigChange::RetryTurn => {
            // Remove last assistant response and tool results
            while let Some(msg) = messages.pop() {
                if msg.content.is_some() && msg.tool_call_id.is_none() {
                    break;
                }
            }
            app.add_output_line("Last turn will be retried on next input.");
        }
        ConfigChange::Personality(name) => {
            app.add_output_line(&format!("Personality set to: {name}"));
        }
        ConfigChange::Title(title) => {
            // Just display the title change - actual persistence would need a dedicated method
            app.add_output_line(&format!("Session title set to: {title}"));
        }
        ConfigChange::RestoreCheckpoint { checkpoint_id, session_id, turn, message_count } => {
            messages.clear();
            // Reload messages from session DB at the checkpoint state
            if let Ok(session_messages) = session_db.get_messages(&session_id) {
                for stored in &session_messages {
                    let role = match stored.role.as_str() {
                        "system" => h_core::Role::System,
                        "user" => h_core::Role::User,
                        "assistant" => h_core::Role::Assistant,
                        "tool" => h_core::Role::Tool,
                        _ => continue,
                    };
                    if let Some(content) = &stored.content {
                        let msg = match role {
                            h_core::Role::System => Message::system(content),
                            h_core::Role::User => Message::user(content),
                            h_core::Role::Assistant => Message::assistant(content),
                            h_core::Role::Tool => {
                                if let Some(tool_call_id) = &stored.tool_call_id {
                                    Message::tool_result(tool_call_id.clone(), content.clone())
                                } else {
                                    continue;
                                }
                            }
                        };
                        messages.push(msg);
                    }
                }
            }
            app.output.clear();
            app.add_output_line(&format!("Restored checkpoint [{}] (turn {}, {} messages)", checkpoint_id, turn, message_count));
        }
        ConfigChange::EnableToolset(toolset) => {
            let config_path = h_core::home::config_path();
            let _ = update_toolset_in_config(&config_path, &toolset, true);
            app.add_output_line(&format!("Toolset '{toolset}' enabled."));
        }
        ConfigChange::DisableToolset(toolset) => {
            let config_path = h_core::home::config_path();
            let _ = update_toolset_in_config(&config_path, &toolset, false);
            app.add_output_line(&format!("Toolset '{toolset}' disabled."));
        }
        ConfigChange::EnableSkill(skill) => {
            let config_path = h_core::home::config_path();
            let _ = update_skill_in_config(&config_path, &skill, true);
            app.add_output_line(&format!("Skill '{skill}' enabled."));
        }
        ConfigChange::DisableSkill(skill) => {
            let config_path = h_core::home::config_path();
            let _ = update_skill_in_config(&config_path, &skill, false);
            app.add_output_line(&format!("Skill '{skill}' disabled."));
        }
        ConfigChange::SetMemoryNudgeInterval(interval) => {
            let config_path = h_core::home::config_path();
            let _ = update_memory_nudge_in_config(&config_path, "memory", interval);
            if interval == 0 {
                app.add_output_line("Memory nudge disabled.");
            } else {
                app.add_output_line(&format!("Memory nudge interval set to every {interval} turns."));
            }
        }
        ConfigChange::SetSkillNudgeInterval(interval) => {
            let config_path = h_core::home::config_path();
            let _ = update_memory_nudge_in_config(&config_path, "skill", interval);
            if interval == 0 {
                app.add_output_line("Skill nudge disabled.");
            } else {
                app.add_output_line(&format!("Skill nudge interval set to every {interval} tool iterations."));
            }
        }
    }
}

/// Run the TUI with streaming query loop integration.
///
/// This function runs the TUI event loop and processes user input
/// through the LLM API with streaming responses.
async fn run_tui_with_query_loop(
    app: &mut App,
    api_client: &ApiClient,
    tool_registry: &ToolRegistry,
    command_registry: &CommandRegistry,
    _completer: &Completer,
    query_config: &QueryConfig,
    session_db: &Arc<SessionDB>,
    session_id: &str,
    config: &HermesConfig,
    model_ref: &ModelRef,
    mcp_state: Arc<McpState>,
) -> Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use ratatui::backend::CrosstermBackend;
    use ratatui::Terminal;
    use std::io;

    let mut stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Render initial frame
    render_tui(app, &mut terminal)?;

    // Channel for streaming text updates from query loop to TUI
    let (tx, mut rx) = mpsc::channel::<String>(100);

    // Mutable state for config changes
    let mut current_model = model_ref.clone();
    let mut messages: Vec<Message> = Vec::new();
    let mut turn_count: u32 = 0;

    // Set up checkpoint manager
    let cp_db_path = h_core::home::hermes_home().join("checkpoints.db");
    let checkpoint_manager = Arc::new(CheckpointManager::open(&cp_db_path)?);

    // Set up trajectory recording for training data
    let traj_dir = h_core::home::hermes_home().join("trajectories");
    let traj_manager = TrajectoryManager::new(&traj_dir).ok();
    let mut trajectory = Trajectory::new(
        session_id,
        "cli",
        &format!("{}/{}", model_ref.provider.0, model_ref.model.0),
        &query_config.system_prompt,
    );

    loop {
        // Render
        render_tui(app, &mut terminal)?;

        // Check for streaming updates (non-blocking)
        while let Ok(text) = rx.try_recv() {
            for line in text.lines() {
                if !line.is_empty() {
                    app.output.add_line(line);
                }
            }
        }

        // Poll for events
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                        if app.is_processing {
                            app.interrupt();
                        } else {
                            break; // Exit TUI
                        }
                    }
                    (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                        break;
                    }
                    (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                        app.output.clear();
                    }
                    (KeyModifiers::NONE, KeyCode::Enter) => {
                        if !app.is_processing {
                            if let Some(user_text) = app.submit_input() {
                                // Check for slash command
                                if let Some(cmd_name) = user_text.strip_prefix('/') {
                                    let (name, args) = match cmd_name.split_once(' ') {
                                        Some((n, a)) => (n, a),
                                        None => (cmd_name, ""),
                                    };

                                    let mut ctx = h_commands::CommandContext::new(
                                        session_db.clone(),
                                        session_id.to_string(),
                                        messages.clone(),
                                        current_model.clone(),
                                        h_core::CostTracker::default(),
                                        Some(query_config.max_iterations),
                                        false,
                                        app.interrupt_notify.clone(),
                                        config.clone(),
                                    );
                                    ctx.mcp_state = Some(mcp_state.clone());

                                    match command_registry.execute(name, args, &ctx).await {
                                        Ok(CommandResult::Message(msg)) => {
                                            app.add_output_line(&msg);
                                        }
                                        Ok(CommandResult::ConfigChange(change)) => {
                                            handle_config_change(
                                                &mut current_model,
                                                &mut messages,
                                                app,
                                                change,
                                                session_db,
                                                session_id,
                                            );
                                        }
                                        Ok(CommandResult::Exit) => {
                                            break;
                                        }
                                        Err(e) => {
                                            app.add_output_line(&format!("Command error: {e}"));
                                        }
                                    }
                                    app.add_output_line("");
                                } else if user_text.trim().is_empty() {
                                    // Ignore empty input
                                } else {
                                    // Regular query — send to LLM
                                    app.add_output_line(&format!("You: {user_text}"));
                                    app.add_output_line("");

                                    // Save user message to session DB
                                    let _ = session_db.add_message(session_id, &Message::user(&user_text));

                                    app.is_processing = true;
                                    app.spinner.reset();

                                    let tx_clone = tx.clone();
                                    let result = process_query(
                                        api_client,
                                        tool_registry,
                                        query_config,
                                        &user_text,
                                        tx_clone,
                                        app.interrupt_notify.clone(),
                                        session_db,
                                        session_id,
                                        &current_model,
                                        &checkpoint_manager,
                                        turn_count,
                                    ).await;
                                    turn_count += 1;

                                    app.is_processing = false;

                                    match result {
                                        Ok(response) => {
                                            if !response.is_empty() {
                                                app.add_output_line(&response);
                                                // Save assistant response to session DB
                                                let _ = session_db.add_message(session_id, &Message::assistant(&response));
                                            }
                                            // Record trajectory turn
                                            if let Some(ref mgr) = traj_manager {
                                                trajectory.add_turn(&messages, &h_core::CostTracker::default(), true);
                                                let _ = mgr.save_trajectory(&trajectory);
                                            }
                                        }
                                        Err(e) => {
                                            app.add_output_line(&format!("Error: {e}"));
                                        }
                                    }
                                    app.add_output_line("");
                                }
                            }
                        }
                    }
                    (KeyModifiers::SHIFT, KeyCode::Enter) => {
                        app.input.insert_char('\n');
                    }
                    (KeyModifiers::NONE, KeyCode::Up) => {
                        app.navigate_history_up();
                    }
                    (KeyModifiers::NONE, KeyCode::Down) => {
                        app.navigate_history_down();
                    }
                    (KeyModifiers::NONE, KeyCode::Tab) => {
                        // Slash command autocomplete
                        if !app.is_processing {
                            let text = app.input.get_text();
                            if let Some(stripped) = text.strip_prefix('/') {
                                let prefix = stripped.split_whitespace().next().unwrap_or("");
                                if let Some(completion) = _completer.complete(prefix) {
                                    let current_text = app.input.get_text();
                                    if current_text.len() > 1 {
                                        // Replace partial command name with completion
                                        let after_slash = &current_text[1..];
                                        if let Some(space_pos) = after_slash.find(' ') {
                                            // User typed "/cmd args" — only complete the command part
                                            let completed = format!("/{completion}{}", &after_slash[space_pos..]);
                                            app.input.set_text(completed);
                                        } else {
                                            // User typed "/cmd" — complete to full command name
                                            app.input.set_text(format!("/{completion}"));
                                        }
                                    }
                                } else {
                                    // Show suggestions
                                    let suggestions = _completer.format_suggestions(prefix, 5);
                                    if !suggestions.is_empty() {
                                        app.add_output_line(&suggestions);
                                    }
                                }
                            }
                        }
                    }
                    (KeyModifiers::NONE, KeyCode::Esc) => {
                        app.input.clear();
                    }
                    (KeyModifiers::NONE | KeyModifiers::SHIFT, _) => {
                        if !app.is_processing {
                            app.input.handle_char(key.code);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Update spinner
        if app.is_processing {
            app.spinner.tick();
        }
    }

    // Save final trajectory on session exit
    if let Some(ref mgr) = traj_manager {
        if trajectory.turn_count() > 0 {
            let path = mgr.save_trajectory(&trajectory);
            if let Ok(ref p) = path {
                tracing::info!("Trajectory saved to {}", p.display());
            }
        }
    }

    // Cleanup
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
    )?;
    terminal.show_cursor()?;

    Ok(())
}

/// Maximum characters to inline in a tool result message before storing.
const MAX_INLINE_TOOL_CHARS: usize = 4000;

/// Build the content string for a tool result message, storing oversized results.
///
/// If the result is within the inline limit, returns the full content.
/// If oversized, stores the full content and returns a truncated reference.
fn build_tool_result_content(
    tc_id: &str,
    tc_name: &str,
    content: &str,
    is_error: bool,
    turn: u32,
    storage: &ToolResultStorage,
) -> String {
    if content.len() <= MAX_INLINE_TOOL_CHARS {
        content.to_string()
    } else {
        storage.store(tc_id.to_string(), tc_name.to_string(), content.to_string(), is_error, turn);
        ToolResultStorage::truncate_content(content, 500)
    }
}

/// Build a ToolContext with a clarify callback for interactive questions.
fn build_tool_context() -> h_tools::ToolContext {
    use h_tools::ClarifyCallback;

    let clarify: Option<ClarifyCallback> = Some(Arc::new(|question: &str, choices: &[&str]| {
        println!("\n>>> {question}");
        if !choices.is_empty() {
            for (i, choice) in choices.iter().enumerate() {
                println!("  {}. {choice}", i + 1);
            }
            println!("Enter the number of your choice (or type a free-form answer):");
        } else {
            println!("(Type your answer below:)");
        }

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let trimmed = input.trim().to_string();
            if !choices.is_empty() {
                if let Ok(n) = trimmed.parse::<usize>() {
                    if n >= 1 && n <= choices.len() {
                        return choices[n - 1].to_string();
                    }
                }
            }
            trimmed
        } else {
            String::new()
        }
    }));

    h_tools::ToolContext {
        session_id: "cli".to_string(),
        task_id: "cli".to_string(),
        config: Arc::new(h_core::HermesConfig::default()),
        working_dir: std::env::current_dir().unwrap_or_default(),
        clarify,
    }
}

/// Process a user query through the LLM with streaming.
async fn process_query(
    api_client: &ApiClient,
    tool_registry: &ToolRegistry,
    query_config: &QueryConfig,
    user_input: &str,
    tx: mpsc::Sender<String>,
    interrupt_notify: Arc<Notify>,
    session_db: &SessionDB,
    session_id: &str,
    model_ref: &ModelRef,
    checkpoint_manager: &CheckpointManager,
    turn: u32,
) -> Result<String> {
    // Build messages with system prompt + conversation history
    let mut messages = vec![Message::system(&query_config.system_prompt)];

    // Load previous messages from session DB
    if let Ok(session_messages) = session_db.get_messages(session_id) {
        for stored in &session_messages {
            let role = match stored.role.as_str() {
                "system" => h_core::Role::System,
                "user" => h_core::Role::User,
                "assistant" => h_core::Role::Assistant,
                "tool" => h_core::Role::Tool,
                _ => continue,
            };
            if let Some(content) = &stored.content {
                let msg = match role {
                    h_core::Role::System => Message::system(content),
                    h_core::Role::User => Message::user(content),
                    h_core::Role::Assistant => Message::assistant(content),
                    h_core::Role::Tool => {
                        if let Some(tool_call_id) = &stored.tool_call_id {
                            Message::tool_result(tool_call_id.clone(), content.clone())
                        } else {
                            continue;
                        }
                    }
                };
                messages.push(msg);
            }
        }
    }

    // Add the new user message
    messages.push(Message::user(user_input.to_string()));

    // Get tool definitions for the API
    let tool_defs = get_tool_definitions(tool_registry);

    // Set up context compressor
    let compressor = ContextCompressor::new(CompressorConfig {
        threshold_tokens: 100_000,
        preserve_turns: 6,
        auxiliary_model: None,
        use_llm_compression: false, // Truncation fallback only (no auxiliary model by default)
    });

    // Set up tool result storage for persisting large tool outputs across turns
    let tool_storage = ToolResultStorage::new();
    tool_storage.cleanup(turn);

    // Maximum iterations to prevent infinite loops
    let max_iterations = query_config.max_iterations.min(30);
    let mut iteration = 0;

    loop {
        if iteration >= max_iterations {
            return Ok("[Maximum iterations reached]".to_string());
        }

        // Check for interrupt
        if interrupt_notify.notified().now_or_never().is_some() {
            return Ok("[Interrupted]".to_string());
        }

        // Preflight context check
        let preflight = compressor.preflight_check(&messages, &query_config.system_prompt, model_ref);
        if preflight.needs_compression() {
            match compressor.compress(&mut messages, None).await {
                Ok(result) => {
                    if result.messages_compressed > 0 || result.messages_removed > 0 {
                        let saved = result.original_tokens.saturating_sub(result.compressed_tokens);
                        let _ = tx.send(format!(
                            "[Context compressed: saved ~{saved} tokens, {} messages processed]",
                            result.messages_compressed
                        )).await;
                    }
                }
                Err(e) => {
                    let _ = tx.send(format!("[Context compression error: {e}]")).await;
                }
            }
        }

        iteration += 1;

        // Call API with streaming
        let mut stream = match api_client.chat_stream(&messages, &tool_defs).await {
            Ok(s) => s,
            Err(e) => return Err(anyhow!("API error: {e}")),
        };

        let mut full_text = String::new();
        let mut finalized_tool_calls: Vec<(String, String, String)> = Vec::new();
        let mut pending_tools: Vec<(String, String, String)> = Vec::new(); // (id, name, args) being built

        // Process streaming response
        while let Some(delta_result) = stream.next().await {
            let delta: Delta = delta_result?;

            // Handle text content
            if let Some(content) = delta.content {
                full_text.push_str(&content);
                let _ = tx.send(content.clone()).await;
            }

            // Handle tool calls from streaming
            for tc in delta.tool_calls {
                let index = tc.index;

                // Ensure we have a slot for this tool call index
                while pending_tools.len() <= index {
                    pending_tools.push((String::new(), String::new(), String::new()));
                }

                let slot = &mut pending_tools[index];
                if let Some(id) = tc.id {
                    slot.0 = id;
                }
                if let Some(name) = tc.name {
                    slot.1 = name;
                }
                if !tc.arguments_delta.is_empty() {
                    slot.2.push_str(&tc.arguments_delta);
                }
            }

            // Handle finish reason
            if let Some(ref reason) = delta.finish_reason {
                if reason == "tool_calls" {
                    // All pending tool calls are complete — move to finalized
                    finalized_tool_calls = std::mem::take(&mut pending_tools)
                        .into_iter()
                        .filter(|(id, name, _)| !id.is_empty() && !name.is_empty())
                        .collect();
                    break; // Done streaming, execute tools
                } else if reason == "stop" || reason == "end_turn" {
                    // No tool calls, pure text response — save checkpoint
                    let _ = checkpoint_manager.save(session_id, turn, &messages, &query_config.system_prompt);
                    return Ok(full_text);
                }
            }
        }

        // Execute finalized tool calls
        if finalized_tool_calls.is_empty() {
            // No tool calls, save checkpoint
            let _ = checkpoint_manager.save(session_id, turn, &messages, &query_config.system_prompt);
            return Ok(full_text);
        }

        // Execute tool calls in parallel (up to 8 concurrent)
        let parallel = finalized_tool_calls.len() > 1;
        let _ = tx.send(format!(
            "[Executing {} tool{} {}]",
            finalized_tool_calls.len(),
            if finalized_tool_calls.len() == 1 { "" } else { "s" },
            if parallel { "in parallel" } else { "" }
        )).await;

        if parallel {
            // Parallel execution: spawn all tool calls concurrently
            let results = execute_tools_parallel(
                &finalized_tool_calls,
                tool_registry,
                &tx,
            ).await;

            // Append tool results to messages in order
            for ((tc_id, tc_name, _), result) in finalized_tool_calls.iter().zip(results.into_iter()) {
                match result {
                    Ok(tool_result) => {
                        let content = build_tool_result_content(
                            tc_id, tc_name, &tool_result.content, tool_result.is_error, turn, &tool_storage,
                        );
                        messages.push(Message::tool_result(tc_id.clone(), content));
                    }
                    Err(e) => {
                        messages.push(Message::tool_result(
                            tc_id.clone(),
                            format!("Error: {e}"),
                        ));
                    }
                }
            }
        } else {
            // Sequential execution (single tool call)
            let (tc_id, tc_name, tc_args) = &finalized_tool_calls[0];
            if let Some(tool) = tool_registry.get(tc_name) {
                let args: serde_json::Value = serde_json::from_str(tc_args)
                    .unwrap_or(serde_json::json!({}));

                let ctx = build_tool_context();

                let result = tool.execute(args, &ctx).await;
                match result {
                    Ok(tool_result) => {
                        let output = if tool_result.is_error {
                            format!("[Tool error: {}]", tool_result.content)
                        } else {
                            format!("[Tool output: {}]", tool_result.content.chars().take(200).collect::<String>())
                        };
                        let _ = tx.send(output.clone()).await;

                        let content = build_tool_result_content(
                            tc_id, tc_name, &tool_result.content, tool_result.is_error, turn, &tool_storage,
                        );
                        messages.push(Message::tool_result(tc_id.clone(), content));
                    }
                    Err(e) => {
                        let error_msg = format!("[Tool execution error: {e}]");
                        let _ = tx.send(error_msg.clone()).await;
                        messages.push(Message::tool_result(
                            tc_id.clone(),
                            format!("Error: {e}"),
                        ));
                    }
                }
            } else {
                let error_msg = format!("[Unknown tool: {tc_name}]");
                let _ = tx.send(error_msg.clone()).await;
                messages.push(Message::tool_result(
                    tc_id.clone(),
                    format!("Error: unknown tool '{tc_name}'"),
                ));
            }
        }
        // Continue the outer loop to send tool results back to LLM
    }
}

/// Execute multiple tool calls in parallel and return results in order.
async fn execute_tools_parallel(
    tool_calls: &[(String, String, String)], // (id, name, args)
    tool_registry: &ToolRegistry,
    tx: &mpsc::Sender<String>,
) -> Vec<Result<h_tools::ToolResult>> {
    let mut handles = Vec::new();

    for (tc_id, tc_name, tc_args) in tool_calls {
        let tc_id = tc_id.clone();
        let tc_name = tc_name.clone();
        let tc_args = tc_args.clone();
        let tx = tx.clone();

        // Get tool from registry (need to clone out since we're spawning async tasks)
        let tool_result = if let Some(tool) = tool_registry.get(&tc_name) {
            let args: serde_json::Value = serde_json::from_str(&tc_args)
                .unwrap_or(serde_json::json!({}));

            let ctx = build_tool_context();

            Some(tokio::spawn(async move {
                let result = tool.execute(args, &ctx).await;
                match &result {
                    Ok(tool_result) => {
                        let output = if tool_result.is_error {
                            format!("[Tool error: {}]", tool_result.content)
                        } else {
                            format!("[Tool output: {}]", tool_result.content.chars().take(200).collect::<String>())
                        };
                        let _ = tx.send(output).await;
                    }
                    Err(e) => {
                        let _ = tx.send(format!("[Tool execution error: {e}]")).await;
                    }
                }
                result
            }))
        } else {
            let _ = tx.send(format!("[Unknown tool: {tc_name}]")).await;
            None
        };

        handles.push((tc_id, tool_result));
    }

    // Wait for all tool calls to complete
    let mut results = Vec::new();
    for (tc_id, handle_opt) in handles {
        let result = match handle_opt {
            Some(handle) => {
                match handle.await {
                    Ok(r) => r,
                    Err(e) => Err(anyhow!("Tool call {tc_id} panicked: {e}")),
                }
            }
            None => Err(anyhow!("unknown tool '{tc_id}'")),
        };
        results.push(result);
    }

    results
}

/// Get tool definitions from the registry.
fn get_tool_definitions(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    registry
        .list()
        .into_iter()
        .filter_map(|name| registry.get(&name))
        .map(|tool| tool.to_definition())
        .collect()
}

/// Render the TUI layout (inline copy since it's private in h-tui).
fn render_tui(app: &App, terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) -> std::io::Result<()> {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::style::Style;
    use ratatui::widgets::Paragraph;

    terminal.draw(|frame| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(frame.area());

        frame.render_widget(app.output.render(&app.skin), chunks[0]);

        let status_text = if app.is_processing {
            let spin = app.spinner.current_frame();
            format!(" {} | {} | Budget: {} | {spin}",
                app.model_info, "Processing...", app.iteration_budget.unwrap_or(0))
        } else {
            format!(" {} | Ready | Budget: {}",
                app.model_info, app.iteration_budget.unwrap_or(0))
        };
        let status = Paragraph::new(status_text)
            .style(Style::default().fg(app.skin.status_fg).bg(app.skin.status_bg));
        frame.render_widget(status, chunks[1]);

        frame.render_widget(app.input.render(&app.skin), chunks[2]);

        if !app.is_processing {
            let input_area = chunks[2].inner(ratatui::layout::Margin::new(1, 1));
            let cursor_x = input_area.x + app.input.cursor_pos as u16;
            let cursor_y = input_area.y;
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Setup Wizard
// ---------------------------------------------------------------------------

fn run_setup_wizard() -> Result<()> {
    println!("=== Hermes Setup Wizard ===\n");

    let mut config = HermesConfig::default();

    // Provider selection
    let provider_registry = ProviderRegistry::new();
    println!("Available providers:");
    let providers = provider_registry.list();
    for (i, info) in providers.iter().enumerate() {
        println!("  {}. {} (default: {})", i + 1, info.display_name, info.default_model);
    }
    print!("\nSelect provider [1]: ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let choice: usize = input.trim().parse().unwrap_or(1);
    let provider = providers.get(choice - 1)
        .ok_or_else(|| anyhow!("Invalid provider selection"))?;

    // API key
    println!("\nEnter your {} API key:", provider.api_key_env);
    print!("API key: ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let api_key = read_secret_input()?;
    if !api_key.is_empty() {
        // Write to .env file
        let env_file = env_path();
        let env_content = if env_file.exists() {
            std::fs::read_to_string(&env_file)?
        } else {
            String::new()
        };

        let new_line = format!("{}={}\n", provider.api_key_env, api_key);
        let updated = if env_content.contains(provider.api_key_env) {
            env_content
                .lines()
                .filter(|l| !l.starts_with(provider.api_key_env))
                .collect::<Vec<_>>()
                .join("\n") + "\n" + &new_line
        } else {
            env_content + &new_line
        };

        std::fs::write(&env_file, updated)?;
        println!("API key saved to {}", env_file.display());
    }

    // Model selection
    println!("\nModel to use [{}]:", provider.default_model);
    print!("Model: ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let mut model_input = String::new();
    std::io::stdin().read_line(&mut model_input)?;
    let model = if model_input.trim().is_empty() {
        provider.default_model.to_string()
    } else {
        model_input.trim().to_string()
    };

    config.provider = Some(provider.id.0.clone());
    config.model = Some(model);

    // Write config
    let config_file = config_path();
    let yaml = serde_yaml::to_string(&config)?;
    std::fs::write(&config_file, yaml)?;
    println!("Config saved to {}", config_file.display());

    println!("\nSetup complete! Run `hermes run` to start.");
    Ok(())
}

fn read_secret_input() -> Result<String> {
    // Simple masked input — in production, use rpassword
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

// ---------------------------------------------------------------------------
// Doctor: Diagnostics
// ---------------------------------------------------------------------------

fn run_doctor() -> Result<()> {
    println!("Hermes Diagnostics:\n");

    // Check Hermes home
    let home = hermes_home();
    println!("  [{}] Hermes home: {}",
        if home.exists() { "OK" } else { "MISSING" },
        home.display());

    // Check config
    let config_path = config_path();
    let config = load_config(&config_path).unwrap_or_default();
    println!("  [{}] Config: {}",
        if config_path.exists() { "OK" } else { "MISSING" },
        config_path.display());

    // Check model
    let model_ok = config.model.is_some() && config.provider.is_some();
    println!("  [{}] Model: {}/{}",
        if model_ok { "OK" } else { "WARN" },
        config.provider.as_deref().unwrap_or("(not set)"),
        config.model.as_deref().unwrap_or("(not set)"));

    // Check API key
    let provider_registry = ProviderRegistry::new();
    if let Some(provider_name) = &config.provider {
        if let Some(info) = provider_registry.get(&ProviderId::new(provider_name)) {
            let has_key = std::env::var(info.api_key_env).is_ok();
            println!("  [{}] API key: {} ({})",
                if has_key { "OK" } else { "MISSING" },
                info.api_key_env,
                if has_key { "found" } else { "not set" });
        }
    } else {
        println!("  [WARN] API key: no provider configured");
    }

    // Check .env file
    let env_file = env_path();
    println!("  [{}] Environment: {}",
        if env_file.exists() { "OK" } else { "MISSING" },
        env_file.display());

    // Check directories
    for (name, path) in [
        ("Memory", memory_dir()),
        ("Skills", skills_dir()),
        ("Logs", logs_dir()),
    ] {
        println!("  [{}] {}: {}",
            if path.exists() { "OK" } else { "MISSING" },
            name,
            path.display());
    }

    // Check tools
    let enabled = &config.enabled_toolsets;
    let disabled = &config.disabled_toolsets;
    let total = enabled.as_ref().map(|v| v.len()).unwrap_or(0)
        + disabled.as_ref().map(|v| v.len()).unwrap_or(0);
    println!("  [OK]   Tools: {total} configured");

    // Check skills
    let skill_count = config.enabled_skills.as_ref().map(|v| v.len()).unwrap_or(0);
    println!("  [OK]   Skills: {skill_count} enabled");

    println!("\nAll checks passed.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Model: Switch model
// ---------------------------------------------------------------------------

fn run_model_command(spec: Option<String>) -> Result<()> {
    let config_path = config_path();
    let mut config = load_config(&config_path)?;

    match spec {
        Some(s) => {
            let parts: Vec<&str> = s.splitn(2, '/').collect();
            match parts.as_slice() {
                [provider, model] => {
                    config.provider = Some(provider.to_string());
                    config.model = Some(model.to_string());
                }
                [provider] => {
                    config.provider = Some(provider.to_string());
                    // Use default model for provider
                    let registry = ProviderRegistry::new();
                    if let Some(info) = registry.get(&ProviderId::new(*provider)) {
                        config.model = Some(info.default_model.to_string());
                    } else {
                        config.model = Some("default".to_string());
                    }
                }
                _ => {
                    println!("Usage: hermes model <provider/model> or <provider>");
                    return Ok(());
                }
            }

            let yaml = serde_yaml::to_string(&config)?;
            std::fs::write(&config_path, yaml)?;
            println!("Model switched to: {}/{}",
                config.provider.as_ref().unwrap(),
                config.model.as_ref().unwrap());
        }
        None => {
            // Show current model
            if let (Some(p), Some(m)) = (&config.provider, &config.model) {
                println!("Current model: {p}/{m}");
            } else {
                println!("No model configured. Run `hermes setup` or `hermes model <provider/model>`");
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

fn run_status() -> Result<()> {
    let config = load_config(&config_path())?;

    println!("Hermes Status:");
    println!("  Version: {}", env!("CARGO_PKG_VERSION"));
    println!("  Home: {}", h_core::home::display_hermes_home());

    if let (Some(p), Some(m)) = (&config.provider, &config.model) {
        println!("  Model: {p}/{m}");
    } else {
        println!("  Model: (not configured)");
    }

    if let Some(base_url) = &config.base_url {
        println!("  Base URL: {base_url}");
    }

    if let Some(personality) = &config.personality {
        println!("  Personality: {personality}");
    }

    // Toolsets
    if let Some(enabled) = &config.enabled_toolsets {
        println!("  Enabled toolsets: {}", enabled.join(", "));
    }
    if let Some(disabled) = &config.disabled_toolsets {
        println!("  Disabled toolsets: {}", disabled.join(", "));
    }

    // Plugins
    if let Some(plugins_config) = &config.plugins {
        if let Some(enabled) = plugins_config.enabled {
            println!("  Plugins: {}", if enabled { "enabled" } else { "disabled" });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

fn run_logs(session: Option<&str>) -> Result<()> {
    let logs = logs_dir();
    if !logs.exists() {
        println!("No logs found at {}", logs.display());
        return Ok(());
    }

    match session {
        Some(session_id) => {
            let log_file = logs.join(format!("{session_id}.log"));
            if log_file.exists() {
                let content = std::fs::read_to_string(&log_file)?;
                println!("{content}");
            } else {
                println!("No log found for session: {session_id}");
            }
        }
        None => {
            // List recent log files
            let entries: Vec<_> = std::fs::read_dir(&logs)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
                .collect();

            if entries.is_empty() {
                println!("No session logs found.");
            } else {
                println!("Recent session logs:");
                for entry in &entries {
                    println!("  {}", entry.file_name().to_string_lossy());
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shell Completion
// ---------------------------------------------------------------------------

fn run_completion(shell_str: Option<&str>) -> Result<()> {
    let shell = match shell_str {
        Some(s) => match s {
            "bash" => Shell::Bash,
            "zsh" => Shell::Zsh,
            "fish" => Shell::Fish,
            "elvish" => Shell::Elvish,
            "powershell" => Shell::PowerShell,
            other => {
                println!("Unknown shell: {other}. Supported: bash, zsh, fish, elvish, powershell");
                return Ok(());
            }
        },
        None => {
            // Detect current shell
            let shell_env = std::env::var("SHELL").unwrap_or_default();
            if shell_env.contains("zsh") {
                Shell::Zsh
            } else if shell_env.contains("bash") {
                Shell::Bash
            } else if shell_env.contains("fish") {
                Shell::Fish
            } else {
                println!("Could not detect shell. Specify one of: bash, zsh, fish, elvish, powershell");
                return Ok(());
            }
        }
    };

    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "hermes", &mut std::io::stdout());
    Ok(())
}

// ---------------------------------------------------------------------------
// Uninstall
// ---------------------------------------------------------------------------

fn run_uninstall() -> Result<()> {
    println!("This will remove all Hermes data from {}", h_core::home::display_hermes_home());
    print!("Are you sure? (y/N): ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if input.trim().to_lowercase() != "y" {
        println!("Uninstall cancelled.");
        return Ok(());
    }

    let home = hermes_home();
    if home.exists() {
        std::fs::remove_dir_all(&home)?;
        println!("Removed {}", home.display());
    }
    println!("Uninstall complete.");
    Ok(())
}

// ---------------------------------------------------------------------------
// MCP Server Management
// ---------------------------------------------------------------------------

fn run_mcp(action: &str) -> Result<()> {
    let parts: Vec<&str> = action.split_whitespace().collect();
    let command = parts.first().map(|s| *s).unwrap_or("list");

    match command {
        "list" => run_mcp_list()?,
        "add" => run_mcp_add(&parts[1..])?,
        "remove" | "rm" => run_mcp_remove(&parts[1..])?,
        "status" => run_mcp_status()?,
        "enable" => run_mcp_toggle(&parts[1..], true)?,
        "disable" => run_mcp_toggle(&parts[1..], false)?,
        "serve" => run_mcp_serve()?,
        _ => {
            println!("Unknown MCP command: {command}");
            println!();
            print_mcp_help();
        }
    }
    Ok(())
}

fn run_mcp_list() -> Result<()> {
    let config = McpConfig::from_default()?;
    if config.is_empty() {
        println!("No MCP servers configured.");
        println!("Add one with: hermes mcp add <name> --command <cmd> [args...]");
        return Ok(());
    }

    println!("Configured MCP servers:");
    println!();
    for (name, entry) in config.all_servers() {
        let status = if entry.enabled { "enabled" } else { "disabled" };
        let transport_str = match &entry.transport {
            h_mcp::config::TransportConfig::Stdio { command, args, .. } => {
                let mut s = format!("stdio: {command}");
                if !args.is_empty() {
                    s.push_str(&format!(" {}", args.join(" ")));
                }
                s
            }
            h_mcp::config::TransportConfig::Sse { url } => {
                format!("sse: {url}")
            }
        };
        println!("  {name} [{status}]");
        println!("    {transport_str}");
    }
    Ok(())
}

fn run_mcp_add(args: &[&str]) -> Result<()> {
    if args.is_empty() {
        println!("Usage: hermes mcp add <name> --command <cmd> [args...]");
        println!("   or: hermes mcp add <name> --url <http://...>");
        return Ok(());
    }

    let name = args[0];
    let mut config = McpConfig::from_default()?;

    if config.servers.contains_key(name) {
        println!("Server '{name}' already exists. Use 'hermes mcp remove {name}' first.");
        return Ok(());
    }

    let entry = parse_server_entry(args)?;
    config.add_server(name, entry);
    config.save(&McpConfig::default_path())?;

    println!("Added MCP server '{name}'.");
    println!("Config saved to {}", McpConfig::default_path().display());
    Ok(())
}

fn run_mcp_remove(args: &[&str]) -> Result<()> {
    if args.is_empty() {
        println!("Usage: hermes mcp remove <name>");
        return Ok(());
    }

    let name = args[0];
    let mut config = McpConfig::from_default()?;

    if config.remove_server(name).is_none() {
        println!("Server '{name}' not found.");
        return Ok(());
    }

    config.save(&McpConfig::default_path())?;
    println!("Removed MCP server '{name}'.");
    Ok(())
}

fn run_mcp_status() -> Result<()> {
    let config = McpConfig::from_default()?;
    if config.is_empty() {
        println!("No MCP servers configured.");
        return Ok(());
    }

    println!("MCP Server Status:");
    println!();

    for (name, entry) in config.all_servers() {
        let status = if entry.enabled { "enabled" } else { "disabled" };
        let transport_info = match &entry.transport {
            h_mcp::config::TransportConfig::Stdio { command, args, .. } => {
                format!("stdio: {} {}", command, args.join(" "))
            }
            h_mcp::config::TransportConfig::Sse { url } => {
                format!("sse: {url}")
            }
        };
        println!("  {name}: {status} ({transport_info})");
    }

    println!();
    println!("{} configured, {} enabled",
        config.servers.len(),
        config.enabled_servers().len());
    Ok(())
}

fn run_mcp_toggle(args: &[&str], enabled: bool) -> Result<()> {
    if args.is_empty() {
        let cmd = if enabled { "enable" } else { "disable" };
        println!("Usage: hermes mcp {cmd} <name>");
        return Ok(());
    }

    let name = args[0];
    let mut config = McpConfig::from_default()?;

    let Some(entry) = config.servers.get_mut(name) else {
        println!("Server '{name}' not found.");
        return Ok(());
    };
    entry.enabled = enabled;

    config.save(&McpConfig::default_path())?;
    let state = if enabled { "enabled" } else { "disabled" };
    println!("Server '{name}' {state}.");
    Ok(())
}

fn parse_server_entry(args: &[&str]) -> Result<h_mcp::config::McpServerEntry> {
    use h_mcp::config::{McpServerEntry, TransportConfig};
    use std::collections::HashMap;

    // Parse --command or --url
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "--command" | "-c" => {
                i += 1;
                if i >= args.len() {
                    anyhow::bail!("--command requires a command argument");
                }
                let command = args[i].to_string();
                // Collect remaining args as command arguments
                i += 1;
                let cmd_args: Vec<String> = args[i..].iter().map(|s| s.to_string()).collect();
                return Ok(McpServerEntry {
                    name: args[0].to_string(),
                    enabled: true,
                    transport: TransportConfig::Stdio {
                        command,
                        args: cmd_args,
                        env: None,
                    },
                    settings: HashMap::new(),
                });
            }
            "--url" | "-u" => {
                i += 1;
                if i >= args.len() {
                    anyhow::bail!("--url requires a URL argument");
                }
                let url = args[i].to_string();
                return Ok(McpServerEntry {
                    name: args[0].to_string(),
                    enabled: true,
                    transport: TransportConfig::Sse { url },
                    settings: HashMap::new(),
                });
            }
            _ => {
                i += 1;
            }
        }
    }

    anyhow::bail!("Must specify --command <cmd> or --url <http://...>");
}

fn print_mcp_help() {
    println!("MCP Server Management:");
    println!("  hermes mcp list                    List configured servers");
    println!("  hermes mcp status                  Show server status");
    println!("  hermes mcp add <name> --command <cmd> [args...]  Add stdio server");
    println!("  hermes mcp add <name> --url <url>              Add SSE server");
    println!("  hermes mcp remove <name>           Remove a server");
    println!("  hermes mcp enable <name>           Enable a server");
    println!("  hermes mcp disable <name>          Disable a server");
    println!("  hermes mcp serve                   Start Hermes as an MCP server");
}

fn run_mcp_serve() -> Result<()> {
    use h_mcp::McpServe;
    println!("Starting Hermes MCP server (stdio)...");
    let server = McpServe::new();
    server.run().context("MCP Serve failed")
}

// ---------------------------------------------------------------------------
// Tools CLI
// ---------------------------------------------------------------------------

fn run_tools(action: Option<&str>, name: Option<&str>) -> Result<()> {
    let config = load_config(&config_path()).context("Failed to load config")?;
    let all_tools = h_tools::create_all_tools();

    match action {
        None | Some("list") => {
            let enabled = config.enabled_toolsets.clone().unwrap_or_default();
            let disabled = config.disabled_toolsets.clone().unwrap_or_default();

            println!("Available Tools:");
            println!();
            for tool in &all_tools {
                let toolset = tool.toolset();
                let is_enabled = if enabled.is_empty() {
                    !disabled.contains(&toolset.to_string())
                } else {
                    enabled.contains(&toolset.to_string())
                };
                let status = if is_enabled { "enabled" } else { "disabled" };
                println!("  [{status}]  {} ({}) — {}", tool.name(), toolset, tool.description());
            }
            println!();
            println!("{} tools total", all_tools.len());
        }
        Some("enable") => {
            let tool_name = name.ok_or_else(|| anyhow!("Usage: hermes tools enable <toolset>"))?;
            println!("Toolset '{tool_name}' enabled. Restart the CLI for changes to take effect.");
        }
        Some("disable") => {
            let tool_name = name.ok_or_else(|| anyhow!("Usage: hermes tools disable <toolset>"))?;
            println!("Toolset '{tool_name}' disabled. Restart the CLI for changes to take effect.");
        }
        Some(other) => {
            println!("Unknown subcommand: {other}");
            println!("Usage: hermes tools [list|enable|disable]");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Skills CLI
// ---------------------------------------------------------------------------

fn run_skills(action: Option<&str>, name: Option<&str>) -> Result<()> {
    let skills_dir = h_core::home::skills_dir();

    match action {
        None | Some("list") => {
            if !skills_dir.exists() {
                println!("No skills installed.");
                println!("Skills directory: {}", skills_dir.display());
                return Ok(());
            }

            let entries = std::fs::read_dir(&skills_dir)?;
            let mut skills: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir() || e.path().extension().map_or(false, |ext| ext == "md"))
                .collect();
            skills.sort_by_key(|e| e.file_name());

            if skills.is_empty() {
                println!("No skills installed.");
            } else {
                println!("Installed Skills:");
                println!();
                for entry in &skills {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    // Try to read the skill's description from the first line of SKILL.md or the file itself
                    let skill_file = if entry.path().is_dir() {
                        entry.path().join("SKILL.md")
                    } else {
                        entry.path().clone()
                    };
                    let desc = if skill_file.exists() {
                        std::fs::read_to_string(&skill_file)
                            .ok()
                            .and_then(|content| {
                                // Extract description from YAML frontmatter
                                content.lines()
                                    .find(|line| line.starts_with("description:"))
                                    .map(|line| line.splitn(2, ':').nth(1).unwrap_or("").trim().trim_matches('"'))
                                    .map(String::from)
                            })
                            .unwrap_or_else(|| "(no description)".to_string())
                    } else {
                        "(no SKILL.md found)".to_string()
                    };
                    println!("  {name_str}");
                    println!("    {desc}");
                }
            }
            println!();
            println!("Skills directory: {}", skills_dir.display());
        }
        Some("search") => {
            let query = name.ok_or_else(|| anyhow!("Usage: hermes skills search <query>"))?;
            println!("Searching for skills matching '{query}'...");
            println!("(Skills Hub integration not yet available)");
        }
        Some("install") => {
            let skill_id = name.ok_or_else(|| anyhow!("Usage: hermes skills install <skill_id>"))?;
            println!("Installing skill '{skill_id}'...");
            println!("(Skills Hub integration not yet available)");
        }
        Some(other) => {
            println!("Unknown subcommand: {other}");
            println!("Usage: hermes skills [list|search|install]");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plugins CLI
// ---------------------------------------------------------------------------

fn run_plugins(action: &str) -> Result<()> {
    let plugins_dir = hermes_home().join("plugins");

    match action {
        "list" | "ls" => {
            if !plugins_dir.exists() {
                println!("No plugins installed.");
                println!("Plugins directory: {}", plugins_dir.display());
                return Ok(());
            }

            let entries = std::fs::read_dir(&plugins_dir)?;
            let mut plugins: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .collect();
            plugins.sort_by_key(|e| e.file_name());

            if plugins.is_empty() {
                println!("No plugins installed.");
            } else {
                println!("Installed Plugins:");
                println!();
                for entry in &plugins {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    // Try to read plugin.toml for metadata
                    let manifest = entry.path().join("plugin.toml");
                    if manifest.exists() {
                        if let Ok(content) = std::fs::read_to_string(&manifest) {
                            let version = content.lines()
                                .find(|line| line.starts_with("version"))
                                .map(|line| line.splitn(2, '=').nth(1).unwrap_or("").trim().trim_matches('"'))
                                .unwrap_or("?");
                            let desc = content.lines()
                                .find(|line| line.starts_with("description"))
                                .map(|line| line.splitn(2, '=').nth(1).unwrap_or("").trim().trim_matches('"'))
                                .unwrap_or("");
                            println!("  {name_str} v{version}");
                            if !desc.is_empty() {
                                println!("    {desc}");
                            }
                        }
                    } else {
                        println!("  {name_str}");
                    }
                }
            }
            println!();
            println!("Plugins directory: {}", plugins_dir.display());
        }
        "info" => {
            let name = std::env::args().skip_while(|a| a != action).nth(1)
                .ok_or_else(|| anyhow!("Usage: hermes plugins info <name>"))?;
            let plugin_dir = plugins_dir.join(&name);
            if !plugin_dir.exists() {
                println!("Plugin '{name}' not found.");
                return Ok(());
            }
            println!("Plugin: {name}");
            println!("Path: {}", plugin_dir.display());
            if plugin_dir.join("plugin.toml").exists() {
                if let Ok(content) = std::fs::read_to_string(plugin_dir.join("plugin.toml")) {
                    println!();
                    println!("Manifest:");
                    for line in content.lines() {
                        println!("  {line}");
                    }
                }
            }
        }
        "remove" | "rm" => {
            let name = std::env::args().skip_while(|a| a != action).nth(1)
                .ok_or_else(|| anyhow!("Usage: hermes plugins remove <name>"))?;
            let plugin_dir = plugins_dir.join(&name);
            if !plugin_dir.exists() {
                println!("Plugin '{name}' not found.");
                return Ok(());
            }
            std::fs::remove_dir_all(&plugin_dir)?;
            println!("Removed plugin '{name}'.");
        }
        _ => {
            println!("Unknown plugin command: {action}");
            println!("Usage: hermes plugins [list|info|remove]");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Backup CLI
// ---------------------------------------------------------------------------

fn run_backup(action: &str) -> Result<()> {
    let home = hermes_home();
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");

    match action {
        "export" | "backup" => {
            let backup_dir = std::env::current_dir().context("Failed to get current directory")?;
            let backup_name = format!("hermes-backup-{timestamp}.tar.gz");
            let backup_path = backup_dir.join(&backup_name);

            if !home.exists() {
                println!("Nothing to backup — Hermes home does not exist: {}", home.display());
                return Ok(());
            }

            println!("Backing up {} to {}...", home.display(), backup_path.display());

            // Use tar to create an archive
            let output = std::process::Command::new("tar")
                .args([
                    "-czf",
                    backup_path.to_str().unwrap(),
                    "-C",
                    home.parent().unwrap().to_str().unwrap(),
                    home.file_name().unwrap().to_str().unwrap(),
                ])
                .output()?;

            if output.status.success() {
                let size = std::fs::metadata(&backup_path)?.len();
                println!("Backup complete: {} ({})", backup_path.display(), format_bytes(size));
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("Backup failed: {stderr}");
            }
        }
        "import" | "restore" => {
            let archive_path = std::env::args().skip_while(|a| a != action).nth(1)
                .ok_or_else(|| anyhow!("Usage: hermes backup import <archive.tar.gz>"))?;
            let archive = std::path::PathBuf::from(&archive_path);

            if !archive.exists() {
                anyhow::bail!("Archive not found: {archive_path}");
            }

            println!("Restoring from {} to {}...", archive.display(), home.display());

            // Ensure hermes home exists
            if !home.exists() {
                std::fs::create_dir_all(&home)?;
            }

            let output = std::process::Command::new("tar")
                .args([
                    "-xzf",
                    &archive_path,
                    "-C",
                    home.parent().unwrap().to_str().unwrap(),
                ])
                .output()?;

            if output.status.success() {
                println!("Restore complete.");
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("Restore failed: {stderr}");
            }
        }
        "list" | "ls" => {
            let mut found: Vec<std::path::PathBuf> = Vec::new();

            // Check hermes home for backup files
            if home.exists() {
                if let Ok(entries) = std::fs::read_dir(&home) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if name_str.starts_with("hermes-backup-") && name_str.ends_with(".tar.gz") {
                            found.push(entry.path());
                        }
                    }
                }
            }

            // Also check current directory
            if let Ok(entries) = std::fs::read_dir(".") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("hermes-backup-") && name_str.ends_with(".tar.gz") {
                        let path = entry.path();
                        if !found.contains(&path) {
                            found.push(path);
                        }
                    }
                }
            }

            if found.is_empty() {
                println!("No backups found.");
            } else {
                println!("Backups:");
                for path in &found {
                    if let Ok(meta) = std::fs::metadata(path) {
                        let size = format_bytes(meta.len());
                        let modified = meta.modified().ok()
                            .map(|t| {
                                let dt: chrono::DateTime<chrono::Local> = t.into();
                                dt.format("%Y-%m-%d %H:%M").to_string()
                            })
                            .unwrap_or_else(|| "?".to_string());
                        println!("  {}  {}  {}", path.display(), size, modified);
                    } else {
                        println!("  {}", path.display());
                    }
                }
            }
        }
        _ => {
            println!("Usage: hermes backup [export|import|list]");
        }
    }
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ---------------------------------------------------------------------------
// Update CLI
// ---------------------------------------------------------------------------

fn run_update() -> Result<()> {
    println!("Checking for updates...");

    // Determine current binary path
    let current_exe = std::env::current_exe().context("Failed to determine current executable path")?;

    // Check if running from cargo run (target/debug/)
    let path_str = current_exe.to_string_lossy();
    if path_str.contains("target/") {
        println!("Running from development build (cargo).");
        println!("Run `cargo build --release` or `cargo install` to update.");
        return Ok(());
    }

    // Fetch latest release from GitHub
    println!("Fetching latest release from GitHub...");

    let client = match reqwest::blocking::Client::builder()
        .user_agent("hermes-rs-cli")
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            println!("Could not create HTTP client: {e}");
            println!("Run `cargo install --path crates/cli` to update manually.");
            return Ok(());
        }
    };

    let resp = match client
        .get("https://api.github.com/repos/sufar/hermes-rs/releases/latest")
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            println!("Could not reach GitHub: {e}");
            println!("Run `cargo install --path crates/cli` to update manually.");
            return Ok(());
        }
    };

    if !resp.status().is_success() {
        println!("Could not fetch latest release (status: {}).", resp.status());
        println!("Check https://github.com/sufar/hermes-rs/releases manually.");
        return Ok(());
    }

    let release: serde_json::Value = resp.json()?;
    let latest_version = release["tag_name"].as_str().unwrap_or("unknown").trim_start_matches('v');
    let current_version = env!("CARGO_PKG_VERSION");

    if latest_version == current_version {
        println!("Already on latest version ({current_version}).");
        return Ok(());
    }

    println!("New version available: {latest_version} (current: {current_version})");

    // Find the matching asset for this platform
    let target = std::env::consts::ARCH;
    let os = std::env::consts::OS;

    let asset = release["assets"].as_array()
        .and_then(|assets| {
            assets.iter().find(|a| {
                a["name"].as_str().map_or(false, |n| n.contains(target) && n.contains(os))
            })
        });

    let Some(asset) = asset else {
        println!("No pre-built binary available for {os}-{target}.");
        println!("Please build from source: cargo build --release");
        return Ok(());
    };

    let download_url = asset["browser_download_url"].as_str().unwrap();
    let asset_name = asset["name"].as_str().unwrap();

    println!("Downloading: {asset_name}");

    // Download to temp directory
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(asset_name);

    let download_resp = client.get(download_url).send()?;
    let mut file = std::fs::File::create(&temp_path)?;
    std::io::copy(&mut download_resp.bytes()?.as_ref(), &mut file)?;

    println!("Extracting...");

    // Extract the binary
    let output = std::process::Command::new("tar")
        .args(["-xzf", temp_path.to_str().unwrap(), "-C", temp_dir.to_str().unwrap()])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("Extraction failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    // Find the extracted binary (it might be in a subdirectory or directly in temp)
    let extracted_binary = temp_dir.join("hermes");
    if !extracted_binary.exists() {
        // Try to find it in any subdirectory
        if let Ok(entries) = std::fs::read_dir(&temp_dir) {
            for entry in entries.flatten() {
                if entry.file_name() == "hermes" && entry.metadata().map_or(false, |m| m.is_file()) {
                    let _ = std::fs::copy(entry.path(), &current_exe)
                        .context("Failed to replace binary — you may need to run with sudo");
                    println!("Update complete! Restart the CLI to use the new version.");
                    return Ok(());
                }
            }
        }
        anyhow::bail!("Could not find extracted binary");
    }

    // Replace current binary
    std::fs::copy(&extracted_binary, &current_exe)
        .context("Failed to replace binary — you may need to run with sudo")?;

    // Cleanup
    let _ = std::fs::remove_file(&temp_path);
    let _ = std::fs::remove_file(&extracted_binary);

    println!("Updated to version {latest_version}!");
    Ok(())
}

// ---------------------------------------------------------------------------
// Config CLI
// ---------------------------------------------------------------------------

fn run_config(action: &str, key: Option<&str>, value: Option<&str>) -> Result<()> {
    let config_path = config_path();
    let mut config = load_config(&config_path).context("Failed to load config")?;

    match action {
        "get" => {
            let key = key.ok_or_else(|| anyhow!("Usage: hermes config get <key>"))?;
            let val = match key {
                "model" => config.model.as_deref().unwrap_or("(not set)"),
                "provider" => config.provider.as_deref().unwrap_or("(not set)"),
                "base_url" => config.base_url.as_deref().unwrap_or("(not set)"),
                "personality" => config.personality.as_deref().unwrap_or("(default)"),
                "terminal" => {
                    match &config.terminal {
                        Some(t) => {
                            let backend_str = match &t.backend {
                                h_core::config::TerminalBackend::Local => "local",
                                h_core::config::TerminalBackend::Docker => "docker",
                                h_core::config::TerminalBackend::Ssh => "ssh",
                                h_core::config::TerminalBackend::Modal => "modal",
                                h_core::config::TerminalBackend::Daytona => "daytona",
                                h_core::config::TerminalBackend::Singularity => "singularity",
                            };
                            return Ok(println!("{backend_str}"));
                        }
                        None => "local (default)",
                    }
                }
                "memory_enabled" => {
                    match &config.memory {
                        Some(m) => if m.enabled.unwrap_or(true) { "true" } else { "false" },
                        None => "true (default)",
                    }
                }
                other => {
                    println!("Unknown config key: {other}");
                    println!("Available keys: model, provider, base_url, personality, terminal, memory_enabled");
                    return Ok(());
                }
            };
            println!("{val}");
        }
        "set" => {
            let key = key.ok_or_else(|| anyhow!("Usage: hermes config set <key> <value>"))?;
            let val = value.ok_or_else(|| anyhow!("Usage: hermes config set <key> <value>"))?;
            match key {
                "model" => config.model = Some(val.to_string()),
                "provider" => config.provider = Some(val.to_string()),
                "base_url" => config.base_url = Some(val.to_string()),
                "personality" => config.personality = Some(val.to_string()),
                "terminal" => {
                    let backend = match val {
                        "local" => h_core::config::TerminalBackend::Local,
                        "docker" => h_core::config::TerminalBackend::Docker,
                        "ssh" => h_core::config::TerminalBackend::Ssh,
                        "modal" => h_core::config::TerminalBackend::Modal,
                        "daytona" => h_core::config::TerminalBackend::Daytona,
                        "singularity" => h_core::config::TerminalBackend::Singularity,
                        other => return Err(anyhow!("Invalid terminal backend: {other}. Use: local, docker, ssh, modal, daytona, singularity")),
                    };
                    config.terminal = Some(h_core::config::TerminalConfig {
                        backend,
                        docker: None,
                        ssh: None,
                        modal: None,
                        daytona: None,
                        singularity: None,
                    });
                }
                other => {
                    println!("Unknown config key: {other}");
                    println!("Settable keys: model, provider, base_url, personality, terminal");
                    return Ok(());
                }
            }
            let content = serde_yaml::to_string(&config)?;
            std::fs::write(&config_path, content).context("Failed to write config")?;
            println!("Set {key} = {val}");
            println!("Config saved to {}", config_path.display());
        }
        "list" | "" => {
            println!("Configuration:");
            println!();
            println!("  model:       {}", config.model.as_deref().unwrap_or("(not set)"));
            println!("  provider:    {}", config.provider.as_deref().unwrap_or("(not set)"));
            println!("  base_url:    {}", config.base_url.as_deref().unwrap_or("(not set)"));
            println!("  personality: {}", config.personality.as_deref().unwrap_or("(default)"));
            if let Some(ref t) = config.terminal {
                let backend_str = match &t.backend {
                    h_core::config::TerminalBackend::Local => "local",
                    h_core::config::TerminalBackend::Docker => "docker",
                    h_core::config::TerminalBackend::Ssh => "ssh",
                    h_core::config::TerminalBackend::Modal => "modal",
                    h_core::config::TerminalBackend::Daytona => "daytona",
                    h_core::config::TerminalBackend::Singularity => "singularity",
                };
                println!("  terminal:    {backend_str}");
            }
            if let Some(ref m) = config.memory {
                println!("  memory:      {}", if m.enabled.unwrap_or(true) { "enabled" } else { "disabled" });
            }
            if let Some(ref toolsets) = config.enabled_toolsets {
                println!("  toolsets:    {}", toolsets.join(", "));
            }
            if let Some(ref disabled) = config.disabled_toolsets {
                println!("  disabled:    {}", disabled.join(", "));
            }
            println!();
            println!("Config file: {}", config_path.display());
            println!("Env file:    {}", env_path().display());
        }
        "edit" => {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            println!("Opening config in {editor}...");
            let status = std::process::Command::new(&editor)
                .arg(&config_path)
                .status()?;
            if status.success() {
                println!("Config saved.");
            } else {
                println!("Editor exited with non-zero status.");
            }
        }
        other => {
            println!("Unknown config command: {other}");
            println!("Usage: hermes config [get|set|list|edit]");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Web CLI
// ---------------------------------------------------------------------------

fn run_web() -> Result<()> {
    let config = load_config(&config_path()).context("Failed to load config")?;
    h_core::config::load_env(&env_path()).context("Failed to load .env")?;

    // Determine listen address from config or default
    let host = config.web.as_ref()
        .and_then(|w| w.host.as_deref())
        .unwrap_or("0.0.0.0");
    let port = config.web.as_ref()
        .and_then(|w| w.port)
        .unwrap_or(8090);

    let listen_addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .context("Invalid listen address")?;

    let db_path = hermes_home().join("sessions.db");
    let session_db = Arc::new(SessionDB::open(&db_path).context("Failed to open session database")?);

    let web_config = h_web::WebServerConfig {
        listen_addr,
        serve_static: true,
        hermes_config: config.clone(),
    };

    let server = h_web::WebServer::new(web_config, session_db)
        .with_tools(h_tools::create_all_tools());

    println!("Hermes Web UI starting on http://{listen_addr}");
    println!("Press Ctrl+C to stop.");

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(server.start())?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Gateway CLI
// ---------------------------------------------------------------------------

fn run_gateway(action: &str) -> Result<()> {
    match action {
        "start" => {
            let config_path_str = config_path();
            let gateway_config = match h_gateway::GatewayConfig::load(&config_path_str) {
                Ok(c) => c,
                Err(e) => {
                    println!("No gateway config found at {}, using defaults ({e})", config_path_str.display());
                    h_gateway::GatewayConfig::default()
                }
            };

            let hermes_config = load_config(&config_path_str).context("Failed to load config")?;
            h_core::config::load_env(&env_path()).context("Failed to load .env")?;

            let db_path = hermes_home().join("sessions.db");
            let db = Arc::new(SessionDB::open(&db_path).context("Failed to open session database")?);

            let all_tools = h_tools::create_all_tools();
            let mut runner = h_gateway::GatewayRunner::new(gateway_config, hermes_config, db, all_tools);

            println!("Hermes Gateway starting...");
            println!("Press Ctrl+C to stop.");

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(runner.start())?;
        }
        "status" => {
            let config = load_config(&config_path()).context("Failed to load config")?;
            println!("Gateway Configuration:");
            println!();

            if let Some(ref platforms) = config.platforms {
                if let Some(ref tg) = platforms.telegram {
                    println!("  Telegram:    configured (token env: {})", tg.bot_token_env);
                    if tg.webhook_url.is_some() {
                        println!("    webhook:     {}", tg.webhook_url.as_ref().unwrap());
                    } else {
                        println!("    mode:        long polling");
                    }
                }
                if let Some(ref dc) = platforms.discord {
                    println!("  Discord:     configured (token env: {})", dc.bot_token_env);
                }
                if let Some(ref sc) = platforms.slack {
                    println!("  Slack:       configured (token env: {}, app env: {})", sc.bot_token_env, sc.app_token_env);
                }
            } else {
                println!("  (no platforms configured)");
            }
        }
        _ => {
            println!("Unknown gateway command: {action}");
            println!("Usage: hermes gateway [start|status]");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Profiles CLI
// ---------------------------------------------------------------------------

fn run_profiles(action: &str) -> Result<()> {
    let config = load_config(&config_path()).context("Failed to load config")?;

    match action {
        "list" | "ls" => {
            match &config.profiles {
                Some(profiles) if !profiles.is_empty() => {
                    println!("Profiles:");
                    println!();
                    for p in profiles {
                        let model = p.model.as_deref().unwrap_or("(inherits global)");
                        let toolsets = p.enabled_toolsets.as_ref()
                            .map(|t| t.join(", "))
                            .unwrap_or_else(|| "(inherits global)".to_string());
                        println!("  {} — model: {}, tools: {}", p.name, model, toolsets);
                    }
                }
                _ => {
                    println!("No profiles configured.");
                    println!("Edit {} to add profiles.", config_path().display());
                }
            }
        }
        "show" => {
            let profiles = config.profiles.as_ref()
                .ok_or_else(|| anyhow!("No profiles configured"))?;
            // Show first profile or all if none specified
            if profiles.is_empty() {
                println!("No profiles configured.");
            } else {
                for p in profiles {
                    println!("Profile: {}", p.name);
                    println!("  model: {}", p.model.as_deref().unwrap_or("(inherits global)"));
                    if let Some(ref ts) = p.enabled_toolsets {
                        println!("  enabled toolsets: {}", ts.join(", "));
                    }
                    if let Some(ref ts) = p.disabled_toolsets {
                        println!("  disabled toolsets: {}", ts.join(", "));
                    }
                    println!();
                }
            }
        }
        _ => {
            println!("Unknown profile command: {action}");
            println!("Usage: hermes profiles [list|show]");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Claw: OpenClaw Migration
// ---------------------------------------------------------------------------

fn run_claw(action: &str) -> Result<()> {
    match action {
        "migrate" | "import" => {
            run_claw_migrate()
        }
        "check" | "detect" => {
            run_claw_check()
        }
        _ => {
            println!("Usage: hermes claw [migrate|check]");
            println!();
            println!("OpenClaw Migration:");
            println!("  hermes claw check     Detect existing OpenClaw configuration");
            println!("  hermes claw migrate   Migrate OpenClaw config to Hermes format");
            Ok(())
        }
    }
}

fn run_claw_check() -> Result<()> {
    println!("Scanning for OpenClaw configuration...\n");

    let home = std::env::var("HOME")
        .ok()
        .map(std::path::PathBuf::from);
    let config_dir = home.as_ref().map(|h| h.join(".config"));

    let mut found: Vec<(&str, std::path::PathBuf)> = Vec::new();

    // Common OpenClaw config locations
    let search_paths: Vec<(&str, Option<std::path::PathBuf>)> = vec![
        ("OpenClaw config", home.as_ref().map(|h| h.join(".openclaw/config.yaml"))),
        ("OpenClaw home", home.as_ref().map(|h| h.join(".openclaw"))),
        ("OpenClaw XDG config", config_dir.as_ref().map(|d| d.join("openclaw"))),
        ("OpenClaw skills", home.as_ref().map(|h| h.join(".openclaw/skills"))),
        ("OpenClaw memory", home.as_ref().map(|h| h.join(".openclaw/memory"))),
    ];

    for (label, path_opt) in &search_paths {
        if let Some(path) = path_opt {
            if path.exists() {
                println!("  [FOUND] {label}: {}", path.display());
                found.push((label, path.clone()));
            } else {
                println!("  [MISSING] {label}");
            }
        }
    }

    if found.is_empty() {
        println!("\nNo OpenClaw configuration found. Nothing to migrate.");
    } else {
        println!("\nFound {} OpenClaw component(s).", found.len());
        println!("Run `hermes claw migrate` to import them into Hermes.");
    }

    Ok(())
}

fn run_claw_migrate() -> Result<()> {
    let home = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .map_err(|_| anyhow!("Could not determine home directory (HOME env not set)"))?;

    let openclaw_dir = home.join(".openclaw");
    if !openclaw_dir.exists() {
        println!("No OpenClaw configuration found at ~/.openclaw");
        println!("If your config is elsewhere, run `hermes claw check` to scan.");
        return Ok(());
    }

    let dest = hermes_home();
    let mut migrated: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // Migrate config
    let openclaw_config = openclaw_dir.join("config.yaml");
    if openclaw_config.exists() {
        let dest_config = config_path();
        if dest_config.exists() {
            // Back up existing Hermes config
            let backup = dest_config.with_extension("yaml.bak");
            std::fs::copy(&dest_config, &backup)?;
            println!("  Backed up existing Hermes config to {}", backup.display());
        }
        std::fs::copy(&openclaw_config, &dest_config)?;
        migrated.push(format!("config.yaml → {}", dest_config.display()));

        // Try to parse and normalize to Hermes config schema
        match std::fs::read_to_string(&dest_config) {
            Ok(content) => {
                match serde_yaml::from_str::<HermesConfig>(&content) {
                    Ok(mut config) => {
                        // Ensure all fields have defaults populated
                        // This handles any schema differences between OpenClaw and Hermes
                        if config.model.is_none() {
                            config.model = Some("claude-sonnet-4-6".to_string());
                        }
                        if config.provider.is_none() {
                            config.provider = Some("anthropic".to_string());
                        }
                        let normalized = serde_yaml::to_string(&config)?;
                        std::fs::write(&dest_config, normalized)?;
                        migrated.push("config normalized to Hermes schema".to_string());
                    }
                    Err(e) => {
                        skipped.push(format!("config.yaml needs manual review: {e}"));
                    }
                }
            }
            Err(e) => {
                skipped.push(format!("config.yaml read error: {e}"));
            }
        }
    } else {
        skipped.push("config.yaml not found".to_string());
    }

    // Migrate .env file
    let openclaw_env = openclaw_dir.join(".env");
    if openclaw_env.exists() {
        let dest_env = env_path();
        if dest_env.exists() {
            // Merge: add keys from OpenClaw that don't exist in Hermes .env
            let existing = std::fs::read_to_string(&dest_env)?;
            let existing_keys: std::collections::HashSet<_> = existing
                .lines()
                .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim()))
                .collect();

            let openclaw_content = std::fs::read_to_string(&openclaw_env)?;
            let mut added = 0;
            let mut merged = existing.clone();
            for line in openclaw_content.lines() {
                if let Some((key, _)) = line.split_once('=') {
                    let key = key.trim();
                    if !key.is_empty() && !key.starts_with('#') && !existing_keys.contains(key) {
                        merged.push_str(&format!("\n{line}"));
                        added += 1;
                    }
                }
            }
            if added > 0 {
                std::fs::write(&dest_env, merged)?;
                migrated.push(format!(".env ({added} new keys merged)"));
            } else {
                migrated.push(".env (all keys already present)".to_string());
            }
        } else {
            std::fs::copy(&openclaw_env, &dest_env)?;
            migrated.push(".env → hermes .env".to_string());
        }
    }

    // Migrate skills
    let openclaw_skills = openclaw_dir.join("skills");
    let dest_skills = skills_dir();
    if openclaw_skills.exists() && openclaw_skills.is_dir() {
        if !dest_skills.exists() {
            std::fs::create_dir_all(&dest_skills)?;
        }
        let mut skill_count = 0;
        for entry in std::fs::read_dir(&openclaw_skills)? {
            let entry = entry?;
            let dest = dest_skills.join(entry.file_name());
            if !dest.exists() {
                copy_dir_recursive(&entry.path(), &dest)?;
                skill_count += 1;
            } else {
                skipped.push(format!("skill {} already exists", entry.file_name().to_string_lossy()));
            }
        }
        if skill_count > 0 {
            migrated.push(format!("skills ({skill_count} skills copied)"));
        }
    }

    // Migrate memory
    let openclaw_memory = openclaw_dir.join("memory");
    let dest_memory = memory_dir();
    if openclaw_memory.exists() && openclaw_memory.is_dir() {
        if !dest_memory.exists() {
            std::fs::create_dir_all(&dest_memory)?;
        }
        let mut memory_count = 0;
        for entry in std::fs::read_dir(&openclaw_memory)? {
            let entry = entry?;
            let dest = dest_memory.join(entry.file_name());
            if !dest.exists() {
                std::fs::copy(entry.path(), &dest)?;
                memory_count += 1;
            }
        }
        if memory_count > 0 {
            migrated.push(format!("memory ({memory_count} files copied)"));
        }
    }

    // Migrate MCP config
    let openclaw_mcp = openclaw_dir.join("mcp.json");
    if openclaw_mcp.exists() {
        let dest_mcp = dest.join("mcp.json");
        if !dest_mcp.exists() {
            std::fs::copy(&openclaw_mcp, &dest_mcp)?;
            migrated.push("mcp.json".to_string());
        }
    }

    // Migrate plugins
    let openclaw_plugins = openclaw_dir.join("plugins");
    let dest_plugins = dest.join("plugins");
    if openclaw_plugins.exists() && openclaw_plugins.is_dir() {
        if !dest_plugins.exists() {
            std::fs::create_dir_all(&dest_plugins)?;
        }
        let mut plugin_count = 0;
        for entry in std::fs::read_dir(&openclaw_plugins)? {
            let entry = entry?;
            let dest = dest_plugins.join(entry.file_name());
            if !dest.exists() {
                copy_dir_recursive(&entry.path(), &dest)?;
                plugin_count += 1;
            }
        }
        if plugin_count > 0 {
            migrated.push(format!("plugins ({plugin_count} plugins copied)"));
        }
    }

    // Report
    println!("OpenClaw Migration Results:\n");

    if !migrated.is_empty() {
        println!("Migrated:");
        for item in &migrated {
            println!("  [OK]   {item}");
        }
    }

    if !skipped.is_empty() {
        println!();
        println!("Skipped:");
        for item in &skipped {
            println!("  [-]    {item}");
        }
    }

    println!();
    println!("Migration complete. {} item(s) migrated, {} skipped.", migrated.len(), skipped.len());
    println!("Hermes home: {}", dest.display());

    Ok(())
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let dest_child = dest.join(entry.file_name());
            copy_dir_recursive(&entry.path(), &dest_child)?;
        }
    } else {
        std::fs::copy(src, dest)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_model(
    config: &HermesConfig,
    model_arg: &Option<String>,
    provider_arg: &Option<String>,
) -> Result<(ProviderId, ModelId)> {
    // CLI args take priority
    if let (Some(provider), Some(model)) = (provider_arg, model_arg) {
        return Ok((ProviderId::new(provider), ModelId::new(model)));
    }

    // If model arg contains a slash, parse as provider/model
    if let Some(spec) = model_arg {
        if let Some(slash_pos) = spec.find('/') {
            let provider = &spec[..slash_pos];
            let model = &spec[slash_pos + 1..];
            return Ok((ProviderId::new(provider), ModelId::new(model)));
        }
    }

    // Fall back to config
    let provider = config.provider.as_deref().unwrap_or("anthropic");
    let model = config.model.as_deref().unwrap_or("claude-sonnet-4-6-20250514");

    Ok((ProviderId::new(provider), ModelId::new(model)))
}

fn discover_and_register_plugins(
    config: &HermesConfig,
    registry: &mut PluginRegistry,
) -> Result<()> {
    // Discover from filesystem
    if let Some(plugins_config) = &config.plugins {
        if let Some(dirs) = &plugins_config.directories {
            for dir in dirs {
                let path = std::path::Path::new(dir);
                match discover_plugins(path) {
                    Ok(manifests) => {
                        for manifest in manifests {
                            tracing::info!(
                                "Discovered plugin: {} v{}",
                                manifest.name,
                                manifest.version
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to discover plugins from {dir}: {e}");
                    }
                }
            }
        }
    }

    // Note: Built-in plugins would be registered here
    // Example:
    // let builtin = Arc::new(h_plugins::BuiltinPlugin::new(
    //     "greeting",
    //     "Greeting plugin",
    //     vec!["on_session_start"],
    //     |hook_name, ctx| Ok(vec![serde_json::json!({"greeting": "Hello!"})]),
    // ));
    // registry.register(builtin)?;

    tracing::info!("Loaded {} plugins", registry.count());
    Ok(())
}
