//! Job Execution History
//!
//! Tracks execution history for scheduled jobs.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// Execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStatus {
    /// Job executed successfully.
    Success,

    /// Job failed.
    Failed,

    /// Job was skipped (disabled or invalid).
    Skipped,

    /// Job is currently running.
    Running,
}

/// History entry for a single execution.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Job ID.
    pub job_id: String,

    /// Execution ID (unique per run).
    pub execution_id: String,

    /// Execution start time.
    pub started_at: u64,

    /// Execution end time (None if running).
    pub ended_at: Option<u64>,

    /// Execution status.
    pub status: ExecutionStatus,

    /// Result preview (first 100 chars).
    pub result_preview: Option<String>,

    /// Error message (if failed).
    pub error: Option<String>,

    /// Retry count.
    pub retry_count: u32,
}

impl HistoryEntry {
    /// Create a new running entry.
    pub fn running(job_id: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            job_id: job_id.into(),
            execution_id: uuid::Uuid::new_v4().to_string(),
            started_at: now,
            ended_at: None,
            status: ExecutionStatus::Running,
            result_preview: None,
            error: None,
            retry_count: 0,
        }
    }

    /// Mark as success.
    pub fn complete_success(&mut self, result: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.ended_at = Some(now);
        self.status = ExecutionStatus::Success;
        self.result_preview = Some(result.chars().take(100).collect());
    }

    /// Mark as failed.
    pub fn complete_failure(&mut self, error: impl Into<String>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.ended_at = Some(now);
        self.status = ExecutionStatus::Failed;
        self.error = Some(error.into());
    }

    /// Mark as skipped.
    pub fn mark_skipped(&mut self, reason: impl Into<String>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.ended_at = Some(now);
        self.status = ExecutionStatus::Skipped;
        self.error = Some(reason.into());
    }

    /// Increment retry count.
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }

    /// Get execution duration in seconds.
    pub fn duration_seconds(&self) -> Option<u64> {
        self.ended_at.map(|end| end - self.started_at)
    }

    /// Check if execution is complete.
    pub fn is_complete(&self) -> bool {
        self.ended_at.is_some()
    }

    /// Check if execution was successful.
    pub fn was_successful(&self) -> bool {
        self.status == ExecutionStatus::Success
    }
}

/// Job execution history manager.
#[derive(Clone)]
pub struct JobHistory {
    /// History entries by job ID.
    entries: Arc<RwLock<HashMap<String, Vec<HistoryEntry>>>>,

    /// Maximum entries per job.
    max_entries_per_job: usize,
}

impl JobHistory {
    /// Create new history manager.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            max_entries_per_job: 100,
        }
    }

    /// Create with custom max entries.
    pub fn with_max_entries(max: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            max_entries_per_job: max,
        }
    }

    /// Record a new execution start.
    pub fn record_start(&self, job_id: &str) -> HistoryEntry {
        let entry = HistoryEntry::running(job_id);

        {
            let mut entries = self.entries.write();
            let job_entries = entries.entry(job_id.to_string()).or_insert_with(Vec::new);
            job_entries.push(entry.clone());

            // Trim if exceeding max
            if job_entries.len() > self.max_entries_per_job {
                job_entries.remove(0);
            }
        }

        entry
    }

    /// Update an execution entry.
    pub fn update(&self, entry: HistoryEntry) {
        let mut entries = self.entries.write();
        if let Some(job_entries) = entries.get_mut(&entry.job_id) {
            // Find and update the entry
            for e in job_entries.iter_mut() {
                if e.execution_id == entry.execution_id {
                    *e = entry;
                    break;
                }
            }
        }
    }

    /// Get history for a job.
    pub fn get_history(&self, job_id: &str) -> Vec<HistoryEntry> {
        let entries = self.entries.read();
        entries.get(job_id).cloned().unwrap_or_default()
    }

    /// Get the last execution for a job.
    pub fn last_execution(&self, job_id: &str) -> Option<HistoryEntry> {
        let entries = self.entries.read();
        entries.get(job_id).and_then(|v| v.last().cloned())
    }

    /// Get all entries.
    pub fn all_entries(&self) -> HashMap<String, Vec<HistoryEntry>> {
        let entries = self.entries.read();
        entries.clone()
    }

    /// Get success count for a job.
    pub fn success_count(&self, job_id: &str) -> usize {
        let entries = self.entries.read();
        entries.get(job_id)
            .map(|v| v.iter().filter(|e| e.was_successful()).count())
            .unwrap_or(0)
    }

    /// Get failure count for a job.
    pub fn failure_count(&self, job_id: &str) -> usize {
        let entries = self.entries.read();
        entries.get(job_id)
            .map(|v| v.iter().filter(|e| e.status == ExecutionStatus::Failed).count())
            .unwrap_or(0)
    }

    /// Clear history for a job.
    pub fn clear_job(&self, job_id: &str) {
        let mut entries = self.entries.write();
        entries.remove(job_id);
    }

    /// Clear all history.
    pub fn clear(&self) {
        let mut entries = self.entries.write();
        entries.clear();
    }

    /// Count total entries.
    pub fn count(&self) -> usize {
        let entries = self.entries.read();
        entries.values().map(|v| v.len()).sum()
    }
}

