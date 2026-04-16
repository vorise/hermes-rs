//! Slash Command Completer
//!
//! Autocomplete for slash commands and tool names.

use std::collections::HashSet;

/// Built-in slash commands for autocomplete.
pub const BUILTIN_COMMANDS: &[&str] = &[
    "/help",
    "/exit",
    "/quit",
    "/clear",
    "/reset",
    "/model",
    "/provider",
    "/tools",
    "/toolsets",
    "/skills",
    "/memory",
    "/config",
    "/save",
    "/load",
    "/sessions",
    "/search",
    "/export",
    "/import",
    "/status",
    "/cost",
    "/version",
    "/debug",
    "/verbose",
    "/quiet",
    "/skin",
    "/banner",
    "/history",
    "/undo",
    "/redo",
];

/// Command completer for autocomplete.
#[derive(Debug, Clone)]
pub struct Completer {
    /// Known commands.
    commands: HashSet<String>,
}

impl Completer {
    /// Create a new completer with built-in commands.
    pub fn new() -> Self {
        Self {
            commands: BUILTIN_COMMANDS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Add a custom command.
    pub fn add_command(&mut self, command: impl Into<String>) {
        self.commands.insert(command.into());
    }

    /// Remove a command.
    pub fn remove_command(&mut self, command: &str) {
        self.commands.remove(command);
    }

    /// Get completions for a partial input.
    ///
    /// Returns matching commands that start with the input.
    pub fn complete(&self, input: &str) -> Vec<String> {
        if input.is_empty() || !input.starts_with('/') {
            return Vec::new();
        }

        // Find matching commands and sort them
        let mut matches: Vec<String> = self.commands
            .iter()
            .filter(|cmd| cmd.starts_with(input))
            .cloned()
            .collect();

        matches.sort();
        matches
    }

    /// Get the best completion (first match).
    pub fn best_completion(&self, input: &str) -> Option<String> {
        self.complete(input).first().cloned()
    }

    /// Check if input is a complete command.
    pub fn is_complete(&self, input: &str) -> bool {
        self.commands.contains(input)
    }

    /// Fuzzy match completions.
    ///
    /// Matches commands that contain the input characters in order,
    /// but not necessarily consecutively.
    pub fn fuzzy_complete(&self, input: &str) -> Vec<String> {
        if input.is_empty() || !input.starts_with('/') {
            return Vec::new();
        }

        let input_lower = input.to_lowercase();
        let chars: Vec<char> = input_lower.chars().collect();

        let mut matches: Vec<String> = self.commands
            .iter()
            .filter(|cmd| {
                let cmd_lower = cmd.to_lowercase();
                let mut cmd_chars = cmd_lower.chars().peekable();

                // Check if all input chars appear in order in command
                for c in &chars {
                    let found = loop {
                        match cmd_chars.next() {
                            Some(cc) if cc == *c => break true,
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

        matches.sort();
        matches
    }

    /// Get all commands.
    pub fn all_commands(&self) -> Vec<String> {
        let mut cmds: Vec<String> = self.commands.iter().cloned().collect();
        cmds.sort();
        cmds
    }

    /// Get command count.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }
}

impl Default for Completer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completer_new() {
        let completer = Completer::new();
        assert!(completer.command_count() > 0);
    }

    #[test]
    fn test_complete_prefix() {
        let completer = Completer::new();

        let completions = completer.complete("/h");
        assert!(completions.contains(&"/help".to_string()));
        assert!(completions.contains(&"/history".to_string()));
    }

    #[test]
    fn test_complete_exact() {
        let completer = Completer::new();

        let completions = completer.complete("/exit");
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0], "/exit");
    }

    #[test]
    fn test_complete_empty() {
        let completer = Completer::new();

        let completions = completer.complete("");
        assert!(completions.is_empty());

        let completions = completer.complete("hello"); // Not a slash command
        assert!(completions.is_empty());
    }

    #[test]
    fn test_best_completion() {
        let completer = Completer::new();

        let best = completer.best_completion("/h");
        assert_eq!(best, Some("/help".to_string())); // Alphabetically first
    }

    #[test]
    fn test_is_complete() {
        let completer = Completer::new();

        assert!(completer.is_complete("/exit"));
        assert!(completer.is_complete("/help"));
        assert!(!completer.is_complete("/unknown"));
        assert!(!completer.is_complete("/h")); // Partial
    }

    #[test]
    fn test_add_remove_command() {
        let mut completer = Completer::new();

        completer.add_command("/custom");
        assert!(completer.is_complete("/custom"));

        completer.remove_command("/custom");
        assert!(!completer.is_complete("/custom"));
    }

    #[test]
    fn test_fuzzy_complete() {
        let completer = Completer::new();

        // Fuzzy match "qt" -> "/quit"
        let completions = completer.fuzzy_complete("/qt");
        assert!(completions.contains(&"/quit".to_string()));
    }
}