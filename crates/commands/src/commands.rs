//! Slash Commands Trait and Types
//!
//! Core types for the Hermes slash command system.

use async_trait::async_trait;
use anyhow::Result;
use h_core::{HermesConfig, Message, CostTracker};
use std::sync::Arc;

/// Slash command trait.
///
/// All slash commands must implement this trait for registration
/// in the command registry.
#[async_trait]
pub trait SlashCommand: Send + Sync {
    /// Command name (e.g., "new", "model", "help").
    fn name(&self) -> &str;

    /// Command aliases (e.g., "reset" for "new").
    fn aliases(&self) -> Vec<&str> {
        vec![]
    }

    /// Human-readable description.
    fn description(&self) -> &str;

    /// Command category for grouping.
    fn category(&self) -> &str {
        "general"
    }

    /// Execute the command.
    ///
    /// Args: raw argument string from user input.
    /// Context: execution context with session state.
    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult>;
}

/// Execution context for commands.
#[derive(Debug, Clone)]
pub struct CommandContext {
    /// Current session ID.
    pub session_id: String,

    /// Hermes configuration.
    pub config: Arc<HermesConfig>,

    /// Current conversation messages.
    pub messages: Vec<Message>,

    /// Cost tracking.
    pub cost: CostTracker,

    /// Iteration count.
    pub iteration: u32,

    /// Working directory.
    pub working_dir: std::path::PathBuf,

    /// Current model reference.
    pub model: h_core::ModelRef,
}

impl CommandContext {
    /// Create a new command context.
    pub fn new(
        session_id: impl Into<String>,
        config: Arc<HermesConfig>,
        messages: Vec<Message>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            config,
            messages,
            cost: CostTracker::new(),
            iteration: 0,
            working_dir: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            model: h_core::ModelRef::default(),
        }
    }

    /// Get total token count.
    pub fn total_tokens(&self) -> u64 {
        self.cost.total_tokens()
    }

    /// Get estimated cost.
    pub fn estimated_cost(&self) -> f64 {
        self.cost.estimated_cost_usd
    }

    /// Get message count.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

/// Command execution result.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandResult {
    /// Display a message to the user.
    Message(String),

    /// Change configuration.
    ConfigChange(ConfigChangeMessage),

    /// Exit the application.
    Exit,

    /// Clear/reset the session.
    ClearSession,

    /// Switch model.
    SwitchModel(h_core::ModelRef),

    /// Trigger context compression.
    CompressContext,

    /// Interrupt current work.
    Interrupt,

    /// Undo last turn.
    Undo,

    /// Retry last turn.
    Retry,

    /// No action (silent success).
    None,
}

/// Configuration change message.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigChangeMessage {
    /// Configuration key that changed.
    pub key: String,

    /// New value.
    pub value: String,

    /// Message to display.
    pub message: String,
}

/// Command help information.
#[derive(Debug, Clone)]
pub struct CommandHelp {
    /// Command name.
    pub name: String,

    /// Aliases.
    pub aliases: Vec<String>,

    /// Description.
    pub description: String,

    /// Usage examples.
    pub usage: Vec<String>,

    /// Category.
    pub category: String,
}

impl CommandHelp {
    /// Create from a SlashCommand.
    pub fn from_command(cmd: &dyn SlashCommand) -> Self {
        Self {
            name: cmd.name().to_string(),
            aliases: cmd.aliases().iter().map(|s| s.to_string()).collect(),
            description: cmd.description().to_string(),
            usage: Vec::new(),
            category: cmd.category().to_string(),
        }
    }

    /// Format as help text.
    pub fn format(&self) -> String {
        let aliases_str = if self.aliases.is_empty() {
            String::new()
        } else {
            format!(" (aliases: {})", self.aliases.join(", "))
        };

        let usage_str = if self.usage.is_empty() {
            String::new()
        } else {
            format!("\n  Usage:\n{}", self.usage.iter().map(|u| format!("    {}", u)).collect::<Vec<_>>().join("\n"))
        };

        format!("/{}{} - {}{}", self.name, aliases_str, self.description, usage_str)
    }
}

/// Parse command name and arguments from input.
///
/// Input should start with "/" followed by command name.
pub fn parse_command_input(input: &str) -> Option<(String, String)> {
    if !input.starts_with('/') {
        return None;
    }

    let rest = input[1..].trim_start();
    let parts = rest.split_whitespace().collect::<Vec<_>>();

    if parts.is_empty() {
        return None;
    }

    let name = parts[0].to_lowercase();
    let args = if parts.len() > 1 {
        parts[1..].join(" ")
    } else {
        String::new()
    };

    Some((name, args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command_input() {
        let input = "/model openai:gpt-4";
        let (name, args) = parse_command_input(input).unwrap();
        assert_eq!(name, "model");
        assert_eq!(args, "openai:gpt-4");

        let input = "/help";
        let (name, args) = parse_command_input(input).unwrap();
        assert_eq!(name, "help");
        assert_eq!(args, "");

        let input = "hello";
        assert!(parse_command_input(input).is_none());
    }

    #[test]
    fn test_command_context_new() {
        let ctx = CommandContext::new(
            "session-1",
            Arc::new(HermesConfig::default()),
            Vec::new(),
        );
        assert_eq!(ctx.session_id, "session-1");
        assert_eq!(ctx.message_count(), 0);
    }

    #[test]
    fn test_command_help_format() {
        let help = CommandHelp {
            name: "model".to_string(),
            aliases: vec!["m".to_string()],
            description: "Switch model".to_string(),
            usage: vec!["/model provider:model".to_string()],
            category: "config".to_string(),
        };

        let formatted = help.format();
        assert!(formatted.contains("/model"));
        assert!(formatted.contains("aliases: m"));
        assert!(formatted.contains("Switch model"));
    }
}