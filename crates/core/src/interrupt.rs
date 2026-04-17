use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, ThreadId};

use parking_lot::Mutex;

/// Thread-scoped interrupt signaling for tool execution.
///
/// Allows safely interrupting long-running tools by signaling
/// an atomic flag that tools should check periodically.
pub struct Interrupt {
    /// Whether an interrupt has been requested.
    requested: AtomicBool,
    /// Thread ID of the agent this interrupt belongs to.
    agent_thread_id: ThreadId,
}

impl Interrupt {
    pub fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            agent_thread_id: thread::current().id(),
        }
    }

    /// Create an interrupt for a specific thread ID (for per-agent isolation).
    pub fn for_thread(thread_id: ThreadId) -> Self {
        Self {
            requested: AtomicBool::new(false),
            agent_thread_id: thread_id,
        }
    }

    /// Request an interrupt (signal the tool to stop).
    pub fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    /// Check if an interrupt has been requested.
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    /// Clear the interrupt signal.
    pub fn clear(&self) {
        self.requested.store(false, Ordering::SeqCst);
    }

    /// Get the agent thread ID this interrupt belongs to.
    pub fn agent_thread_id(&self) -> ThreadId {
        self.agent_thread_id
    }

    /// Check if this interrupt belongs to the current thread.
    pub fn is_for_current_thread(&self) -> bool {
        thread::current().id() == self.agent_thread_id
    }
}

impl Default for Interrupt {
    fn default() -> Self {
        Self::new()
    }
}

/// Manages interrupts for multiple agents with per-agent isolation.
pub struct InterruptManager {
    /// Per-agent interrupts keyed by session ID.
    interrupts: Mutex<std::collections::HashMap<String, Arc<Interrupt>>>,
}

