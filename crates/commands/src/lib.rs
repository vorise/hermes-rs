use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use h_core::{CostTracker, HermesConfig, Message, ModelRef};
use h_core::session_db::SessionDB;
use h_mcp::McpState;
use tokio::sync::Notify;

mod checkpoint;
mod compress;
mod config_cmd;
mod doctor;
mod export;
mod help;
mod insights;
mod mcp;
mod memory;
mod model;
mod new;
mod nudge;
mod personality;
mod platforms;
mod retry;
mod sethome;
mod skills;
mod speak;
mod status;
mod stop;
mod summarize;
mod title;
mod tools;
mod undo;
mod voice;
mod usage;

/// Result of executing a slash command.
pub enum CommandResult {
    /// Display a message to the user.
    Message(String),
    /// Change the query/config (e.g., model switch).
    ConfigChange(ConfigChange),
    /// Exit the session.
    Exit,
}

/// Configuration change requested by a command.
pub enum ConfigChange {
    /// Switch to a new model.
    Model { provider: String, model: String },
    /// Start a fresh session.
    NewSession,
    /// Clear current session messages.
    ClearSession,
    /// Undo the last turn.
    UndoTurn,
    /// Retry the last turn.
    RetryTurn,
    /// Set personality.
    Personality(String),
    /// Set session title.
    Title(String),
    /// Restore from a checkpoint.
    RestoreCheckpoint {
        checkpoint_id: i64,
        session_id: String,
        turn: u32,
        message_count: usize,
    },
    /// Enable a toolset.
    EnableToolset(String),
    /// Disable a toolset.
    DisableToolset(String),
    /// Enable a skill.
    EnableSkill(String),
    /// Disable a skill.
    DisableSkill(String),
    /// Set memory nudge interval (0 = disabled).
    SetMemoryNudgeInterval(u32),
    /// Set skill nudge interval (0 = disabled).
    SetSkillNudgeInterval(u32),
}

/// Context available to all command handlers.
pub struct CommandContext {
    /// Current session database.
    pub session_db: Arc<SessionDB>,
    /// Current session ID.
    pub session_id: String,
    /// Current messages in the session.
    pub messages: Vec<Message>,
    /// Current model reference.
    pub model: ModelRef,
    /// Current cost tracking.
    pub cost: CostTracker,
    /// Iteration budget remaining.
    pub budget_remaining: Option<u32>,
    /// Whether currently processing.
    pub is_processing: bool,
    /// Interrupt signal sender.
    pub interrupt_notify: Arc<Notify>,
    /// Hermes home directory config.
    pub hermes_config: HermesConfig,
    /// Shared MCP state (client + tool registry), if available.
    pub mcp_state: Option<Arc<McpState>>,
}

impl CommandContext {
    pub fn new(
        session_db: Arc<SessionDB>,
        session_id: String,
        messages: Vec<Message>,
        model: ModelRef,
        cost: CostTracker,
        budget_remaining: Option<u32>,
        is_processing: bool,
        interrupt_notify: Arc<Notify>,
        hermes_config: HermesConfig,
    ) -> Self {
        Self {
            session_db,
            session_id,
            messages,
            model,
            cost,
            budget_remaining,
            is_processing,
            interrupt_notify,
            hermes_config,
            mcp_state: None,
        }
    }
}

/// A slash command handler.
#[async_trait]
pub trait SlashCommand: Send + Sync {
    /// Command name (without leading `/`).
    fn name(&self) -> &str;

    /// Alternative names (e.g., `["reset"]` for `/new` alias).
    fn aliases(&self) -> Vec<&str> {
        vec![]
    }

    /// Short description shown in help.
    fn description(&self) -> &str;

    /// Category for grouping (general, session, model, etc.).
    fn category(&self) -> &str {
        "general"
    }

    /// Execute the command with the given arguments string.
    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult>;
}

/// Command registry and dispatcher.
pub struct CommandRegistry {
    commands: Vec<Box<dyn SlashCommand>>,
}

impl CommandRegistry {
    pub fn new(commands: Vec<Box<dyn SlashCommand>>) -> Self {
        Self { commands }
    }

