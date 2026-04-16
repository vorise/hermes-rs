//! Message Formatter
//!
//! Formats messages for different platform rendering styles.

use std::path::Path;

/// Message format type.
#[derive(Debug, Clone, Copy)]
pub enum FormatType {
    /// Plain text.
    Plain,
    /// Markdown.
    Markdown,
    /// HTML.
    Html,
    /// Slack mrkdwn.
    Mrkdwn,
    /// Discord markdown.
    DiscordMarkdown,
    /// Telegram MarkdownV2.
    TelegramMarkdownV2,
}

impl FormatType {
    /// Get format name.
    pub fn name(&self) -> &str {
        match self {
            FormatType::Plain => "plain",
            FormatType::Markdown => "markdown",
            FormatType::Html => "html",
            FormatType::Mrkdwn => "mrkdwn",
            FormatType::DiscordMarkdown => "discord",
            FormatType::TelegramMarkdownV2 => "telegram_v2",
        }
    }
}

/// Message formatter.
pub struct MessageFormatter {
    /// Target format.
    format: FormatType,

    /// Maximum message length.
    max_length: Option<usize>,
}

impl MessageFormatter {
    /// Create new formatter.
    pub fn new(format: FormatType) -> Self {
        Self {
            format,
            max_length: None,
        }
    }

    /// Create with max length.
    pub fn with_max_length(format: FormatType, max_length: usize) -> Self {
        Self {
            format,
            max_length: Some(max_length),
        }
    }

    /// Get format type.
    pub fn format(&self) -> FormatType {
        self.format
    }

    /// Format text for the target platform.
    pub fn format_text(&self, text: &str) -> String {
        let formatted = match self.format {
            FormatType::Plain => self.format_plain(text),
            FormatType::Markdown => self.format_markdown(text),
            FormatType::Html => self.format_html(text),
            FormatType::Mrkdwn => self.format_mrkdwn(text),
            FormatType::DiscordMarkdown => self.format_discord(text),
            FormatType::TelegramMarkdownV2 => self.format_telegram_v2(text),
        };

        // Apply length limit if set
        if let Some(max) = self.max_length {
            if formatted.len() > max {
                // Truncate and add ellipsis
                let truncated = formatted.chars().take(max - 3).collect::<String>();
                format!("{}...", truncated)
            } else {
                formatted
            }
        } else {
            formatted
        }
    }

    /// Format for plain text (no special formatting).
    fn format_plain(&self, text: &str) -> String {
        text.to_string()
    }

    /// Format for standard Markdown.
    fn format_markdown(&self, text: &str) -> String {
        // Standard markdown - minimal escaping needed
        text.to_string()
    }

