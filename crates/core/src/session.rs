//! Session metadata for Hermes Agent.
//!
//! Provides session tracking structures for conversation persistence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{CostTracker, ModelRef};

/// Session metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session ID
    pub id: String,

    /// Source of the session (cli, telegram, discord, etc.)
    pub source: String,

    /// User ID (platform-specific)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Model used for this session
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,

    /// Model configuration JSON
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config: Option<String>,

    /// System prompt used
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,

    /// Parent session ID (for compression chains)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,

    /// Session start timestamp
    pub started_at: DateTime<Utc>,

    /// Session end timestamp
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,

    /// Reason for session end
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,

    /// Message count
    #[serde(default)]
    pub message_count: u64,

    /// Tool call count
    #[serde(default)]
    pub tool_call_count: u64,

    /// Cost tracking
    #[serde(default)]
    pub cost: CostTracker,

    /// Session title (auto-generated or user-set)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Session {
    /// Create a new session with a generated ID.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source: source.into(),
            user_id: None,
            model: None,
            model_config: None,
            system_prompt: None,
            parent_session_id: None,
            started_at: Utc::now(),
            ended_at: None,
            end_reason: None,
            message_count: 0,
            tool_call_count: 0,
            cost: CostTracker::default(),
            title: None,
        }
    }

    /// Create a new session with specified source and user ID.
    pub fn new_with_user(source: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source: source.into(),
            user_id: Some(user_id.into()),
            model: None,
            model_config: None,
            system_prompt: None,
            parent_session_id: None,
            started_at: Utc::now(),
            ended_at: None,
            end_reason: None,
            message_count: 0,
            tool_call_count: 0,
            cost: CostTracker::default(),
            title: None,
        }
    }

    /// Mark the session as ended.
    pub fn end(&mut self, reason: impl Into<String>) {
        self.ended_at = Some(Utc::now());
        self.end_reason = Some(reason.into());
    }

    /// Increment message count.
    pub fn increment_messages(&mut self) {
        self.message_count += 1;
    }

    /// Increment tool call count.
    pub fn increment_tool_calls(&mut self) {
        self.tool_call_count += 1;
    }

    /// Set the session title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    /// Update cost tracking.
    pub fn update_cost(&mut self, cost: &CostTracker) {
        self.cost.merge(cost);
    }
}

/// Session summary for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub source: String,
    pub title: Option<String>,
    pub started_at: DateTime<Utc>,
    pub message_count: u64,
    pub estimated_cost_usd: f64,
}

impl From<&Session> for SessionSummary {
    fn from(session: &Session) -> Self {
        Self {
            id: session.id.clone(),
            source: session.source.clone(),
            title: session.title.clone(),
            started_at: session.started_at,
            message_count: session.message_count,
            estimated_cost_usd: session.cost.estimated_cost_usd,
        }
    }
}

/// Session source types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionSource {
    Cli,
    Telegram,
    Discord,
    Slack,
    WhatsApp,
    Signal,
    Matrix,
    Web,
    Acp,
    Webhook,
    Email,
    Sms,
    Other,
}

impl std::fmt::Display for SessionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionSource::Cli => write!(f, "cli"),
            SessionSource::Telegram => write!(f, "telegram"),
            SessionSource::Discord => write!(f, "discord"),
            SessionSource::Slack => write!(f, "slack"),
            SessionSource::WhatsApp => write!(f, "whatsapp"),
            SessionSource::Signal => write!(f, "signal"),
            SessionSource::Matrix => write!(f, "matrix"),
            SessionSource::Web => write!(f, "web"),
            SessionSource::Acp => write!(f, "acp"),
            SessionSource::Webhook => write!(f, "webhook"),
            SessionSource::Email => write!(f, "email"),
            SessionSource::Sms => write!(f, "sms"),
            SessionSource::Other => write!(f, "other"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_new() {
        let session = Session::new("cli");
        assert!(!session.id.is_empty());
        assert_eq!(session.source, "cli");
        assert!(session.started_at <= Utc::now());
    }

    #[test]
    fn test_session_end() {
        let mut session = Session::new("cli");
        session.end("user_exit");

        assert!(session.ended_at.is_some());
        assert_eq!(session.end_reason, Some("user_exit".to_string()));
    }

    #[test]
    fn test_session_counters() {
        let mut session = Session::new("cli");

        session.increment_messages();
        session.increment_messages();
        session.increment_tool_calls();

        assert_eq!(session.message_count, 2);
        assert_eq!(session.tool_call_count, 1);
    }
}