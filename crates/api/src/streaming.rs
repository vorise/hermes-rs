use h_core::{ToolCall, ToolCallFunction};
use serde::{Deserialize, Serialize};

/// Streamed delta from an LLM response.
#[derive(Debug, Clone)]
pub struct Delta {
    pub content: Option<String>,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<StreamToolCall>,
    pub finish_reason: Option<String>,
}

/// A tool call received during streaming.
#[derive(Debug, Clone)]
pub struct StreamToolCall {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments_delta: String,
}

/// Complete API response for non-streaming mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<ApiResponseMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<ApiResponseMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    pub index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponseMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ApiToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<PromptTokenDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<CompletionTokenDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cached_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning_tokens: Option<u64>,
}

impl ApiResponse {
    /// Extract the first choice's text content.
    pub fn text_content(&self) -> Option<String> {
        self.choices.first().and_then(|c| {
            c.message
                .as_ref()
                .or(c.delta.as_ref())
                .and_then(|m| m.content.clone())
        })
    }

    /// Extract tool calls from the response.
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.choices
            .first()
            .and_then(|c| {
                c.message
                    .as_ref()
                    .or(c.delta.as_ref())
                    .and_then(|m| m.tool_calls.as_ref())
            })
            .into_iter()
            .flatten()
            .map(|tc| ToolCall {
                id: tc.id.clone(),
                function: tc.function.clone(),
            })
            .collect()
    }

    /// Convert usage to CostTracker.
    pub fn to_cost_tracker(&self) -> h_core::CostTracker {
        let Some(usage) = &self.usage else {
            return h_core::CostTracker::default();
        };

        let prompt_details = usage.prompt_tokens_details.as_ref();
        let completion_details = usage.completion_tokens_details.as_ref();

        h_core::CostTracker {
            input_tokens: usage.prompt_tokens.unwrap_or(0),
            output_tokens: usage.completion_tokens.unwrap_or(0),
            cache_read_tokens: prompt_details
                .and_then(|d| d.cached_tokens)
                .unwrap_or(0),
            reasoning_tokens: completion_details
                .and_then(|d| d.reasoning_tokens)
                .unwrap_or(0),
            api_call_count: 1,
            ..Default::default()
        }
    }
}

/// Parse a streaming SSE line into a Delta.
pub fn parse_sse_line(line: &str) -> Option<Delta> {
    let line = line.strip_prefix("data: ")?;
    if line.trim() == "[DONE]" {
        return Some(Delta {
            content: None,
            reasoning: None,
            tool_calls: vec![],
            finish_reason: Some("stop".to_string()),
        });
    }

    let chunk: serde_json::Value = serde_json::from_str(line).ok()?;
    let choice = chunk.get("choices").and_then(|c| c.as_array()).and_then(|a| a.first())?;

    let delta = choice.get("delta")?;
    let content: Option<String> = delta
        .get("content")
        .and_then(|v: &serde_json::Value| v.as_str())
        .map(|s: &str| s.to_string());
    let reasoning: Option<String> = delta
        .get("reasoning")
        .or(delta.get("thinking"))
        .and_then(|v: &serde_json::Value| v.as_str())
        .map(|s: &str| s.to_string());

    let finish_reason: Option<String> = choice
        .get("finish_reason")
        .and_then(|v: &serde_json::Value| v.as_str())
        .map(|s: &str| s.to_string());

    let tool_calls = parse_stream_tool_calls(delta.get("tool_calls"))
        .unwrap_or_default();

    Some(Delta {
        content,
        reasoning,
        tool_calls,
        finish_reason,
    })
}

fn parse_stream_tool_calls(value: Option<&serde_json::Value>) -> Option<Vec<StreamToolCall>> {
    let arr = value?.as_array()?;
    let mut calls = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        calls.push(StreamToolCall {
            index: item.get("index").and_then(|v| v.as_u64()).unwrap_or(i as u64) as usize,
            id: item.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
            name: item
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            arguments_delta: item
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
        });
    }
    Some(calls)
}

/// Anthropic-style streaming event.
#[derive(Debug, Clone)]
pub enum AnthropicEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolUseStart { name: String, id: String },
    ToolUseInputDelta(String),
    MessageStop,
}

/// Parse an Anthropic SSE event.
pub fn parse_anthropic_sse(event_type: &str, data: &str) -> Option<AnthropicEvent> {
    match event_type {
        "content_block_delta" => {
            let value: serde_json::Value = serde_json::from_str(data).ok()?;
            let delta = value.get("delta")?;
            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                Some(AnthropicEvent::TextDelta(text.to_string()))
            } else if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                Some(AnthropicEvent::ThinkingDelta(thinking.to_string()))
            } else if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                Some(AnthropicEvent::ToolUseInputDelta(partial.to_string()))
            } else {
                None
            }
        }
        "content_block_start" => {
            let value: serde_json::Value = serde_json::from_str(data).ok()?;
            let block = value.get("content_block")?;
            let type_ = block.get("type")?.as_str()?;
            if type_ == "tool_use" {
                Some(AnthropicEvent::ToolUseStart {
                    name: block.get("name")?.as_str()?.to_string(),
                    id: block.get("id")?.as_str()?.to_string(),
                })
            } else if type_ == "text" {
                Some(AnthropicEvent::TextDelta(
                    block
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                ))
            } else {
                None
            }
        }
        "message_stop" => Some(AnthropicEvent::MessageStop),
        _ => None,
    }
}