impl InterruptManager {
    pub fn new() -> Self {
        Self {
            interrupts: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Register a new interrupt for a session.
    pub fn register(&self, session_id: &str) -> Arc<Interrupt> {
        let interrupt = Arc::new(Interrupt::new());
        self.interrupts.lock().insert(session_id.to_string(), interrupt.clone());
        interrupt
    }

    /// Register a new interrupt for a specific thread.
    pub fn register_for_thread(&self, session_id: &str, thread_id: ThreadId) -> Arc<Interrupt> {
        let interrupt = Arc::new(Interrupt::for_thread(thread_id));
        self.interrupts.lock().insert(session_id.to_string(), interrupt.clone());
        interrupt
    }

    /// Get the interrupt for a session.
    pub fn get(&self, session_id: &str) -> Option<Arc<Interrupt>> {
        self.interrupts.lock().get(session_id).cloned()
    }

    /// Request an interrupt for a session.
    pub fn request_interrupt(&self, session_id: &str) -> bool {
        if let Some(interrupt) = self.get(session_id) {
            interrupt.request();
            true
        } else {
            false
        }
    }

    /// Clear the interrupt for a session.
    pub fn clear_interrupt(&self, session_id: &str) {
        if let Some(interrupt) = self.get(session_id) {
            interrupt.clear();
        }
    }

    /// Remove the interrupt for a session.
    pub fn remove(&self, session_id: &str) -> bool {
        self.interrupts.lock().remove(session_id).is_some()
    }

    /// Get all active session IDs that have pending interrupts.
    pub fn pending_sessions(&self) -> Vec<String> {
        self.interrupts
            .lock()
            .iter()
            .filter(|(_, i)| i.is_requested())
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Clear all interrupts.
    pub fn clear_all(&self) {
        let interrupts = self.interrupts.lock();
        for (_, interrupt) in interrupts.iter() {
            interrupt.clear();
        }
    }

    /// Get the number of registered interrupts.
    pub fn len(&self) -> usize {
        self.interrupts.lock().len()
    }

    /// Check if any interrupts are registered.
    pub fn is_empty(&self) -> bool {
        self.interrupts.lock().is_empty()
    }
}

impl Default for InterruptManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interrupt_new_is_not_requested() {
        let interrupt = Interrupt::new();
        assert!(!interrupt.is_requested());
    }

    #[test]
    fn test_interrupt_request_and_clear() {
        let interrupt = Interrupt::new();
        assert!(!interrupt.is_requested());
        interrupt.request();
        assert!(interrupt.is_requested());
        interrupt.clear();
        assert!(!interrupt.is_requested());
    }

    #[test]
    fn test_interrupt_for_current_thread() {
        let interrupt = Interrupt::new();
        assert!(interrupt.is_for_current_thread());
    }

    #[test]
    fn test_interrupt_for_specific_thread() {
        let current = thread::current().id();
        let interrupt = Interrupt::for_thread(current);
        assert!(interrupt.is_for_current_thread());
        assert_eq!(interrupt.agent_thread_id(), current);
    }

    #[test]
    fn test_interrupt_manager_register_and_get() {
        let manager = InterruptManager::new();
        let interrupt = manager.register("session-1");
        assert!(manager.get("session-1").is_some());
        assert!(!interrupt.is_requested());
    }

    #[test]
    fn test_interrupt_manager_request_interrupt() {
        let manager = InterruptManager::new();
        manager.register("session-1");

        assert!(manager.request_interrupt("session-1"));
        let interrupt = manager.get("session-1").unwrap();
        assert!(interrupt.is_requested());

        // Non-existent session
        assert!(!manager.request_interrupt("nonexistent"));
    }

    #[test]
    fn test_interrupt_manager_clear_interrupt() {
        let manager = InterruptManager::new();
        manager.register("session-1");
        manager.request_interrupt("session-1");

        let interrupt = manager.get("session-1").unwrap();
        assert!(interrupt.is_requested());

        manager.clear_interrupt("session-1");
        assert!(!interrupt.is_requested());
    }

    #[test]
    fn test_interrupt_manager_remove() {
        let manager = InterruptManager::new();
        manager.register("session-1");
        assert!(manager.remove("session-1"));
        assert!(manager.get("session-1").is_none());
        assert!(!manager.remove("session-1"));
    }

    #[test]
    fn test_interrupt_manager_pending_sessions() {
        let manager = InterruptManager::new();
        manager.register("session-1");
        manager.register("session-2");

        assert!(manager.pending_sessions().is_empty());

        manager.request_interrupt("session-1");
        let pending = manager.pending_sessions();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0], "session-1");
    }

    #[test]
    fn test_interrupt_manager_clear_all() {
        let manager = InterruptManager::new();
        manager.register("session-1");
        manager.register("session-2");
        manager.request_interrupt("session-1");
        manager.request_interrupt("session-2");

        manager.clear_all();
        assert!(manager.pending_sessions().is_empty());
    }

    #[test]
    fn test_interrupt_manager_len() {
        let manager = InterruptManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);

        manager.register("session-1");
        manager.register("session-2");
        assert_eq!(manager.len(), 2);
        assert!(!manager.is_empty());
    }

    #[test]
    fn test_interrupt_default() {
        let interrupt: Interrupt = Default::default();
        assert!(!interrupt.is_requested());
    }

    #[test]
    fn test_interrupt_manager_default() {
        let manager: InterruptManager = Default::default();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_interrupt_thread_safety() {
        let interrupt = Arc::new(Interrupt::new());
        let interrupt_clone = interrupt.clone();

        let handle = thread::spawn(move || {
            // Request interrupt from another thread
            interrupt_clone.request();
        });

        handle.join().unwrap();
        assert!(interrupt.is_requested());
    }

    #[test]
    fn test_interrupt_manager_per_agent_isolation() {
        let manager = InterruptManager::new();
        manager.register("agent-a");
        manager.register("agent-b");

        // Interrupt agent-a, verify agent-b is unaffected
        manager.request_interrupt("agent-a");

        let interrupt_a = manager.get("agent-a").unwrap();
        let interrupt_b = manager.get("agent-b").unwrap();

        assert!(interrupt_a.is_requested());
        assert!(!interrupt_b.is_requested());
    }
}
