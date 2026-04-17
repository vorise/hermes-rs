/// Slash command completer for Tab-based autocomplete.
///
/// Provides fuzzy-matching suggestions for commands as the user types.

/// A single completion suggestion.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub display: String,
    pub description: String,
}

/// Completer for slash commands.
pub struct Completer {
    /// Available command names.
    commands: Vec<String>,
    /// Command descriptions keyed by name.
    descriptions: Vec<(String, String)>,
}

impl Completer {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            descriptions: Vec::new(),
        }
    }

    /// Register a command name and description.
    pub fn register(&mut self, name: &str, description: &str, aliases: &[&str]) {
        self.commands.push(name.to_string());
        for alias in aliases {
            self.commands.push(alias.to_string());
        }
        self.descriptions
            .push((name.to_string(), description.to_string()));
    }

    /// Get suggestions matching the given prefix.
    pub fn suggest(&self, prefix: &str, max: usize) -> Vec<Suggestion> {
        if prefix.is_empty() {
            return self
                .descriptions
                .iter()
                .take(max)
                .map(|(name, desc)| Suggestion {
                    display: format!("/{name}"),
                    description: desc.clone(),
                })
                .collect();
        }

        let prefix_lower = prefix.to_lowercase();
        let mut matches: Vec<(usize, Suggestion)> = self
            .descriptions
            .iter()
            .filter_map(|(name, desc)| {
                let name_lower = name.to_lowercase();
                // Exact prefix match (highest priority)
                if name_lower.starts_with(&prefix_lower) {
                    return Some((
                        0,
                        Suggestion {
                            display: format!("/{name}"),
                            description: desc.clone(),
                        },
                    ));
                }
                // Substring match
                if name_lower.contains(&prefix_lower) {
                    return Some((
                        1,
                        Suggestion {
                            display: format!("/{name}"),
                            description: desc.clone(),
                        },
                    ));
                }
                None
            })
            .collect();

        // Sort by priority (prefix first, then substring)
        matches.sort_by_key(|(priority, _)| *priority);
        matches
            .into_iter()
            .take(max)
            .map(|(_, s)| s)
            .collect()
    }

    /// Get the best single completion for a prefix.
    pub fn complete(&self, prefix: &str) -> Option<String> {
        let prefix_lower = prefix.to_lowercase();
        // Find commands that start with the prefix
        let matches: Vec<&str> = self
            .commands
            .iter()
            .filter(|cmd| cmd.to_lowercase().starts_with(&prefix_lower))
            .map(|s| s.as_str())
            .collect();

        if matches.len() == 1 {
            // Only one match — return it
            Some(matches[0].to_string())
        } else if matches.is_empty() {
            None
        } else {
            // Multiple matches — find the longest common prefix
            let common = longest_common_prefix(&matches);
            if common.len() > prefix_lower.len() {
                Some(common)
            } else {
                // Can't disambiguate further
                None
            }
        }
    }

    /// Format suggestions as a display string for the TUI.
    pub fn format_suggestions(&self, prefix: &str, max: usize) -> String {
        let suggestions = self.suggest(prefix, max);
        if suggestions.is_empty() {
            return String::new();
        }

        let mut result = String::from("\n");
        for s in &suggestions {
            result.push_str(&format!("  {:<20} {}\n", s.display, s.description));
        }
        result
    }
}

impl Default for Completer {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a completer from command data.
pub fn build_completer(
    commands: &[(&str, &str, &[&str])], // (name, description, aliases)
) -> Completer {
    let mut completer = Completer::new();
    for (name, desc, aliases) in commands {
        completer.register(name, desc, aliases);
    }
    completer
}

/// Find the longest common prefix of a set of strings.
fn longest_common_prefix(strs: &[&str]) -> String {
    if strs.is_empty() {
        return String::new();
    }
    if strs.len() == 1 {
        return strs[0].to_string();
    }

    let first = strs[0];
    let mut end = 0;

    while end < first.len() {
        let ch = first.as_bytes()[end];
        for s in &strs[1..] {
            if end >= s.len() || s.as_bytes()[end] != ch {
                return first[..end].to_string();
            }
        }
        end += 1;
    }

    first.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_completer() -> Completer {
        let mut c = Completer::new();
        c.register("help", "Show help", &["h"]);
        c.register("model", "Switch model", &[]);
        c.register("memory", "View memories", &[]);
        c.register("new", "New session", &["reset", "clear"]);
        c.register("compress", "Compress context", &[]);
        c
    }

    #[test]
    fn test_suggest_empty() {
        let c = Completer::new();
        assert!(c.suggest("h", 5).is_empty());
    }

    #[test]
    fn test_suggest_prefix() {
        let c = test_completer();
        let suggestions = c.suggest("h", 5);
        assert_eq!(suggestions.len(), 1); // help
        assert!(suggestions[0].display == "/help");
    }

    #[test]
    fn test_suggest_all() {
        let c = test_completer();
        let suggestions = c.suggest("", 10);
        assert!(!suggestions.is_empty());
    }

    #[test]
    fn test_complete_single_match() {
        let c = test_completer();
        let result = c.complete("comp");
        assert_eq!(result, Some("compress".to_string()));
    }

    #[test]
    fn test_complete_no_match() {
        let c = test_completer();
        let result = c.complete("xyz");
        assert_eq!(result, None);
    }

    #[test]
    fn test_complete_multiple_matches() {
        let c = test_completer();
        // "m" matches both "model" and "memory"
        let result = c.complete("m");
        // Both start with "m", so longest common prefix is just "m"
        // which is not longer than the prefix, so None
        assert_eq!(result, None);
    }

    #[test]
    fn test_format_suggestions() {
        let c = test_completer();
        let formatted = c.format_suggestions("h", 5);
        assert!(formatted.contains("/help"));
    }

    #[test]
    fn test_longest_common_prefix() {
        assert_eq!(longest_common_prefix(&["hello", "help"]), "hel");
        assert_eq!(longest_common_prefix(&["a", "ab", "abc"]), "a");
        assert_eq!(longest_common_prefix(&["test"]), "test");
        assert_eq!(longest_common_prefix(&[]), "");
    }
}
