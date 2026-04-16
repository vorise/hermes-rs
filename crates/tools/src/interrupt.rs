use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Thread-scoped interrupt signaling for tool execution.
///
/// Uses an `AtomicBool` for lock-free signaling across threads.
/// Each agent/session gets its own `InterruptGuard` with per-agent isolation.
#[derive(Debug, Clone)]
pub struct InterruptGuard {
    flag: Arc<AtomicBool>,
}

impl InterruptGuard {
    /// Create a new interrupt guard (not interrupted).
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal an interrupt.
    pub fn interrupt(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Clear the interrupt signal.
    pub fn clear(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }

    /// Check whether an interrupt has been signaled.
    pub fn is_interrupted(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Create a cheap clone that shares the same interrupt flag.
    /// Clones can be sent to other tasks/threads to check/set the interrupt.
    pub fn clone(&self) -> Self {
        Self {
            flag: Arc::clone(&self.flag),
        }
    }
}

impl Default for InterruptGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Check whether an interrupt has been signaled.
///
/// Returns `true` if the interrupt flag is set, allowing tool execution
/// to abort early.
pub fn check_interrupt(guard: &InterruptGuard) -> bool {
    guard.is_interrupted()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_interrupt_guard_new_is_not_interrupted() {
        let guard = InterruptGuard::new();
        assert!(!guard.is_interrupted());
    }

    #[test]
    fn test_interrupt_sets_flag() {
        let guard = InterruptGuard::new();
        assert!(!guard.is_interrupted());
        guard.interrupt();
        assert!(guard.is_interrupted());
    }

    #[test]
    fn test_clear_resets_flag() {
        let guard = InterruptGuard::new();
        guard.interrupt();
        assert!(guard.is_interrupted());
        guard.clear();
        assert!(!guard.is_interrupted());
    }

    #[test]
    fn test_clone_shares_same_flag() {
        let guard = InterruptGuard::new();
        let clone = guard.clone();
        assert!(!guard.is_interrupted());
        assert!(!clone.is_interrupted());

        guard.interrupt();
        assert!(clone.is_interrupted());

        clone.clear();
        assert!(!guard.is_interrupted());
    }

    #[test]
    fn test_interrupt_across_threads() {
        let guard = InterruptGuard::new();
        let shared = guard.clone();
        let counter = Arc::new(AtomicUsize::new(0));
        let shared_counter = Arc::clone(&counter);

        let handle = thread::spawn(move || {
            // Simulate work
            thread::sleep(Duration::from_millis(10));
            // Signal interrupt
            shared.interrupt();
            shared_counter.fetch_add(1, Ordering::SeqCst);
        });

        // Poll in main thread
        while !guard.is_interrupted() {
            thread::sleep(Duration::from_millis(1));
        }

        handle.join().unwrap();
        assert!(guard.is_interrupted());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_check_interrupt_helper() {
        let guard = InterruptGuard::new();
        assert!(!check_interrupt(&guard));
        guard.interrupt();
        assert!(check_interrupt(&guard));
    }
}
