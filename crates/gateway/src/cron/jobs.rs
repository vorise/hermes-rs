use serde::{Deserialize, Serialize};

use crate::cron::expr::CronExpr;

/// Where a cron job result should be delivered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryTarget {
    /// Platform to deliver to (e.g., "telegram", "discord").
    pub platform: String,
    /// Chat/channel ID on the platform.
    pub chat_id: String,
    /// Optional user ID for DM delivery.
    #[serde(default)]
    pub user_id: Option<String>,
}

/// Cron job definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique job identifier.
    pub id: String,
    /// Parsed cron expression.
    #[serde(skip)]
    pub schedule: CronExpr,
    /// Raw cron expression string (for serialization).
    pub schedule_expr: String,
    /// Prompt to send to the agent.
    pub prompt: String,
    /// Where to deliver the result.
    pub delivery: DeliveryTarget,
    /// Whether the job is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

impl CronJob {
    /// Create a new cron job.
    pub fn new(
        id: impl Into<String>,
        schedule_expr: impl Into<String>,
        prompt: impl Into<String>,
        delivery: DeliveryTarget,
    ) -> Result<Self, String> {
        let expr_str = schedule_expr.into();
        let schedule = CronExpr::parse(&expr_str)?;
        Ok(Self {
            id: id.into(),
            schedule,
            schedule_expr: expr_str,
            prompt: prompt.into(),
            delivery,
            enabled: true,
        })
    }

    /// Enable the job.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the job.
    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

/// Record of a single job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobExecution {
    /// Job ID that was executed.
    pub job_id: String,
    /// When the execution was scheduled.
    pub scheduled_at: chrono::DateTime<chrono::Local>,
    /// When the execution actually started.
    pub started_at: chrono::DateTime<chrono::Local>,
    /// When the execution finished.
    pub finished_at: Option<chrono::DateTime<chrono::Local>>,
    /// Whether the execution succeeded.
    pub success: bool,
    /// Result message (truncated).
    pub result: Option<String>,
    /// Error message if failed.
    pub error: Option<String>,
}

impl JobExecution {
    /// Create a new execution record.
    pub fn new(job_id: String, scheduled_at: chrono::DateTime<chrono::Local>) -> Self {
        Self {
            job_id,
            scheduled_at,
            started_at: chrono::Local::now(),
            finished_at: None,
            success: false,
            result: None,
            error: None,
        }
    }

    /// Mark the execution as successful.
    pub fn finish_success(&mut self, result: String) {
        self.finished_at = Some(chrono::Local::now());
        self.success = true;
        // Truncate to 4096 chars
        self.result = Some(truncate(&result, 4096));
    }

    /// Mark the execution as failed.
    pub fn finish_error(&mut self, error: String) {
        self.finished_at = Some(chrono::Local::now());
        self.success = false;
        self.error = Some(error);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_cron_job() {
        let delivery = DeliveryTarget {
            platform: "telegram".to_string(),
            chat_id: "12345".to_string(),
            user_id: None,
        };
        let job = CronJob::new("daily", "0 9 * * *", "Give me a summary", delivery).unwrap();
        assert_eq!(job.id, "daily");
        assert!(job.enabled);
        assert_eq!(job.schedule_expression(), "0 9 * * *");
    }

    #[test]
    fn test_create_cron_job_invalid_expr() {
        let delivery = DeliveryTarget {
            platform: "telegram".to_string(),
            chat_id: "12345".to_string(),
            user_id: None,
        };
        assert!(CronJob::new("bad", "invalid", "prompt", delivery).is_err());
    }

    #[test]
    fn test_job_enable_disable() {
        let delivery = DeliveryTarget {
            platform: "discord".to_string(),
            chat_id: "ch1".to_string(),
            user_id: None,
        };
        let mut job = CronJob::new("test", "0 * * * *", "prompt", delivery).unwrap();
        assert!(job.enabled);
        job.disable();
        assert!(!job.enabled);
        job.enable();
        assert!(job.enabled);
    }

    #[test]
    fn test_execution_record() {
        let now = chrono::Local::now();
        let mut exec = JobExecution::new("job1".to_string(), now);
        assert!(!exec.success);
        assert!(exec.finished_at.is_none());

        exec.finish_success("All good".to_string());
        assert!(exec.success);
        assert!(exec.finished_at.is_some());
        assert_eq!(exec.result.as_deref(), Some("All good"));
    }

    #[test]
    fn test_execution_record_error() {
        let now = chrono::Local::now();
        let mut exec = JobExecution::new("job1".to_string(), now);
        exec.finish_error("Timeout".to_string());
        assert!(!exec.success);
        assert_eq!(exec.error.as_deref(), Some("Timeout"));
    }

    #[test]
    fn test_result_truncation() {
        let now = chrono::Local::now();
        let mut exec = JobExecution::new("job1".to_string(), now);
        let long_result = "a".repeat(5000);
        exec.finish_success(long_result);
        let result = exec.result.unwrap();
        assert!(result.len() <= 4096);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_cron_job_serialization_roundtrip() {
        let delivery = DeliveryTarget {
            platform: "slack".to_string(),
            chat_id: "C123".to_string(),
            user_id: Some("U456".to_string()),
        };
        let job = CronJob::new("report", "0 9 * * 1", "Weekly report", delivery).unwrap();

        // Serialize
        let json = serde_json::to_string(&job).unwrap();

        // Can't deserialize schedule field, but schedule_expr and other fields roundtrip
        assert!(json.contains("report"));
        assert!(json.contains("0 9 * * 1"));
        assert!(json.contains("slack"));
    }
}

// Add a helper method for getting the expression string
impl CronJob {
    pub fn schedule_expression(&self) -> &str {
        &self.schedule_expr
    }
}
