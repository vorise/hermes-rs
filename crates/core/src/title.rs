/// Generate a session title from the first user message.
///
/// When an LLM client is available, it can generate a concise title.
/// Without one, falls back to extracting a text snippet.
pub fn generate_title(first_message: &str, max_length: usize) -> String {
    if first_message.is_empty() {
        return "New Session".to_string();
    }

    // Fallback: extract first meaningful line, truncate
    let first_line = first_message
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(first_message);

    let title = first_line.trim();

    if title.len() <= max_length {
        title.to_string()
    } else {
        // Truncate at word boundary
        let truncated = &title[..max_length];
        // Find last space to avoid cutting words
        if let Some(pos) = truncated.rfind(' ') {
            format!("{}...", &truncated[..pos])
        } else {
            format!("{truncated}...")
        }
    }
}

/// Strip common prefixes/suffixes that make poor titles.
pub fn clean_title(title: &str) -> String {
    let t = title.trim();

    // Remove common conversational prefixes
    let lower = t.to_lowercase();
    for prefix in &["hey, ", "hi, ", "hello, ", "hey ", "hi ", "hello "] {
        if lower.starts_with(prefix) {
            let rest = &t[prefix.len()..];
            if !rest.is_empty() {
                let cleaned = rest.strip_suffix('?').unwrap_or(rest);
                return capitalize_first(cleaned.trim());
            }
        }
    }

    // Remove question marks at end for cleaner titles
    let t = t.strip_suffix('?').unwrap_or(t);

    capitalize_first(t)
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        let mut result = String::new();
        result.push(first.to_uppercase().next().unwrap_or(first));
        result.push_str(chars.as_str());
        result
    } else {
        s.to_string()
    }
}

/// Generate a cleaned title from a user message.
pub fn title_from_message(message: &str, max_length: usize) -> String {
    let raw = generate_title(message, max_length + 10); // Extra space for cleaning
    let cleaned = clean_title(&raw);
    // Re-truncate after cleaning
    if cleaned.len() <= max_length {
        cleaned
    } else {
        let truncated = &cleaned[..max_length];
        if let Some(pos) = truncated.rfind(' ') {
            format!("{}...", &truncated[..pos])
        } else {
            format!("{truncated}...")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_title_empty() {
        assert_eq!(generate_title("", 50), "New Session");
    }

    #[test]
    fn test_generate_title_short() {
        assert_eq!(generate_title("Hello world", 50), "Hello world");
    }

    #[test]
    fn test_generate_title_truncate() {
        let long = "This is a very long message that should be truncated because it exceeds the maximum length";
        let title = generate_title(long, 30);
        assert!(title.len() <= 33); // 30 + "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_generate_title_multiline() {
        let msg = "\n\nHello, this is the first line\nSecond line";
        let title = generate_title(msg, 50);
        assert_eq!(title, "Hello, this is the first line");
    }

    #[test]
    fn test_clean_title_remove_hello() {
        assert_eq!(clean_title("hello, can you help me?"), "Can you help me");
        assert_eq!(clean_title("Hi, I need assistance"), "I need assistance");
    }

    #[test]
    fn test_clean_title_remove_question_mark() {
        assert_eq!(clean_title("How does this work?"), "How does this work");
    }

    #[test]
    fn test_clean_title_capitalize() {
        assert_eq!(clean_title("fix the bug in auth"), "Fix the bug in auth");
    }

    #[test]
    fn test_title_from_message_combined() {
        let msg = "hey, can you help me refactor the authentication module?";
        let title = title_from_message(msg, 40);
        assert!(!title.starts_with("hey"));
        assert!(title.contains("efactor"));
    }

    #[test]
    fn test_title_from_message_long() {
        let msg = "I have a really long question about how the authentication system works and whether we can improve the token refresh mechanism";
        let title = title_from_message(msg, 30);
        assert!(title.len() <= 33);
        assert!(title.ends_with("..."));
    }
}
