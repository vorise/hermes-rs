use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::skin_engine::Skin;

/// Scrolling output area widget.
pub struct OutputArea {
    lines: Vec<Vec<Span<'static>>>,
    scroll_offset: usize,
    max_lines: usize,
}

impl OutputArea {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            scroll_offset: 0,
            max_lines: 1000,
        }
    }

    pub fn add_line(&mut self, line: &str) {
        self.lines
            .push(vec![Span::styled(line.to_string(), Style::default())]);
        self.trim_lines();
        self.scroll_to_bottom();
    }

    pub fn add_styled_line(&mut self, spans: Vec<Span<'static>>) {
        self.lines.push(spans);
        self.trim_lines();
        self.scroll_to_bottom();
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset < self.lines.len().saturating_sub(1) {
            self.scroll_offset += 1;
        }
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll_offset = 0;
    }

    fn trim_lines(&mut self) {
        if self.lines.len() > self.max_lines {
            let excess = self.lines.len() - self.max_lines;
            self.lines.drain(..excess);
        }
    }

    fn visible_lines(&self) -> &[Vec<Span<'static>>] {
        if self.scroll_offset == 0 {
            self.lines.as_slice()
        } else {
            let start = self.lines.len().saturating_sub(self.scroll_offset + 1);
            let end = self.lines.len() - self.scroll_offset;
            &self.lines[start..end.min(self.lines.len())]
        }
    }

    pub fn render(&self, skin: &Skin) -> Paragraph<'_> {
        let styled_lines: Vec<Line<'_>> = self
            .visible_lines()
            .iter()
            .map(|spans| {
                Line::from(
                    spans
                        .iter()
                        .map(|s| {
                            Span::styled(
                                s.content.clone(),
                                Style::default()
                                    .fg(s.style.fg.unwrap_or(skin.output_fg))
                                    .bg(s.style.bg.unwrap_or(Color::Reset))
                                    .add_modifier(s.style.add_modifier),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();

        Paragraph::new(styled_lines)
            .block(Block::default().borders(Borders::NONE))
            .style(Style::default().fg(skin.output_fg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_line() {
        let mut output = OutputArea::new();
        output.add_line("hello");
        assert_eq!(output.lines.len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut output = OutputArea::new();
        output.add_line("hello");
        output.add_line("world");
        output.clear();
        assert!(output.lines.is_empty());
        assert_eq!(output.scroll_offset, 0);
    }

    #[test]
    fn test_trim_lines() {
        let mut output = OutputArea::new();
        output.max_lines = 3;
        output.add_line("line 1");
        output.add_line("line 2");
        output.add_line("line 3");
        output.add_line("line 4");
        assert_eq!(output.lines.len(), 3);
    }

    #[test]
    fn test_scroll() {
        let mut output = OutputArea::new();
        output.add_line("line 1");
        output.add_line("line 2");
        output.add_line("line 3");
        output.scroll_up();
        assert_eq!(output.scroll_offset, 1);
        output.scroll_down();
        assert_eq!(output.scroll_offset, 0);
    }
}
