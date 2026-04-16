//! Input Area
//!
//! Fixed input area with multiline support and key bindings.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Input area state.
#[derive(Debug, Clone)]
pub struct InputArea {
    /// Current input text.
    text: String,

    /// Cursor position (byte offset in text).
    cursor: usize,

    /// Input history.
    history: Vec<String>,

    /// Current history index (for navigation).
    history_index: Option<usize>,

    /// Maximum history size.
    max_history: usize,

    /// Multiline mode (Shift+Enter adds newline).
    multiline: bool,

    /// Pending input (ready to submit).
    pending_input: Option<String>,
}

impl InputArea {
    /// Create a new input area.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            max_history: 100,
            multiline: false,
            pending_input: None,
        }
    }

    /// Set multiline mode.
    pub fn set_multiline(&mut self, multiline: bool) {
        self.multiline = multiline;
    }

    /// Get current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Get cursor position.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Check if there's pending input ready to submit.
    pub fn has_pending_input(&self) -> bool {
        self.pending_input.is_some()
    }

    /// Take pending input (clears it).
    pub fn take_pending_input(&mut self) -> Option<String> {
        self.pending_input.take()
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) && self.multiline {
                    // Shift+Enter: add newline
                    self.insert_char('\n');
                } else {
                    // Enter: submit
                    self.submit();
                }
                true
            }
            KeyCode::Char(c) => {
                self.insert_char(c);
                true
            }
            KeyCode::Backspace => {
                self.delete_before_cursor();
                true
            }
            KeyCode::Delete => {
                self.delete_at_cursor();
                true
            }
            KeyCode::Left => {
                self.move_cursor_left();
                true
            }
            KeyCode::Right => {
                self.move_cursor_right();
                true
            }
            KeyCode::Up => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    // Alt+Up: move to previous line in multiline
                    self.move_line_up();
                } else {
                    // Up: history navigation
                    self.history_prev();
                }
                true
            }
            KeyCode::Down => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    // Alt+Down: move to next line in multiline
                    self.move_line_down();
                } else {
                    // Down: history navigation
                    self.history_next();
                }
                true
            }
            KeyCode::Home => {
                self.move_cursor_to_start();
                true
            }
            KeyCode::End => {
                self.move_cursor_to_end();
                true
            }
            KeyCode::Esc => {
                // Clear input
                self.clear();
                true
            }
            _ => false,
        }
    }

    /// Insert a character at cursor position.
    fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Delete character before cursor.
    fn delete_before_cursor(&mut self) {
        if self.cursor > 0 {
            // Find the character before cursor
            let prev_char_len = self.text[..self.cursor]
                .chars()
                .rev()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);

            self.text.remove(self.cursor - prev_char_len);
            self.cursor -= prev_char_len;
        }
    }

    /// Delete character at cursor.
    fn delete_at_cursor(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    /// Move cursor left.
    fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            let prev_char_len = self.text[..self.cursor]
                .chars()
                .rev()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor -= prev_char_len;
        }
    }

    /// Move cursor right.
    fn move_cursor_right(&mut self) {
        if self.cursor < self.text.len() {
            let next_char_len = self.text[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor += next_char_len;
        }
    }

    /// Move to previous line (multiline).
    fn move_line_up(&mut self) {
        // Find current line start
        let current_line_start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);

        if current_line_start > 0 {
            // Find previous line start
            let prev_line_start = self.text[..current_line_start - 1]
                .rfind('\n')
                .map(|i| i + 1)
                .unwrap_or(0);

            // Move cursor to same column in previous line
            let col = self.cursor - current_line_start;
            let prev_line_end = self.text[prev_line_start..]
                .find('\n')
                .map(|i| prev_line_start + i)
                .unwrap_or(self.text.len());

            self.cursor = prev_line_start + std::cmp::min(col, prev_line_end - prev_line_start);
        }
    }

    /// Move to next line (multiline).
    fn move_line_down(&mut self) {
        // Find current line end
        let current_line_end = self.text[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.text.len());

        if current_line_end < self.text.len() {
            // Find current line start
            let current_line_start = self.text[..self.cursor]
                .rfind('\n')
                .map(|i| i + 1)
                .unwrap_or(0);

            let col = self.cursor - current_line_start;

            // Move to start of next line
            let next_line_start = current_line_end + 1;
            let next_line_end = self.text[next_line_start..]
                .find('\n')
                .map(|i| next_line_start + i)
                .unwrap_or(self.text.len());

            self.cursor = next_line_start + std::cmp::min(col, next_line_end - next_line_start);
        }
    }

    /// Move cursor to start.
    fn move_cursor_to_start(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end.
    fn move_cursor_to_end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Clear input.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.history_index = None;
    }

    /// Submit current input.
    fn submit(&mut self) {
        if !self.text.is_empty() {
            // Add to history
            if !self.history.contains(&self.text) {
                self.history.push(self.text.clone());
                if self.history.len() > self.max_history {
                    self.history.remove(0);
                }
            }

            // Set as pending
            self.pending_input = Some(self.text.clone());

            // Clear input
            self.clear();
        }
    }

    /// Navigate to previous history item.
    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        match self.history_index {
            Some(i) if i > 0 => {
                self.history_index = Some(i - 1);
            }
            None => {
                self.history_index = Some(self.history.len() - 1);
            }
            _ => {}
        }

        if let Some(i) = self.history_index {
            self.text = self.history[i].clone();
            self.cursor = self.text.len();
        }
    }

    /// Navigate to next history item.
    fn history_next(&mut self) {
        match self.history_index {
            Some(i) if i < self.history.len() - 1 => {
                self.history_index = Some(i + 1);
                self.text = self.history[i + 1].clone();
                self.cursor = self.text.len();
            }
            Some(_) => {
                // At end of history, clear input
                self.history_index = None;
                self.clear();
            }
            None => {}
        }
    }

    /// Set input text directly.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
    }

    /// Get history size.
    pub fn history_size(&self) -> usize {
        self.history.len()
    }
}

impl Default for InputArea {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_new() {
        let input = InputArea::new();
        assert!(input.text().is_empty());
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn test_insert_char() {
        let mut input = InputArea::new();

        input.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        input.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        assert_eq!(input.text(), "hi");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn test_delete() {
        let mut input = InputArea::new();

        input.set_text("hello");
        input.cursor = 5;

        input.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(input.text(), "hell");

        input.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(input.text(), "hell");
    }

    #[test]
    fn test_cursor_movement() {
        let mut input = InputArea::new();
        input.set_text("hello");

        input.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(input.cursor(), 4);

        input.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(input.cursor(), 0);

        input.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn test_submit() {
        let mut input = InputArea::new();
        input.set_text("test message");

        input.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(input.has_pending_input());
        assert_eq!(input.take_pending_input(), Some("test message".to_string()));
        assert!(input.text().is_empty());
    }

    #[test]
    fn test_multiline() {
        let mut input = InputArea::new();
        input.set_multiline(true);

        input.set_text("line 1");
        input.cursor = 6;

        // Shift+Enter should add newline
        input.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        assert_eq!(input.text(), "line 1\n");
        assert!(!input.has_pending_input());
    }

    #[test]
    fn test_history() {
        let mut input = InputArea::new();

        // Submit some messages
        input.set_text("first");
        input.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        input.set_text("second");
        input.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(input.history_size(), 2);

        // Navigate history
        input.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(input.text(), "second");

        input.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(input.text(), "first");

        input.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(input.text(), "second");
    }

    #[test]
    fn test_clear() {
        let mut input = InputArea::new();
        input.set_text("some text");

        input.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(input.text().is_empty());
    }
}