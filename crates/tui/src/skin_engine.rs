use ratatui::style::Color;

/// Color theme for the TUI.
#[derive(Debug, Clone)]
pub struct Skin {
    /// Foreground color for the output area.
    pub output_fg: Color,
    /// Background color for the output area.
    pub output_bg: Color,
    /// Foreground color for the input area.
    pub input_fg: Color,
    /// Background color for the input area.
    pub input_bg: Color,
    /// Foreground color for the status bar.
    pub status_fg: Color,
    /// Background color for the status bar.
    pub status_bg: Color,
    /// Cursor foreground color.
    pub cursor_fg: Color,
    /// Cursor background color.
    pub cursor_bg: Color,
}

impl Default for Skin {
    fn default() -> Self {
        Self {
            output_fg: Color::White,
            output_bg: Color::Reset,
            input_fg: Color::Green,
            input_bg: Color::Reset,
            status_fg: Color::Cyan,
            status_bg: Color::DarkGray,
            cursor_fg: Color::Black,
            cursor_bg: Color::White,
        }
    }
}

/// Predefined skins / themes.
impl Skin {
    pub fn dark() -> Self {
        Self {
            output_fg: Color::Gray,
            output_bg: Color::Black,
            input_fg: Color::Yellow,
            input_bg: Color::Black,
            status_fg: Color::White,
            status_bg: Color::DarkGray,
            cursor_fg: Color::Black,
            cursor_bg: Color::Yellow,
        }
    }

    pub fn light() -> Self {
        Self {
            output_fg: Color::Black,
            output_bg: Color::White,
            input_fg: Color::Blue,
            input_bg: Color::White,
            status_fg: Color::Black,
            status_bg: Color::Gray,
            cursor_fg: Color::White,
            cursor_bg: Color::Blue,
        }
    }

    pub fn gruvbox() -> Self {
        // Gruvbox dark inspired theme
        Self {
            output_fg: Color::Rgb(235, 219, 178),
            output_bg: Color::Rgb(40, 40, 40),
            input_fg: Color::Rgb(184, 187, 38),
            input_bg: Color::Rgb(40, 40, 40),
            status_fg: Color::Rgb(251, 241, 199),
            status_bg: Color::Rgb(60, 56, 54),
            cursor_fg: Color::Rgb(40, 40, 40),
            cursor_bg: Color::Rgb(251, 241, 199),
        }
    }

    pub fn catppuccin() -> Self {
        // Catppuccin Mocha inspired theme
        Self {
            output_fg: Color::Rgb(205, 214, 244),
            output_bg: Color::Rgb(30, 30, 46),
            input_fg: Color::Rgb(166, 227, 161),
            input_bg: Color::Rgb(30, 30, 46),
            status_fg: Color::Rgb(245, 224, 220),
            status_bg: Color::Rgb(49, 50, 68),
            cursor_fg: Color::Rgb(30, 30, 46),
            cursor_bg: Color::Rgb(245, 189, 196),
        }
    }
}
