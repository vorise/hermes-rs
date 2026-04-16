/// Spinner animation for the status bar.
pub struct Spinner {
    frames: Vec<&'static str>,
    current: usize,
}

impl Default for Spinner {
    fn default() -> Self {
        Self {
            // Simple ASCII spinner frames
            frames: vec![
                "|", "/", "-", "\\",
                "|", "/", "-", "\\",
                "|", "/",
            ],
            current: 0,
        }
    }
}

impl Spinner {
    pub fn new(frames: Vec<&'static str>) -> Self {
        Self {
            frames,
            current: 0,
        }
    }

    pub fn current_frame(&self) -> &str {
        self.frames.get(self.current).unwrap_or(&"...")
    }

    pub fn tick(&mut self) {
        self.current = (self.current + 1) % self.frames.len();
    }

    pub fn reset(&mut self) {
        self.current = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_frames() {
        let spinner = Spinner::default();
        assert!(!spinner.frames.is_empty());
        assert_eq!(spinner.frames.len(), 10);
    }

    #[test]
    fn test_spinner_tick() {
        let mut spinner = Spinner::default();
        let first = spinner.current_frame().to_string();
        spinner.tick();
        assert_ne!(first, spinner.current_frame());
    }

    #[test]
    fn test_spinner_wraps_around() {
        let mut spinner = Spinner::default();
        for _ in 0..spinner.frames.len() {
            spinner.tick();
        }
        assert_eq!(spinner.current_frame(), spinner.frames[0]);
    }

    #[test]
    fn test_spinner_reset() {
        let mut spinner = Spinner::default();
        spinner.tick();
        spinner.tick();
        spinner.reset();
        assert_eq!(spinner.current, 0);
        assert_eq!(spinner.current_frame(), spinner.frames[0]);
    }

    #[test]
    fn test_custom_spinner() {
        let spinner = Spinner::new(vec!["/", "-", "\\"]);
        assert_eq!(spinner.current_frame(), "/");
    }
}
