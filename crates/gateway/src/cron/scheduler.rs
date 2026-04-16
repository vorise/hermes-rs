//! Cron Scheduler
//!
//! Manages scheduled job execution with tokio timers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::RwLock;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use thiserror::Error;
use tracing::{info, warn, error};

use super::schedule::{CronSchedule, ScheduleError};
use super::job::CronJob;
use super::history::{JobHistory, HistoryEntry};
use crate::event::GatewayEvent;

/// Scheduler error.
#[derive(Debug, Error)]
pub enum SchedulerError {
    /// Schedule parsing error.
    #[error("Schedule error: {0}")]
    Schedule(#[from] ScheduleError),

    /// Job not found.
    #[error("Job not found: {0}")]
    JobNotFound(String),

    /// Scheduler already running.
    #[error("Scheduler already running")]
    AlreadyRunning,

    /// Scheduler not running.
    #[error("Scheduler not running")]
    NotRunning,

    /// Send error.
    #[error("Failed to send event: {0}")]
    SendError(String),
}

/// Scheduler state.
struct SchedulerState {
    /// Job schedules parsed.
    schedules: HashMap<String, CronSchedule>,

    /// Next execution times.
    next_times: HashMap<String, std::time::SystemTime>,
}

/// Cron scheduler.
pub struct CronScheduler {
    /// Registered jobs.
    jobs: Arc<RwLock<HashMap<String, CronJob>>>,

    /// Execution history.
    history: JobHistory,

    /// Scheduler state.
    state: Arc<RwLock<SchedulerState>>,

    /// Background task handle.
    handle: Option<JoinHandle<()>>,

    /// Running flag.
    running: Arc<RwLock<bool>>,
}

impl CronScheduler {
    /// Create new scheduler.
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            history: JobHistory::new(),
            state: Arc::new(RwLock::new(SchedulerState {
                schedules: HashMap::new(),
                next_times: HashMap::new(),
            })),
            handle: None,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Add a job.
    pub fn add_job(&mut self, job: CronJob) -> Result<(), SchedulerError> {
        let schedule = CronSchedule::parse(&job.schedule)?;

        // Calculate next execution time
        let next_time = schedule.next_after(std::time::SystemTime::now());

        let job_id = job.id.clone();

        {
            let mut jobs = self.jobs.write();
            jobs.insert(job_id.clone(), job);
        }

        {
            let mut state = self.state.write();
            state.schedules.insert(job_id.clone(), schedule);
            state.next_times.insert(job_id.clone(), next_time);
        }

        info!("Added cron job: {} (next run in {:?})", job_id, next_time);
        Ok(())
    }

    /// Remove a job.
    pub fn remove_job(&mut self, id: &str) -> Result<CronJob, SchedulerError> {
        {
            let mut state = self.state.write();
            state.schedules.remove(id);
            state.next_times.remove(id);
        }

        let mut jobs = self.jobs.write();
        jobs.remove(id)
            .ok_or_else(|| SchedulerError::JobNotFound(id.to_string()))
    }

    /// Get a job by ID.
    pub fn get_job(&self, id: &str) -> Option<CronJob> {
        let jobs = self.jobs.read();
        jobs.get(id).cloned()
    }

    /// Update a job.
    pub fn update_job(&mut self, job: CronJob) -> Result<(), SchedulerError> {
        self.remove_job(&job.id)?;
        self.add_job(job)?;
        Ok(())
    }

    /// Enable/disable a job.
    pub fn set_job_enabled(&mut self, id: &str, enabled: bool) -> Result<(), SchedulerError> {
        let mut jobs = self.jobs.write();
        let job = jobs.get_mut(id)
            .ok_or_else(|| SchedulerError::JobNotFound(id.to_string()))?;
        job.set_enabled(enabled);
        info!("Job {} enabled: {}", id, enabled);
        Ok(())
    }

    /// Get all jobs.
    pub fn all_jobs(&self) -> Vec<CronJob> {
        let jobs = self.jobs.read();
        jobs.values().cloned().collect()
    }

    /// Get enabled jobs.
    pub fn enabled_jobs(&self) -> Vec<CronJob> {
        let jobs = self.jobs.read();
        jobs.values().filter(|j| j.enabled).cloned().collect()
    }

