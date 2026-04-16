use crossterm::event::KeyCode;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::skin_engine::Skin;

/// Multiline input area widget.
pub struct InputArea {
    pub lines: Vec<String>,
    pub cursor_line: usize,
    pub cursor_pos: usize,
    pub scroll_offset: usize,
}

impl InputArea {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_pos: 0,
            scroll_offset: 0,
        }
    }

    pub fn get_text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn set_text(&mut self, text: String) {
        self.lines = text
            .split('\n')
            .map(|s| s.to_string())
            .collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_line = self.lines.len() - 1;
        self.cursor_pos = self.lines[self.cursor_line].len();
    }

    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.cursor_line = 0;
        self.cursor_pos = 0;
        self.scroll_offset = 0;
    }

    pub fn handle_char(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char(c) => {
                self.insert_char(c);
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    let line = &mut self.lines[self.cursor_line];
                    line.remove(self.cursor_pos - 1);
                    self.cursor_pos -= 1;
                } else if self.cursor_line > 0 {
                    let current = self.lines.remove(self.cursor_line);
                    self.cursor_line -= 1;
                    let prev_len = self.lines[self.cursor_line].len();
                    self.lines[self.cursor_line].push_str(&current);
                    self.cursor_pos = prev_len;
                }
            }
            KeyCode::Delete => {
                let line = &self.lines[self.cursor_line];
                if self.cursor_pos < line.len() {
                    self.lines[self.cursor_line].remove(self.cursor_pos);
                } else if self.cursor_line < self.lines.len() - 1 {
                    let next = self.lines.remove(self.cursor_line + 1);
                    self.lines[self.cursor_line].push_str(&next);
                }
            }
            KeyCode::Left => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                } else if self.cursor_line > 0 {
                    self.cursor_line -= 1;
                    self.cursor_pos = self.lines[self.cursor_line].len();
                }
            }
            KeyCode::Right => {
                let line = &self.lines[self.cursor_line];
                if self.cursor_pos < line.len() {
                    self.cursor_pos += 1;
                } else if self.cursor_line < self.lines.len() - 1 {
                    self.cursor_line += 1;
                    self.cursor_pos = 0;
                }
            }
            KeyCode::Up => {
                if self.cursor_line > 0 {
                    let current_pos = self.cursor_pos;
                    self.cursor_line -= 1;
                    self.cursor_pos = current_pos.min(self.lines[self.cursor_line].len());
                }
            }
            KeyCode::Down => {
                if self.cursor_line < self.lines.len() - 1 {
                    let current_pos = self.cursor_pos;
                    self.cursor_line += 1;
                    self.cursor_pos = current_pos.min(self.lines[self.cursor_line].len());
                }
            }
            KeyCode::Home => {
                self.cursor_pos = 0;
            }
            KeyCode::End => {
                self.cursor_pos = self.lines[self.cursor_line].len();
            }
            _ => {}
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.lines[self.cursor_line].insert(self.cursor_pos, c);
        self.cursor_pos += 1;
    }

    pub fn render(&self, skin: &Skin) -> Paragraph<'_> {
        let visible_lines: Vec<Line<'_>> = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                if i == self.cursor_line {
                    let before = &line[..self.cursor_pos];
                    let after = &line[self.cursor_pos..];
                    Line::from(vec![
                        Span::styled(before, Style::default().fg(skin.input_fg)),
                        Span::styled(
                            if self.cursor_pos < line.len() {
                                line[self.cursor_pos..self.cursor_pos + 1].to_string()
                            } else {
                                " ".to_string()
                            },
                            Style::default().bg(skin.cursor_bg).fg(skin.cursor_fg),
                        ),
                        Span::styled(after, Style::default().fg(skin.input_fg)),
                    ])
                } else {
                    Line::from(Span::styled(line, Style::default().fg(skin.input_fg)))
                }
            })
            .collect();

        let prefix = "> ";
        let lines_with_prefix: Vec<Line<'_>> = visible_lines
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                if i == 0 {
                    let mut spans = vec![Span::styled(
                        prefix,
                        Style::default().fg(Color::Yellow),
                    )];
                    spans.extend(line.spans);
                    Line::from(spans)
                } else {
                    let mut spans = vec![Span::raw("  ")];
                    spans.extend(line.spans);
                    Line::from(spans)
                }
            })
            .collect();

        Paragraph::new(lines_with_prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_input_is_empty() {
        let input = InputArea::new();
        assert_eq!(input.get_text(), "");
    }

    #[test]
    fn test_insert_char() {
        let mut input = InputArea::new();
        input.insert_char('h');
        input.insert_char('i');
        assert_eq!(input.get_text(), "hi");
    }

    #[test]
    fn test_backspace() {
        let mut input = InputArea::new();
        input.insert_char('a');
        input.insert_char('b');
        input.handle_char(KeyCode::Backspace);
        assert_eq!(input.get_text(), "a");
    }

    #[test]
    fn test_set_and_clear() {
        let mut input = InputArea::new();
        input.set_text("hello\nworld".to_string());
        assert_eq!(input.get_text(), "hello\nworld");
        input.clear();
        assert_eq!(input.get_text(), "");
    }

    #[test]
    fn test_cursor_movement() {
        let mut input = InputArea::new();
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        input.handle_char(KeyCode::Left);
        assert_eq!(input.cursor_pos, 2);
        input.handle_char(KeyCode::Home);
        assert_eq!(input.cursor_pos, 0);
        input.handle_char(KeyCode::End);
        assert_eq!(input.cursor_pos, 3);
    }
}
