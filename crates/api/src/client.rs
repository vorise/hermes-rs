//! API Client for Hermes Agent.
//!
//! Multi-provider LLM API client supporting OpenAI Chat Completions
//! and Anthropic Messages API protocols.

use anyhow::{anyhow, Result};
use futures::{Stream, StreamExt};
use h_core::{
    ApiResponse, ApiUsage, Content, CostTracker, Message, ModelId, ModelRef,
    ProviderId, Role, ToolCall, ToolCallFunction, ToolDefinition,
};
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::task::{Context, Poll};
use tracing::debug;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt as TokioStreamExt;

use crate::registry::{resolve_api_key, resolve_base_url, ProviderRegistry};

/// API mode/protocol to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApiMode {
    #[default]
    ChatCompletions,
    AnthropicMessages,
}

/// Configuration for API client.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub provider: ProviderId,
    pub model: ModelId,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub mode: Option<ApiMode>,
    pub timeout_seconds: u64,
}

impl ApiConfig {
    pub fn from_model_ref(model_ref: ModelRef) -> Self {
        Self {
            provider: model_ref.provider,
            model: model_ref.model,
            base_url: None,
            api_key: None,
            mode: None,
            timeout_seconds: 120,
        }
    }

    pub fn resolve(&self, registry: &ProviderRegistry) -> Result<ResolvedApiConfig> {
        let info = registry.get(&self.provider)
            .ok_or_else(|| anyhow!("Provider not found: {}", self.provider))?;

        Ok(ResolvedApiConfig {
            provider: self.provider.clone(),
            model: self.model.clone(),
            base_url: self.base_url.clone().unwrap_or_else(|| resolve_base_url(info)),
            api_key: self.api_key.clone().or_else(|| resolve_api_key(info))
                .ok_or_else(|| anyhow!("No API key for {}", self.provider))?,
            mode: self.mode.unwrap_or_else(|| detect_api_mode(&self.provider)),
            timeout_seconds: self.timeout_seconds,
            supports_tools: info.supports_tools,
            supports_vision: info.supports_vision,
            supports_reasoning: info.supports_reasoning,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedApiConfig {
    pub provider: ProviderId,
    pub model: ModelId,
    pub base_url: String,
    pub api_key: String,
    pub mode: ApiMode,
    pub timeout_seconds: u64,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_reasoning: bool,
}

fn detect_api_mode(provider: &ProviderId) -> ApiMode {
    match provider.as_str() {
        "anthropic" => ApiMode::AnthropicMessages,
        _ => ApiMode::ChatCompletions,
    }
}

/// LLM API Client.
pub struct ApiClient {
    http: Client,
    config: ResolvedApiConfig,
}

impl ApiClient {
    pub fn new(config: ResolvedApiConfig) -> Result<Self> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()?;
        Ok(Self { http, config })
    }

    pub fn from_config(config: ApiConfig, registry: &ProviderRegistry) -> Result<Self> {
        Self::new(config.resolve(registry)?)
    }

    pub fn model_ref(&self) -> ModelRef {
        ModelRef { provider: self.config.provider.clone(), model: self.config.model.clone() }
    }

    pub fn mode(&self) -> ApiMode { self.config.mode }

    /// Non-streaming chat request.
    pub async fn chat(&self, messages: &[Message], tools: &[ToolDefinition]) -> Result<ApiResponse> {
        match self.config.mode {
            ApiMode::ChatCompletions => self.openai_chat(messages, tools).await,
            ApiMode::AnthropicMessages => self.anthropic_chat(messages, tools).await,
        }
    }

    /// Streaming chat request.
    pub async fn chat_stream(&self, messages: &[Message], tools: &[ToolDefinition]) -> Result<SseStream> {
        match self.config.mode {
            ApiMode::ChatCompletions => self.openai_stream(messages, tools).await,
            ApiMode::AnthropicMessages => self.anthropic_stream(messages, tools).await,
        }
    }

    // ---- OpenAI Chat Completions ----

    async fn openai_chat(&self, messages: &[Message], tools: &[ToolDefinition]) -> Result<ApiResponse> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let body = self.build_openai_body(messages, tools, false);

        debug!("OpenAI POST {}", url);
        let resp = self.http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(classify_error(resp.status(), &resp.text().await?));
        }

        let json: OpenAIResp = resp.json().await?;
        let choice = json.choices.first().ok_or_else(|| anyhow!("No choices"))?;

        Ok(ApiResponse {
            id: json.id,
            model: ModelId::new(&json.model),
            choices: vec![h_core::ApiChoice {
                index: 0,
                message: Message {
                    role: Role::Assistant,
                    content: choice.message.content.clone().map(Content::Text),
                    tool_calls: choice.message.tool_calls.clone().map(|tc| tc.into_iter().map(|t| ToolCall {
                        id: t.id,
                        function: ToolCallFunction { name: t.function.name, arguments: t.function.arguments },
                    }).collect()),
                    tool_call_id: None,
                    name: None,
                    reasoning: None,
                },
                finish_reason: choice.finish_reason.clone(),
            }],
            usage: Some(ApiUsage {
                prompt_tokens: json.usage.prompt_tokens,
                completion_tokens: json.usage.completion_tokens,
                total_tokens: json.usage.total_tokens,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        })
    }

    async fn openai_stream(&self, messages: &[Message], tools: &[ToolDefinition]) -> Result<SseStream> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let body = self.build_openai_body(messages, tools, true);

        let resp = self.http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(classify_error(resp.status(), &resp.text().await?));
        }

        Ok(SseStream::new(resp, ApiMode::ChatCompletions))
    }

    fn build_openai_body(&self, messages: &[Message], tools: &[ToolDefinition], stream: bool) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.config.model.as_str(),
            "messages": messages.iter().map(msg_to_openai).collect::<Vec<_>>(),
            "stream": stream,
        });
        if !tools.is_empty() && self.config.supports_tools {
            body["tools"] = serde_json::to_value(tools).unwrap();
        }
        body
    }

    // ---- Anthropic Messages ----

    async fn anthropic_chat(&self, messages: &[Message], tools: &[ToolDefinition]) -> Result<ApiResponse> {
        let url = format!("{}/messages", self.config.base_url);
        let body = self.build_anthropic_body(messages, tools, false);

        debug!("Anthropic POST {}", url);
        let resp = self.http
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(classify_error(resp.status(), &resp.text().await?));
        }

        let json: AnthropicResp = resp.json().await?;
        let text = json.content.iter()
            .filter(|c| c.type_ == "text")
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("");

        let tool_calls = json.content.iter()
            .filter(|c| c.type_ == "tool_use")
            .map(|c| ToolCall {
                id: c.id.clone().unwrap_or_default(),
                function: ToolCallFunction {
                    name: c.name.clone().unwrap_or_default(),
                    arguments: serde_json::to_string(&c.input).unwrap_or_default(),
                },
            })
            .collect::<Vec<_>>();

        Ok(ApiResponse {
            id: json.id,
            model: ModelId::new(&json.model),
            choices: vec![h_core::ApiChoice {
                index: 0,
                message: Message {
                    role: Role::Assistant,
                    content: Some(Content::Text(text)),
                    tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
                    tool_call_id: None,
                    name: None,
                    reasoning: None,
                },
                finish_reason: json.stop_reason.clone(),
            }],
            usage: Some(ApiUsage {
                prompt_tokens: json.usage.input_tokens,
                completion_tokens: json.usage.output_tokens,
                total_tokens: json.usage.input_tokens + json.usage.output_tokens,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        })
    }

    async fn anthropic_stream(&self, messages: &[Message], tools: &[ToolDefinition]) -> Result<SseStream> {
        let url = format!("{}/messages", self.config.base_url);
        let body = self.build_anthropic_body(messages, tools, true);

        let resp = self.http
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(classify_error(resp.status(), &resp.text().await?));
        }

        Ok(SseStream::new(resp, ApiMode::AnthropicMessages))
    }

    fn build_anthropic_body(&self, messages: &[Message], tools: &[ToolDefinition], stream: bool) -> serde_json::Value {
        let (sys, other): (Vec<_>, Vec<_>) = messages.iter().partition(|m| m.role == Role::System);
        let system = sys.iter().filter_map(|m| m.content.as_ref()?.as_text()).collect::<Vec<_>>().join("\n");

        let mut body = serde_json::json!({
            "model": self.config.model.as_str(),
            "max_tokens": 8192,
            "messages": other.iter().map(|m| msg_to_anthropic(*m)).collect::<Vec<_>>(),
        });

        if !system.is_empty() { body["system"] = serde_json::Value::String(system); }
        if stream { body["stream"] = serde_json::Value::Bool(true); }
        if !tools.is_empty() && self.config.supports_tools {
            body["tools"] = serde_json::to_value(tools.iter().map(AnthropicToolDef::from_openai).collect::<Vec<_>>()).unwrap();
        }
        body
    }
}

