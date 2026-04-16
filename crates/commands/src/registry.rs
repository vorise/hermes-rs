//! Command Registry
//!
//! Registry for slash commands with fuzzy matching and dispatch.

use anyhow::{Result, Context};
use crate::commands::{SlashCommand, CommandContext, CommandResult, CommandHelp, parse_command_input};
use std::collections::HashMap;

/// Command registry.
pub struct CommandRegistry {
    /// Registered commands by name.
    commands: HashMap<String, Box<dyn SlashCommand>>,

    /// Aliases map to command names.
    aliases: HashMap<String, String>,

    /// Categories for grouping.
    categories: HashMap<String, Vec<String>>,
}

impl CommandRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            aliases: HashMap::new(),
            categories: HashMap::new(),
        }
    }

    /// Create registry with all built-in commands.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        builtin::register_builtin_commands(&mut registry);
        registry
    }

    /// Register a command.
    pub fn register(&mut self, command: Box<dyn SlashCommand>) {
        let name = command.name().to_lowercase();

        // Register aliases
        for alias in command.aliases() {
            self.aliases.insert(alias.to_lowercase(), name.clone());
        }

        // Register by category
        let category = command.category().to_string();
        self.categories
            .entry(category)
            .or_default()
            .push(name.clone());

        // Store command
        self.commands.insert(name, command);
    }

    /// Get a command by name or alias.
    pub fn get(&self, name: &str) -> Option<&dyn SlashCommand> {
        let name_lower = name.to_lowercase();

        // Try direct lookup
        if let Some(cmd) = self.commands.get(&name_lower) {
            return Some(cmd.as_ref());
        }

        // Try alias lookup
        if let Some(real_name) = self.aliases.get(&name_lower) {
            return self.commands.get(real_name).map(|c| c.as_ref());
        }

        None
    }

    /// Check if a command exists.
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Get all command names.
    pub fn command_names(&self) -> Vec<&str> {
        self.commands.keys().map(|s| s.as_str()).collect()
    }

    /// Get commands by category.
    pub fn by_category(&self, category: &str) -> Vec<&dyn SlashCommand> {
        self.categories
            .get(category)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|n| self.commands.get(n).map(|c| c.as_ref()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all categories.
    pub fn categories(&self) -> Vec<&str> {
        self.categories.keys().map(|s| s.as_str()).collect()
    }

    /// Get command count.
    pub fn count(&self) -> usize {
        self.commands.len()
    }

    /// Dispatch a command from input string.
    ///
    /// Parses the input and executes the matching command.
    pub async fn dispatch(
        &self,
        input: &str,
        ctx: &CommandContext,
    ) -> Result<CommandResult> {
        let (name, args) = parse_command_input(input)
            .context("Invalid command format - must start with /")?;

        let command = self.get(&name)
            .with_context(|| format!("Unknown command: /{}", name))?;

        command.execute(&args, ctx).await
    }

    /// Fuzzy match commands.
    ///
    /// Returns commands that contain the query characters in order.
    pub fn fuzzy_match(&self, query: &str) -> Vec<String> {
        let query_lower = query.to_lowercase();
        let chars: Vec<char> = query_lower.chars().collect();

        let mut matches: Vec<String> = self.commands.keys()
            .filter(|name| {
                let name_lower = name.to_lowercase();
                let mut name_chars = name_lower.chars().peekable();

                // Check if all query chars appear in order
                for c in &chars {
                    let found = loop {
                        match name_chars.next() {
                            Some(nc) if nc == *c => break true,
                            Some(_) => continue,
                            None => break false,
                        }
                    };
                    if !found {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        // Also check aliases
        for (alias, real_name) in &self.aliases {
            let alias_lower = alias.to_lowercase();
            let mut alias_chars = alias_lower.chars().peekable();

            let matches_query = chars.iter().all(|c| {
                loop {
                    match alias_chars.next() {
                        Some(ac) if ac == *c => break true,
                        Some(_) => continue,
                        None => break false,
                    }
                }
            });

            if matches_query && !matches.contains(real_name) {
                matches.push(format!("{} (alias for {})", alias, real_name));
            }
        }

        matches.sort();
        matches
    }

    /// Get completions for partial input.
    pub fn completions(&self, partial: &str) -> Vec<String> {
        if !partial.starts_with('/') {
            return Vec::new();
        }

        let name = partial[1..].to_lowercase();

        // Find exact and fuzzy matches
        let mut completions: Vec<String> = Vec::new();

        // Exact prefix matches
        for cmd_name in self.commands.keys() {
            if cmd_name.starts_with(&name) {
                completions.push(format!("/{}", cmd_name));
            }
        }

        // Alias matches
        for alias in self.aliases.keys() {
            if alias.starts_with(&name) {
                completions.push(format!("/{}", alias));
            }
        }

        completions.sort();
        completions.dedup();
        completions
    }

    /// Get help for a specific command.
    pub fn help(&self, name: &str) -> Option<CommandHelp> {
        self.get(name).map(|c| CommandHelp::from_command(c))
    }

    /// Get all help text.
    pub fn help_all(&self) -> String {
        let mut lines: Vec<String> = Vec::new();

        // Group by category
        for category in self.categories.keys() {
            lines.push(format!("\n[{}]", category));

            let commands = self.by_category(category);
            for cmd in commands {
                let help = CommandHelp::from_command(cmd);
                lines.push(format!("  {}", help.format()));
            }
        }

        lines.join("\n")
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

// ============================================================================
// Builtin Command Registration
// ============================================================================

mod builtin {
    use crate::commands::{SlashCommand, CommandContext, CommandResult};
    use async_trait::async_trait;
    use anyhow::Result;

    // P0 Commands

    /// /new, /reset - Clear session
    pub struct NewCommand;

    #[async_trait]
    impl SlashCommand for NewCommand {
        fn name(&self) -> &str { "new" }
        fn aliases(&self) -> Vec<&str> { vec!["reset", "clear"] }
        fn description(&self) -> &str { "Start a new session, clearing all messages" }
        fn category(&self) -> &str { "session" }

        async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
            Ok(CommandResult::ClearSession)
        }
    }

    /// /model - Switch model
    pub struct ModelCommand;

    #[async_trait]
    impl SlashCommand for ModelCommand {
        fn name(&self) -> &str { "model" }
        fn aliases(&self) -> Vec<&str> { vec!["m"] }
        fn description(&self) -> &str { "Switch to a different model (provider:model format)" }
        fn category(&self) -> &str { "config" }

        async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
            if args.is_empty() {
                return Ok(CommandResult::Message(format!(
                    "Current model: {}\nUsage: /model provider:model (e.g., /model openai:gpt-4)",
                    ctx.model
                )));
            }

            let model_ref = h_core::ModelRef::parse(args)
                .ok_or_else(|| anyhow::anyhow!("Invalid model format. Use provider:model"))?;

            Ok(CommandResult::SwitchModel(model_ref))
        }
    }

    /// /compress - Trigger context compression
    pub struct CompressCommand;

    #[async_trait]
    impl SlashCommand for CompressCommand {
        fn name(&self) -> &str { "compress" }
        fn aliases(&self) -> Vec<&str> { vec!["ctx", "context"] }
        fn description(&self) -> &str { "Trigger context compression manually" }
        fn category(&self) -> &str { "session" }

        async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
            Ok(CommandResult::CompressContext)
        }
    }

    /// /stop - Interrupt current work
    pub struct StopCommand;

    #[async_trait]
    impl SlashCommand for StopCommand {
        fn name(&self) -> &str { "stop" }
        fn aliases(&self) -> Vec<&str> { vec!["interrupt", "cancel"] }
        fn description(&self) -> &str { "Stop/interrupt current work" }
        fn category(&self) -> &str { "control" }

        async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
            Ok(CommandResult::Interrupt)
        }
    }

    // P1 Commands

    /// /usage - Show token/cost usage
    pub struct UsageCommand;

    #[async_trait]
    impl SlashCommand for UsageCommand {
        fn name(&self) -> &str { "usage" }
        fn aliases(&self) -> Vec<&str> { vec!["cost", "tokens"] }
        fn description(&self) -> &str { "Show token usage and estimated cost" }
        fn category(&self) -> &str { "info" }

        async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
            let cost = &ctx.cost;
            Ok(CommandResult::Message(format!(
                "Token Usage:\n  Input: {}\n  Output: {}\n  Cache Read: {}\n  Cache Write: {}\n  Reasoning: {}\n  Total: {}\n\nCost: ${:.4}\nAPI Calls: {}",
                cost.input_tokens,
                cost.output_tokens,
                cost.cache_read_tokens,
                cost.cache_write_tokens,
                cost.reasoning_tokens,
                cost.total_tokens(),
                cost.estimated_cost_usd,
                cost.api_call_count
            ).to_string()))
        }
    }

    /// /undo - Undo last turn
    pub struct UndoCommand;

    #[async_trait]
    impl SlashCommand for UndoCommand {
        fn name(&self) -> &str { "undo" }
        fn aliases(&self) -> Vec<&str> { vec![] }
        fn description(&self) -> &str { "Undo the last conversation turn" }
        fn category(&self) -> &str { "session" }

        async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
            Ok(CommandResult::Undo)
        }
    }

    /// /retry - Retry last turn
    pub struct RetryCommand;

    #[async_trait]
    impl SlashCommand for RetryCommand {
        fn name(&self) -> &str { "retry" }
        fn aliases(&self) -> Vec<&str> { vec![] }
        fn description(&self) -> &str { "Retry the last turn with a different response" }
        fn category(&self) -> &str { "session" }

        async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
            Ok(CommandResult::Retry)
        }
    }

    /// /tools - List/enable/disable tools
    pub struct ToolsCommand;

    #[async_trait]
    impl SlashCommand for ToolsCommand {
        fn name(&self) -> &str { "tools" }
        fn aliases(&self) -> Vec<&str> { vec!["toolsets"] }
        fn description(&self) -> &str { "List or manage tools/toolsets" }
        fn category(&self) -> &str { "config" }

        async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandResult> {
            if args.is_empty() {
                let enabled = ctx.config.enabled_toolsets.as_ref()
                    .map(|t| t.join(", "))
                    .unwrap_or_else(|| "all".to_string());
                let disabled = ctx.config.disabled_toolsets.as_ref()
                    .map(|t| t.join(", "))
                    .unwrap_or_else(|| "none".to_string());

                return Ok(CommandResult::Message(format!(
                    "Tools Configuration:\n  Enabled: {}\n  Disabled: {}\n\nUsage: /tools [enable|disable] <toolset>",
                    enabled, disabled
                )));
            }

            // TODO: Implement tool enable/disable
            Ok(CommandResult::Message(format!("Tool management: {}", args)))
        }
    }

    /// /status - Show session status
    pub struct StatusCommand;

    #[async_trait]
    impl SlashCommand for StatusCommand {
        fn name(&self) -> &str { "status" }
        fn aliases(&self) -> Vec<&str> { vec!["info"] }
        fn description(&self) -> &str { "Show current session status" }
        fn category(&self) -> &str { "info" }

        async fn execute(&self, _args: &str, ctx: &CommandContext) -> Result<CommandResult> {
            Ok(CommandResult::Message(format!(
                "Session Status:\n  ID: {}\n  Model: {}\n  Messages: {}\n  Iterations: {}\n  Tokens: {}\n  Cost: ${:.4}\n  Working Dir: {}",
                ctx.session_id,
                ctx.model,
                ctx.message_count(),
                ctx.iteration,
                ctx.total_tokens(),
                ctx.estimated_cost(),
                ctx.working_dir.display()
            )))
        }
    }

    /// /help - Show help
    pub struct HelpCommand;

    #[async_trait]
    impl SlashCommand for HelpCommand {
        fn name(&self) -> &str { "help" }
        fn aliases(&self) -> Vec<&str> { vec!["h", "?"] }
        fn description(&self) -> &str { "Show help for commands" }
        fn category(&self) -> &str { "general" }

        async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
            // Get registry from context (placeholder - would need proper injection)
            // For now, show generic help
            if args.is_empty() {
                return Ok(CommandResult::Message(
                    "Hermes Slash Commands:\n\n\
                     Session: /new, /reset, /undo, /retry, /compress\n\
                     Config: /model, /tools, /personality\n\
                     Info: /usage, /status, /help\n\
                     Control: /stop, /exit\n\n\
                     Use /help <command> for details.".to_string()
                ));
            }

            // Would dispatch to registry.help(args)
            Ok(CommandResult::Message(format!("Help for: {}", args)))
        }
    }

    /// /exit - Exit application
    pub struct ExitCommand;

    #[async_trait]
    impl SlashCommand for ExitCommand {
        fn name(&self) -> &str { "exit" }
        fn aliases(&self) -> Vec<&str> { vec!["quit", "q"] }
        fn description(&self) -> &str { "Exit Hermes" }
        fn category(&self) -> &str { "general" }

        async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandResult> {
            Ok(CommandResult::Exit)
        }
    }

    /// Register all builtin commands.
    pub fn register_builtin_commands(registry: &mut crate::CommandRegistry) {
        // P0 commands
        registry.register(Box::new(NewCommand));
        registry.register(Box::new(ModelCommand));
        registry.register(Box::new(CompressCommand));
        registry.register(Box::new(StopCommand));

        // P1 commands
        registry.register(Box::new(UsageCommand));
        registry.register(Box::new(UndoCommand));
        registry.register(Box::new(RetryCommand));
        registry.register(Box::new(ToolsCommand));
        registry.register(Box::new(StatusCommand));
        registry.register(Box::new(HelpCommand));
        registry.register(Box::new(ExitCommand));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use h_core::HermesConfig;

    fn make_context() -> CommandContext {
        CommandContext::new(
            "test-session",
            Arc::new(HermesConfig::default()),
            Vec::new(),
        )
    }

    #[test]
    fn test_registry_new() {
        let registry = CommandRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_registry_with_builtins() {
        let registry = CommandRegistry::with_builtins();
        assert!(registry.count() >= 10);
    }

    #[test]
    fn test_get_command() {
        let registry = CommandRegistry::with_builtins();

        assert!(registry.get("new").is_some());
        assert!(registry.get("model").is_some());
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn test_aliases() {
        let registry = CommandRegistry::with_builtins();

        // "reset" is alias for "new"
        assert!(registry.get("reset").is_some());
        assert_eq!(registry.get("reset").unwrap().name(), "new");

        // "q" is alias for "exit"
        assert!(registry.get("q").is_some());
        assert_eq!(registry.get("q").unwrap().name(), "exit");
    }

    #[test]
    fn test_completions() {
        let registry = CommandRegistry::with_builtins();

        let completions = registry.completions("/m");
        assert!(completions.contains(&"/model".to_string()));
        assert!(completions.contains(&"/m".to_string())); // alias

        let completions = registry.completions("/n");
        assert!(completions.contains(&"/new".to_string()));
    }

    #[test]
    fn test_fuzzy_match() {
        let registry = CommandRegistry::with_builtins();

        let matches = registry.fuzzy_match("mdl");
        assert!(matches.contains(&"model".to_string()));
    }

    #[test]
    fn test_categories() {
        let registry = CommandRegistry::with_builtins();

        let categories = registry.categories();
        assert!(categories.contains(&"session"));
        assert!(categories.contains(&"config"));
        assert!(categories.contains(&"info"));

        let session_cmds = registry.by_category("session");
        assert!(session_cmds.len() >= 3);
    }

    #[tokio::test]
    async fn test_dispatch() {
        let registry = CommandRegistry::with_builtins();
        let ctx = make_context();

        let result = registry.dispatch("/new", &ctx).await.unwrap();
        assert_eq!(result, CommandResult::ClearSession);

        let result = registry.dispatch("/exit", &ctx).await.unwrap();
        assert_eq!(result, CommandResult::Exit);
    }

    #[tokio::test]
    async fn test_dispatch_with_args() {
        let registry = CommandRegistry::with_builtins();
        let ctx = make_context();

        let result = registry.dispatch("/model openai:gpt-4", &ctx).await.unwrap();
        match result {
            CommandResult::SwitchModel(model_ref) => {
                assert_eq!(model_ref.provider.as_str(), "openai");
                assert_eq!(model_ref.model.as_str(), "gpt-4");
            }
            _ => panic!("Expected SwitchModel result"),
        }
    }

    #[tokio::test]
    async fn test_dispatch_unknown() {
        let registry = CommandRegistry::with_builtins();
        let ctx = make_context();

        let result = registry.dispatch("/unknown", &ctx).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_help() {
        let registry = CommandRegistry::with_builtins();

        let help = registry.help("model").unwrap();
        assert_eq!(help.name, "model");
        assert!(help.description.contains("Switch"));
    }
}