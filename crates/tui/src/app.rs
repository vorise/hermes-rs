use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::sync::Arc;
use tokio::sync::Notify;

use crate::completer::Completer;
use crate::input::InputArea;
use crate::output::OutputArea;
use crate::skin_engine::Skin;
use crate::spinner::Spinner;

/// Main application state for the TUI.
pub struct App {
    pub input: InputArea,
    pub output: OutputArea,
    pub skin: Skin,
    pub spinner: Spinner,
    pub completer: Completer,
    pub running: bool,
    pub is_processing: bool,
    pub model_info: String,
    pub iteration_budget: Option<u32>,
    pub interrupt_notify: Arc<Notify>,
    pub input_history: Vec<String>,
    pub history_index: usize,
    /// Current autocomplete state: the typed prefix before Tab was pressed.
    pub autocomplete_prefix: Option<String>,
    /// Suggestions currently being displayed.
    pub autocomplete_suggestions: Option<String>,
}

impl App {
    pub fn new(interrupt_notify: Arc<Notify>) -> Self {
        Self {
            input: InputArea::new(),
            output: OutputArea::new(),
            skin: Skin::default(),
            spinner: Spinner::default(),
            completer: Completer::new(),
            running: true,
            is_processing: false,
            model_info: String::new(),
            iteration_budget: None,
            interrupt_notify,
            input_history: Vec::new(),
            history_index: 0,
            autocomplete_prefix: None,
            autocomplete_suggestions: None,
        }
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model_info = model.to_string();
        self
    }

    /// Register slash commands with the completer.
    /// `commands` is a slice of (name, description, aliases) tuples.
    pub fn with_commands(&mut self, commands: &[(&str, &str, &[&str])]) {
        self.completer = crate::completer::build_completer(commands);
    }

    pub fn submit_input(&mut self) -> Option<String> {
        let text = self.input.get_text();
        if text.trim().is_empty() {
            return None;
        }
        self.input_history.push(text.clone());
        self.history_index = self.input_history.len();
        self.input.clear();
        Some(text)
    }

    pub fn navigate_history_up(&mut self) {
        if self.history_index > 0 {
            self.history_index -= 1;
            if let Some(entry) = self.input_history.get(self.history_index) {
                self.input.set_text(entry.clone());
            }
        }
    }

    pub fn navigate_history_down(&mut self) {
        if self.history_index < self.input_history.len() {
            self.history_index += 1;
            if self.history_index < self.input_history.len() {
                if let Some(entry) = self.input_history.get(self.history_index) {
                    self.input.set_text(entry.clone());
                }
            } else {
                self.input.clear();
            }
        }
    }

    pub fn add_output_line(&mut self, line: &str) {
        self.output.add_line(line);
    }

    pub fn add_output_styled(&mut self, line: Vec<Span<'static>>) {
        self.output.add_styled_line(line);
    }

    pub fn interrupt(&self) {
        self.interrupt_notify.notify_one();
    }
}

/// Render the TUI layout.
fn render(app: &App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    terminal.draw(|frame| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),        // Output area
                Constraint::Length(1),     // Status bar
                Constraint::Length(3),     // Input area
            ])
            .split(frame.area());

        // Output area
        let output_widget = app.output.render(&app.skin);
        frame.render_widget(output_widget, chunks[0]);

        // Status bar
        let status_text = if app.is_processing {
            let spin = app.spinner.current_frame();
            format!(" {} | {} | Budget: {} | {spin}",
                app.model_info,
                "Processing...",
                app.iteration_budget.unwrap_or(0))
        } else {
            format!(" {} | Ready | Budget: {}",
                app.model_info,
                app.iteration_budget.unwrap_or(0))
        };
        let status = Paragraph::new(status_text)
            .style(Style::default()
                .fg(app.skin.status_fg)
                .bg(app.skin.status_bg));
        frame.render_widget(status, chunks[1]);

        // Input area
        let input_widget = app.input.render(&app.skin);
        frame.render_widget(input_widget, chunks[2]);

        // Set cursor position
        if !app.is_processing {
            let input_area = chunks[2].inner(ratatui::layout::Margin::new(1, 1));
            let cursor_x = input_area.x + app.input.cursor_pos as u16;
            let cursor_y = input_area.y;
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    })?;
    Ok(())
}

/// Run the TUI event loop.
pub async fn run_tui(app: &mut App) -> io::Result<()> {
    let mut stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_tui_inner(app, &mut terminal).await;

    // Cleanup
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_tui_inner(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> io::Result<()> {
    while app.running {
        render(app, terminal)?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                handle_key_event(app, key);
            }
        }

        // Update spinner animation
        if app.is_processing {
            app.spinner.tick();
        }
    }

    Ok(())
}