// Message converters
fn msg_to_openai(m: &Message) -> serde_json::Value {
    let mut obj = serde_json::json!({ "role": m.role.to_string() });
    if let Some(c) = &m.content {
        obj["content"] = match c {
            Content::Text(t) => serde_json::Value::String(t.clone()),
            Content::Multi(p) => serde_json::to_value(p).unwrap(),
        };
    }
    if let Some(tc) = &m.tool_calls { obj["tool_calls"] = serde_json::to_value(tc).unwrap(); }
    if let Some(id) = &m.tool_call_id { obj["tool_call_id"] = serde_json::Value::String(id.clone()); }
    obj
}

fn msg_to_anthropic(m: &Message) -> serde_json::Value {
    let mut obj = serde_json::json!({ "role": m.role.to_string() });
    if let Some(c) = &m.content {
        obj["content"] = match c {
            Content::Text(t) => serde_json::json!([{ "type": "text", "text": t }]),
            Content::Multi(p) => serde_json::to_value(p).unwrap(),
        };
    }
    obj
}

// Response types
#[derive(Deserialize)]
struct OpenAIResp {
    id: String,
    model: String,
    choices: Vec<OpenAIChoice>,
    usage: OpenAIUsage,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMsg,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIMsg {
    content: Option<String>,
    #[serde(default)] tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Deserialize, Clone)]
struct OpenAIToolCall {
    id: String,
    function: OpenAIFn,
}

#[derive(Deserialize, Clone)]
struct OpenAIFn { name: String, arguments: String }

#[derive(Deserialize)]
struct OpenAIUsage { prompt_tokens: u64, completion_tokens: u64, total_tokens: u64 }

#[derive(Deserialize)]
struct AnthropicResp {
    id: String,
    model: String,
    content: Vec<AnthropicContent>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")] type_: String,
    #[serde(default)] text: String,
    #[serde(default)] id: Option<String>,
    #[serde(default)] name: Option<String>,
    #[serde(default)] input: serde_json::Value,
}

#[derive(Deserialize)]
struct AnthropicUsage { input_tokens: u64, output_tokens: u64 }

#[derive(Serialize)]
struct AnthropicToolDef { name: String, description: String, input_schema: serde_json::Value }

impl AnthropicToolDef {
    fn from_openai(t: &ToolDefinition) -> Self {
        Self { name: t.function.name.clone(), description: t.function.description.clone(), input_schema: t.function.parameters.clone() }
    }
}

// ---- Streaming ----

/// Delta from streaming response.
#[derive(Clone, Debug)]
pub struct StreamDelta {
    pub text: Option<String>,
    pub tool_call: Option<ToolCallDelta>,
    pub is_final: bool,
    pub finish_reason: Option<String>,
    pub usage: Option<CostTracker>,
}

#[derive(Clone, Debug)]
pub struct ToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

/// SSE stream wrapper using tokio_stream.
pub struct SseStream {
    inner: tokio_stream::wrappers::ReceiverStream<Result<StreamDelta>>,
}

impl SseStream {
    fn new(resp: Response, mode: ApiMode) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(32);

        // Spawn a task to read chunks and send deltas
        tokio::spawn(async move {
            let mut buf = String::new();
            let mut body_stream = resp.bytes_stream();

            while let Some(chunk_result) = TokioStreamExt::next(&mut body_stream).await {
                match chunk_result {
                    Ok(bytes) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(pos) = buf.find('\n') {
                            let line_str = buf[..pos].trim().to_string();
                            buf = buf[pos + 1..].to_string();
                            let line = line_str.as_str();

                            if line.is_empty() || line.starts_with(':') { continue; }
                            if let Some(data) = line.strip_prefix("data: ") {
                                if data.is_empty() { continue; }

                                let delta = parse_sse_data(data, mode);
                                let is_final = delta.as_ref().map(|d| d.is_final).unwrap_or(false);

                                if tx.send(delta).await.is_err() { return; }
                                if is_final { return; }
                            }
                        }
                    }
                    Err(e) => {
                        tx.send(Err(anyhow!("Stream error: {}", e))).await.ok();
                        return;
                    }
                }
            }
        });

        Self { inner: tokio_stream::wrappers::ReceiverStream::new(rx) }
    }
}

