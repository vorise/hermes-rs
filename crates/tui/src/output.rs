//! Output Region
//!
//! Scrolling output area for displaying messages.

use std::collections::VecDeque;

/// Output message types.
#[derive(Debug, Clone)]
pub enum OutputMessage {
    /// User message.
    User(String),

    /// Assistant message.
    Assistant(String),

    /// System message.
    System(String),

    /// Tool output.
    Tool { name: String, output: String },

    /// Error message.
    Error(String),

    /// Status/progress message.
    Status(String),

    /// Separator line.
    Separator,
}

/// Output region state.
#[derive(Debug, Clone)]
pub struct OutputRegion {
    /// Messages to display.
    messages: VecDeque<OutputMessage>,

    /// Maximum number of messages to keep.
    max_messages: usize,

    /// Scroll offset (lines from bottom).
    scroll_offset: usize,

    /// Whether auto-scroll is enabled.
    auto_scroll: bool,
}

impl OutputRegion {
    /// Create a new output region.
    pub fn new() -> Self {
        Self {
            messages: VecDeque::new(),
            max_messages: 1000,
            scroll_offset: 0,
            auto_scroll: true,
        }
    }

    /// Set maximum messages.
    pub fn with_max_messages(mut self, max: usize) -> Self {
        self.max_messages = max;
        self
    }

    /// Add a message.
    pub fn push(&mut self, message: OutputMessage) {
        // Check if we need to trim
        if self.messages.len() >= self.max_messages {
            self.messages.pop_front();
        }

        self.messages.push_back(message);

        // Auto-scroll to bottom
        if self.auto_scroll {
            self.scroll_offset = 0;
        }
    }

    /// Add user message.
    pub fn user(&mut self, text: impl Into<String>) {
        self.push(OutputMessage::User(text.into()));
    }

    /// Add assistant message.
    pub fn assistant(&mut self, text: impl Into<String>) {
        self.push(OutputMessage::Assistant(text.into()));
    }

    /// Add system message.
    pub fn system(&mut self, text: impl Into<String>) {
        self.push(OutputMessage::System(text.into()));
    }

    /// Add tool output.
    pub fn tool(&mut self, name: impl Into<String>, output: impl Into<String>) {
        self.push(OutputMessage::Tool {
            name: name.into(),
            output: output.into(),
        });
    }

    /// Add error message.
    pub fn error(&mut self, text: impl Into<String>) {
        self.push(OutputMessage::Error(text.into()));
    }

    /// Add status message.
    pub fn status(&mut self, text: impl Into<String>) {
        self.push(OutputMessage::Status(text.into()));
    }

    /// Add separator.
    pub fn separator(&mut self) {
        self.push(OutputMessage::Separator);
    }

    /// Clear all messages.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
    }

    /// Get all messages.
    pub fn messages(&self) -> &VecDeque<OutputMessage> {
        &self.messages
    }

    /// Get message count.
    pub fn count(&self) -> usize {
        self.messages.len()
    }

    /// Scroll up by n lines.
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset += n;
        self.auto_scroll = false;
    }

    /// Scroll down by n lines.
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        if self.scroll_offset == 0 {
            self.auto_scroll = true;
        }
    }

    /// Scroll to bottom.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    /// Scroll to top.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = self.count();
        self.auto_scroll = false;
    }

    /// Get scroll offset.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Check if auto-scroll is enabled.
    pub fn is_auto_scroll(&self) -> bool {
        self.auto_scroll
    }

    /// Enable/disable auto-scroll.
    pub fn set_auto_scroll(&mut self, enabled: bool) {
        self.auto_scroll = enabled;
        if enabled {
            self.scroll_offset = 0;
        }
    }

    /// Get messages for display (from bottom with scroll offset).
    pub fn get_display_messages(&self, height: usize) -> Vec<&OutputMessage> {
        let total = self.messages.len();
        let skip = if self.scroll_offset > 0 {
            total.saturating_sub(height).saturating_sub(self.scroll_offset)
        } else {
            total.saturating_sub(height)
        };

        self.messages.iter().skip(skip).take(height).collect()
    }

    /// Estimate total lines (for scrolling).
    pub fn estimated_lines(&self) -> usize {
        // Each message is roughly 1-3 lines depending on length
        self.messages.iter().map(|m| {
            match m {
                OutputMessage::User(t) => estimate_lines(t),
                OutputMessage::Assistant(t) => estimate_lines(t),
                OutputMessage::System(t) => estimate_lines(t),
                OutputMessage::Tool { output, .. } => estimate_lines(output) + 1,
                OutputMessage::Error(t) => estimate_lines(t),
                OutputMessage::Status(t) => estimate_lines(t),
                OutputMessage::Separator => 1,
            }
        }).sum()
    }
}

/// Estimate lines for text based on length.
fn estimate_lines(text: &str) -> usize {
    // Assume 80 chars per line
    (text.len() / 80 + 1).max(1)
}

impl Default for OutputRegion {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_new() {
        let output = OutputRegion::new();
        assert_eq!(output.count(), 0);
        assert!(output.is_auto_scroll());
    }

    #[test]
    fn test_push_messages() {
        let mut output = OutputRegion::new();

        output.user("Hello");
        output.assistant("Hi there!");
        output.system("System message");

        assert_eq!(output.count(), 3);
    }

    #[test]
    fn test_tool_message() {
        let mut output = OutputRegion::new();

        output.tool("read_file", "File contents...");

        assert_eq!(output.count(), 1);
    }

    #[test]
    fn test_clear() {
        let mut output = OutputRegion::new();

        output.user("Message 1");
        output.user("Message 2");
        assert_eq!(output.count(), 2);

        output.clear();
        assert_eq!(output.count(), 0);
    }

    #[test]
    fn test_scroll() {
        let mut output = OutputRegion::new();

        // Add many messages
        for i in 0..100 {
            output.user(format!("Message {}", i));
        }

        // Scroll up
        output.scroll_up(10);
        assert_eq!(output.scroll_offset(), 10);
        assert!(!output.is_auto_scroll());

        // Scroll down
        output.scroll_down(5);
        assert_eq!(output.scroll_offset(), 5);

        // Scroll to bottom
        output.scroll_to_bottom();
        assert_eq!(output.scroll_offset(), 0);
        assert!(output.is_auto_scroll());
    }

    #[test]
    fn test_max_messages() {
        let mut output = OutputRegion::new().with_max_messages(10);

        // Add more than max
        for i in 0..20 {
            output.user(format!("Message {}", i));
        }

        assert_eq!(output.count(), 10);
        // First message should be "Message 10"
        let first = output.messages().front().unwrap();
        match first {
            OutputMessage::User(text) => assert!(text.contains("Message 10")),
            _ => panic!("Expected User message"),
        }
    }

    #[test]
    fn test_display_messages() {
        let mut output = OutputRegion::new();

        for i in 0..20 {
            output.user(format!("Message {}", i));
        }

        let display = output.get_display_messages(10);
        assert_eq!(display.len(), 10);
        // Should show messages 10-19
    }
}