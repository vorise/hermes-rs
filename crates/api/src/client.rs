use anyhow::Result;
use futures_util::StreamExt;
use h_core::{Message, ModelId, ProviderId, ToolDefinition};
use reqwest::Client;

type BoxedStream = futures::stream::BoxStream<'static, Result<Delta>>;

use crate::error::{classify_error, ApiError, RetryConfig, with_retry};
use crate::provider::ApiMode;
use crate::streaming::{
    parse_anthropic_sse, parse_sse_line, AnthropicEvent, ApiResponse, Delta, StreamToolCall,
};

/// Configuration for creating an ApiClient.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub provider: ProviderId,
    pub model: ModelId,
    pub base_url: String,
    pub api_key: String,
    pub api_mode: ApiMode,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub reasoning_effort: Option<String>,
}

impl ApiConfig {
    /// Resolve API key from environment variable.
    pub fn resolve_key(env_var: &str) -> Option<String> {
        std::env::var(env_var).ok().filter(|k| !k.is_empty())
    }
}

/// HTTP client for LLM API calls.
pub struct ApiClient {
    http: Client,
    base_url: String,
    api_key: String,
    mode: ApiMode,
    #[allow(dead_code)]
    provider: ProviderId,
    model: ModelId,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    #[allow(dead_code)]
    reasoning_effort: Option<String>,
    retry_config: RetryConfig,
}