    /// Execute a command by name, returning "Did you mean...?" on no match.
    pub async fn execute(
        &self,
        name: &str,
        args: &str,
        ctx: &CommandContext,
    ) -> Result<CommandResult> {
        if let Some(cmd) = self.find(name) {
            return cmd.execute(args, ctx).await;
        }
        // Fuzzy suggestion
        let suggestions = self.fuzzy_suggestions(name, 3);
        let msg = if suggestions.is_empty() {
            format!("Unknown command: /{name}")
        } else {
            format!(
                "Unknown command: /{name}. Did you mean: {}?",
                suggestions
                    .iter()
                    .map(|s| format!("/{s}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        Ok(CommandResult::Message(msg))
    }

    /// List all commands grouped by category.
    pub fn list_commands(&self) -> Vec<(&str, Vec<&dyn SlashCommand>)> {
        let mut groups: std::collections::BTreeMap<&str, Vec<&dyn SlashCommand>> =
            std::collections::BTreeMap::new();
        for cmd in &self.commands {
            groups
                .entry(cmd.category())
                .or_default()
                .push(cmd.as_ref());
        }
        groups.into_iter().collect()
    }

    /// Find a command by name or alias (case-insensitive).
    pub fn find(&self, name: &str) -> Option<&dyn SlashCommand> {
        let name_lower = name.to_lowercase();
        self.commands
            .iter()
            .find(|cmd| {
                cmd.name().to_lowercase() == name_lower
                    || cmd
                        .aliases()
                        .iter()
                        .any(|a| a.to_lowercase() == name_lower)
            })
            .map(|c| c.as_ref())
    }

    /// Fuzzy match suggestions for an unknown command name.
    fn fuzzy_suggestions(&self, input: &str, max: usize) -> Vec<String> {
        let input_lower = input.to_lowercase();
        let mut scored: Vec<(&str, usize)> = self
            .commands
            .iter()
            .flat_map(|cmd| {
                let mut names = vec![cmd.name()];
                names.extend(cmd.aliases());
                names
            })
            .map(|name| {
                let score = levenshtein(&input_lower, &name.to_lowercase());
                (name, score)
            })
            .filter(|(_, score)| *score <= 4)
            .collect();
        scored.sort_by_key(|(_, score)| *score);
        scored
            .into_iter()
            .take(max)
            .map(|(name, _)| name.to_string())
            .collect()
    }
}

/// Simple Levenshtein distance for fuzzy matching.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    let mut dp = vec![vec![0usize; b_len + 1]; a_len + 1];
    for i in 0..=a_len {
        dp[i][0] = i;
    }
    for j in 0..=b_len {
        dp[0][j] = j;
    }
    for (i, ca) in a.chars().enumerate() {
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            dp[i + 1][j + 1] = *[
                dp[i][j + 1] + 1,
                dp[i + 1][j] + 1,
                dp[i][j] + cost,
            ]
            .iter()
            .min()
            .unwrap();
        }
    }
    dp[a_len][b_len]
}

/// Register all commands.
pub fn all_commands() -> Vec<Box<dyn SlashCommand>> {
    vec![
        Box::new(crate::new::NewCommand),
        Box::new(crate::new::ClearCommand),
        Box::new(crate::model::ModelCommand),
        Box::new(crate::compress::CompressCommand),
        Box::new(crate::stop::StopCommand),
        Box::new(crate::usage::UsageCommand),
        Box::new(crate::undo::UndoCommand),
        Box::new(crate::retry::RetryCommand),
        Box::new(crate::tools::ToolsCommand),
        Box::new(crate::skills::SkillsCommand),
        Box::new(crate::memory::MemoryCommand),
        Box::new(crate::status::StatusCommand),
        Box::new(crate::help::HelpCommand),
        Box::new(crate::title::TitleCommand),
        Box::new(crate::export::ExportCommand),
        Box::new(crate::personality::PersonalityCommand),
        Box::new(crate::summarize::SummarizeCommand),
        Box::new(crate::insights::InsightsCommand),
        Box::new(crate::doctor::DoctorCommand),
        Box::new(crate::mcp::McpCommand),
        Box::new(crate::config_cmd::ConfigCommand),
        Box::new(crate::platforms::PlatformsCommand),
        Box::new(crate::sethome::SethomeCommand),
        Box::new(crate::speak::SpeakCommand),
        Box::new(crate::voice::VoiceCommand),
        Box::new(crate::checkpoint::CheckpointCommand),
        Box::new(crate::nudge::NudgeCommand),
    ]
}
