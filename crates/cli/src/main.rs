use anyhow::{anyhow, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use h_api::{ApiConfig, ApiClient, ProviderRegistry};
use h_api::streaming::Delta;
use h_core::logging::init_logging;
use h_core::{
    config::load_config,
    home::{config_path, env_path, hermes_home, ensure_hermes_home, logs_dir, memory_dir, skills_dir},
    HermesConfig, ModelId, ModelRef, ProviderId, Message, ToolDefinition,
};
use h_core::session::Session;
use h_core::session_db::SessionDB;
use h_commands::{all_commands, CommandRegistry};
use h_plugins::{PluginRegistry, discover_plugins};
use h_query::{PromptBuilder, QueryConfig, ToolRegistry};
use h_tui::{App, print_banner};
use h_tools::ToolContext;
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
        Some(HermesCommand::Uninstall) => {
            run_uninstall()?;
        }
        Some(cmd) => {
            println!("{cmd:?} — command not yet implemented");
            println!("Run `hermes run` to start the interactive CLI.");
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

    // Set up command registry
    let _command_registry = CommandRegistry::new(all_commands());

    // Interrupt notification
    let interrupt_notify = Arc::new(Notify::new());

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

    // Add welcome message
    app.add_output_line("Welcome to Hermes Agent. Type your message or use /help for commands.");
    app.add_output_line("");

    // Run TUI with streaming query loop integration
    run_tui_with_query_loop(
        &mut app,
        &api_client,
        &tool_registry,
        &query_config,
        &session_db,
        &session_id,
        &config,
    ).await?;

    // Close session
    let _ = session_db.end_session(&session_id, "user_exit");

    Ok(())
}

/// Run the TUI with streaming query loop integration.
///
/// This function runs the TUI event loop and processes user input
/// through the LLM API with streaming responses.
async fn run_tui_with_query_loop(
    app: &mut App,
    api_client: &ApiClient,
    tool_registry: &ToolRegistry,
    query_config: &QueryConfig,
    _session_db: &SessionDB,
    _session_id: &str,
    _config: &HermesConfig,
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

    loop {
        // Render
        render_tui(app, &mut terminal)?;

        // Check for streaming updates (non-blocking)
        while let Ok(text) = rx.try_recv() {
            // Append streamed text to output
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
                                app.add_output_line(&format!("You: {user_text}"));
                                app.add_output_line("");

                                // Process the query
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
                                ).await;

                                app.is_processing = false;

                                match result {
                                    Ok(response) => {
                                        if !response.is_empty() {
                                            app.add_output_line(&response);
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
                        // TODO: slash command autocomplete
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

/// Process a user query through the LLM with streaming.
async fn process_query(
    api_client: &ApiClient,
    tool_registry: &ToolRegistry,
    query_config: &QueryConfig,
    user_input: &str,
    tx: mpsc::Sender<String>,
    interrupt_notify: Arc<Notify>,
) -> Result<String> {
    // Build messages with system prompt
    let mut messages = vec![Message::system(&query_config.system_prompt)];
    messages.push(Message::user(user_input.to_string()));

    // Get tool definitions for the API
    let tool_defs = get_tool_definitions(tool_registry);

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

        iteration += 1;

        // Call API with streaming
        let mut stream = match api_client.chat_stream(&messages, &tool_defs).await {
            Ok(s) => s,
            Err(e) => return Err(anyhow!("API error: {e}")),
        };

        let mut full_text = String::new();
        let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)
        let mut current_tool_id: Option<String> = None;
        let mut current_tool_name: Option<String> = None;
        let mut current_tool_args = String::new();

        // Process streaming response
        while let Some(delta_result) = stream.next().await {
            let delta: Delta = delta_result?;

            // Handle text content
            if let Some(content) = delta.content {
                full_text.push_str(&content);
                // Stream to TUI
                let _ = tx.send(content.clone()).await;
            }

            // Handle tool calls from streaming
            for tc in delta.tool_calls {
                if let Some(id) = tc.id {
                    current_tool_id = Some(id);
                }
                if let Some(name) = tc.name {
                    current_tool_name = Some(name.clone());
                    current_tool_args.clear();
                }
                if !tc.arguments_delta.is_empty() {
                    current_tool_args.push_str(&tc.arguments_delta);
                }

                // If we have a complete tool call (finish_reason or tool call end)
                if let (Some(id), Some(name)) = (&current_tool_id, &current_tool_name) {
                    if delta.finish_reason.is_some() || !tc.arguments_delta.is_empty() {
                        // Check if we should finalize this tool call
                        tool_calls.push((
                            id.clone(),
                            name.clone(),
                            current_tool_args.clone(),
                        ));
                    }
                }
            }

            // Handle finish reason
            if let Some(ref reason) = delta.finish_reason {
                if reason == "stop" || reason == "end_turn" {
                    if tool_calls.is_empty() {
                        // No tool calls, we're done
                        return Ok(full_text);
                    }

                    // Execute tool calls
                    for (tc_id, tc_name, tc_args) in &tool_calls {
                        if let Some(tool) = tool_registry.get(tc_name) {
                            let args: serde_json::Value = serde_json::from_str(tc_args)
                                .unwrap_or(serde_json::json!({}));

                            let ctx = ToolContext {
                                session_id: "cli".to_string(),
                                task_id: "cli".to_string(),
                                config: Arc::new(h_core::HermesConfig::default()),
                                working_dir: std::env::current_dir().unwrap_or_default(),
                            };

                            let result = tool.execute(args, &ctx).await;
                            match result {
                                Ok(tool_result) => {
                                    let output = if tool_result.is_error {
                                        format!("[Tool error: {}]", tool_result.content)
                                    } else {
                                        format!("[Tool output: {}]", tool_result.content.chars().take(200).collect::<String>())
                                    };
                                    let _ = tx.send(output.clone()).await;

                                    // Add tool result to messages
                                    messages.push(Message::tool_result(
                                        tc_id.clone(),
                                        tool_result.content,
                                    ));
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

                    // Clear tool calls and continue the loop for LLM to process tool results
                    tool_calls.clear();
                    current_tool_args.clear();
                    break; // Continue outer loop to send tool results back to LLM
                }
            }
        }
    }
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
