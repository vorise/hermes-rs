use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::base::PlatformAdapter;
use crate::cron::jobs::{CronJob, JobExecution};

/// Event sent from the scheduler when a job is due.
#[derive(Debug, Clone)]
pub enum SchedulerEvent {
    /// A cron job is due to run.
    JobDue {
        job: CronJob,
        scheduled_at: chrono::DateTime<chrono::Local>,
    },
}

/// The cron scheduler that manages all cron jobs.
pub struct CronScheduler {
    jobs: Vec<CronJob>,
    history: Vec<JobExecution>,
    handle: Option<JoinHandle<()>>,
    max_history: usize,
}

impl CronScheduler {
    /// Create a new empty scheduler.
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
            history: Vec::new(),
            handle: None,
            max_history: 100,
        }
    }

    /// Set the maximum number of history entries to keep.
    pub fn with_max_history(mut self, n: usize) -> Self {
        self.max_history = n;
        self
    }

    /// Add a cron job.
    pub fn add_job(&mut self, job: CronJob) {
        self.jobs.push(job);
    }

    /// Remove a cron job by ID.
    pub fn remove_job(&mut self, id: &str) -> bool {
        let len_before = self.jobs.len();
        self.jobs.retain(|j| j.id != id);
        self.jobs.len() < len_before
    }

    /// Get a job by ID.
    pub fn get_job(&self, id: &str) -> Option<&CronJob> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// Enable a job by ID.
    pub fn enable_job(&mut self, id: &str) -> bool {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.enable();
            true
        } else {
            false
        }
    }

    /// Disable a job by ID.
    pub fn disable_job(&mut self, id: &str) -> bool {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.disable();
            true
        } else {
            false
        }
    }

    /// List all jobs.
    pub fn list_jobs(&self) -> &[CronJob] {
        &self.jobs
    }

    /// Get execution history for a job.
    pub fn job_history(&self, job_id: &str) -> Vec<&JobExecution> {
        self.history.iter().filter(|e| e.job_id == job_id).collect()
    }

    /// Get all execution history.
    pub fn all_history(&self) -> &[JobExecution] {
        &self.history
    }

    /// Start the scheduler loop.
    ///
    /// This spawns a background task that checks for due jobs and sends
    /// events to the provided channel.
    pub fn start(&mut self, tx: mpsc::UnboundedSender<SchedulerEvent>) {
        if self.handle.is_some() {
            return; // Already running
        }

        let jobs = Arc::new(Mutex::new(self.jobs.clone()));
        let history = Arc::new(Mutex::new(Vec::new()));
        let max_history = self.max_history;

        let handle = tokio::spawn(async move {
            run_scheduler_loop(jobs, history, tx, max_history).await;
        });

        self.handle = Some(handle);
    }

    /// Stop the scheduler.
    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }

    /// Check if the scheduler is running.
    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    /// Record an execution result.
    pub fn record_execution(&mut self, execution: JobExecution) {
        self.history.push(execution);
        // Trim old history
        if self.history.len() > self.max_history {
            let remove = self.history.len() - self.max_history;
            self.history.drain(..remove);
        }
    }

    /// Get the number of enabled jobs.
    pub fn enabled_count(&self) -> usize {
        self.jobs.iter().filter(|j| j.enabled).count()
    }

    /// Get the total number of jobs.
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    /// Check if there are no jobs.
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}

impl Default for CronScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal scheduler loop.
async fn run_scheduler_loop(
    jobs: Arc<Mutex<Vec<CronJob>>>,
    history: Arc<Mutex<Vec<JobExecution>>>,
    tx: mpsc::UnboundedSender<SchedulerEvent>,
    max_history: usize,
) {
    use chrono::Local;
    use tokio::time::{sleep, Duration};

    loop {
        // Check every 30 seconds
        sleep(Duration::from_secs(30)).await;

        let now = Local::now();
        let jobs_snapshot = {
            let guard = jobs.lock();
            guard.iter().filter(|j| j.enabled).cloned().collect::<Vec<_>>()
        };

        for job in &jobs_snapshot {
            // Check if the job matches the current minute (within tolerance)
            if job.schedule.matches(&now) {
                match tx.send(SchedulerEvent::JobDue {
                    job: job.clone(),
                    scheduled_at: now,
                }) {
                    Ok(()) => {
                        let mut hist_guard = history.lock();
                        hist_guard.push(JobExecution::new(job.id.clone(), now));
                        // Trim history
                        if hist_guard.len() > max_history {
                            let remove = hist_guard.len() - max_history;
                            hist_guard.drain(..remove);
                        }
                    }
                    Err(_) => {
                        tracing::error!(job_id = %job.id, "Failed to send scheduler event, channel closed");
                        break;
                    }
                }
            }
        }
    }
}

/// Execute a cron job and deliver the result to the target platform.
///
/// This is called by the gateway when a `SchedulerEvent::JobDue` is received.
/// The job prompt is sent to an LLM (via the auxiliary client) for processing,
/// and the result is delivered to the configured platform.
pub async fn execute_job(
    job: &CronJob,
    platform: &Arc<dyn PlatformAdapter>,
) -> Result<String> {
    tracing::info!(job_id = %job.id, "Executing cron job");

    // Build the user-facing prompt for the job
    let system_prompt = "You are an automated cron job executor. \
        Follow the instructions below and provide a concise, actionable response.";
    let user_prompt = format!(
        "Cron job '{}' scheduled task:\n\n{}",
        job.id, job.prompt
    );

    // Execute the job using the auxiliary LLM client
    let result_text = match execute_job_with_llm(system_prompt, &user_prompt).await {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(job_id = %job.id, error = %e, "LLM execution failed, returning prompt as fallback");
            // Fallback: return the prompt itself if LLM is not available
            format!("Cron job '{}' (LLM unavailable, showing prompt):\n\n{}", job.id, job.prompt)
        }
    };

    // Deliver result to platform
    platform
        .send_message(&job.delivery.chat_id, &result_text)
        .await?;

    tracing::info!(job_id = %job.id, "Cron job completed");
    Ok(result_text)
}