fn parse_sse_data(data: &str, mode: ApiMode) -> Result<StreamDelta> {
    match mode {
        ApiMode::ChatCompletions => parse_openai_delta(data),
        ApiMode::AnthropicMessages => parse_anthropic_delta(data),
    }
}

fn parse_openai_delta(data: &str) -> Result<StreamDelta> {
    if data == "[DONE]" {
        return Ok(StreamDelta { text: None, tool_call: None, is_final: true, finish_reason: None, usage: None });
    }
    let d: OpenAIStreamChunk = serde_json::from_str(data)?;
    let text = d.choices.iter().find_map(|c| c.delta.content.clone());
    let tc = d.choices.iter().find_map(|c| c.delta.tool_calls.first().map(|t| ToolCallDelta {
        index: t.index.unwrap_or(0), id: t.id.clone(), name: t.function.name.clone(), arguments: t.function.arguments.clone()
    }));
    let fin = d.choices.iter().find_map(|c| c.finish_reason.clone());
    Ok(StreamDelta { text, tool_call: tc, is_final: fin.is_some(), finish_reason: fin, usage: None })
}

fn parse_anthropic_delta(data: &str) -> Result<StreamDelta> {
    let e: AnthropicStreamEvent = serde_json::from_str(data)?;
    Ok(match e.type_.as_str() {
        "content_block_delta" => StreamDelta {
            text: e.delta.and_then(|d| if d.type_ == "text_delta" { d.text } else { None }),
            tool_call: None, is_final: false, finish_reason: None, usage: None,
        },
        "message_stop" => StreamDelta { text: None, tool_call: None, is_final: true, finish_reason: Some("stop".to_string()), usage: None },
        "message_delta" => StreamDelta {
            text: None, tool_call: None, is_final: true,
            finish_reason: e.delta.and_then(|d| d.stop_reason),
            usage: e.usage.map(|u| CostTracker { input_tokens: u.input_tokens, output_tokens: u.output_tokens, ..Default::default() }),
        },
        _ => StreamDelta { text: None, tool_call: None, is_final: false, finish_reason: None, usage: None },
    })
}