    /// Format for HTML.
    fn format_html(&self, text: &str) -> String {
        // Escape HTML entities
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    /// Format for Slack mrkdwn.
    fn format_mrkdwn(&self, text: &str) -> String {
        // Slack mrkdwn has limited markdown support
        // Escape < and > which are used for links
        text.replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    /// Format for Discord markdown.
    fn format_discord(&self, text: &str) -> String {
        // Discord supports most standard markdown
        text.to_string()
    }

    /// Format for Telegram MarkdownV2.
    fn format_telegram_v2(&self, text: &str) -> String {
        // Telegram MarkdownV2 requires extensive escaping
        // Must escape: _ * [ ] ( ) ~ > # + - = | { } . ! `
        let mut escaped = String::new();
        for c in text.chars() {
            match c {
                '_' | '*' | '[' | ']' | '(' | ')' | '~' | '>' | '#' | '+' | '-' | '=' | '|' | '{' | '}' | '.' | '!' | '`' => {
                    escaped.push('\\');
                    escaped.push(c);
                }
                _ => escaped.push(c),
            }
        }
        escaped
    }

    /// Format code block.
    pub fn format_code_block(&self, code: &str, language: Option<&str>) -> String {
        match self.format {
            FormatType::Plain => format!("```\n{}\n```", code),
            FormatType::Markdown | FormatType::DiscordMarkdown => {
                if let Some(lang) = language {
                    format!("```{}\n{}\n```", lang, code)
                } else {
                    format!("```\n{}\n```", code)
                }
            }
            FormatType::Html => {
                format!("<pre><code>{}</code></pre>", self.format_html(code))
            }
            FormatType::Mrkdwn => {
                format!("```\n{}\n```", code)
            }
            FormatType::TelegramMarkdownV2 => {
                // Need to escape inside code block
                let escaped = code.replace('\\', "\\\\").replace('`', "\\`");
                format!("```\n{}\n```", escaped)
            }
        }
    }

    /// Format inline code.
    pub fn format_inline_code(&self, code: &str) -> String {
        match self.format {
            FormatType::Plain => code.to_string(),
            FormatType::Markdown | FormatType::DiscordMarkdown | FormatType::Mrkdwn => {
                format!("`{}`", code)
            }
            FormatType::Html => {
                format!("<code>{}</code>", self.format_html(code))
            }
            FormatType::TelegramMarkdownV2 => {
                format!("`{}`", self.format_telegram_v2(code))
            }
        }
    }

    /// Format bold text.
    pub fn format_bold(&self, text: &str) -> String {
        match self.format {
            FormatType::Plain => text.to_string(),
            FormatType::Markdown | FormatType::DiscordMarkdown | FormatType::Mrkdwn => {
                format!("**{}**", text)
            }
            FormatType::Html => {
                format!("<b>{}</b>", self.format_html(text))
            }
            FormatType::TelegramMarkdownV2 => {
                format!("*{}*", self.format_telegram_v2(text))
            }
        }
    }

    /// Format italic text.
    pub fn format_italic(&self, text: &str) -> String {
        match self.format {
            FormatType::Plain => text.to_string(),
            FormatType::Markdown | FormatType::DiscordMarkdown => {
                format!("*{}*", text)
            }
            FormatType::Html => {
                format!("<i>{}</i>", self.format_html(text))
            }
            FormatType::Mrkdwn => {
                format!("_{}_", text)
            }
            FormatType::TelegramMarkdownV2 => {
                format!("_{}_", self.format_telegram_v2(text))
            }
        }
    }

    /// Format link.
    pub fn format_link(&self, text: &str, url: &str) -> String {
        match self.format {
            FormatType::Plain => format!("{} ({})", text, url),
            FormatType::Markdown | FormatType::DiscordMarkdown => {
                format!("[{}]({})", text, url)
            }
            FormatType::Html => {
                format!("<a href=\"{}\">{}</a>", url, self.format_html(text))
            }
            FormatType::Mrkdwn => {
                format!("<{}|{}>", url, text)
            }
            FormatType::TelegramMarkdownV2 => {
                format!("[{}]({})", self.format_telegram_v2(text), url)
            }
        }
    }

    /// Format file mention.
    pub fn format_file(&self, path: &Path, name: Option<&str>) -> String {
        let name = name.unwrap_or_else(|| path.file_name().and_then(|n| n.to_str()).unwrap_or("file"));
        match self.format {
            FormatType::Plain => format!("File: {}", name),
            FormatType::Markdown | FormatType::DiscordMarkdown => {
                format!("📎 **{}**", name)
            }
            FormatType::Html => {
                format!("📎 <b>{}</b>", self.format_html(name))
            }
            FormatType::Mrkdwn => {
                format!("📎 *{}*", name)
            }
            FormatType::TelegramMarkdownV2 => {
                format!("📎 *{}*", self.format_telegram_v2(name))
            }
        }
    }

    /// Split long message into chunks.
    pub fn split_message(&self, text: &str) -> Vec<String> {
        if let Some(max) = self.max_length {
            if text.len() <= max {
                return vec![text.to_string()];
            }

            // Split by paragraphs or sentences
            let mut chunks = Vec::new();
            let mut current = String::new();

            for paragraph in text.split("\n\n") {
                if current.len() + paragraph.len() + 2 > max {
                    if !current.is_empty() {
                        chunks.push(current.clone());
                        current.clear();
                    }

                    // If single paragraph is too long, split by sentences
                    if paragraph.len() > max {
                        for sentence in paragraph.split('.') {
                            if current.len() + sentence.len() + 1 > max {
                                if !current.is_empty() {
                                    chunks.push(current.clone());
                                    current.clear();
                                }
                            }
                            if !current.is_empty() {
                                current.push('.');
                            }
                            current.push_str(sentence);

                            // If still too long without delimiters, force split by words
                            if current.len() > max {
                                // Force split at max length
                                while current.len() > max {
                                    let split_point = max.min(current.len());
                                    chunks.push(current.chars().take(split_point).collect());
                                    current = current.chars().skip(split_point).collect();
                                }
                            }
                        }
                    } else {
                        current.push_str(paragraph);
                    }
                } else {
                    if !current.is_empty() {
                        current.push_str("\n\n");
                    }
                    current.push_str(paragraph);
                }
            }

            if !current.is_empty() {
                chunks.push(current);
            }

            chunks
        } else {
            vec![text.to_string()]
        }
    }
}

impl Default for MessageFormatter {
    fn default() -> Self {
        Self::new(FormatType::Markdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_plain() {
        let formatter = MessageFormatter::new(FormatType::Plain);
        let result = formatter.format_text("Hello **world**");
        assert_eq!(result, "Hello **world**");
    }

    #[test]
    fn test_format_html() {
        let formatter = MessageFormatter::new(FormatType::Html);
        let result = formatter.format_text("<script>alert('xss')</script>");
        assert!(result.contains("&lt;"));
        assert!(result.contains("&gt;"));
    }

    #[test]
    fn test_format_markdown() {
        let formatter = MessageFormatter::new(FormatType::Markdown);
        let result = formatter.format_text("**bold**");
        assert_eq!(result, "**bold**");
    }

    #[test]
    fn test_format_telegram_v2() {
        let formatter = MessageFormatter::new(FormatType::TelegramMarkdownV2);
        let result = formatter.format_text("Hello *world*");
        assert!(result.contains('\\'));
    }

    #[test]
    fn test_format_bold() {
        let formatter = MessageFormatter::new(FormatType::Markdown);
        let result = formatter.format_bold("test");
        assert_eq!(result, "**test**");
    }

    #[test]
    fn test_format_code_block() {
        let formatter = MessageFormatter::new(FormatType::Markdown);
        let result = formatter.format_code_block("code", Some("rust"));
        assert!(result.contains("```rust"));
    }

    #[test]
    fn test_format_link() {
        let formatter = MessageFormatter::new(FormatType::Markdown);
        let result = formatter.format_link("link", "http://example.com");
        assert_eq!(result, "[link](http://example.com)");
    }

    #[test]
    fn test_split_message() {
        let formatter = MessageFormatter::with_max_length(FormatType::Plain, 50);
        let long = "This is a very long message that needs to be split into multiple chunks because it exceeds the maximum length.";
        let chunks = formatter.split_message(long);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.len() <= 50);
        }
    }

    #[test]
    fn test_max_length_truncate() {
        let formatter = MessageFormatter::with_max_length(FormatType::Plain, 10);
        let result = formatter.format_text("This is a long message");
        assert!(result.len() <= 10);
        assert!(result.ends_with("..."));
    }
}