impl ApiClient {
    pub fn new(config: ApiConfig) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()?,
            base_url: config.base_url,
            api_key: config.api_key,
            mode: config.api_mode,
            provider: config.provider,
            model: config.model,
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            reasoning_effort: config.reasoning_effort,
            retry_config: RetryConfig::default(),
        })
    }

    /// Set the retry configuration.
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Make a non-streaming chat request with automatic retry for transient errors.
    pub async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<ApiResponse> {
        with_retry(&self.retry_config, || async {
            self.do_chat(messages, tools).await
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Internal: execute a single chat attempt with ApiError for retry classification.
    async fn do_chat(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> std::result::Result<ApiResponse, ApiError> {
        match &self.mode {
            ApiMode::ChatCompletions => self.chat_completions_inner(messages, tools).await,
            ApiMode::CodexResponses => self.codex_responses_inner(messages, tools).await,
            ApiMode::AnthropicMessages => self.anthropic_messages_inner(messages, tools).await,
        }
    }

    /// Make a streaming chat request (no retry — streaming is stateful).
    pub async fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<BoxedStream> {
        match &self.mode {
            ApiMode::ChatCompletions => {
                self.chat_completions_stream(messages, tools).await
            }
            ApiMode::CodexResponses => {
                let resp = self.codex_responses_inner(messages, tools).await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let deltas: Vec<Result<Delta>> = vec![Ok(Delta {
                    content: resp.text_content(),
                    reasoning: None,
                    tool_calls: resp
                        .tool_calls()
                        .into_iter()
                        .enumerate()
                        .map(|(i, tc)| StreamToolCall {
                            index: i,
                            id: Some(tc.id),
                            name: Some(tc.function.name),
                            arguments_delta: tc.function.arguments,
                        })
                        .collect(),
                    finish_reason: Some("stop".to_string()),
                })];
                Ok(futures::stream::iter(deltas).boxed())
            }
            ApiMode::AnthropicMessages => {
                self.anthropic_messages_stream(messages, tools).await
            }
        }
    }

    async fn chat_completions_inner(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> std::result::Result<ApiResponse, ApiError> {
        let body = self.build_openai_body(messages, tools, false);

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        let body = resp.text().await?;

        if status >= 400 {
            return Err(classify_error(status, &body));
        }

        serde_json::from_str::<ApiResponse>(&body)
            .map_err(|e| ApiError::StreamParseError(e.to_string()))
    }

    async fn chat_completions_stream(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<BoxedStream> {
        let body = self.build_openai_body(messages, tools, true);

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().await?;
            return Err(classify_error(status, &body).into());
        }

        let stream = resp.bytes_stream();
        let buffer = String::new();

        Ok(futures::stream::unfold(
            (stream, buffer),
            |(mut stream, mut buffer)| async move {
                loop {
                    // Check buffer for complete SSE messages
                    if let Some(pos) = buffer.find("\n\n") {
                        let chunk = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();
                        if let Some(delta) = parse_sse_chunk(&chunk) {
                            return Some((Ok(delta), (stream, buffer)));
                        }
                        continue;
                    }

                    // Read more from stream
                    match stream.next().await {
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                        }
                        Some(Err(e)) => {
                            return Some((Err(e.into()), (stream, buffer)));
                        }
                        None => {
                            // Drain remaining buffer
                            if !buffer.trim().is_empty() {
                                if let Some(delta) = parse_sse_chunk(&buffer) {
                                    return Some((Ok(delta), (stream, String::new())));
                                }
                            }
                            return None;
                        }
                    }
                }
            },
        )
        .boxed())
    }

    async fn codex_responses_inner(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> std::result::Result<ApiResponse, ApiError> {
        let body = self.build_openai_body(messages, tools, false);

        let resp = self
            .http
            .post(format!("{}/responses", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("OpenAI-Beta", "responses=v1")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        let body = resp.text().await?;

        if status >= 400 {
            return Err(classify_error(status, &body));
        }

        self.parse_codex_response_inner(&body)
            .map_err(|e| ApiError::StreamParseError(e.to_string()))
    }

    async fn anthropic_messages_inner(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> std::result::Result<ApiResponse, ApiError> {
        let body = self.build_anthropic_body(messages, tools, false);

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        let body = resp.text().await?;

        if status >= 400 {
            return Err(classify_error(status, &body));
        }

        self.parse_anthropic_response_inner(&body)
            .map_err(|e| ApiError::StreamParseError(e.to_string()))
    }

    async fn anthropic_messages_stream(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<BoxedStream> {
        let body = self.build_anthropic_body(messages, tools, true);

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .header("anthropic-beta", "prompt-caching-2024-07-31")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().await?;
            return Err(classify_error(status, &body).into());
        }

        let stream = resp.bytes_stream();
        let buffer = String::new();

        Ok(futures::stream::unfold(
            (stream, buffer),
            |(mut stream, mut buffer)| async move {
                loop {
                    if let Some(pos) = buffer.find("\n\n") {
                        let chunk = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();
                        if let Some(delta) = parse_anthropic_sse_chunk(&chunk) {
                            return Some((Ok(delta), (stream, buffer)));
                        }
                        continue;
                    }

                    match stream.next().await {
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                        }
                        Some(Err(e)) => {
                            return Some((Err(e.into()), (stream, buffer)));
                        }
                        None => {
                            if !buffer.trim().is_empty() {
                                if let Some(delta) = parse_anthropic_sse_chunk(&buffer) {
                                    return Some((Ok(delta), (stream, String::new())));
                                }
                            }
                            return None;
                        }
                    }
                }
            },
        )
        .boxed())
    }

    fn build_openai_body(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        stream: bool,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model.0,
            "messages": messages_to_openai(messages),
        });

        if stream {
            body["stream"] = serde_json::Value::Bool(true);
        }

        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools).unwrap_or_default();
            body["tool_choice"] = serde_json::Value::String("auto".to_string());
        }

        if let Some(max_tokens) = self.max_tokens {
            body["max_tokens"] = serde_json::Value::Number(max_tokens.into());
        }

        if let Some(temp) = self.temperature {
            body["temperature"] = serde_json::Value::Number(
                serde_json::Number::from_f64(temp).unwrap_or_else(|| serde_json::Number::from_f64(1.0).unwrap()),
            );
        }

        body
    }

    fn build_anthropic_body(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        stream: bool,
    ) -> serde_json::Value {
        let system_msg = messages
            .iter()
            .find(|m| m.role == h_core::Role::System)
            .and_then(|m| m.content.as_ref())
            .and_then(|c| c.as_text());

        let user_messages: Vec<&Message> = messages
            .iter()
            .filter(|m| m.role != h_core::Role::System)
            .collect();

        let mut body = serde_json::json!({
            "model": self.model.0,
            "messages": messages_to_anthropic(&user_messages),
            "max_tokens": self.max_tokens.unwrap_or(4096),
        });

        if let Some(system) = system_msg {
            body["system"] = serde_json::json!([{
                "type": "text",
                "text": system,
                "cache_control": { "type": "ephemeral" }
            }]);
        }

        if stream {
            body["stream"] = serde_json::Value::Bool(true);
        }

        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(
                tools.iter().map(|t| &t.function).collect::<Vec<_>>(),
            )
            .unwrap_or_default();
        }

        if let Some(_effort) = &self.reasoning_effort {
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": 16000
            });
        }

        body
    }

    fn parse_codex_response_inner(&self, body: &str) -> Result<ApiResponse, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(body)?;

        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let model = value
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let output = value.get("output").and_then(|v| v.as_array());
        let mut text_content = String::new();

        if let Some(outputs) = output {
            for item in outputs {
                if let Some(type_) = item.get("type").and_then(|v| v.as_str()) {
                    if type_ == "message" {
                        if let Some(content_items) =
                            item.get("content").and_then(|v| v.as_array())
                        {
                            for ci in content_items {
                                if let Some(ct) = ci.get("type").and_then(|v| v.as_str()) {
                                    if ct == "output_text" {
                                        if let Some(text) =
                                            ci.get("text").and_then(|v| v.as_str())
                                        {
                                            text_content.push_str(text);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let usage = value.get("usage").map(|u| {
            serde_json::from_value::<crate::streaming::Usage>(u.clone()).unwrap_or_default()
        });

        Ok(ApiResponse {
            id,
            model,
            choices: vec![crate::streaming::Choice {
                message: Some(crate::streaming::ApiResponseMessage {
                    role: Some("assistant".to_string()),
                    content: if text_content.is_empty() {
                        None
                    } else {
                        Some(text_content)
                    },
                    reasoning: None,
                    tool_calls: None,
                }),
                delta: None,
                finish_reason: Some("stop".to_string()),
                index: 0,
            }],
            usage,
        })
    }

    fn parse_anthropic_response_inner(&self, body: &str) -> Result<ApiResponse, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(body)?;

        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let model = value
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let content = value.get("content").and_then(|v| v.as_array());
        let mut text_content = String::new();
        let mut tool_calls = Vec::new();

        if let Some(blocks) = content {
            for block in blocks {
                if let Some(type_) = block.get("type").and_then(|v| v.as_str()) {
                    match type_ {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                text_content.push_str(text);
                            }
                        }
                        "tool_use" => {
                            if let (Some(name), Some(tc_id), Some(input)) = (
                                block.get("name").and_then(|v| v.as_str()),
                                block.get("id").and_then(|v| v.as_str()),
                                block.get("input"),
                            ) {
                                tool_calls.push(crate::streaming::ApiToolCall {
                                    id: tc_id.to_string(),
                                    type_: "function".to_string(),
                                    function: h_core::ToolCallFunction {
                                        name: name.to_string(),
                                        arguments: serde_json::to_string(input)?,
                                    },
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let usage = value.get("usage").map(|u| {
            let input_tokens = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let output_tokens = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let cache_read = u
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            crate::streaming::Usage {
                prompt_tokens: Some(input_tokens),
                completion_tokens: Some(output_tokens),
                total_tokens: Some(input_tokens + output_tokens),
                prompt_tokens_details: Some(crate::streaming::PromptTokenDetails {
                    cached_tokens: Some(cache_read),
                }),
                completion_tokens_details: None,
            }
        });

        Ok(ApiResponse {
            id,
            model,
            choices: vec![crate::streaming::Choice {
                message: Some(crate::streaming::ApiResponseMessage {
                    role: Some("assistant".to_string()),
                    content: if text_content.is_empty() {
                        None
                    } else {
                        Some(text_content)
                    },
                    reasoning: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                }),
                delta: None,
                finish_reason: Some(
                    value
                        .get("stop_reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("stop")
                        .to_string(),
                ),
                index: 0,
            }],
            usage,
        })
    }

    /// Original parse methods kept for compatibility.
    #[allow(dead_code)]
    fn parse_codex_response(&self, body: &str) -> Result<ApiResponse> {
        self.parse_codex_response_inner(body)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    #[allow(dead_code)]
    fn parse_anthropic_response(&self, body: &str) -> Result<ApiResponse> {
        self.parse_anthropic_response_inner(body)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

/// Convert internal messages to OpenAI format.
fn messages_to_openai(messages: &[Message]) -> serde_json::Value {
    serde_json::to_value(
        messages
            .iter()
            .map(|m| {
                let mut obj = serde_json::Map::new();
                obj.insert(
                    "role".to_string(),
                    serde_json::Value::String(m.role.to_string()),
                );
                if let Some(ref content) = m.content {
                    if let Some(text) = content.as_text() {
                        obj.insert(
                            "content".to_string(),
                            serde_json::Value::String(text.to_string()),
                        );
                    }
                }
                if let Some(ref tool_calls) = m.tool_calls {
                    obj.insert(
                        "tool_calls".to_string(),
                        serde_json::to_value(tool_calls).unwrap_or_default(),
                    );
                }
                if let Some(ref tool_call_id) = m.tool_call_id {
                    obj.insert(
                        "tool_call_id".to_string(),
                        serde_json::Value::String(tool_call_id.clone()),
                    );
                }
                if let Some(ref reasoning) = m.reasoning {
                    obj.insert(
                        "reasoning".to_string(),
                        serde_json::Value::String(reasoning.clone()),
                    );
                }
                serde_json::Value::Object(obj)
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default()
}

/// Convert internal messages to Anthropic format.
fn messages_to_anthropic(messages: &[&Message]) -> serde_json::Value {
    serde_json::to_value(
        messages
            .iter()
            .map(|m| {
                let mut obj = serde_json::Map::new();
                obj.insert(
                    "role".to_string(),
                    serde_json::Value::String(m.role.to_string()),
                );
                if let Some(ref content) = m.content {
                    if let Some(text) = content.as_text() {
                        obj.insert(
                            "content".to_string(),
                            serde_json::Value::String(text.to_string()),
                        );
                    }
                }
                if let Some(ref tool_calls) = m.tool_calls {
                    obj.insert(
                        "tool_calls".to_string(),
                        serde_json::to_value(tool_calls).unwrap_or_default(),
                    );
                }
                if let Some(ref tool_call_id) = m.tool_call_id {
                    obj.insert(
                        "tool_call_id".to_string(),
                        serde_json::Value::String(tool_call_id.clone()),
                    );
                }
                serde_json::Value::Object(obj)
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default()
}

fn parse_sse_chunk(chunk: &str) -> Option<Delta> {
    for line in chunk.lines() {
        if let Some(delta) = parse_sse_line(line) {
            return Some(delta);
        }
    }
    None
}

fn parse_anthropic_sse_chunk(chunk: &str) -> Option<Delta> {
    let mut event_type = String::new();
    let mut data_lines = Vec::new();

    for line in chunk.lines() {
        if let Some(et) = line.strip_prefix("event: ") {
            event_type = et.trim().to_string();
        } else if let Some(d) = line.strip_prefix("data: ") {
            data_lines.push(d);
        }
    }

    if event_type.is_empty() || data_lines.is_empty() {
        return None;
    }

    let data = data_lines.join("\n");
    match parse_anthropic_sse(&event_type, &data) {
        Some(AnthropicEvent::TextDelta(text)) => Some(Delta {
            content: Some(text),
            reasoning: None,
            tool_calls: vec![],
            finish_reason: None,
        }),
        Some(AnthropicEvent::ThinkingDelta(thinking)) => Some(Delta {
            content: None,
            reasoning: Some(thinking),
            tool_calls: vec![],
            finish_reason: None,
        }),
        Some(AnthropicEvent::ToolUseStart { name, id }) => Some(Delta {
            content: None,
            reasoning: None,
            tool_calls: vec![StreamToolCall {
                index: 0,
                id: Some(id),
                name: Some(name),
                arguments_delta: String::new(),
            }],
            finish_reason: None,
        }),
        Some(AnthropicEvent::ToolUseInputDelta(delta)) => Some(Delta {
            content: None,
            reasoning: None,
            tool_calls: vec![StreamToolCall {
                index: 0,
                id: None,
                name: None,
                arguments_delta: delta,
            }],
            finish_reason: None,
        }),
        Some(AnthropicEvent::MessageStop) => Some(Delta {
            content: None,
            reasoning: None,
            tool_calls: vec![],
            finish_reason: Some("end_turn".to_string()),
        }),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ProviderRegistry;

    #[test]
    fn test_api_config_key_resolution() {
        // SAFETY: test-only
        unsafe { std::env::set_var("TEST_API_KEY", "test-key-123") };
        let key = ApiConfig::resolve_key("TEST_API_KEY");
        assert_eq!(key, Some("test-key-123".to_string()));
        // SAFETY: test-only cleanup
        unsafe { std::env::remove_var("TEST_API_KEY") };
    }

    #[test]
    fn test_api_config_missing_key() {
        let key = ApiConfig::resolve_key("NONEXISTENT_KEY_12345");
        assert_eq!(key, None);
    }

    #[test]
    fn test_provider_registry_api_mode() {
        let registry = ProviderRegistry::new();
        let anthropic = registry.get(&ProviderId::new("anthropic")).unwrap();
        assert!(matches!(anthropic.api_mode, ApiMode::AnthropicMessages));

        let openai = registry.get(&ProviderId::new("openai")).unwrap();
        assert!(matches!(openai.api_mode, ApiMode::ChatCompletions));
    }

    #[test]
    fn test_messages_to_openai() {
        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user("Hello"),
        ];
        let result = messages_to_openai(&messages);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].get("role").unwrap().as_str().unwrap(), "system");
        assert_eq!(arr[1].get("role").unwrap().as_str().unwrap(), "user");
    }

    #[test]
    fn test_messages_to_anthropic() {
        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user("Hello"),
        ];
        // System messages are filtered out
        let user_messages: Vec<&Message> = messages
            .iter()
            .filter(|m| m.role != h_core::Role::System)
            .collect();
        let result = messages_to_anthropic(&user_messages);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("role").unwrap().as_str().unwrap(), "user");
    }
}