impl Stream for SseStream {
    type Item = Result<StreamDelta>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_next_unpin(cx)
    }
}

#[derive(Deserialize)]
struct OpenAIStreamChunk { choices: Vec<OpenAIStreamChoice> }

#[derive(Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIStreamDelta {
    #[serde(default)] content: Option<String>,
    #[serde(default)] tool_calls: Vec<OpenAIStreamToolCall>,
}

#[derive(Deserialize)]
struct OpenAIStreamToolCall {
    #[serde(default)] index: Option<u32>,
    #[serde(default)] id: Option<String>,
    #[serde(default)] function: OpenAIStreamFn,
}

#[derive(Deserialize, Default)]
struct OpenAIStreamFn {
    #[serde(default)] name: Option<String>,
    #[serde(default)] arguments: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")] type_: String,
    #[serde(default)] delta: Option<AnthropicStreamDelta>,
    #[serde(default)] usage: Option<AnthropicStreamUsage>,
}

#[derive(Deserialize)]
struct AnthropicStreamDelta {
    #[serde(rename = "type", default)] type_: String,
    #[serde(default)] text: Option<String>,
    #[serde(default)] stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicStreamUsage {
    #[serde(default)] input_tokens: u64,
    #[serde(default)] output_tokens: u64,
}

// Error handling
fn classify_error(status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    match status.as_u16() {
        401 => anyhow!("Auth error (401): Invalid API key"),
        403 => anyhow!("Forbidden (403)"),
        404 => anyhow!("Not found (404)"),
        429 => anyhow!("Rate limited (429)"),
        500..=599 => anyhow!("Server error ({})", status),
        400 => anyhow!("Bad request: {}", body),
        _ => anyhow!("Error ({}): {}", status, body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_detection() {
        assert_eq!(detect_api_mode(&ProviderId::new("anthropic")), ApiMode::AnthropicMessages);
        assert_eq!(detect_api_mode(&ProviderId::new("openai")), ApiMode::ChatCompletions);
        assert_eq!(detect_api_mode(&ProviderId::new("openrouter")), ApiMode::ChatCompletions);
    }
}