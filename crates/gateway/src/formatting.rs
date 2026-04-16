use crate::base::MessageFormat;

/// Format a message for a specific platform.
///
/// Converts the internal markdown representation to the platform's
/// supported format.
pub fn format_message(text: &str, format: MessageFormat) -> String {
    match format {
        MessageFormat::Plain => strip_markdown(text),
        MessageFormat::Markdown => text.to_string(),
        MessageFormat::Html => markdown_to_html(text),
        MessageFormat::Mrkdwn => markdown_to_mrkdwn(text),
    }
}

/// Strip all markdown formatting, returning plain text.
pub fn strip_markdown(text: &str) -> String {
    let mut result = text.to_string();

    // Remove bold
    result = result.replace("**", "").replace("__", "");
    // Remove italic
    result = result.replace("*", "").replace("_", "");
    // Remove code blocks
    result = result.replace("```", "").replace("`", "");
    // Remove links but keep text
    while let Some(start) = result.find('[') {
        if let Some(end_bracket) = result[start..].find(']') {
            let end_paren = result[start + end_bracket..].find("](");
            if let Some(paren_offset) = end_paren {
                let link_end = result[start + end_bracket + paren_offset..].find(')');
                if let Some(link_end_offset) = link_end {
                    let full_end = start + end_bracket + paren_offset + link_end_offset;
                    let link_text = result[start + 1..start + end_bracket].to_string();
                    result.replace_range(start..=full_end, &link_text);
                } else {
                    break;
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }
    // Remove headings
    result = result
        .lines()
        .map(|line| line.trim_start_matches('#').trim_start().to_string())
        .collect::<Vec<_>>()
        .join("\n");

    result
}

/// Convert markdown to HTML.
fn markdown_to_html(text: &str) -> String {
    let mut html = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            html.push_str("<br>\n");
            continue;
        }

        // Headings
        if let Some(heading) = trimmed.strip_prefix("### ") {
            html.push_str(&format!("<h3>{}</h3>\n", format_inline(heading)));
        } else if let Some(heading) = trimmed.strip_prefix("## ") {
            html.push_str(&format!("<h2>{}</h2>\n", format_inline(heading)));
        } else if let Some(heading) = trimmed.strip_prefix("# ") {
            html.push_str(&format!("<h1>{}</h1>\n", format_inline(heading)));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let content = trimmed.chars().skip(2).collect::<String>();
            html.push_str(&format!("<li>{}</li>\n", format_inline(&content)));
        } else {
            html.push_str(&format!("<p>{}</p>\n", format_inline(trimmed)));
        }
    }

    html
}

/// Format inline markdown elements as HTML.
fn format_inline(text: &str) -> String {
    let mut result = text.to_string();

    // Bold
    while let Some(start) = result.find("**") {
        if let Some(end) = result[start + 2..].find("**") {
            let end = start + 2 + end;
            let inner = &result[start + 2..end];
            result.replace_range(start..=end + 1, &format!("<strong>{inner}</strong>"));
        } else {
            break;
        }
    }

    // Italic
    while let Some(start) = result.find('*') {
        if let Some(end) = result[start + 1..].find('*') {
            let end = start + 1 + end;
            let inner = &result[start + 1..end];
            result.replace_range(start..=end, &format!("<em>{inner}</em>"));
        } else {
            break;
        }
    }

    // Code
    while let Some(start) = result.find('`') {
        if let Some(end) = result[start + 1..].find('`') {
            let end = start + 1 + end;
            let inner = &result[start + 1..end];
            result.replace_range(start..=end, &format!("<code>{inner}</code>"));
        } else {
            break;
        }
    }

    result
}

/// Convert markdown to Slack-style mrkdwn.
fn markdown_to_mrkdwn(text: &str) -> String {
    let mut result = text.to_string();

    // Bold: **text** -> *text*
    // Use a placeholder to avoid the italic loop matching the result
    const BOLD_OPEN: &str = "\x00B";
    const BOLD_CLOSE: &str = "\x00/b\x00";
    while let Some(start) = result.find("**") {
        if let Some(end) = result[start + 2..].find("**") {
            let end = start + 2 + end;
            let inner = &result[start + 2..end];
            result.replace_range(start..=end + 1, &format!("{BOLD_OPEN}{inner}{BOLD_CLOSE}"));
        } else {
            break;
        }
    }

    // Italic: *text* -> _text_
    while let Some(start) = result.find('*') {
        if let Some(end) = result[start + 1..].find('*') {
            let end = start + 1 + end;
            let inner = &result[start + 1..end];
            result.replace_range(start..=end, &format!("_{inner}_"));
        } else {
            break;
        }
    }

    // Restore bold markers to actual asterisks
    result = result.replace(BOLD_OPEN, "*").replace(BOLD_CLOSE, "*");

    // Code: `text` -> `text` (same in mrkdwn)
    // Code blocks: ```text``` -> ```text``` (same)

    // Headings: just remove # prefix and wrap in *bold*
    result = result
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("### ") {
                format!("*{}*", &trimmed[4..])
            } else if trimmed.starts_with("## ") {
                format!("*{}*", &trimmed[3..])
            } else if trimmed.starts_with("# ") {
                format!("*{}*", &trimmed[2..])
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    result
}

/// Truncate text to a maximum length, adding ellipsis if truncated.
pub fn truncate(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let mut truncated = text[..max_len.saturating_sub(3)].to_string();
        truncated.push_str("...");
        truncated
    }
}

/// Split a long message into chunks suitable for a platform.
///
/// Most platforms have a message length limit (e.g., Telegram: 4096 chars).
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > max_len {
            if !current.is_empty() {
                chunks.push(current);
            }
            current = word.to_string();
        } else if current.is_empty() {
            current = word.to_string();
        } else {
            current.push(' ');
            current.push_str(word);
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::MessageFormat;

    #[test]
    fn test_strip_markdown() {
        let text = "**Bold** and *italic* and `code`";
        let result = format_message(text, MessageFormat::Plain);
        assert_eq!(result, "Bold and italic and code");
    }

    #[test]
    fn test_format_markdown_passthrough() {
        let text = "**Bold** and *italic*";
        let result = format_message(text, MessageFormat::Markdown);
        assert_eq!(result, text);
    }

    #[test]
    fn test_markdown_to_html() {
        let text = "# Hello\n**Bold** text";
        let result = format_message(text, MessageFormat::Html);
        assert!(result.contains("<h1>Hello</h1>"));
        assert!(result.contains("<p>"));
        assert!(result.contains("<strong>Bold</strong>"));
    }

    #[test]
    fn test_markdown_to_mrkdwn() {
        let text = "**Bold** and *italic*";
        let result = format_message(text, MessageFormat::Mrkdwn);
        assert!(result.contains("*Bold*"));
        assert!(result.contains("_italic_"));
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        let result = truncate("hello world this is long", 10);
        assert_eq!(result.len(), 10);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_split_message() {
        let text = "one two three four five";
        let chunks = split_message(text, 10);
        assert!(chunks.len() >= 3);
        for chunk in &chunks {
            assert!(chunk.len() <= 10);
        }
    }

    #[test]
    fn test_split_message_short() {
        let text = "short";
        let chunks = split_message(text, 100);
        assert_eq!(chunks, vec!["short"]);
    }
}
