//! TUI Application
//!
//! Main application state and render loop.

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect, Margin},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::time::{Duration, Instant};

use crate::input::InputArea;
use crate::output::{OutputMessage, OutputRegion};
use crate::spinner::Spinner;
use crate::skin_engine::SkinEngine;
use crate::completer::Completer;

/// Application state.
pub struct App {
    /// Is the application running?
    running: bool,

    /// Input area state.
    input: InputArea,

    /// Output region state.
    output: OutputRegion,

    /// Spinner animation.
    spinner: Spinner,

    /// Spinner state (is processing).
    spinner_active: bool,

    /// Skin/theme engine.
    skin: SkinEngine,

    /// Command completer.
    completer: Completer,

    /// Completion suggestions (for display).
    completions: Vec<String>,

    /// Selected completion index.
    completion_index: Option<usize>,

    /// Status message.
    status: String,

    /// Model name for display.
    model_name: String,

    /// Iteration count.
    iteration: u32,

    /// Cost tracker (approximate).
    cost: f64,
}

impl App {
    /// Create a new application.
    pub fn new() -> Self {
        Self {
            running: true,
            input: InputArea::new(),
            output: OutputRegion::new(),
            spinner: Spinner::new(),
            spinner_active: false,
            skin: SkinEngine::new(),
            completer: Completer::new(),
            completions: Vec::new(),
            completion_index: None,
            status: "Ready".to_string(),
            model_name: "claude-sonnet-4".to_string(),
            iteration: 0,
            cost: 0.0,
        }
    }

    /// Run the application.
    pub fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main loop
        let tick_rate = Duration::from_millis(100);
        let mut last_tick = Instant::now();

        while self.running {
            // Handle events
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }

            // Tick for spinner
            if last_tick.elapsed() >= tick_rate {
                if self.spinner_active {
                    self.spinner.tick();
                }
                last_tick = Instant::now();
            }

            // Render
            terminal.draw(|f| self.render(f))?;
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;

