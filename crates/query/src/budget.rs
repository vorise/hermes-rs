use std::sync::atomic::{AtomicU32, Ordering};

/// Thread-safe iteration budget for the query loop.
pub struct IterationBudget {
    max_total: u32,
    used: AtomicU32,
}

impl IterationBudget {
    pub fn new(max_total: u32) -> Self {
        Self {
            max_total,
            used: AtomicU32::new(0),
        }
    }

    /// Try to consume one iteration. Returns true if budget remains.
    pub fn consume(&self) -> bool {
        let current = self.used.fetch_add(1, Ordering::SeqCst);
        current < self.max_total
    }

    /// Refund one iteration (e.g., for execute_code extra turns).
    pub fn refund(&self) {
        self.used.fetch_sub(1, Ordering::SeqCst);
    }

    /// Remaining iterations.
    pub fn remaining(&self) -> u32 {
        self.max_total.saturating_sub(self.used.load(Ordering::SeqCst))
    }

    /// Total iterations used so far.
    pub fn used(&self) -> u32 {
        self.used.load(Ordering::SeqCst)
    }

    /// Whether budget is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.used.load(Ordering::SeqCst) >= self.max_total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_consumption() {
        let budget = IterationBudget::new(3);
        assert!(budget.consume());
        assert!(budget.consume());
        assert!(budget.consume());
        assert!(!budget.consume());
        assert!(budget.is_exhausted());
    }

    #[test]
    fn test_budget_refund() {
        let budget = IterationBudget::new(2);
        assert!(budget.consume()); // used=1
        assert!(budget.consume()); // used=2
        assert!(!budget.consume()); // used=3, exhausted
        budget.refund(); // used=2
        budget.refund(); // used=1
        assert!(budget.consume()); // used=2, was 1 < 2
    }

    #[test]
    fn test_budget_remaining() {
        let budget = IterationBudget::new(10);
        assert_eq!(budget.remaining(), 10);
        budget.consume();
        budget.consume();
        assert_eq!(budget.remaining(), 8);
    }
}