    /// Count jobs.
    pub fn count(&self) -> usize {
        let jobs = self.jobs.read();
        jobs.len()
    }

    /// Get history for a job.
    pub fn get_history(&self, job_id: &str) -> Vec<HistoryEntry> {
        self.history.get_history(job_id)
    }

    /// Get job statistics.
    pub fn job_stats(&self, job_id: &str) -> JobStats {
        JobStats {
            success_count: self.history.success_count(job_id),
            failure_count: self.history.failure_count(job_id),
            last_execution: self.history.last_execution(job_id),
            next_execution: self.state.read().next_times.get(job_id).copied(),
        }
    }

    /// Start the scheduler.
    pub fn start(&mut self, gateway_tx: UnboundedSender<GatewayEvent>) -> Result<(), SchedulerError> {
        if *self.running.read() {
            return Err(SchedulerError::AlreadyRunning);
        }

        *self.running.write() = true;

        let jobs = self.jobs.clone();
        let state = self.state.clone();
        let history = Arc::new(self.history.clone());  // Need Arc for sharing
        let running = self.running.clone();

        let handle = tokio::spawn(async move {
            scheduler_loop(jobs, state, history, gateway_tx, running).await;
        });

        self.handle = Some(handle);
        info!("Cron scheduler started");
        Ok(())
    }

    /// Stop the scheduler.
    pub fn stop(&mut self) -> Result<(), SchedulerError> {
        if !*self.running.read() {
            return Err(SchedulerError::NotRunning);
        }

        *self.running.write() = false;

        if let Some(handle) = self.handle.take() {
            handle.abort();
        }

        info!("Cron scheduler stopped");
        Ok(())
    }

    /// Check if scheduler is running.
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// Clear all jobs.
    pub fn clear(&mut self) {
        let mut jobs = self.jobs.write();
        jobs.clear();

        let mut state = self.state.write();
        state.schedules.clear();
        state.next_times.clear();
    }

    /// Trigger a job manually.
    pub fn trigger_job(&self, id: &str, gateway_tx: &UnboundedSender<GatewayEvent>) -> Result<(), SchedulerError> {
        let jobs = self.jobs.read();
        let job = jobs.get(id)
            .ok_or_else(|| SchedulerError::JobNotFound(id.to_string()))?;

        if !job.enabled {
            warn!("Job {} is disabled, skipping manual trigger", id);
            return Ok(());
        }

        execute_job(job, gateway_tx, &self.history)?;
        Ok(())
    }
}

/// Job statistics.
#[derive(Debug)]
pub struct JobStats {
    /// Successful execution count.
    pub success_count: usize,

    /// Failed execution count.
    pub failure_count: usize,

    /// Last execution entry.
    pub last_execution: Option<HistoryEntry>,

