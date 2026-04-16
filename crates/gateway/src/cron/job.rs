//! Cron Job Definition
//!
//! Defines scheduled jobs for automated prompts.

use serde::{Deserialize, Serialize};

/// Delivery target for job results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryTarget {
    /// Send to a specific platform chat.
    PlatformChat {
        platform: String,
        chat_id: String,
    },

    /// Send to a DM (paired from group).
    DmChat {
        platform: String,
        dm_chat_id: String,
    },

    /// Send to all connected platforms.
    AllPlatforms,

    /// Store in session for later retrieval.
    Session {
        session_id: String,
    },
}

impl DeliveryTarget {
    /// Create a platform chat target.
    pub fn new_platform(platform: impl Into<String>, chat_id: impl Into<String>) -> Self {
        Self::PlatformChat {
            platform: platform.into(),
            chat_id: chat_id.into(),
        }
    }

    /// Create a DM chat target.
    pub fn new_dm(platform: impl Into<String>, dm_chat_id: impl Into<String>) -> Self {
        Self::DmChat {
            platform: platform.into(),
            dm_chat_id: dm_chat_id.into(),
        }
    }

    /// Create a session target.
    pub fn new_session(session_id: impl Into<String>) -> Self {
        Self::Session {
            session_id: session_id.into(),
        }
    }

    /// Create an all-platforms target.
    pub fn new_all_platforms() -> Self {
        Self::AllPlatforms
    }

    /// Get the platform name if applicable.
    pub fn get_platform(&self) -> Option<&str> {
        match self {
            Self::PlatformChat { platform, .. } => Some(platform),
            Self::DmChat { platform, .. } => Some(platform),
            Self::AllPlatforms => None,
            Self::Session { .. } => None,
        }
    }

    /// Get the chat ID if applicable.
    pub fn get_chat_id(&self) -> Option<&str> {
        match self {
            Self::PlatformChat { chat_id, .. } => Some(chat_id),
            Self::DmChat { dm_chat_id, .. } => Some(dm_chat_id),
            Self::AllPlatforms => None,
            Self::Session { .. } => None,
        }
    }
}

/// Cron job configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique job ID.
    pub id: String,

    /// Job name for display.
    #[serde(default)]
    pub name: String,

    /// Cron schedule expression.
    pub schedule: String,

    /// Prompt to send to the agent.
    pub prompt: String,

    /// Where to deliver the result.
    pub delivery: DeliveryTarget,

    /// Is the job enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Maximum retries on failure.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Job description.
    #[serde(default)]
    pub description: String,

    /// Tags for filtering.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_max_retries() -> u32 {
    3
}

impl CronJob {
    /// Create a new cron job.
    pub fn new(
        id: impl Into<String>,
        schedule: impl Into<String>,
        prompt: impl Into<String>,
        delivery: DeliveryTarget,
    ) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            schedule: schedule.into(),
            prompt: prompt.into(),
            delivery,
            enabled: true,
            max_retries: 3,
            description: String::new(),
            tags: Vec::new(),
        }
    }

    /// Set job name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set job description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Add a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Set max retries.
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Enable/disable the job.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if job matches a tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delivery_target_platform() {
        let target = DeliveryTarget::new_platform("telegram", "chat-123");
        assert_eq!(target.get_platform(), Some("telegram"));
        assert_eq!(target.get_chat_id(), Some("chat-123"));
    }

    #[test]
    fn test_delivery_target_dm() {
        let target = DeliveryTarget::new_dm("telegram", "dm-456");
        assert_eq!(target.get_platform(), Some("telegram"));
        assert_eq!(target.get_chat_id(), Some("dm-456"));
    }

    #[test]
    fn test_delivery_target_session() {
        let target = DeliveryTarget::new_session("session-789");
        assert_eq!(target.get_platform(), None);
        assert_eq!(target.get_chat_id(), None);
    }

    #[test]
    fn test_cron_job_new() {
        let job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check the logs",
            DeliveryTarget::new_platform("telegram", "chat-123"),
        );
        assert_eq!(job.id, "job-1");
        assert_eq!(job.schedule, "0 9 * * *");
        assert!(job.enabled);
        assert_eq!(job.max_retries, 3);
    }

    #[test]
    fn test_cron_job_with_options() {
        let job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check the logs",
            DeliveryTarget::new_platform("telegram", "chat-123"),
        )
        .with_name("Daily Log Check")
        .with_description("Checks logs for errors")
        .with_tag("monitoring")
        .with_max_retries(5);

        assert_eq!(job.name, "Daily Log Check");
        assert_eq!(job.description, "Checks logs for errors");
        assert!(job.has_tag("monitoring"));
        assert_eq!(job.max_retries, 5);
    }

    #[test]
    fn test_cron_job_set_enabled() {
        let mut job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check the logs",
            DeliveryTarget::new_all_platforms(),
        );
        job.set_enabled(false);
        assert!(!job.enabled);
    }

    #[test]
    fn test_cron_job_serialize_deserialize() {
        let job = CronJob::new(
            "job-1",
            "0 9 * * *",
            "Check the logs",
            DeliveryTarget::new_platform("telegram", "chat-123"),
        )
        .with_name("Test Job");

        let json = serde_json::to_string(&job).unwrap();
        let parsed: CronJob = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, job.id);
        assert_eq!(parsed.schedule, job.schedule);
        assert_eq!(parsed.name, job.name);
    }
}