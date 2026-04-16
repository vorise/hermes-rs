use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Message;

/// Session metadata stored in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    pub started_at: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Session {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source: source.into(),
            user_id: None,
            model: None,
            model_config: None,
            system_prompt: None,
            parent_session_id: None,
            started_at: Utc::now().timestamp_millis() as f64 / 1000.0,
            ended_at: None,
            end_reason: None,
            message_count: 0,
            tool_call_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            billing_provider: None,
            billing_base_url: None,
            estimated_cost_usd: None,
            title: None,
        }
    }
}

/// A stored message with its database row id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<String>, // JSON
    pub tool_name: Option<String>,
    pub timestamp: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<String>,
}

impl StoredMessage {
    pub fn to_message(&self) -> anyhow::Result<Message> {
        let role = match self.role.as_str() {
            "system" => crate::Role::System,
            "user" => crate::Role::User,
            "assistant" => crate::Role::Assistant,
            "tool" => crate::Role::Tool,
            _ => anyhow::bail!("unknown role: {}", self.role),
        };

        let content = self.content.clone().map(crate::Content::Text);

        let tool_calls = if let Some(ref json) = self.tool_calls {
            Some(serde_json::from_str(json)?)
        } else {
            None
        };

        Ok(Message {
            role,
            content,
            tool_calls,
            tool_call_id: self.tool_call_id.clone(),
            name: self.tool_name.clone(),
            reasoning: self.reasoning.clone(),
        })
    }
}

/// Search result from FTS5.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub session_id: String,
    pub message_id: i64,
    pub content: String,
    pub score: f64,
    pub timestamp: DateTime<Utc>,
}

/// Session summary for display.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub message_count: i64,
    pub started_at: DateTime<Utc>,
    pub estimated_cost_usd: Option<f64>,
    pub source: String,
}