fn handle_key_event(app: &mut App, key: KeyEvent) {
    match (key.modifiers, key.code) {
        // Ctrl+C: interrupt or exit
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            if app.is_processing {
                app.interrupt();
            } else {
                app.running = false;
            }
        }

        // Ctrl+D: exit
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
            app.running = false;
        }

        // Ctrl+L: clear output
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
            app.output.clear();
        }

        // Enter: submit input
        (KeyModifiers::NONE, KeyCode::Enter) => {
            if !app.is_processing {
                if let Some(text) = app.submit_input() {
                    app.add_output_line(&format!("User: {text}"));
                }
            }
        }

        // Shift+Enter: newline in input
        (KeyModifiers::SHIFT, KeyCode::Enter) => {
            app.input.insert_char('\n');
        }

        // Up: history navigation
        (KeyModifiers::NONE, KeyCode::Up) => {
            app.navigate_history_up();
        }

        // Down: history navigation
        (KeyModifiers::NONE, KeyCode::Down) => {
            app.navigate_history_down();
        }

        // Tab: autocomplete
        (KeyModifiers::NONE, KeyCode::Tab) => {
            if !app.is_processing {
                handle_autocomplete(app);
            }
        }

        // Escape: cancel input and autocomplete
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.autocomplete_prefix = None;
            app.autocomplete_suggestions = None;
            app.input.clear();
        }

        // Regular input
        (KeyModifiers::NONE | KeyModifiers::SHIFT, _) => {
            if !app.is_processing {
                app.input.handle_char(key.code);
            }
        }

        _ => {}
    }
}

/// Handle Tab-based autocomplete for slash commands.
fn handle_autocomplete(app: &mut App) {
    let text = app.input.get_text();

    // Only autocomplete for slash commands
    if !text.starts_with('/') {
        return;
    }

    // Extract the prefix after the `/`
    let prefix = text[1..].trim();

    // Check if we're cycling through suggestions
    if let Some(ref saved_prefix) = app.autocomplete_prefix {
        if prefix == saved_prefix {
            // User pressed Tab again — cycle to next suggestion
            if let Some(ref suggestions) = app.autocomplete_suggestions {
                // Show next suggestion (simplified: just re-display the list)
                app.output.add_line(&format!("\n{suggestions}"));
                return;
            }
        }
    }

    // Try to complete
    if let Some(completion) = app.completer.complete(prefix) {
        app.input.set_text(format!("/{completion}"));
        app.autocomplete_prefix = Some(prefix.to_string());
        app.autocomplete_suggestions = None;
    } else {
        // No single completion — show suggestions
        let formatted = app.completer.format_suggestions(prefix, 5);
        if !formatted.is_empty() {
            app.autocomplete_prefix = Some(prefix.to_string());
            app.autocomplete_suggestions = Some(formatted.clone());
            app.output.add_line(&formatted);
        }
    }
}

/// Print an ASCII art banner before TUI starts.
pub fn print_banner(version: &str, model: &str) {
    println!(r#"
 █░█ █▀▀ █▀▄   █▀ █▀▀ █▀   █░█░█ █▀▀ █▀▀ █▀▀ █░█ █▀░ █▀   █▄░█ █░█ ▀█▀ █▀█ █▀▄   █▀█ █▀█ █▀▄ █░▀ █░█ ▀█▀
 █▄█ ██▄ █▄   ▄█ ██▄ ▄█   ▀▄▀▄▀ ██▄ █▀░ ██▄ █▄█ ▄█ ▄█   █░▀█ █▄█ ░█░ █▀▄ █▄▀   █▀▀ █▀▄ █▄▀ █▄▀ █▄█ ░█░
"#);
    println!("  v{version}  |  Model: {model}");
    println!("  Press Ctrl+C to exit, Ctrl+D to quit, Ctrl+L to clear\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[test]
    fn test_app_creation() {
        let notify = Arc::new(Notify::new());
        let app = App::new(notify);
        assert!(app.running);
        assert!(!app.is_processing);
    }

    #[test]
    fn test_submit_input() {
        let notify = Arc::new(Notify::new());
        let mut app = App::new(notify);
        app.input.set_text("hello".to_string());

        let result = app.submit_input();
        assert_eq!(result, Some("hello".to_string()));
        assert_eq!(app.input_history.len(), 1);
    }

    #[test]
    fn test_empty_input_not_submitted() {
        let notify = Arc::new(Notify::new());
        let mut app = App::new(notify);

        let result = app.submit_input();
        assert_eq!(result, None);
    }

    #[test]
    fn test_history_navigation() {
        let notify = Arc::new(Notify::new());
        let mut app = App::new(notify);
        app.input.set_text("first".to_string());
        app.submit_input();
        app.input.set_text("second".to_string());
        app.submit_input();

        app.navigate_history_up();
        assert_eq!(app.input.get_text(), "second");

        app.navigate_history_up();
        assert_eq!(app.input.get_text(), "first");
    }

    #[test]
    fn test_autocomplete_completes_single_match() {
        let notify = Arc::new(Notify::new());
        let mut app = App::new(notify);
        app.with_commands(&[("compress", "Compress context", &[]), ("model", "Switch model", &[])]);

        app.input.set_text("/comp".to_string());
        handle_autocomplete(&mut app);
        assert_eq!(app.input.get_text(), "/compress");
    }

    #[test]
    fn test_autocomplete_ignores_non_slash() {
        let notify = Arc::new(Notify::new());
        let mut app = App::new(notify);
        app.with_commands(&[("help", "Show help", &[])]);

        app.input.set_text("help".to_string());
        handle_autocomplete(&mut app);
        // No change — doesn't start with `/`
        assert_eq!(app.input.get_text(), "help");
    }

    #[test]
    fn test_autocomplete_shows_suggestions() {
        let notify = Arc::new(Notify::new());
        let mut app = App::new(notify);
        app.with_commands(&[("model", "Switch model", &[]), ("memory", "View memories", &[])]);

        app.input.set_text("/m".to_string());
        handle_autocomplete(&mut app);
        // Can't disambiguate "m" further, so shows suggestions
        assert!(app.autocomplete_suggestions.is_some());
    }
}
