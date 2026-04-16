//! Skin Engine
//!
//! Theme/skin customization for the TUI.

use ratatui::style::{Color, Style};

/// Theme/skin configuration.
#[derive(Debug, Clone)]
pub struct Skin {
    /// Primary text color.
    pub text: Color,

    /// Accent color for highlights.
    pub accent: Color,

    /// Background color.
    pub background: Option<Color>,

    /// Error/error message color.
    pub error: Color,

    /// Success/success message color.
    pub success: Color,

    /// Warning/warning message color.
    pub warning: Color,

    /// User message color.
    pub user: Color,

    /// Assistant message color.
    pub assistant: Color,

    /// System message color.
    pub system: Color,

    /// Tool output color.
    pub tool: Color,

    /// Spinner color.
    pub spinner: Color,

    /// Status bar colors.
    pub status_bar: StatusBarSkin,

    /// Input area colors.
    pub input: InputSkin,

    /// Border style.
    pub border: BorderSkin,
}

impl Default for Skin {
    fn default() -> Self {
        Self::dark()
    }
}

impl Skin {
    /// Create a dark theme (default).
    pub fn dark() -> Self {
        Self {
            text: Color::White,
            accent: Color::Cyan,
            background: None,
            error: Color::Red,
            success: Color::Green,
            warning: Color::Yellow,
            user: Color::Blue,
            assistant: Color::Green,
            system: Color::Gray,
            tool: Color::Magenta,
            spinner: Color::Cyan,
            status_bar: StatusBarSkin::default(),
            input: InputSkin::default(),
            border: BorderSkin::default(),
        }
    }

    /// Create a light theme.
    pub fn light() -> Self {
        Self {
            text: Color::Black,
            accent: Color::Blue,
            background: Some(Color::White),
            error: Color::Red,
            success: Color::Green,
            warning: Color::Yellow,
            user: Color::Blue,
            assistant: Color::DarkGray,
            system: Color::Gray,
            tool: Color::LightMagenta,
            spinner: Color::Blue,
            status_bar: StatusBarSkin::light(),
            input: InputSkin::light(),
            border: BorderSkin::light(),
        }
    }

    /// Create a monokai-inspired theme.
    pub fn monokai() -> Self {
        Self {
            text: Color::White,
            accent: Color::Rgb(229, 220, 120), // Yellow-ish
            background: None,
            error: Color::Rgb(249, 38, 114), // Red-pink
            success: Color::Rgb(166, 226, 46), // Green
            warning: Color::Rgb(253, 151, 31), // Orange
            user: Color::Rgb(102, 217, 239), // Cyan
            assistant: Color::Rgb(230, 219, 116), // Yellow
            system: Color::Gray,
            tool: Color::Rgb(174, 129, 255), // Purple
            spinner: Color::Rgb(249, 38, 114),
            status_bar: StatusBarSkin::default(),
            input: InputSkin::default(),
            border: BorderSkin::default(),
        }
    }

    /// Get text style.
    pub fn text_style(&self) -> Style {
        Style::default().fg(self.text)
    }

    /// Get accent style.
    pub fn accent_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Get error style.
    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    /// Get success style.
    pub fn success_style(&self) -> Style {
        Style::default().fg(self.success)
    }

    /// Get user message style.
    pub fn user_style(&self) -> Style {
        Style::default().fg(self.user)
    }

    /// Get assistant message style.
    pub fn assistant_style(&self) -> Style {
        Style::default().fg(self.assistant)
    }

    /// Get tool style.
    pub fn tool_style(&self) -> Style {
        Style::default().fg(self.tool)
    }

    /// Get spinner style.
    pub fn spinner_style(&self) -> Style {
        Style::default().fg(self.spinner)
    }
}

/// Status bar skin.
#[derive(Debug, Clone)]
pub struct StatusBarSkin {
    pub background: Option<Color>,
    pub text: Color,
    pub accent: Color,
}

impl Default for StatusBarSkin {
    fn default() -> Self {
        Self {
            background: None,
            text: Color::White,
            accent: Color::Cyan,
        }
    }
}

impl StatusBarSkin {
    pub fn light() -> Self {
        Self {
            background: Some(Color::Gray),
            text: Color::Black,
            accent: Color::Blue,
        }
    }
}

/// Input area skin.
#[derive(Debug, Clone)]
pub struct InputSkin {
    pub background: Option<Color>,
    pub text: Color,
    pub prompt: Color,
    pub cursor: Color,
}

impl Default for InputSkin {
    fn default() -> Self {
        Self {
            background: None,
            text: Color::White,
            prompt: Color::Cyan,
            cursor: Color::Cyan,
        }
    }
}

impl InputSkin {
    pub fn light() -> Self {
        Self {
            background: Some(Color::White),
            text: Color::Black,
            prompt: Color::Blue,
            cursor: Color::Blue,
        }
    }
}

/// Border skin.
#[derive(Debug, Clone)]
pub struct BorderSkin {
    pub color: Color,
    pub rounded: bool,
}

impl Default for BorderSkin {
    fn default() -> Self {
        Self {
            color: Color::Cyan,
            rounded: true,
        }
    }
}

impl BorderSkin {
    pub fn light() -> Self {
        Self {
            color: Color::Gray,
            rounded: true,
        }
    }
}

/// Skin engine for managing themes.
#[derive(Debug, Clone)]
pub struct SkinEngine {
    /// Current skin.
    skin: Skin,
}

impl SkinEngine {
    /// Create with default skin.
    pub fn new() -> Self {
        Self {
            skin: Skin::default(),
        }
    }

    /// Get current skin.
    pub fn skin(&self) -> &Skin {
        &self.skin
    }

    /// Set skin.
    pub fn set_skin(&mut self, skin: Skin) {
        self.skin = skin;
    }

    /// Switch to dark theme.
    pub fn use_dark(&mut self) {
        self.skin = Skin::dark();
    }

    /// Switch to light theme.
    pub fn use_light(&mut self) {
        self.skin = Skin::light();
    }

    /// Switch to monokai theme.
    pub fn use_monokai(&mut self) {
        self.skin = Skin::monokai();
    }
}

impl Default for SkinEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skin_default() {
        let skin = Skin::default();
        assert_eq!(skin.text, Color::White);
    }

    #[test]
    fn test_skin_dark() {
        let skin = Skin::dark();
        assert_eq!(skin.text, Color::White);
        assert_eq!(skin.accent, Color::Cyan);
    }

    #[test]
    fn test_skin_light() {
        let skin = Skin::light();
        assert_eq!(skin.text, Color::Black);
        assert!(skin.background.is_some());
    }

    #[test]
    fn test_skin_styles() {
        let skin = Skin::dark();

        let style = skin.text_style();
        assert_eq!(style.fg, Some(Color::White));

        let style = skin.user_style();
        assert_eq!(style.fg, Some(Color::Blue));
    }

    #[test]
    fn test_skin_engine() {
        let mut engine = SkinEngine::new();

        engine.use_light();
        assert_eq!(engine.skin().text, Color::Black);

        engine.use_monokai();
        assert_eq!(engine.skin().accent, Color::Rgb(229, 220, 120));
    }
}