/// Send a prompt to the LLM via the auxiliary client.
async fn execute_job_with_llm(_system_prompt: &str, user_prompt: &str) -> Result<String> {
    use h_api::auxiliary::{AuxiliaryClient, AuxiliaryConfig, AuxiliaryTask};

    let config = AuxiliaryConfig {
        model: String::new(), // Use task-specific default
        provider: "anthropic".to_string(),
        ..Default::default()
    };
    let client = AuxiliaryClient::with_config(config);

    match client.execute(AuxiliaryTask::TextProcessing, user_prompt).await {
        Ok(result) => Ok(result.text),
        Err(e) => {
            // Try OpenAI fallback
            let config2 = AuxiliaryConfig {
                model: String::new(),
                provider: "openai".to_string(),
                ..Default::default()
            };
            let client2 = AuxiliaryClient::with_config(config2);
            match client2.execute(AuxiliaryTask::TextProcessing, user_prompt).await {
                Ok(result) => Ok(result.text),
                Err(e2) => anyhow::bail!("Anthropic: {e}; OpenAI: {e2}"),
            }
        }
    }
}

/// Parse a natural language schedule and create a cron job.
pub fn create_job_from_natural(
    id: impl Into<String>,
    natural: &str,
    prompt: impl Into<String>,
    delivery: crate::cron::jobs::DeliveryTarget,
) -> Result<CronJob, String> {
    let expr = crate::cron::natural::parse_natural(natural)?;
    CronJob::new(id, expr, prompt, delivery)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::jobs::DeliveryTarget;

    #[test]
    fn test_scheduler_new_is_empty() {
        let scheduler = CronScheduler::new();
        assert!(scheduler.is_empty());
        assert!(!scheduler.is_running());
    }

    #[test]
    fn test_add_and_list_jobs() {
        let mut scheduler = CronScheduler::new();
        let delivery = DeliveryTarget {
            platform: "telegram".to_string(),
            chat_id: "123".to_string(),
            user_id: None,
        };
        let job = CronJob::new("daily", "0 9 * * *", "Daily report", delivery).unwrap();
        scheduler.add_job(job);

        assert_eq!(scheduler.len(), 1);
        assert_eq!(scheduler.enabled_count(), 1);
        assert!(scheduler.get_job("daily").is_some());
    }

    #[test]
    fn test_remove_job() {
        let mut scheduler = CronScheduler::new();
        let delivery = DeliveryTarget {
            platform: "telegram".to_string(),
            chat_id: "123".to_string(),
            user_id: None,
        };
        let job = CronJob::new("j1", "0 9 * * *", "prompt", delivery).unwrap();
        scheduler.add_job(job);
        assert!(scheduler.remove_job("j1"));
        assert!(!scheduler.remove_job("j1")); // Already removed
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_enable_disable_job() {
        let mut scheduler = CronScheduler::new();
        let delivery = DeliveryTarget {
            platform: "discord".to_string(),
            chat_id: "ch1".to_string(),
            user_id: None,
        };
        let job = CronJob::new("j1", "0 9 * * *", "prompt", delivery).unwrap();
        scheduler.add_job(job);

        assert!(scheduler.disable_job("j1"));
        assert_eq!(scheduler.enabled_count(), 0);

        assert!(scheduler.enable_job("j1"));
        assert_eq!(scheduler.enabled_count(), 1);

        // Nonexistent job
        assert!(!scheduler.disable_job("nonexistent"));
    }

    #[test]
    fn test_job_history() {
        let mut scheduler = CronScheduler::new();
        let now = chrono::Local::now();
        let exec1 = JobExecution::new("j1".to_string(), now);
        let exec2 = JobExecution::new("j1".to_string(), now);
        let exec3 = JobExecution::new("j2".to_string(), now);

        scheduler.record_execution(exec1);
        scheduler.record_execution(exec2);
        scheduler.record_execution(exec3);

        assert_eq!(scheduler.job_history("j1").len(), 2);
        assert_eq!(scheduler.job_history("j2").len(), 1);
        assert_eq!(scheduler.job_history("j3").len(), 0);
        assert_eq!(scheduler.all_history().len(), 3);
    }

    #[test]
    fn test_history_trimming() {
        let mut scheduler = CronScheduler::new().with_max_history(3);
        let now = chrono::Local::now();

        for i in 0..5 {
            let exec = JobExecution::new(format!("j{i}"), now);
            scheduler.record_execution(exec);
        }

        assert_eq!(scheduler.all_history().len(), 3);
    }

    #[test]
    fn test_create_job_from_natural() {
        let delivery = DeliveryTarget {
            platform: "slack".to_string(),
            chat_id: "C123".to_string(),
            user_id: None,
        };
        let job = create_job_from_natural("nat", "every 5 minutes", "Check status", delivery).unwrap();
        assert_eq!(job.id, "nat");
        assert_eq!(job.schedule_expression(), "*/5 * * * *");
    }

    #[test]
    fn test_create_job_from_natural_invalid() {
        let delivery = DeliveryTarget {
            platform: "telegram".to_string(),
            chat_id: "123".to_string(),
            user_id: None,
        };
        assert!(create_job_from_natural("bad", "once upon a time", "prompt", delivery).is_err());
    }
}