    /// Next scheduled execution time.
    pub next_execution: Option<std::time::SystemTime>,
}

/// Main scheduler loop.
async fn scheduler_loop(
    jobs: Arc<RwLock<HashMap<String, CronJob>>>,
    state: Arc<RwLock<SchedulerState>>,
    history: Arc<JobHistory>,
    gateway_tx: UnboundedSender<GatewayEvent>,
    running: Arc<RwLock<bool>>,
) {
    info!("Scheduler loop started");

    while *running.read() {
        // Find jobs that need to run
        let now = std::time::SystemTime::now();
        let to_run: Vec<String> = {
            let state = state.read();
            state.next_times.iter()
                .filter(|(_, next)| **next <= now)
                .map(|(id, _)| id.clone())
                .collect()
        };

        // Execute jobs
        for job_id in to_run {
            let job = {
                let jobs = jobs.read();
                jobs.get(&job_id).cloned()
            };

            if let Some(job) = job {
                if job.enabled {
                    if let Err(e) = execute_job(&job, &gateway_tx, &history) {
                        error!("Failed to execute job {}: {}", job_id, e);
                    }
                }

                // Update next execution time
                let schedule = {
                    let state = state.read();
                    state.schedules.get(&job_id).cloned()
                };

                if let Some(schedule) = schedule {
                    let next = schedule.next_after(now);
                    let mut state = state.write();
                    state.next_times.insert(job_id, next);
                }
            }
        }

        // Sleep for a short interval
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    info!("Scheduler loop ended");
}

/// Execute a single job.
fn execute_job(
    job: &CronJob,
    gateway_tx: &UnboundedSender<GatewayEvent>,
    history: &JobHistory,
) -> Result<(), SchedulerError> {
    info!("Executing job: {}", job.id);

    // Record execution start
    let mut entry = history.record_start(&job.id);

    // Create the event to send
    let event = GatewayEvent::CronTrigger {
        job_id: job.id.clone(),
        prompt: job.prompt.clone(),
        delivery: job.delivery.clone(),
    };

    // Send to gateway
    if let Err(e) = gateway_tx.send(event) {
        entry.complete_failure(format!("Failed to send event: {}", e));
        history.update(entry);
        return Err(SchedulerError::SendError(e.to_string()));
    }

    // Mark as success (actual execution handled by gateway)
    entry.complete_success("Triggered successfully");
    history.update(entry);

    Ok(())
}

impl Default for CronScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::job::DeliveryTarget;

    #[test]
    fn test_scheduler_new() {
        let scheduler = CronScheduler::new();
        assert_eq!(scheduler.count(), 0);
        assert!(!scheduler.is_running());
    }

    #[test]
    fn test_add_job() {
        let mut scheduler = CronScheduler::new();
        let job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check logs",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        );

        scheduler.add_job(job).unwrap();
        assert_eq!(scheduler.count(), 1);
    }

    #[test]
    fn test_remove_job() {
        let mut scheduler = CronScheduler::new();
        let job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check logs",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        );

        scheduler.add_job(job).unwrap();
        let removed = scheduler.remove_job("job-1").unwrap();
        assert_eq!(removed.id, "job-1");
        assert_eq!(scheduler.count(), 0);
    }

    #[test]
    fn test_remove_job_not_found() {
        let mut scheduler = CronScheduler::new();
        let result = scheduler.remove_job("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_job() {
        let mut scheduler = CronScheduler::new();
        let job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check logs",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        );

        scheduler.add_job(job.clone()).unwrap();
        let found = scheduler.get_job("job-1").unwrap();
        assert_eq!(found.id, job.id);
    }

    #[test]
    fn test_update_job() {
        let mut scheduler = CronScheduler::new();
        let job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check logs",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        );

        scheduler.add_job(job).unwrap();

        let updated = CronJob::new(
            "job-1",
            "0 10 * * *",
            "New prompt",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        );

        scheduler.update_job(updated).unwrap();
        let found = scheduler.get_job("job-1").unwrap();
        assert_eq!(found.schedule, "0 10 * * *");
    }

    #[test]
    fn test_set_job_enabled() {
        let mut scheduler = CronScheduler::new();
        let job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check logs",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        );

        scheduler.add_job(job).unwrap();
        scheduler.set_job_enabled("job-1", false).unwrap();

        let found = scheduler.get_job("job-1").unwrap();
        assert!(!found.enabled);
    }

    #[test]
    fn test_enabled_jobs() {
        let mut scheduler = CronScheduler::new();
        let job1 = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check logs",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        );
        let mut job2 = CronJob::new(
            "job-2",
            "0 10 * * *",
            "Check status",
            DeliveryTarget::new_platform("telegram", "chat-2"),
        );
        job2.set_enabled(false);

        scheduler.add_job(job1).unwrap();
        scheduler.add_job(job2).unwrap();

        let enabled = scheduler.enabled_jobs();
        assert_eq!(enabled.len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut scheduler = CronScheduler::new();
        scheduler.add_job(CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check logs",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        ).clone()).unwrap();

        scheduler.clear();
        assert_eq!(scheduler.count(), 0);
    }

    #[test]
    fn test_invalid_schedule() {
        let mut scheduler = CronScheduler::new();
        let job = CronJob::new(
            "job-1",
            "invalid",
            "Check logs",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        );

        let result = scheduler.add_job(job);
        assert!(result.is_err());
    }

    #[test]
    fn test_job_stats() {
        let mut scheduler = CronScheduler::new();
        let job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check logs",
            DeliveryTarget::new_platform("telegram", "chat-1"),
        );

        scheduler.add_job(job).unwrap();
        let stats = scheduler.job_stats("job-1");
        assert!(stats.next_execution.is_some());
    }
}