        Ok(())
    }

    /// Handle key events.
    fn handle_key(&mut self, key: event::KeyEvent) {
        // Handle special keys first
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+C: interrupt
                self.spinner_active = false;
                self.status = "Interrupted".to_string();
                return;
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+D: exit
                self.running = false;
                return;
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+L: clear screen
                self.output.clear();
                return;
            }
            KeyCode::Tab => {
                // Tab: autocomplete
                self.autocomplete();
                return;
            }
            KeyCode::Esc => {
                // Esc: clear completions
                self.completions.clear();
                self.completion_index = None;
                return;
            }
            _ => {}
        }

        // Pass to input handler
        let handled = self.input.handle_key(key);

        // Check for submitted input
        if let Some(text) = self.input.take_pending_input() {
            self.process_input(text);
        }

        // Update completions if input changed
        if handled {
            self.update_completions();
        }
    }

    /// Process submitted input.
    fn process_input(&mut self, text: String) {
        // Add to output
        self.output.user(&text);

        // Check for slash commands
        if text.starts_with('/') {
            self.handle_command(&text);
        } else {
            // Regular message - simulate processing
            self.spinner_active = true;
            self.status = "Processing...".to_string();

            // Simulate response (placeholder)
            self.output.assistant("Response placeholder - API integration needed");
            self.spinner_active = false;
            self.status = "Ready".to_string();
        }
    }

    /// Handle slash commands.
    fn handle_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let command = parts[0];

        match command {
            "/help" => {
                self.output.system("Available commands: /help, /exit, /clear, /status, /skin");
            }
            "/exit" | "/quit" => {
                self.running = false;
            }
            "/clear" => {
                self.output.clear();
            }
            "/status" => {
                self.output.system(format!(
                    "Status: {} | Model: {} | Cost: ${:.4}",
                    self.status, self.model_name, self.cost
                ));
            }
            "/skin" => {
                if parts.len() > 1 {
                    match parts[1] {
                        "dark" => self.skin.use_dark(),
                        "light" => self.skin.use_light(),
                        "monokai" => self.skin.use_monokai(),
                        _ => self.output.error("Unknown skin: dark, light, monokai"),
                    }
                    self.output.status(format!("Skin changed to {}", parts[1]));
                } else {
                    self.output.system("Skins: dark, light, monokai");
                }
            }
            _ => {
                self.output.error(format!("Unknown command: {}", command));
            }
        }
    }

    /// Update completion suggestions.
    fn update_completions(&mut self) {
        let text = self.input.text();
        if text.starts_with('/') {
            self.completions = self.completer.complete(text);
            self.completion_index = None;
        } else {
            self.completions.clear();
            self.completion_index = None;
        }
    }

    /// Apply autocomplete.
    fn autocomplete(&mut self) {
        if !self.completions.is_empty() {
            // Select next completion
            let idx = match self.completion_index {
                Some(i) => (i + 1) % self.completions.len(),
                None => 0,
            };
            self.completion_index = Some(idx);

            // Apply completion
            let completion = &self.completions[idx];
            self.input.set_text(completion);
            self.completions.clear();
            self.completion_index = None;
        }
    }

    /// Render the UI.
    pub fn render(&self, f: &mut Frame) {
        let skin = self.skin.skin();

        // Layout: banner, output, input
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Banner/status
                Constraint::Min(10),   // Output
                Constraint::Length(3), // Input
            ])
            .split(f.area());

        // Render banner/status bar
        self.render_banner(f, chunks[0], skin);

        // Render output region
        self.render_output(f, chunks[1], skin);

        // Render input area
        self.render_input(f, chunks[2], skin);
    }

    /// Render banner/status bar.
    fn render_banner(&self, f: &mut Frame, area: Rect, skin: &crate::skin_engine::Skin) {
        let spinner_frame = if self.spinner_active {
            self.spinner.current_frame()
        } else {
            ""
        };

        let status_text = format!(
            "Hermes v0.1 | {} | {} | Cost: ${:.4} {}",
            self.model_name, self.status, self.cost, spinner_frame
        );

        let banner = Paragraph::new(status_text)
            .style(skin.accent_style())
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(skin.accent)),
            );

        f.render_widget(banner, area);
    }

    /// Render output region.
    fn render_output(&self, f: &mut Frame, area: Rect, skin: &crate::skin_engine::Skin) {
        // Build lines from messages
        let lines: Vec<Line<'_>> = self
            .output
            .messages()
            .iter()
            .map(|msg| self.message_to_line(msg, skin))
            .collect();

        let output = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::NONE)
                    .style(skin.text_style()),
            );

        f.render_widget(output, area);
    }

    /// Convert message to styled line.
    fn message_to_line<'a>(&self, msg: &'a OutputMessage, skin: &crate::skin_engine::Skin) -> Line<'a> {
        match msg {
            OutputMessage::User(text) => {
                Line::from(vec![
                    Span::styled("User: ", skin.user_style()),
                    Span::styled(text.clone(), skin.text_style()),
                ])
            }
            OutputMessage::Assistant(text) => {
                Line::from(vec![
                    Span::styled("Assistant: ", skin.assistant_style()),
                    Span::styled(text.clone(), skin.text_style()),
                ])
            }
            OutputMessage::System(text) => {
                Line::from(Span::styled(text.clone(), skin.accent_style()))
            }
            OutputMessage::Tool { name, output } => {
                Line::from(vec![
                    Span::styled(format!("Tool: {}", name), skin.tool_style()),
                    Span::styled(format!(" {}", output), skin.text_style()),
                ])
            }
            OutputMessage::Error(text) => {
                Line::from(Span::styled(text.clone(), skin.error_style()))
            }
            OutputMessage::Status(text) => {
                Line::from(Span::styled(text.clone(), skin.accent_style()))
            }
            OutputMessage::Separator => {
                Line::from(Span::styled("─".repeat(40), Style::default().fg(Color::DarkGray)))
            }
        }
    }

    /// Render input area.
    fn render_input(&self, f: &mut Frame, area: Rect, skin: &crate::skin_engine::Skin) {
        let prompt = "> ";
        let input_text = self.input.text();

        // Show completions if available
        let completion_hint = if !self.completions.is_empty() {
            let idx = self.completion_index.unwrap_or(0);
            let completion = &self.completions[idx];
            completion.strip_prefix(input_text).unwrap_or("")
        } else {
            ""
        };

        let styled_text: Vec<Span> = vec![
            Span::styled(prompt, skin.accent_style()),
            Span::styled(input_text, skin.text_style()),
            Span::styled(completion_hint, Style::default().fg(Color::DarkGray)),
        ];

        let input = Paragraph::new(Line::from(styled_text))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(skin.accent)),
            );

        f.render_widget(input, area);

        // Set cursor position (after prompt + input cursor offset)
        let cursor_x = (prompt.len() + self.input.cursor()) as u16;
        let cursor_y = area.y + 1; // Inside the block
        f.set_cursor_position((cursor_x, cursor_y));
    }

    /// Stop the application.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Set model name.
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model_name = model.into();
    }

    /// Set status.
    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    /// Set spinner active.
    pub fn set_spinner_active(&mut self, active: bool) {
        self.spinner_active = active;
    }

    /// Add cost.
    pub fn add_cost(&mut self, cost: f64) {
        self.cost += cost;
    }

    /// Get output region.
    pub fn output(&self) -> &OutputRegion {
        &self.output
    }

    /// Get mutable output region.
    pub fn output_mut(&mut self) -> &mut OutputRegion {
        &mut self.output
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new() {
        let app = App::new();
        assert!(app.is_running());
        assert!(app.output().count() == 0);
    }

    #[test]
    fn test_app_stop() {
        let mut app = App::new();
        app.stop();
        assert!(!app.is_running());
    }

    #[test]
    fn test_app_setters() {
        let mut app = App::new();

        app.set_model("gpt-4");
        assert_eq!(app.model_name, "gpt-4");

        app.set_status("Loading");
        assert_eq!(app.status, "Loading");

        app.set_spinner_active(true);
        assert!(app.spinner_active);

        app.add_cost(0.01);
        assert_eq!(app.cost, 0.01);
    }

    #[test]
    fn test_output_mut() {
        let mut app = App::new();

        app.output_mut().user("Hello");
        assert_eq!(app.output().count(), 1);
    }
}