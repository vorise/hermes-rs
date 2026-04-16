//! Spinner Animation
//!
//! Kawaii spinner frames for status display.

/// Default spinner frames (10 frames, classic braille dots).
pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Alternative spinner styles.
pub const SPINNER_DOTS: &[&str] = &["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"];
pub const SPINNER_LINE: &[&str] = &["-", "=", "≡", "≣", "≡", "="];
pub const SPINNER_MOON: &[&str] = &["🌑", "🌒", "🌓", "🌔", "🌕", " waxing gibbous", "🌖", "🌗", "🌘"];
pub const SPINNER_EARTH: &[&str] = &["🌍", "🌎", "🌏"];
pub const SPINNER_HEARTS: &[&str] = &["💛", "💙", "💜", "💚", "❤️"];
pub const SPINNER_CLOCK: &[&str] = &["🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚", "🕛"];

/// Spinner animation state.
#[derive(Debug, Clone)]
pub struct Spinner {
    /// Current frame index.
    frame_idx: usize,

    /// Total frames.
    frames: &'static [&'static str],

    /// Animation interval in milliseconds.
    interval_ms: u64,
}

impl Spinner {
    /// Create a new spinner with default frames.
    pub fn new() -> Self {
        Self {
            frame_idx: 0,
            frames: SPINNER_FRAMES,
            interval_ms: 100,
        }
    }

    /// Create a spinner with custom frames.
    pub fn with_frames(frames: &'static [&'static str]) -> Self {
        Self {
            frame_idx: 0,
            frames,
            interval_ms: 100,
        }
    }

    /// Set animation interval.
    pub fn with_interval(mut self, interval_ms: u64) -> Self {
        self.interval_ms = interval_ms;
        self
    }

    /// Get current frame.
    pub fn current_frame(&self) -> &str {
        self.frames[self.frame_idx]
    }

    /// Advance to next frame (tick).
    pub fn tick(&mut self) {
        self.frame_idx = (self.frame_idx + 1) % self.frames.len();
    }

    /// Reset to first frame.
    pub fn reset(&mut self) {
        self.frame_idx = 0;
    }

    /// Get animation interval.
    pub fn interval(&self) -> u64 {
        self.interval_ms
    }

    /// Get total frame count.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Create a dots spinner.
    pub fn dots() -> Self {
        Self::with_frames(SPINNER_DOTS)
    }

    /// Create a line spinner.
    pub fn line() -> Self {
        Self::with_frames(SPINNER_LINE)
    }

    /// Create a moon spinner.
    pub fn moon() -> Self {
        Self::with_frames(SPINNER_MOON)
    }

    /// Create an earth spinner.
    pub fn earth() -> Self {
        Self::with_frames(SPINNER_EARTH)
    }

    /// Create a hearts spinner.
    pub fn hearts() -> Self {
        Self::with_frames(SPINNER_HEARTS)
    }

    /// Create a clock spinner.
    pub fn clock() -> Self {
        Self::with_frames(SPINNER_CLOCK)
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_new() {
        let spinner = Spinner::new();
        assert_eq!(spinner.frame_count(), 10);
        assert_eq!(spinner.current_frame(), "⠋");
    }

    #[test]
    fn test_spinner_tick() {
        let mut spinner = Spinner::new();

        spinner.tick();
        assert_eq!(spinner.current_frame(), "⠙");

        spinner.tick();
        assert_eq!(spinner.current_frame(), "⠹");

        // Tick all remaining frames to wrap around (8 more to reach frame 0)
        for _ in 0..8 {
            spinner.tick();
        }
        assert_eq!(spinner.current_frame(), "⠋"); // Back to first
    }

    #[test]
    fn test_spinner_reset() {
        let mut spinner = Spinner::new();

        spinner.tick();
        spinner.tick();
        spinner.tick();

        spinner.reset();
        assert_eq!(spinner.current_frame(), "⠋");
    }

    #[test]
    fn test_spinner_custom_frames() {
        let spinner = Spinner::dots();
        assert_eq!(spinner.frame_count(), 8);
        assert_eq!(spinner.current_frame(), "⠁");

        let spinner = Spinner::earth();
        assert_eq!(spinner.frame_count(), 3);
        assert_eq!(spinner.current_frame(), "🌍");
    }
}