impl Default for JobHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_entry_running() {
        let entry = HistoryEntry::running("job-1");
        assert_eq!(entry.job_id, "job-1");
        assert_eq!(entry.status, ExecutionStatus::Running);
        assert!(entry.ended_at.is_none());
    }

    #[test]
    fn test_history_entry_complete_success() {
        let mut entry = HistoryEntry::running("job-1");
        entry.complete_success("Task completed successfully");
        assert_eq!(entry.status, ExecutionStatus::Success);
        assert!(entry.ended_at.is_some());
        assert!(entry.result_preview.is_some());
    }

    #[test]
    fn test_history_entry_complete_failure() {
        let mut entry = HistoryEntry::running("job-1");
        entry.complete_failure("Network error");
        assert_eq!(entry.status, ExecutionStatus::Failed);
        assert!(entry.error.is_some());
    }

    #[test]
    fn test_history_entry_mark_skipped() {
        let mut entry = HistoryEntry::running("job-1");
        entry.mark_skipped("Job disabled");
        assert_eq!(entry.status, ExecutionStatus::Skipped);
    }

    #[test]
    fn test_job_history_new() {
        let history = JobHistory::new();
        assert_eq!(history.count(), 0);
    }

    #[test]
    fn test_job_history_record_start() {
        let history = JobHistory::new();
        let entry = history.record_start("job-1");
        assert_eq!(entry.status, ExecutionStatus::Running);
        assert_eq!(history.count(), 1);
    }

    #[test]
    fn test_job_history_update() {
        let history = JobHistory::new();
        let entry = history.record_start("job-1");

        let mut updated = entry.clone();
        updated.complete_success("Done");

        history.update(updated);

        let last = history.last_execution("job-1").unwrap();
        assert_eq!(last.status, ExecutionStatus::Success);
    }

    #[test]
    fn test_job_history_get_history() {
        let history = JobHistory::new();
        history.record_start("job-1");
        history.record_start("job-1");

        let entries = history.get_history("job-1");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_job_history_counts() {
        let history = JobHistory::new();

        let mut entry1 = history.record_start("job-1");
        entry1.complete_success("Done");
        history.update(entry1);

        let mut entry2 = history.record_start("job-1");
        entry2.complete_failure("Error");
        history.update(entry2);

        assert_eq!(history.success_count("job-1"), 1);
        assert_eq!(history.failure_count("job-1"), 1);
    }

    #[test]
    fn test_job_history_max_entries() {
        let history = JobHistory::with_max_entries(5);

        for _ in 0..10 {
            history.record_start("job-1");
        }

        let entries = history.get_history("job-1");
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn test_job_history_clear() {
        let history = JobHistory::new();
        history.record_start("job-1");
        history.clear_job("job-1");
        assert_eq!(history.count(), 0);
    }
}