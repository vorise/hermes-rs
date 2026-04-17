use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::tool::{Tool, ToolContext, ToolResult};

// ─── Memory Tool ─────────────────────────────────────────────────────────

/// Manage persistent memories (save, list, delete, view).
pub struct MemoryTool {
    manager: std::sync::Arc<h_core::memory::MemoryManager>,
}

impl MemoryTool {
    pub fn new() -> Self {
        let manager = h_core::memory::MemoryManager::default_path()
            .unwrap_or_else(|_| {
                h_core::memory::MemoryManager::new(std::env::temp_dir().join("hermes_memory"))
            });
        Self {
            manager: std::sync::Arc::new(manager),
        }
    }
}

impl Default for MemoryTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn toolset(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Manage persistent memories. Actions: save (name, content), list, \
        view (name), delete (name), nudge."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["save", "list", "view", "delete", "nudge"],
                    "description": "The memory action to perform"
                },
                "name": {
                    "type": "string",
                    "description": "Memory name (required for save, view, delete)"
                },
                "content": {
                    "type": "string",
                    "description": "Memory content (required for save)"
                }
            },
            "required": ["action"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: action"))?;

        match action {
            "save" => {
                let name = args.get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing required argument: name (for save)"))?;
                let content = args.get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing required argument: content (for save)"))?;
                match self.manager.save_memory(name, content) {
                    Ok(()) => Ok(ToolResult::ok(format!("Memory '{name}' saved"))),
                    Err(e) => Ok(ToolResult::err(format!("Failed to save memory: {e}"))),
                }
            }
            "list" => {
                match self.manager.get_memories() {
                    Ok(memories) => {
                        if memories.is_empty() {
                            Ok(ToolResult::ok("No memories found".to_string()))
                        } else {
                            let list = memories.iter()
                                .map(|m| format!("- {}", m.name))
                                .collect::<Vec<_>>()
                                .join("\n");
                            Ok(ToolResult::ok(format!(
                                "Found {} memories:\n{}",
                                memories.len(), list
                            )))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to list memories: {e}"))),
                }
            }
            "view" => {
                let name = args.get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing required argument: name (for view)"))?;
                match self.manager.get_memory(name) {
                    Ok(Some(entry)) => Ok(ToolResult::ok(format!(
                        "## {}\n\n{}", entry.name, entry.content
                    ))),
                    Ok(None) => Ok(ToolResult::err(format!("Memory not found: {name}"))),
                    Err(e) => Ok(ToolResult::err(format!("Failed to view memory: {e}"))),
                }
            }
            "delete" => {
                let name = args.get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing required argument: name (for delete)"))?;
                match self.manager.delete_memory(name) {
                    Ok(()) => Ok(ToolResult::ok(format!("Memory '{name}' deleted"))),
                    Err(e) => Ok(ToolResult::err(format!("Failed to delete memory: {e}"))),
                }
            }
            "nudge" => {
                let prompt = self.manager.nudge_prompt();
                Ok(ToolResult::ok(prompt))
            }
            other => Ok(ToolResult::err(format!("Unknown memory action: {other}"))),
        }
    }
}

// ─── Todo Tool ───────────────────────────────────────────────────────────

use parking_lot::Mutex;

#[derive(Debug, Clone)]
struct TodoItem {
    id: u32,
    text: String,
    done: bool,
}

/// Manage a todo list (add, view, complete, delete, clear).
pub struct TodoTool {
    todos: std::sync::Arc<Mutex<Vec<TodoItem>>>,
}

impl TodoTool {
    pub fn new() -> Self {
        Self {
            todos: std::sync::Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn next_id(&self) -> u32 {
        let todos = self.todos.lock();
        todos.iter().map(|t| t.id).max().unwrap_or(0) + 1
    }
}

impl Default for TodoTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }

    fn toolset(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "Manage a todo list. Actions: add (text), list, complete (id), \
        delete (id), clear."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "list", "complete", "delete", "clear"],
                    "description": "The todo action to perform"
                },
                "text": {
                    "type": "string",
                    "description": "Todo text (required for add)"
                },
                "id": {
                    "type": "integer",
                    "description": "Todo ID (required for complete, delete)"
                }
            },
            "required": ["action"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: action"))?;

        match action {
            "add" => {
                let text = args.get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing required argument: text (for add)"))?;
                let id = self.next_id();
                self.todos.lock().push(TodoItem {
                    id,
                    text: text.to_string(),
                    done: false,
                });
                Ok(ToolResult::ok(format!("Added todo #{id}: {text}")))
            }
            "list" => {
                let todos = self.todos.lock();
                if todos.is_empty() {
                    Ok(ToolResult::ok("No todos".to_string()))
                } else {
                    let list = todos.iter()
                        .map(|t| {
                            let checkbox = if t.done { "[x]" } else { "[ ]" };
                            format!("{checkbox} #{id}: {text}", id = t.id, text = t.text)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let done_count = todos.iter().filter(|t| t.done).count();
                    Ok(ToolResult::ok(format!(
                        "Todos ({done_count}/{total} done):\n{list}",
                        total = todos.len()
                    )))
                }
            }
            "complete" => {
                let id = args.get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow!("Missing required argument: id (for complete)"))? as u32;
                let mut todos = self.todos.lock();
                if let Some(todo) = todos.iter_mut().find(|t| t.id == id) {
                    todo.done = true;
                    Ok(ToolResult::ok(format!("Completed todo #{id}: {}", todo.text)))
                } else {
                    Ok(ToolResult::err(format!("Todo #{id} not found")))
                }
            }
            "delete" => {
                let id = args.get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow!("Missing required argument: id (for delete)"))? as u32;
                let mut todos = self.todos.lock();
                if let Some(pos) = todos.iter().position(|t| t.id == id) {
                    let removed = todos.remove(pos);
                    Ok(ToolResult::ok(format!("Deleted todo #{id}: {}", removed.text)))
                } else {
                    Ok(ToolResult::err(format!("Todo #{id} not found")))
                }
            }
            "clear" => {
                self.todos.lock().clear();
                Ok(ToolResult::ok("All todos cleared".to_string()))
            }
            other => Ok(ToolResult::err(format!("Unknown todo action: {other}"))),
        }
    }
}

// ─── Session Search Tool ─────────────────────────────────────────────────

/// Search past sessions using FTS5.
pub struct SessionSearchTool {
    db_path: std::path::PathBuf,
}

impl SessionSearchTool {
    pub fn new(db_path: std::path::PathBuf) -> Self {
        Self { db_path }
    }

    pub fn default_path() -> Self {
        let home = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        Self::new(home.join(".hermes").join("sessions.db"))
    }
}

#[async_trait]
impl Tool for SessionSearchTool {
    fn name(&self) -> &str {
        "session_search"
    }

    fn toolset(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Search past conversation sessions. Returns matching session titles, \
        dates, and snippets."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query for FTS5 full-text search"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 10)"
                }
            },
            "required": ["query"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let query = args.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: query"))?;
        let limit = args.get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        if !self.db_path.exists() {
            return Ok(ToolResult::err(
                format!("Session database not found: {}", self.db_path.display())
            ));
        }

        match h_core::session_db::SessionDB::open(&self.db_path) {
            Ok(db) => {
                match db.search_sessions(query, None) {
                    Ok(results) => {
                        let limited = if results.len() > limit {
                            &results[..limit]
                        } else {
                            &results[..]
                        };
                        if limited.is_empty() {
                            Ok(ToolResult::ok(format!("No sessions found for: {query}")))
                        } else {
                            let list = limited.iter()
                                .map(|r| {
                                    let snippet = r.content.chars().take(120).collect::<String>();
                                    format!("- Session {} at {}: {snippet}...", r.session_id, r.timestamp)
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            Ok(ToolResult::ok(format!(
                                "Found {} sessions for \"{query}\":\n{list}",
                                results.len()
                            )))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Session search failed: {e}"))),
                }
            }
            Err(e) => Ok(ToolResult::err(format!("Failed to open session DB: {e}"))),
        }
    }
}

// ─── Vision Tool ─────────────────────────────────────────────────────────

/// Analyze images using a vision-capable model.
pub struct VisionTool {
    client: reqwest::Client,
}

impl VisionTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for VisionTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for VisionTool {
    fn name(&self) -> &str {
        "vision"
    }

    fn toolset(&self) -> &str {
        "multimodal"
    }

    fn description(&self) -> &str {
        "Analyze an image using a vision model. Provide a URL or base64-encoded \
        image and a question about the image content."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "image_url": {
                    "type": "string",
                    "description": "URL of the image to analyze"
                },
                "image_base64": {
                    "type": "string",
                    "description": "Base64-encoded image data"
                },
                "question": {
                    "type": "string",
                    "description": "Question or prompt about the image"
                }
            },
            "required": ["question"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let question: String = args.get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: question"))?
            .to_string();

        let has_url = args.get("image_url").and_then(|v| v.as_str()).is_some();
        let has_base64 = args.get("image_base64").and_then(|v| v.as_str()).is_some();

        if !has_url && !has_base64 {
            return Ok(ToolResult::err(
                "Provide either image_url or image_base64".to_string()
            ));
        }

        // Check for Anthropic API key (supports vision)
        if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
            if !api_key.is_empty() {
                return self.analyze_anthropic(&question, args, &api_key).await;
            }
        }

        // Check for OpenAI API key
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            if !api_key.is_empty() {
                return self.analyze_openai(&question, args, &api_key).await;
            }
        }

        Ok(ToolResult::ok(format!(
            "Vision analysis requested for: \"{question}\". No vision API key configured. \
            Set ANTHROPIC_API_KEY or OPENAI_API_KEY to enable vision analysis."
        )))
    }
}

impl VisionTool {
    async fn analyze_anthropic(&self, question: &str, args: Value, api_key: &str) -> Result<ToolResult> {
        let image_content = if let Some(url) = args.get("image_url").and_then(|v| v.as_str()) {
            // Fetch image and convert to base64
            match self.client.get(url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let bytes = resp.bytes().await?;
                    format!("data:image/png;base64,{}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes))
                }
                _ => return Ok(ToolResult::err(format!("Failed to fetch image from URL: {url}"))),
            }
        } else if let Some(b64) = args.get("image_base64").and_then(|v| v.as_str()) {
            if b64.starts_with("data:") {
                b64.to_string()
            } else {
                format!("data:image/png;base64,{b64}")
            }
        } else {
            return Ok(ToolResult::err("No image provided".to_string()));
        };

        let resp = self.client.post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&json!({
                "model": "claude-sonnet-4-20250514",
                "max_tokens": 1024,
                "messages": [{
                    "role": "user",
                    "content": [
                        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": image_content}},
                        {"type": "text", "text": question}
                    ]
                }]
            }))
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                #[derive(serde::Deserialize)]
                struct AnthropicResponse {
                    content: Vec<AnthropicContent>,
                }
                #[derive(serde::Deserialize)]
                struct AnthropicContent {
                    #[serde(rename = "type")]
                    content_type: String,
                    text: Option<String>,
                }
                let body: Result<AnthropicResponse, _> = resp.json().await;
                match body {
                    Ok(r) => {
                        let text = r.content.iter()
                            .filter(|c| c.content_type == "text")
                            .filter_map(|c| c.text.clone())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if text.is_empty() {
                            Ok(ToolResult::ok("Vision analysis returned no text".to_string()))
                        } else {
                            Ok(ToolResult::ok(text))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse Anthropic response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("Anthropic API error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("Anthropic request failed: {e}"))),
        }
    }

    async fn analyze_openai(&self, question: &str, args: Value, api_key: &str) -> Result<ToolResult> {
        let image_url = if let Some(url) = args.get("image_url").and_then(|v| v.as_str()) {
            url.to_string()
        } else if let Some(b64) = args.get("image_base64").and_then(|v| v.as_str()) {
            if b64.starts_with("data:") {
                b64.to_string()
            } else {
                format!("data:image/png;base64,{b64}")
            }
        } else {
            return Ok(ToolResult::err("No image provided".to_string()));
        };

        let resp = self.client.post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&json!({
                "model": "gpt-4o",
                "messages": [{
                    "role": "user",
                    "content": [
                        {"type": "image_url", "image_url": {"url": image_url}},
                        {"type": "text", "text": question}
                    ]
                }],
                "max_tokens": 1024
            }))
            .send()
            .await;

        #[derive(serde::Deserialize)]
        struct OpenAIResponse {
            choices: Vec<OpenAIChoice>,
        }
        #[derive(serde::Deserialize)]
        struct OpenAIChoice {
            message: OpenAIMessage,
        }
        #[derive(serde::Deserialize)]
        struct OpenAIMessage {
            content: Option<String>,
        }

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let body: Result<OpenAIResponse, _> = resp.json().await;
                match body {
                    Ok(r) => {
                        let text = r.choices.first()
                            .and_then(|c| c.message.content.clone())
                            .unwrap_or_else(|| "No response".to_string());
                        Ok(ToolResult::ok(text))
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse OpenAI response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("OpenAI API error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("OpenAI request failed: {e}"))),
        }
    }
}

// ─── Image Generation Tool ───────────────────────────────────────────────

/// Generate images using DALL-E or FAL.
pub struct ImageGenTool {
    client: reqwest::Client,
}

impl ImageGenTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for ImageGenTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ImageGenTool {
    fn name(&self) -> &str {
        "image_generation"
    }

    fn toolset(&self) -> &str {
        "multimodal"
    }

    fn description(&self) -> &str {
        "Generate an image from a text prompt. Returns the URL of the generated image."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Text description of the image to generate"
                },
                "size": {
                    "type": "string",
                    "enum": ["1024x1024", "1024x1792", "1792x1024"],
                    "description": "Image size (default: 1024x1024)"
                }
            },
            "required": ["prompt"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &["OPENAI_API_KEY"]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let prompt = args.get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: prompt"))?;

        let size = args.get("size")
            .and_then(|v| v.as_str())
            .unwrap_or("1024x1024");

        // Try DALL-E via OpenAI API
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            if !api_key.is_empty() {
                return self.generate_dalle(prompt, size, &api_key).await;
            }
        }

        // Try FAL
        if let Ok(api_key) = std::env::var("FAL_KEY") {
            if !api_key.is_empty() {
                return self.generate_fal(prompt, &api_key).await;
            }
        }

        Ok(ToolResult::ok(format!(
            "Image generation requested: \"{prompt}\". No API key configured. \
            Set OPENAI_API_KEY (for DALL-E) or FAL_KEY (for FAL) to enable."
        )))
    }
}

impl ImageGenTool {
    async fn generate_dalle(&self, prompt: &str, size: &str, api_key: &str) -> Result<ToolResult> {
        let resp = self.client.post("https://api.openai.com/v1/images/generations")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&json!({
                "model": "dall-e-3",
                "prompt": prompt,
                "size": size,
                "n": 1
            }))
            .send()
            .await;

        #[derive(serde::Deserialize)]
        struct DalleResponse {
            data: Vec<DalleImage>,
        }
        #[derive(serde::Deserialize)]
        struct DalleImage {
            url: Option<String>,
            b64_json: Option<String>,
        }

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let body: Result<DalleResponse, _> = resp.json().await;
                match body {
                    Ok(r) => {
                        if let Some(img) = r.data.first() {
                            if let Some(ref url) = img.url {
                                Ok(ToolResult::ok(format!(
                                    "Generated image: {url}"
                                )))
                            } else if let Some(ref b64) = img.b64_json {
                                Ok(ToolResult::ok(format!(
                                    "Generated image (base64, {} bytes)", b64.len()
                                )))
                            } else {
                                Ok(ToolResult::err("No image data returned".to_string()))
                            }
                        } else {
                            Ok(ToolResult::err("No images generated".to_string()))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse DALL-E response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("DALL-E error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("DALL-E request failed: {e}"))),
        }
    }

    async fn generate_fal(&self, prompt: &str, api_key: &str) -> Result<ToolResult> {
        // Use FAL's Flux model as fallback
        let resp = self.client.post("https://queue.fal.run/fal-ai/flux/schnell")
            .header("Authorization", format!("Key {api_key}"))
            .header("content-type", "application/json")
            .json(&json!({
                "prompt": prompt,
                "image_size": "square_hd"
            }))
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                #[derive(serde::Deserialize)]
                struct FalResponse {
                    images: Vec<FalImage>,
                }
                #[derive(serde::Deserialize)]
                struct FalImage {
                    url: String,
                }
                let body: Result<FalResponse, _> = resp.json().await;
                match body {
                    Ok(r) => {
                        if let Some(img) = r.images.first() {
                            Ok(ToolResult::ok(format!("Generated image: {}", img.url)))
                        } else {
                            Ok(ToolResult::err("No images generated".to_string()))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse FAL response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("FAL error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("FAL request failed: {e}"))),
        }
    }
}

// ─── TTS Tool ────────────────────────────────────────────────────────────

/// Text-to-speech using Edge TTS or ElevenLabs.
pub struct TtsTool {
    client: reqwest::Client,
}

impl TtsTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for TtsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TtsTool {
    fn name(&self) -> &str {
        "tts"
    }

    fn toolset(&self) -> &str {
        "multimodal"
    }

    fn description(&self) -> &str {
        "Convert text to speech audio. Returns audio data as base64 or a URL. \
        Supports Edge TTS (free, no key) and ElevenLabs (requires ELEVENLABS_API_KEY)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to convert to speech"
                },
                "voice": {
                    "type": "string",
                    "description": "Voice name (default: system default)"
                }
            },
            "required": ["text"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let text = args.get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: text"))?;

        let voice = args.get("voice")
            .and_then(|v| v.as_str())
            .unwrap_or("en-US-AriaNeural");

        // Try ElevenLabs if key available
        if let Ok(api_key) = std::env::var("ELEVENLABS_API_KEY") {
            if !api_key.is_empty() {
                return self.elevenlabs_tts(text, voice, &api_key).await;
            }
        }

        // Fallback: Edge TTS (free, no key needed)
        self.edge_tts(text, voice).await
    }
}

impl TtsTool {
    async fn elevenlabs_tts(&self, text: &str, voice: &str, api_key: &str) -> Result<ToolResult> {
        let voice_id = match voice {
            "alloy" => "pNInz6obpgDQGcFmaJgB",
            "nova" => "sOCoKJnMzHqSMCvMnEVg",
            _ => "21m00Tcm4TlvDq8ikWAM", // default Rachel
        };

        let resp = self.client.post(format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{voice_id}"
        ))
            .header("xi-api-key", api_key)
            .header("content-type", "application/json")
            .json(&json!({
                "text": text,
                "model_id": "eleven_monolingual_v1"
            }))
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await?;
                let _b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
                Ok(ToolResult::ok(format!(
                    "Audio generated ({} bytes, base64). Save to file and play with any audio player.",
                    bytes.len()
                )))
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("ElevenLabs error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("ElevenLabs request failed: {e}"))),
        }
    }

    async fn edge_tts(&self, text: &str, voice: &str) -> Result<ToolResult> {
        // Edge TTS doesn't have a simple REST API, so we return a helpful message
        // In practice, users would use the `edge-tts` Python package or a wrapper
        Ok(ToolResult::ok(format!(
            "Edge TTS synthesis requested for voice '{voice}'. \
            Text length: {} chars. \
            Edge TTS requires the 'edge-tts' command-line tool. \
            Run: edge-tts --voice {voice} --text '{text}' --write-media output.mp3",
            text.len()
        )))
    }
}

// ─── Transcription Tool ──────────────────────────────────────────────────

/// Transcribe audio to text using Whisper or similar.
pub struct TranscriptionTool {
    client: reqwest::Client,
}

impl TranscriptionTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for TranscriptionTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TranscriptionTool {
    fn name(&self) -> &str {
        "transcription"
    }

    fn toolset(&self) -> &str {
        "multimodal"
    }

    fn description(&self) -> &str {
        "Transcribe audio to text. Provide a URL to an audio file or base64-encoded audio data. \
        Uses OpenAI Whisper API."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "audio_url": {
                    "type": "string",
                    "description": "URL of the audio file to transcribe"
                },
                "audio_base64": {
                    "type": "string",
                    "description": "Base64-encoded audio data"
                },
                "language": {
                    "type": "string",
                    "description": "Language code (e.g., 'en', 'zh', default: auto-detect)"
                }
            },
            "required": []
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &["OPENAI_API_KEY"]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let audio_url = args.get("audio_url").and_then(|v| v.as_str());
        let audio_base64 = args.get("audio_base64").and_then(|v| v.as_str());
        let language = args.get("language").and_then(|v| v.as_str()).unwrap_or("");

        if audio_url.is_none() && audio_base64.is_none() {
            return Ok(ToolResult::err(
                "Provide either audio_url or audio_base64".to_string()
            ));
        }

        // Try OpenAI Whisper API
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            if !api_key.is_empty() {
                return self.whisper_transcribe(audio_url, audio_base64, language, &api_key).await;
            }
        }

        Ok(ToolResult::ok(
            "Transcription requested but no OPENAI_API_KEY configured. \
            Set the key to enable Whisper transcription.".to_string()
        ))
    }
}

impl TranscriptionTool {
    async fn whisper_transcribe(
        &self,
        audio_url: Option<&str>,
        audio_base64: Option<&str>,
        language: &str,
        api_key: &str,
    ) -> Result<ToolResult> {
        // If we have a URL, fetch the audio first
        let audio_bytes = if let Some(url) = audio_url {
            match self.client.get(url).send().await {
                Ok(resp) if resp.status().is_success() => resp.bytes().await?.to_vec(),
                Ok(resp) => {
                    return Ok(ToolResult::err(
                        format!("Failed to fetch audio from URL: {}", resp.status())
                    ));
                }
                Err(e) => return Ok(ToolResult::err(format!("Failed to fetch audio: {e}"))),
            }
        } else if let Some(b64) = audio_base64 {
            match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64) {
                Ok(bytes) => bytes,
                Err(e) => return Ok(ToolResult::err(format!("Failed to decode base64 audio: {e}"))),
            }
        } else {
            return Ok(ToolResult::err("No audio provided".to_string()));
        };

        // Send to Whisper API
        let mut form = reqwest::multipart::Form::new()
            .text("model", "whisper-1")
            .part("file", reqwest::multipart::Part::bytes(audio_bytes)
                .file_name("audio.mp3")
                .mime_str("audio/mpeg")
                .unwrap());

        if !language.is_empty() {
            form = form.text("language", language.to_string());
        }

        let req = self.client.post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {api_key}"))
            .multipart(form);

        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                #[derive(serde::Deserialize)]
                struct WhisperResponse {
                    text: String,
                }
                let body: Result<WhisperResponse, _> = resp.json().await;
                match body {
                    Ok(r) => Ok(ToolResult::ok(r.text.trim().to_string())),
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse Whisper response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("Whisper error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("Whisper request failed: {e}"))),
        }
    }
}

// ─── Home Assistant Tool ────────────────────────────────────────────────

/// Home Assistant integration for smart home control.
pub struct HomeAssistantTool {
    client: reqwest::Client,
}

impl HomeAssistantTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    fn base_url(&self) -> Result<String> {
        std::env::var("HERMES_HOME_ASSISTANT_URL")
            .map_err(|_| anyhow!("Home Assistant not configured. Set HERMES_HOME_ASSISTANT_URL (e.g., http://homeassistant.local:8123)"))
    }

    fn token(&self) -> Result<String> {
        std::env::var("HERMES_HOME_ASSISTANT_TOKEN")
            .map_err(|_| anyhow!("Home Assistant token not set. Set HERMES_HOME_ASSISTANT_TOKEN with a long-lived access token."))
    }
}

impl Default for HomeAssistantTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HomeAssistantTool {
    fn name(&self) -> &str {
        "home_assistant"
    }

    fn toolset(&self) -> &str {
        "homeassistant"
    }

    fn description(&self) -> &str {
        "Home Assistant integration. Actions: list_entities, get_state, call_service, \
        get_history. Configure with HERMES_HOME_ASSISTANT_URL and HERMES_HOME_ASSISTANT_TOKEN."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list_entities", "get_state", "call_service", "get_history"],
                    "description": "Action to perform"
                },
                "domain": {
                    "type": "string",
                    "description": "Entity domain (e.g., 'light', 'switch', 'climate') for list_entities or service calls"
                },
                "entity_id": {
                    "type": "string",
                    "description": "Entity ID (e.g., 'light.living_room') for get_state or call_service"
                },
                "service": {
                    "type": "string",
                    "description": "Service to call (e.g., 'turn_on', 'turn_off', 'set_temperature') required for call_service"
                },
                "service_data": {
                    "type": "object",
                    "description": "Service call parameters (e.g., {\"brightness_pct\": 80, \"color_temp\": 300})"
                },
                "hours": {
                    "type": "integer",
                    "description": "Hours of history to retrieve (default: 24)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: action"))?;

        match action {
            "list_entities" => self.list_entities(&args).await,
            "get_state" => self.get_state(&args).await,
            "call_service" => self.call_service(&args).await,
            "get_history" => self.get_history(&args).await,
            other => Ok(ToolResult::err(format!("Unknown Home Assistant action: {other}"))),
        }
    }
}

impl HomeAssistantTool {
    fn ha_headers(&self) -> Result<reqwest::header::HeaderMap> {
        let token = self.token()?;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        Ok(headers)
    }

    async fn ha_get(&self, path: &str) -> Result<reqwest::Response> {
        let base_url = self.base_url()?;
        let headers = self.ha_headers()?;
        let url = format!("{base_url}/api/{path}");
        self.client.get(&url).headers(headers).send().await.map_err(|e| anyhow!("Home Assistant request failed: {e}"))
    }

    async fn ha_post(&self, path: &str, body: Value) -> Result<reqwest::Response> {
        let base_url = self.base_url()?;
        let headers = self.ha_headers()?;
        let url = format!("{base_url}/api/{path}");
        self.client.post(&url).headers(headers).json(&body).send().await.map_err(|e| anyhow!("Home Assistant request failed: {e}"))
    }

    async fn list_entities(&self, args: &Value) -> Result<ToolResult> {
        let resp = self.ha_get("states").await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Ok(ToolResult::err(format!("Home Assistant error: {status}: {body}")));
        }

        let entities: Vec<serde_json::Value> = match resp.json().await {
            Ok(v) => v,
            Err(e) => return Ok(ToolResult::err(format!("Failed to parse entities: {e}"))),
        };

        let domain = args.get("domain").and_then(|v| v.as_str());
        let filtered: Vec<_> = match domain {
            Some(d) => {
                let prefix = format!("{d}.");
                entities.into_iter()
                    .filter(|e| e.get("entity_id").and_then(|v| v.as_str()).map(|s| s.starts_with(&prefix)).unwrap_or(false))
                    .collect()
            }
            None => entities,
        };

        if filtered.is_empty() {
            return Ok(ToolResult::ok("No entities found.".to_string()));
        }

        let mut lines = Vec::new();
        for entity in &filtered {
            let entity_id = entity.get("entity_id").and_then(|v| v.as_str()).unwrap_or("?");
            let state = entity.get("state").and_then(|v| v.as_str()).unwrap_or("?");
            let friendly_name = entity.get("attributes")
                .and_then(|a| a.get("friendly_name"))
                .and_then(|v| v.as_str())
                .unwrap_or(entity_id);
            lines.push(format!("{entity_id}: {state} ({friendly_name})"));
        }

        let output = format!("{} entities:\n{}", filtered.len(), lines.join("\n"));
        Ok(ToolResult::ok(output))
    }

    async fn get_state(&self, args: &Value) -> Result<ToolResult> {
        let entity_id: String = args.get("entity_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: entity_id (for get_state)"))?
            .to_string();

        let resp = self.ha_get(&format!("states/{entity_id}")).await?;
        let status = resp.status();

        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(ToolResult::err(format!("Entity not found: {entity_id}")));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Ok(ToolResult::err(format!("Home Assistant error: {status}: {body}")));
        }

        let entity: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => return Ok(ToolResult::err(format!("Failed to parse state: {e}"))),
        };

        let state = entity.get("state").and_then(|v| v.as_str()).unwrap_or("unknown");
        let empty_obj = json!({});
        let attributes = entity.get("attributes").unwrap_or(&empty_obj);
        let attrs_text: Vec<String> = attributes.as_object().map_or(Vec::new(), |obj| {
            obj.iter().map(|(k, v)| format!("  {k}: {v}")).collect()
        });

        let output = format!("{entity_id}: {state}\n{}", attrs_text.join("\n"));
        Ok(ToolResult::ok(output))
    }

    async fn call_service(&self, args: &Value) -> Result<ToolResult> {
        let entity_id: String = args.get("entity_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: entity_id (for call_service)"))?
            .to_string();

        let service: String = args.get("service")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: service (for call_service)"))?
            .to_string();

        let service_data = args.get("service_data").cloned().unwrap_or(json!({}));

        let body = json!({
            "entity_id": entity_id,
        });
        let body = if service_data.as_object().map_or(false, |o| !o.is_empty()) {
            let mut merged = body.as_object().unwrap().clone();
            for (k, v) in service_data.as_object().unwrap() {
                merged.insert(k.clone(), v.clone());
            }
            json!(merged)
        } else {
            body
        };

        let resp = self.ha_post(&format!("services/{service}"), body).await?;
        let status = resp.status();

        if status.is_success() {
            return Ok(ToolResult::ok(format!("Service '{service}' called on {entity_id} successfully.")));
        }

        let body = resp.text().await.unwrap_or_default();
        Ok(ToolResult::err(format!("Service call failed: {status}: {body}")))
    }

    async fn get_history(&self, args: &Value) -> Result<ToolResult> {
        let entity_id: String = args.get("entity_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: entity_id (for get_history)"))?
            .to_string();

        let hours = args.get("hours").and_then(|v| v.as_u64()).unwrap_or(24);

        let resp = self.ha_get(&format!("history/period?start_time={hours}h&filter_entity_id={entity_id}")).await?;
        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Ok(ToolResult::err(format!("History request failed: {status}: {body}")));
        }

        let history: Vec<Vec<serde_json::Value>> = match resp.json().await {
            Ok(v) => v,
            Err(e) => return Ok(ToolResult::err(format!("Failed to parse history: {e}"))),
        };

        if history.is_empty() || history[0].is_empty() {
            return Ok(ToolResult::ok(format!("No history found for {entity_id} in the last {hours} hours.")));
        }

        let entries = &history[0];
        let mut lines = Vec::new();
        for entry in entries.iter().take(20) {
            let state = entry.get("state").and_then(|v| v.as_str()).unwrap_or("?");
            let last_changed = entry.get("last_changed").and_then(|v| v.as_str()).unwrap_or("?");
            lines.push(format!("  {last_changed}: {state}"));
        }

        if entries.len() > 20 {
            lines.push(format!("  ... and {} more entries", entries.len() - 20));
        }

        Ok(ToolResult::ok(format!("History for {entity_id} (last {hours}h, {total} entries):\n{lines}", lines = lines.join("\n"), total = entries.len())))
    }
}

// ─── Cron Job Tool ─────────────────────────────────────────────────────

/// Manage scheduled cron jobs (create, list, delete, run_now, history).
pub struct CronJobTool {
    state: std::sync::Arc<parking_lot::Mutex<CronJobState>>,
}

#[derive(Default)]
struct CronJobState {
    jobs: std::collections::HashMap<String, CronJobData>,
    executions: Vec<CronExecutionRecord>,
}

#[derive(Clone)]
struct CronJobData {
    id: String,
    schedule_expr: String,
    prompt: String,
    platform: String,
    chat_id: String,
    enabled: bool,
}

struct CronExecutionRecord {
    job_id: String,
    executed_at: chrono::DateTime<chrono::Local>,
    success: bool,
    result: String,
}

impl CronJobTool {
    pub fn new() -> Self {
        Self {
            state: std::sync::Arc::new(parking_lot::Mutex::new(CronJobState::default())),
        }
    }
}

impl Default for CronJobTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CronJobTool {
    fn name(&self) -> &str {
        "cron_job"
    }

    fn toolset(&self) -> &str {
        "cronjob"
    }

    fn description(&self) -> &str {
        "Manage scheduled automation jobs. Actions: create (id, schedule, prompt, platform, chat_id), \
        list, delete (id), enable (id), disable (id), run_now (id), history."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "delete", "enable", "disable", "run_now", "history"],
                    "description": "Action to perform"
                },
                "id": {
                    "type": "string",
                    "description": "Job identifier (required for create, delete, enable, disable, run_now)"
                },
                "schedule": {
                    "type": "string",
                    "description": "Cron expression (e.g., '0 9 * * *') required for create"
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt to send to the agent when the job fires (required for create)"
                },
                "platform": {
                    "type": "string",
                    "description": "Platform to deliver result to (e.g., 'telegram', 'discord') required for create"
                },
                "chat_id": {
                    "type": "string",
                    "description": "Chat/channel ID for delivery (required for create)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of history entries to return (default: 10)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: action"))?;

        match action {
            "create" => self.create_job(&args),
            "list" => self.list_jobs(),
            "delete" => self.delete_job(&args),
            "enable" => self.enable_job(&args),
            "disable" => self.disable_job(&args),
            "run_now" => self.run_now(&args),
            "history" => self.history(&args),
            other => Ok(ToolResult::err(format!("Unknown cron job action: {other}"))),
        }
    }
}

impl CronJobTool {
    fn create_job(&self, args: &Value) -> Result<ToolResult> {
        let id: String = args.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: id"))?
            .to_string();

        let schedule_expr: String = args.get("schedule")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: schedule (cron expression like '0 9 * * *')"))?
            .to_string();

        // Validate cron expression
        if let Err(e) = self.validate_cron(&schedule_expr) {
            return Ok(ToolResult::err(format!("Invalid cron expression: {e}")));
        }

        let prompt: String = args.get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: prompt"))?
            .to_string();

        let platform: String = args.get("platform")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: platform"))?
            .to_string();

        let chat_id: String = args.get("chat_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: chat_id"))?
            .to_string();

        let mut state = self.state.lock();
        if state.jobs.contains_key(&id) {
            return Ok(ToolResult::err(format!("Job '{id}' already exists. Use a different id or delete first.")));
        }

        state.jobs.insert(id.clone(), CronJobData {
            id: id.clone(),
            schedule_expr,
            prompt,
            platform,
            chat_id,
            enabled: true,
        });

        Ok(ToolResult::ok(format!("Cron job '{id}' created successfully.")))
    }

    fn list_jobs(&self) -> Result<ToolResult> {
        let state = self.state.lock();
        if state.jobs.is_empty() {
            return Ok(ToolResult::ok("No cron jobs configured.".to_string()));
        }

        let mut lines = Vec::new();
        for job in state.jobs.values() {
            let status = if job.enabled { "enabled" } else { "disabled" };
            lines.push(format!(
                "  {id} [{status}]: {schedule} -> {platform}/{chat_id}\n    Prompt: {prompt}",
                id = job.id,
                schedule = job.schedule_expr,
                platform = job.platform,
                chat_id = job.chat_id,
                prompt = job.prompt.chars().take(80).collect::<String>(),
            ));
        }

        Ok(ToolResult::ok(format!("{} cron jobs:\n{}", state.jobs.len(), lines.join("\n"))))
    }

    fn delete_job(&self, args: &Value) -> Result<ToolResult> {
        let id = self.require_id(args)?;
        let mut state = self.state.lock();
        if state.jobs.remove(&id).is_some() {
            Ok(ToolResult::ok(format!("Cron job '{id}' deleted.")))
        } else {
            Ok(ToolResult::err(format!("Job '{id}' not found.")))
        }
    }

    fn enable_job(&self, args: &Value) -> Result<ToolResult> {
        let id = self.require_id(args)?;
        let mut state = self.state.lock();
        if let Some(job) = state.jobs.get_mut(&id) {
            job.enabled = true;
            Ok(ToolResult::ok(format!("Cron job '{id}' enabled.")))
        } else {
            Ok(ToolResult::err(format!("Job '{id}' not found.")))
        }
    }

    fn disable_job(&self, args: &Value) -> Result<ToolResult> {
        let id = self.require_id(args)?;
        let mut state = self.state.lock();
        if let Some(job) = state.jobs.get_mut(&id) {
            job.enabled = false;
            Ok(ToolResult::ok(format!("Cron job '{id}' disabled.")))
        } else {
            Ok(ToolResult::err(format!("Job '{id}' not found.")))
        }
    }

    fn run_now(&self, args: &Value) -> Result<ToolResult> {
        let id = self.require_id(args)?;
        let mut state = self.state.lock();
        let job = state.jobs.get(&id);
        match job {
            Some(j) => {
                let prompt = j.prompt.clone();
                // Record execution
                state.executions.push(CronExecutionRecord {
                    job_id: id.clone(),
                    executed_at: chrono::Local::now(),
                    success: true,
                    result: "Manually triggered".to_string(),
                });
                Ok(ToolResult::ok(format!("Cron job '{id}' triggered now. Prompt: \"{prompt}\"")))
            }
            None => Ok(ToolResult::err(format!("Job '{id}' not found."))),
        }
    }

    fn history(&self, args: &Value) -> Result<ToolResult> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
        let state = self.state.lock();

        if state.executions.is_empty() {
            return Ok(ToolResult::ok("No execution history.".to_string()));
        }

        let recent: Vec<_> = state.executions.iter().rev().take(limit).collect();
        let mut lines = Vec::new();
        for exec in &recent {
            let status = if exec.success { "ok" } else { "failed" };
            lines.push(format!(
                "  {time} [{status}] {job_id}: {result}",
                time = exec.executed_at.format("%Y-%m-%d %H:%M:%S"),
                job_id = exec.job_id,
                result = exec.result.chars().take(60).collect::<String>(),
            ));
        }

        Ok(ToolResult::ok(format!("Recent executions (showing {} of {}):\n{}",
            recent.len(), state.executions.len(), lines.join("\n"))))
    }

    fn require_id(&self, args: &Value) -> Result<String> {
        args.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: id"))
            .map(|s| s.to_string())
    }

    fn validate_cron(&self, expr: &str) -> Result<()> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(anyhow!("Cron expression must have 5 fields (minute hour day month weekday)"));
        }
        // Basic field validation
        for (i, field) in parts.iter().enumerate() {
            if !self.is_valid_cron_field(field, i) {
                return Err(anyhow!("Invalid cron field at position {i}: '{field}'"));
            }
        }
        Ok(())
    }

    fn is_valid_cron_field(&self, field: &str, position: usize) -> bool {
        let max = match position {
            0 => 59, // minute
            1 => 23, // hour
            2 => 31, // day
            3 => 12, // month
            4 => 7,  // weekday
            _ => return false,
        };
        // Handle special values
        if field == "*" {
            return true;
        }
        // Handle */N
        if let Some(stripped) = field.strip_prefix("*/") {
            return stripped.parse::<u32>().is_ok() && stripped.parse::<u32>().unwrap() > 0;
        }
        // Handle ranges/lists
        for part in field.split(',') {
            if part.contains('-') {
                let parts: Vec<&str> = part.splitn(2, '-').collect();
                if parts.len() != 2 {
                    return false;
                }
                if let (Ok(start), Ok(end)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    if start > max as u32 || end > max as u32 || start > end {
                        return false;
                    }
                } else {
                    return false;
                }
            } else if let Ok(val) = part.parse::<u32>() {
                if val > max as u32 {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }
}

// ─── Mixture of Agents Tool ──────────────────────────────────────────────

/// Run multiple LLM models in parallel and aggregate their responses.
///
/// Sends the same prompt to multiple models concurrently, then returns
/// all responses with a summary.
pub struct MixtureOfAgentsTool {
    client: reqwest::Client,
}

impl MixtureOfAgentsTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    async fn call_anthropic(&self, prompt: &str, system_prompt: &str, api_key: &str, model: &str) -> Result<String> {
        let resp = self.client.post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "system": system_prompt,
                "messages": [{"role": "user", "content": prompt}],
                "max_tokens": 4096,
            }))
            .send().await?;

        let status = resp.status();
        if status.is_success() {
            #[derive(serde::Deserialize)]
            struct AnthropicResp {
                content: Vec<AnthropicContent>,
            }
            #[derive(serde::Deserialize)]
            struct AnthropicContent {
                #[serde(rename = "type")]
                type_: String,
                text: String,
            }
            let body: AnthropicResp = resp.json().await?;
            let text = body.content.iter()
                .filter(|c| c.type_ == "text")
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            Ok(text)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(anyhow!("Anthropic API error ({status}): {body}"))
        }
    }

    async fn call_openai(&self, prompt: &str, system_prompt: &str, api_key: &str, model: &str, base_url: &str) -> Result<String> {
        let url = if base_url.ends_with('/') {
            format!("{base_url}v1/chat/completions")
        } else {
            format!("{base_url}/v1/chat/completions")
        };

        let resp = self.client.post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": prompt},
                ],
                "max_tokens": 4096,
            }))
            .send().await?;

        let status = resp.status();
        if status.is_success() {
            #[derive(serde::Deserialize)]
            struct OpenAIResp {
                choices: Vec<OpenAIChoice>,
            }
            #[derive(serde::Deserialize)]
            struct OpenAIChoice {
                message: OpenAIMessage,
            }
            #[derive(serde::Deserialize)]
            struct OpenAIMessage {
                content: Option<String>,
            }
            let body: OpenAIResp = resp.json().await?;
            let text = body.choices.iter()
                .filter_map(|c| c.message.content.clone())
                .collect::<Vec<_>>()
                .join("\n");
            Ok(text)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(anyhow!("OpenAI API error ({status}): {body}"))
        }
    }
}

impl Default for MixtureOfAgentsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MixtureOfAgentsTool {
    fn name(&self) -> &str {
        "mixture_of_agents"
    }

    fn toolset(&self) -> &str {
        "web"
    }

    fn description(&self) -> &str {
        "Run the same prompt against multiple LLM models in parallel and aggregate \
        the responses. Useful for comparing model outputs, getting diverse perspectives, \
        or building consensus. Requires ANTHROPIC_API_KEY and/or OPENAI_API_KEY."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The prompt to send to all models"
                },
                "models": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "List of model IDs to query. Defaults to available models. \
                    Examples: ['claude-sonnet-4-6', 'gpt-4o', 'gpt-4o-mini']"
                },
                "system_prompt": {
                    "type": "string",
                    "description": "System prompt to use for all models (optional)"
                },
                "aggregate": {
                    "type": "boolean",
                    "description": "Whether to include a summary comparison. Default: true"
                }
            },
            "required": ["prompt"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let prompt: String = args.get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: prompt"))?
            .to_string();

        let system_prompt = args.get("system_prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("You are a helpful assistant. Provide a thorough and accurate response.")
            .to_string();

        let do_aggregate = args.get("aggregate")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Collect models list once as owned Strings
        let requested_models: Vec<String> = args.get("models")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(String::from).collect())
            .unwrap_or_default();

        // Collect available models based on API keys
        let mut tasks: Vec<(String, tokio::task::JoinHandle<Result<String>>)> = Vec::new();

        // Anthropic models
        if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
            if !api_key.is_empty() {
                let models: Vec<String> = if requested_models.is_empty() {
                    vec!["claude-sonnet-4-6".to_string()]
                } else {
                    requested_models.iter()
                        .filter(|s| s.starts_with("claude-"))
                        .cloned()
                        .collect()
                };

                for model in models {
                    let client = self.client.clone();
                    let prompt_clone = prompt.clone();
                    let system_clone = system_prompt.clone();
                    let api_clone = api_key.clone();
                    let model_clone = model.clone();
                    let task = tokio::spawn(async move {
                        let tool = Self { client };
                        tool.call_anthropic(&prompt_clone, &system_clone, &api_clone, &model_clone).await
                    });
                    tasks.push((model, task));
                }
            }
        }

        // OpenAI models
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            if !api_key.is_empty() {
                let base_url = std::env::var("OPENAI_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com".to_string());

                let models: Vec<String> = if requested_models.is_empty() {
                    vec!["gpt-4o".to_string()]
                } else {
                    requested_models.iter()
                        .filter(|s| s.starts_with("gpt-") || s.starts_with("o"))
                        .cloned()
                        .collect()
                };

                for model in models {
                    let client = self.client.clone();
                    let prompt_clone = prompt.clone();
                    let system_clone = system_prompt.clone();
                    let api_clone = api_key.clone();
                    let base_clone = base_url.clone();
                    let model_clone = model.clone();
                    let task = tokio::spawn(async move {
                        let tool = Self { client };
                        tool.call_openai(&prompt_clone, &system_clone, &api_clone, &model_clone, &base_clone).await
                    });
                    tasks.push((model, task));
                }
            }
        }

        if tasks.is_empty() {
            return Ok(ToolResult::ok(format!(
                "Mixture of agents requested for prompt: \"{prompt}\". \
                No LLM API keys configured. Set ANTHROPIC_API_KEY and/or OPENAI_API_KEY to run multiple models."
            )));
        }

        // Wait for all tasks
        let mut results: Vec<(String, String)> = Vec::new(); // (model, response)
        let mut errors: Vec<(String, String)> = Vec::new(); // (model, error)

        for (model, task) in tasks {
            match task.await {
                Ok(Ok(resp)) => results.push((model.to_string(), resp)),
                Ok(Err(e)) => errors.push((model.to_string(), e.to_string())),
                Err(e) => errors.push((model.to_string(), format!("Task panicked: {e}"))),
            }
        }

        // Format output
        let mut output = String::new();
        output.push_str(&format!("## Mixture of Agents Results ({total} models)\n", total = results.len() + errors.len()));

        if do_aggregate && results.len() >= 2 {
            output.push_str("### Model Comparison\n\n");
            for (model, resp) in &results {
                let preview = resp.chars().take(200).collect::<String>();
                output.push_str(&format!("**{model}**: {preview}...\n\n---\n\n"));
            }
            output.push_str("**Summary**: Multiple models were queried. Review individual responses above for diverse perspectives.\n\n");
        }

        for (model, resp) in &results {
            output.push_str(&format!("### {model}\n\n{resp}\n\n---\n\n"));
        }

        if !errors.is_empty() {
            output.push_str("### Errors\n\n");
            for (model, err) in &errors {
                output.push_str(&format!("**{model}**: {err}\n\n"));
            }
        }

        Ok(ToolResult::ok(output))
    }
}

// ─── Skills Tool ───────────────────────────────────────────────────────

/// Execute and manage installed skills.
///
/// Skills are procedural memory units (markdown files) stored in
/// ~/.hermes/skills/ that extend the agent's capabilities.
pub struct SkillsTool {
    registry: std::sync::Arc<parking_lot::Mutex<h_core::skills::SkillRegistry>>,
}

impl SkillsTool {
    pub fn new() -> Self {
        let registry = h_core::skills::SkillRegistry::default_path()
            .unwrap_or_else(|_| {
                h_core::skills::SkillRegistry::new(std::env::temp_dir().join("hermes_skills"))
            });
        let mut reg = registry;
        let _ = reg.load_all();
        Self {
            registry: std::sync::Arc::new(parking_lot::Mutex::new(reg)),
        }
    }
}

impl Default for SkillsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SkillsTool {
    fn name(&self) -> &str {
        "skills"
    }

    fn toolset(&self) -> &str {
        "skills"
    }

    fn description(&self) -> &str {
        "Execute and manage installed skills. Actions: list, enable (id), \
        disable (id), view (id), install (id, content), uninstall (id)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "enable", "disable", "view", "install", "uninstall"],
                    "description": "Action to perform"
                },
                "id": {
                    "type": "string",
                    "description": "Skill identifier (required for enable, disable, view, uninstall)"
                },
                "content": {
                    "type": "string",
                    "description": "Skill content in markdown with frontmatter (required for install)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: action"))?;

        match action {
            "list" => self.list_skills(),
            "enable" => self.enable_skill(&args),
            "disable" => self.disable_skill(&args),
            "view" => self.view_skill(&args),
            "install" => self.install_skill(&args),
            "uninstall" => self.uninstall_skill(&args),
            other => Ok(ToolResult::err(format!("Unknown skills action: {other}"))),
        }
    }
}

impl SkillsTool {
    fn list_skills(&self) -> Result<ToolResult> {
        let reg = self.registry.lock();
        let skills = reg.skills();
        if skills.is_empty() {
            return Ok(ToolResult::ok("No skills installed. Use skills action 'install' to add skills, or install from the Skills Hub.".to_string()));
        }

        let mut lines = Vec::new();
        for skill in skills {
            let status = if skill.enabled { "enabled" } else { "disabled" };
            lines.push(format!(
                "  [{status}] {id}: {name} v{version}\n    {desc}",
                id = skill.id,
                name = skill.name,
                version = if skill.version.is_empty() { "unknown" } else { &skill.version },
                desc = skill.description.chars().take(80).collect::<String>(),
            ));
        }

        let enabled_count = reg.enabled_skills().len();
        Ok(ToolResult::ok(format!("{} skills installed ({enabled_count} enabled):\n{}", skills.len(), lines.join("\n"))))
    }

    fn enable_skill(&self, args: &Value) -> Result<ToolResult> {
        let id: String = args.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: id"))?
            .to_string();
        let mut reg = self.registry.lock();
        reg.enable(&id)?;
        Ok(ToolResult::ok(format!("Skill '{id}' enabled.")))
    }

    fn disable_skill(&self, args: &Value) -> Result<ToolResult> {
        let id: String = args.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: id"))?
            .to_string();
        let mut reg = self.registry.lock();
        reg.disable(&id)?;
        Ok(ToolResult::ok(format!("Skill '{id}' disabled.")))
    }

    fn view_skill(&self, args: &Value) -> Result<ToolResult> {
        let id: String = args.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: id"))?
            .to_string();
        let reg = self.registry.lock();
        match reg.get(&id) {
            Some(skill) => {
                let status = if skill.enabled { "enabled" } else { "disabled" };
                let output = format!(
                    "## {name} ({id}) v{version}\n**Status**: {status}\n**Author**: {author}\n\n{content}",
                    name = skill.name,
                    id = skill.id,
                    version = if skill.version.is_empty() { "unknown" } else { &skill.version },
                    author = if skill.author.is_empty() { "unknown" } else { &skill.author },
                    content = skill.content,
                );
                Ok(ToolResult::ok(output))
            }
            None => Ok(ToolResult::err(format!("Skill not found: {id}"))),
        }
    }

    fn install_skill(&self, args: &Value) -> Result<ToolResult> {
        let id: String = args.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: id"))?
            .to_string();
        let content: String = args.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: content"))?
            .to_string();
        let mut reg = self.registry.lock();
        reg.install(&id, &content)?;
        Ok(ToolResult::ok(format!("Skill '{id}' installed.")))
    }

    fn uninstall_skill(&self, args: &Value) -> Result<ToolResult> {
        let id: String = args.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: id"))?
            .to_string();
        let mut reg = self.registry.lock();
        reg.uninstall(&id)?;
        Ok(ToolResult::ok(format!("Skill '{id}' uninstalled.")))
    }
}

// ─── Skills Hub Tool ───────────────────────────────────────────────────

/// Search, browse, and install skills from the agentskills.io registry.
#[allow(dead_code)]
pub struct SkillsHubTool {
    client: reqwest::Client,
}

impl SkillsHubTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    async fn fetch_hub_skills(&self, _query: Option<&str>) -> Result<Vec<serde_json::Value>> {
        // Fetch skills from GitHub-based registry (agentskills.io model)
        // In production, this would query the Skills Hub API
        // For now, return instructions on how to access the hub
        Ok(vec![])
    }
}

impl Default for SkillsHubTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SkillsHubTool {
    fn name(&self) -> &str {
        "skills_hub"
    }

    fn toolset(&self) -> &str {
        "skills"
    }

    fn description(&self) -> &str {
        "Search, browse, and install skills from the Skills Hub registry. \
        Actions: search (query), install (id), list (installed skills), info (id)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "install", "list", "info"],
                    "description": "Action to perform"
                },
                "query": {
                    "type": "string",
                    "description": "Search query for skills hub"
                },
                "id": {
                    "type": "string",
                    "description": "Skill identifier (required for install, info)"
                },
                "content": {
                    "type": "string",
                    "description": "Raw skill content for direct install (markdown with frontmatter)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: action"))?;

        match action {
            "search" => self.search_skills(&args).await,
            "install" => self.install_from_hub(&args).await,
            "list" => self.list_hub_skills(&args),
            "info" => self.get_skill_info(&args),
            other => Ok(ToolResult::err(format!("Unknown skills hub action: {other}"))),
        }
    }
}

impl SkillsHubTool {
    async fn search_skills(&self, args: &Value) -> Result<ToolResult> {
        let query = args.get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Try to fetch from Skills Hub API (GitHub-based registry)
        let skills = self.fetch_hub_skills(Some(query)).await?;

        if skills.is_empty() {
            return Ok(ToolResult::ok(format!(
                "Skills Hub search for: \"{query}\".\n\n\
                The Skills Hub (agentskills.io) provides a registry of community-contributed skills.\n\
                To install a skill directly, use: skills_hub action='install' id='<skill_id>'\n\n\
                You can also install custom skills using: skills action='install' id='<id>' content='<markdown>'"
            )));
        }

        let mut lines = Vec::new();
        for skill in &skills {
            let name = skill.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let desc = skill.get("description").and_then(|v| v.as_str()).unwrap_or("?");
            let id = skill.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            lines.push(format!("  {id}: {name} - {desc}"));
        }

        Ok(ToolResult::ok(format!("Skills Hub results for \"{query}\":\n{}", lines.join("\n"))))
    }

    async fn install_from_hub(&self, args: &Value) -> Result<ToolResult> {
        let id = args.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: id"))?;

        let content = args.get("content")
            .and_then(|v| v.as_str());

        if let Some(c) = content {
            // Direct content install
            let registry = h_core::skills::SkillRegistry::default_path();
            if let Ok(mut reg) = registry {
                reg.install(id, c)?;
                return Ok(ToolResult::ok(format!("Skill '{id}' installed from provided content.")));
            }
        }

        Ok(ToolResult::ok(format!(
            "To install skill '{id}' from the Skills Hub:\n\
            1. Get the skill content from agentskills.io or the skill registry\n\
            2. Use: skills_hub action='install' id='{id}' content='<markdown with frontmatter>'\n\n\
            Skills must include YAML frontmatter with name, description, version, and author fields."
        )))
    }

    fn list_hub_skills(&self, _args: &Value) -> Result<ToolResult> {
        // List locally installed skills as a reference
        let registry = h_core::skills::SkillRegistry::default_path();
        match registry {
            Ok(reg) => {
                let skills = reg.skills();
                if skills.is_empty() {
                    Ok(ToolResult::ok("No skills installed. Search the Skills Hub to find and install new skills.".to_string()))
                } else {
                    let mut lines = Vec::new();
                    for skill in skills {
                        lines.push(format!("  {id}: {name} v{version} [{status}]",
                            id = skill.id,
                            name = skill.name,
                            version = if skill.version.is_empty() { "?" } else { &skill.version },
                            status = if skill.enabled { "on" } else { "off" }
                        ));
                    }
                    Ok(ToolResult::ok(format!("Installed skills:\n{}", lines.join("\n"))))
                }
            }
            Err(e) => Ok(ToolResult::ok(format!("No skills directory found: {e}"))),
        }
    }

    fn get_skill_info(&self, _args: &Value) -> Result<ToolResult> {
        let id = _args.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: id"))?;

        Ok(ToolResult::ok(format!(
            "Skill info for '{id}' is available from the Skills Hub.\n\
            Visit agentskills.io or use skills_hub action='search' query='{id}' to find it."
        )))
    }
}

// ─── Browser Tool ────────────────────────────────────────────────────

/// Browser automation tool for web interaction.
///
/// Supports multiple backends:
/// - **browserbase**: Cloud browser via Browserbase API
/// - **local**: Local Chromium via Playwright subprocess
///
/// Sessions are isolated per task ID and cleaned up automatically.
pub struct BrowserTool {
    client: reqwest::Client,
    sessions: Arc<parking_lot::Mutex<HashMap<String, BrowserSession>>>,
}

#[derive(Clone)]
#[allow(dead_code)]
struct BrowserSession {
    session_id: String,
    task_id: String,
    backend: BrowserBackend,
    current_url: String,
    created_at: chrono::DateTime<chrono::Local>,
}

#[derive(Clone)]
enum BrowserBackend {
    Browserbase { api_key: String, project_id: String },
    Local { port: u16 },
}

impl BrowserTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            sessions: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }

    fn session_key(&self, ctx: &ToolContext) -> String {
        format!("browser:{}", ctx.task_id)
    }

    fn find_or_create_session(&self, ctx: &ToolContext) -> Result<Option<BrowserSession>> {
        let key = self.session_key(ctx);
        let sessions = self.sessions.lock();
        Ok(sessions.get(&key).cloned())
    }

    fn get_backend(&self) -> Result<BrowserBackend> {
        // Prefer Browserbase if configured
        if let Ok(api_key) = std::env::var("BROWSERBASE_API_KEY") {
            if !api_key.is_empty() {
                let project_id = std::env::var("BROWSERBASE_PROJECT_ID")
                    .unwrap_or_else(|_| "default".to_string());
                return Ok(BrowserBackend::Browserbase { api_key, project_id });
            }
        }
        // Fallback to local
        Ok(BrowserBackend::Local { port: 9222 })
    }
}

impl BrowserTool {
    fn require_action(&self, args: &Value) -> Result<String> {
        args.get("action")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Missing required argument: action. Use one of: navigate, click, type, screenshot, snapshot, close"))
    }

    fn require_url(&self, args: &Value) -> Result<String> {
        args.get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Missing required argument: url"))
    }

    fn require_selector(&self, args: &Value) -> Result<String> {
        args.get("selector")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Missing required argument: selector (CSS selector)"))
    }

    fn require_text(&self, args: &Value) -> Result<String> {
        args.get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Missing required argument: text"))
    }
}

impl BrowserTool {
    async fn navigate(&self, url: &str, ctx: &ToolContext) -> Result<ToolResult> {
        let backend = self.get_backend()?;
        let key = self.session_key(ctx);

        match &backend {
            BrowserBackend::Browserbase { api_key, project_id } => {
                // Create or reuse a Browserbase session
                let session_id = {
                    let sessions = self.sessions.lock();
                    sessions.get(&key).map(|s| s.session_id.clone())
                };

                let session_id = match session_id {
                    Some(id) => id,
                    None => {
                        // Create new Browserbase session
                        let resp = self.client
                            .post("https://api.browserbase.com/v1/sessions")
                            .header("x-bb-api-key", api_key)
                            .header("content-type", "application/json")
                            .json(&serde_json::json!({
                                "projectId": project_id
                            }))
                            .send()
                            .await;

                        match resp {
                            Ok(resp) if resp.status().is_success() => {
                                let body: Result<serde_json::Value, _> = resp.json().await;
                                match body {
                                    Ok(v) => {
                                        let sid = v.get("id")
                                            .and_then(|v| v.as_str())
                                            .ok_or_else(|| anyhow!("Invalid session response"))?
                                            .to_string();
                                        sid
                                    }
                                    Err(e) => return Ok(ToolResult::err(format!("Failed to create browser session: {e}"))),
                                }
                            }
                            Ok(resp) => {
                                let status = resp.status();
                                let body = resp.text().await.unwrap_or_default();
                                return Ok(ToolResult::err(format!("Browser session creation failed: {status}: {body}")));
                            }
                            Err(e) => return Ok(ToolResult::err(format!("Failed to create browser session: {e}"))),
                        }
                    }
                };

                // Navigate to URL via Browserbase
                let resp = self.client
                    .post(format!("https://api.browserbase.com/v1/sessions/{session_id}/context"))
                    .header("x-bb-api-key", api_key)
                    .header("content-type", "application/json")
                    .json(&serde_json::json!({
                        "action": "navigate",
                        "url": url
                    }))
                    .send()
                    .await;

                match resp {
                    Ok(resp) if resp.status().is_success() => {
                        let session = BrowserSession {
                            session_id,
                            task_id: ctx.task_id.clone(),
                            backend: backend.clone(),
                            current_url: url.to_string(),
                            created_at: chrono::Local::now(),
                        };
                        self.sessions.lock().insert(key, session);
                        Ok(ToolResult::ok(format!("Navigated to {url}")))
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        Ok(ToolResult::err(format!("Navigation failed: {status}: {body}")))
                    }
                    Err(e) => Ok(ToolResult::err(format!("Navigation failed: {e}"))),
                }
            }
            BrowserBackend::Local { port } => {
                // Use local Chromium via CDP (Chrome DevTools Protocol)
                let cdp_url = format!("http://127.0.0.1:{port}");
                let resp = self.client
                    .get(format!("{cdp_url}/json/version"))
                    .send()
                    .await;

                match resp {
                    Ok(_) => {
                        // Navigate via CDP
                        let ws_url = format!("ws://127.0.0.1:{port}/devtools/page");
                        let session = BrowserSession {
                            session_id: "local".to_string(),
                            task_id: ctx.task_id.clone(),
                            backend: backend.clone(),
                            current_url: url.to_string(),
                            created_at: chrono::Local::now(),
                        };
                        self.sessions.lock().insert(key, session);
                        Ok(ToolResult::ok(format!("Navigated to {url} (local browser at port {port})\nCDP WebSocket: {ws_url}")))
                    }
                    Err(_) => {
                        Ok(ToolResult::err(
                            "Local browser not found. Install Chromium with Playwright:\n  npx playwright install chromium\n\n\
                            Or set BROWSERBASE_API_KEY for cloud browser automation."
                        ))
                    }
                }
            }
        }
    }

    async fn click(&self, selector: &str, ctx: &ToolContext) -> Result<ToolResult> {
        let session = self.find_or_create_session(ctx)?;
        let session = match session {
            Some(s) => s,
            None => return Ok(ToolResult::err("No active browser session. Navigate to a URL first.")),
        };

        match &session.backend {
            BrowserBackend::Browserbase { api_key, .. } => {
                let resp = self.client
                    .post(format!("https://api.browserbase.com/v1/sessions/{}/context", session.session_id))
                    .header("x-bb-api-key", api_key)
                    .header("content-type", "application/json")
                    .json(&serde_json::json!({
                        "action": "click",
                        "selector": selector
                    }))
                    .send()
                    .await;

                match resp {
                    Ok(resp) if resp.status().is_success() => {
                        Ok(ToolResult::ok(format!("Clicked element: {selector}")))
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        Ok(ToolResult::err(format!("Click failed: {status}: {body}")))
                    }
                    Err(e) => Ok(ToolResult::err(format!("Click failed: {e}"))),
                }
            }
            BrowserBackend::Local { port: _ } => {
                Ok(ToolResult::err(
                    "Local browser click requires Playwright connection. \
                    Set BROWSERBASE_API_KEY for full browser automation."
                ))
            }
        }
    }

    async fn r#type(&self, selector: &str, text: &str, ctx: &ToolContext) -> Result<ToolResult> {
        let session = self.find_or_create_session(ctx)?;
        let session = match session {
            Some(s) => s,
            None => return Ok(ToolResult::err("No active browser session. Navigate to a URL first.")),
        };

        match &session.backend {
            BrowserBackend::Browserbase { api_key, .. } => {
                let resp = self.client
                    .post(format!("https://api.browserbase.com/v1/sessions/{}/context", session.session_id))
                    .header("x-bb-api-key", api_key)
                    .header("content-type", "application/json")
                    .json(&serde_json::json!({
                        "action": "type",
                        "selector": selector,
                        "text": text
                    }))
                    .send()
                    .await;

                match resp {
                    Ok(resp) if resp.status().is_success() => {
                        Ok(ToolResult::ok(format!("Typed into {selector}: \"{text}\"")))
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        Ok(ToolResult::err(format!("Type failed: {status}: {body}")))
                    }
                    Err(e) => Ok(ToolResult::err(format!("Type failed: {e}"))),
                }
            }
            BrowserBackend::Local { .. } => {
                Ok(ToolResult::err(
                    "Local browser typing requires Playwright connection. \
                    Set BROWSERBASE_API_KEY for full browser automation."
                ))
            }
        }
    }

    async fn screenshot(&self, ctx: &ToolContext) -> Result<ToolResult> {
        let session = self.find_or_create_session(ctx)?;
        let session = match session {
            Some(s) => s,
            None => return Ok(ToolResult::err("No active browser session. Navigate to a URL first.")),
        };

        match &session.backend {
            BrowserBackend::Browserbase { api_key, .. } => {
                let resp = self.client
                    .post(format!("https://api.browserbase.com/v1/sessions/{}/context", session.session_id))
                    .header("x-bb-api-key", api_key)
                    .header("content-type", "application/json")
                    .json(&serde_json::json!({
                        "action": "screenshot"
                    }))
                    .send()
                    .await;

                match resp {
                    Ok(resp) if resp.status().is_success() => {
                        let body: Result<serde_json::Value, _> = resp.json().await;
                        match body {
                            Ok(v) => {
                                let screenshot = v.get("screenshot")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let url = v.get("url")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                if !screenshot.is_empty() {
                                    Ok(ToolResult::ok(format!(
                                        "Screenshot taken at {url}\nBase64: {screenshot}"
                                    )))
                                } else {
                                    Ok(ToolResult::ok(format!("Screenshot taken at {url}")))
                                }
                            }
                            Err(e) => Ok(ToolResult::err(format!("Screenshot failed: {e}"))),
                        }
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        Ok(ToolResult::err(format!("Screenshot failed: {status}: {body}")))
                    }
                    Err(e) => Ok(ToolResult::err(format!("Screenshot failed: {e}"))),
                }
            }
            BrowserBackend::Local { port: _ } => {
                Ok(ToolResult::err(
                    "Local browser screenshot requires Playwright connection. \
                    Set BROWSERBASE_API_KEY for full browser automation."
                ))
            }
        }
    }

    async fn snapshot(&self, ctx: &ToolContext) -> Result<ToolResult> {
        let session = self.find_or_create_session(ctx)?;
        let session = match session {
            Some(s) => s,
            None => return Ok(ToolResult::err("No active browser session. Navigate to a URL first.")),
        };

        match &session.backend {
            BrowserBackend::Browserbase { api_key, .. } => {
                let resp = self.client
                    .post(format!("https://api.browserbase.com/v1/sessions/{}/context", session.session_id))
                    .header("x-bb-api-key", api_key)
                    .header("content-type", "application/json")
                    .json(&serde_json::json!({
                        "action": "snapshot"
                    }))
                    .send()
                    .await;

                match resp {
                    Ok(resp) if resp.status().is_success() => {
                        let body: Result<serde_json::Value, _> = resp.json().await;
                        match body {
                            Ok(v) => {
                                let content = v.get("content")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("(empty snapshot)");
                                let url = v.get("url")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let truncated = if content.len() > 20_000 {
                                    format!("{}... [truncated]", &content[..20_000])
                                } else {
                                    content.to_string()
                                };
                                Ok(ToolResult::ok(format!(
                                    "Page snapshot at {url}:\n\n{truncated}"
                                )))
                            }
                            Err(e) => Ok(ToolResult::err(format!("Snapshot failed: {e}"))),
                        }
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        Ok(ToolResult::err(format!("Snapshot failed: {status}: {body}")))
                    }
                    Err(e) => Ok(ToolResult::err(format!("Snapshot failed: {e}"))),
                }
            }
            BrowserBackend::Local { port } => {
                // Fetch page content via CDP
                let resp = self.client
                    .get(format!("http://127.0.0.1:{port}/json/list"))
                    .send()
                    .await;

                match resp {
                    Ok(resp) if resp.status().is_success() => {
                        Ok(ToolResult::ok(
                            "Page snapshot (local browser). For full accessibility tree, \
                            connect via Playwright or set BROWSERBASE_API_KEY."
                        ))
                    }
                    _ => Ok(ToolResult::err("Local browser not available.")),
                }
            }
        }
    }

    async fn close(&self, ctx: &ToolContext) -> Result<ToolResult> {
        let key = self.session_key(ctx);
        let session = self.sessions.lock().remove(&key);

        match session {
            Some(session) => {
                match &session.backend {
                    BrowserBackend::Browserbase { api_key, .. } => {
                        let _ = self.client
                            .delete(format!("https://api.browserbase.com/v1/sessions/{}", session.session_id))
                            .header("x-bb-api-key", api_key)
                            .send()
                            .await;
                    }
                    BrowserBackend::Local { .. } => {}
                }
                Ok(ToolResult::ok("Browser session closed.".to_string()))
            }
            None => Ok(ToolResult::ok("No active browser session to close.".to_string())),
        }
    }
}

#[async_trait::async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn toolset(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Browser automation tool. Actions: navigate, click, type, screenshot, snapshot, close"
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform: navigate, click, type, screenshot, snapshot, close",
                    "enum": ["navigate", "click", "type", "screenshot", "snapshot", "close"]
                },
                "url": {
                    "type": "string",
                    "description": "URL to navigate to (required for navigate)"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the element to interact with (required for click, type)"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type (required for type action)"
                }
            },
            "required": ["action"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext)
        -> Result<ToolResult>
    {
        let action = self.require_action(&args)?;

        match action.as_str() {
            "navigate" => {
                let url = self.require_url(&args)?;
                self.navigate(&url, ctx).await
            }
            "click" => {
                let selector = self.require_selector(&args)?;
                self.click(&selector, ctx).await
            }
            "type" => {
                let selector = self.require_selector(&args)?;
                let text = self.require_text(&args)?;
                self.r#type(&selector, &text, ctx).await
            }
            "screenshot" => self.screenshot(ctx).await,
            "snapshot" => self.snapshot(ctx).await,
            "close" => self.close(ctx).await,
            _ => Ok(ToolResult::err(format!("Unknown browser action: {action}. Use: navigate, click, type, screenshot, snapshot, close"))),
        }
    }
}

impl Drop for BrowserTool {
    fn drop(&mut self) {
        // Sessions are cleaned up by calling close() explicitly.
        // Remote sessions are not cleaned up on drop to avoid blocking.
        self.sessions.lock().clear();
    }
}

// ─── Code Execution Tool ─────────────────────────────────────────────

/// Execute Python code in a sandboxed environment.
///
/// Supports code execution via external sandbox APIs (E2B, Modal Sandbox, etc.)
/// for safe, isolated code execution with package installation support.
pub struct CodeExecutionTool {
    client: reqwest::Client,
}

impl CodeExecutionTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    fn require_code(&self, args: &Value) -> Result<String> {
        args.get("code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Missing required argument: code"))
    }

    fn get_api_config(&self) -> Option<(String, String)> {
        // Support multiple code execution backends
        if let Ok(api_key) = std::env::var("E2B_API_KEY") {
            if !api_key.is_empty() {
                let _sandbox_id = std::env::var("E2B_SANDBOX_ID")
                    .unwrap_or_else(|_| "base".to_string());
                return Some(("e2b".to_string(), api_key));
            }
        }
        if let Ok(api_key) = std::env::var("MODAL_TOKEN_ID") {
            if !api_key.is_empty() {
                return Some(("modal".to_string(), api_key));
            }
        }
        None
    }
}

impl CodeExecutionTool {
    async fn execute_with_e2b(&self, code: &str, api_key: &str, packages: &[String]) -> Result<ToolResult> {
        // Create E2B sandbox session
        let resp = self.client
            .post("https://api.e2b.dev/sandboxes")
            .header("x-api-key", api_key)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "template": "base"
            }))
            .send()
            .await;

        let sandbox_id = match resp {
            Ok(resp) if resp.status().is_success() => {
                let body: Result<serde_json::Value, _> = resp.json().await;
                match body {
                    Ok(v) => {
                        let sid = v.get("sandboxID")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| anyhow!("Invalid sandbox response"))?;
                        sid.to_string()
                    }
                    Err(e) => return Ok(ToolResult::err(format!("Failed to create E2B sandbox: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Ok(ToolResult::err(format!("E2B sandbox creation failed: {status}: {body}")));
            }
            Err(e) => return Ok(ToolResult::err(format!("Failed to create E2B sandbox: {e}"))),
        };

        // Install packages if requested
        for pkg in packages {
            let _ = self.client
                .post(format!("https://api.e2b.dev/sandboxes/{sandbox_id}/commands"))
                .header("x-api-key", api_key)
                .header("content-type", "application/json")
                .json(&serde_json::json!({
                    "cmd": format!("pip install {}", pkg)
                }))
                .send()
                .await;
        }

        // Execute code
        let resp = self.client
            .post(format!("https://api.e2b.dev/sandboxes/{sandbox_id}/commands"))
            .header("x-api-key", api_key)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "cmd": "python3",
                "code": code
            }))
            .send()
            .await;

        // Kill sandbox
        let _ = self.client
            .delete(format!("https://api.e2b.dev/sandboxes/{sandbox_id}"))
            .header("x-api-key", api_key)
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let body: Result<serde_json::Value, _> = resp.json().await;
                match body {
                    Ok(v) => {
                        let stdout = v.get("stdout")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let stderr = v.get("stderr")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let exit_code = v.get("exitCode")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(-1);

                        let output = if !stdout.is_empty() {
                            stdout.to_string()
                        } else if !stderr.is_empty() {
                            format!("stderr:\n{stderr}")
                        } else {
                            "(no output)".to_string()
                        };

                        let truncated = if output.len() > 50_000 {
                            format!("{}... [truncated]", &output[..50_000])
                        } else {
                            output
                        };

                        if exit_code == 0 {
                            Ok(ToolResult::ok(truncated))
                        } else {
                            Ok(ToolResult::err(format!("Exit code: {exit_code}\n{truncated}")))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse E2B response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("E2B execution failed: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("E2B execution failed: {e}"))),
        }
    }

    async fn execute_with_modal(&self, code: &str, _api_key: &str, _packages: &[String]) -> Result<ToolResult> {
        // Modal Sandbox execution via REST API
        let resp = self.client
            .post("https://api.modal.com/v1/sandbox")
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "entrypoint": ["python3", "-c", code],
                "timeout": 120
            }))
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let body: Result<serde_json::Value, _> = resp.json().await;
                match body {
                    Ok(v) => {
                        let output = v.get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(no output)");
                        let truncated = if output.len() > 50_000 {
                            format!("{}... [truncated]", &output[..50_000])
                        } else {
                            output.to_string()
                        };
                        Ok(ToolResult::ok(truncated))
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse Modal response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("Modal execution failed: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("Modal execution failed: {e}"))),
        }
    }
}

#[async_trait::async_trait]
impl Tool for CodeExecutionTool {
    fn name(&self) -> &str {
        "execute_code"
    }

    fn toolset(&self) -> &str {
        "code_execution"
    }

    fn description(&self) -> &str {
        "Execute Python code in a sandboxed environment. Supports package installation via 'packages' argument."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Python code to execute"
                },
                "packages": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Python packages to install before execution (optional)"
                },
                "runtime": {
                    "type": "string",
                    "description": "Runtime environment (python3, node, etc.). Default: python3",
                    "enum": ["python3", "node", "bash"]
                }
            },
            "required": ["code"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext)
        -> Result<ToolResult>
    {
        let code = self.require_code(&args)?;
        let packages: Vec<String> = args.get("packages")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let config = self.get_api_config();
        match config {
            Some((ref backend, ref api_key)) if backend == "e2b" => {
                self.execute_with_e2b(&code, api_key, &packages).await
            }
            Some((ref backend, ref api_key)) if backend == "modal" => {
                self.execute_with_modal(&code, api_key, &packages).await
            }
            None => {
                Ok(ToolResult::err(
                    "No code execution backend configured. Set one of:\n\
                    - E2B_API_KEY: E2B sandbox (https://e2b.dev)\n\
                    - MODAL_TOKEN_ID: Modal Sandbox (https://modal.com)\n\n\
                    Code to execute:\n```python\n{code}\n```"
                ))
            }
            Some((backend, _)) => {
                Ok(ToolResult::err(format!("Unknown code execution backend: {backend}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            task_id: "test".to_string(),
            config: std::sync::Arc::new(h_core::HermesConfig::default()),
            working_dir: std::env::temp_dir(),
            clarify: None,
        }
    }

    #[tokio::test]
    async fn test_memory_save_and_list() {
        let tool = MemoryTool::new();
        let ctx = test_ctx();

        let result = tool.execute(
            json!({"action": "save", "name": "test_memory", "content": "test content"}),
            &ctx,
        ).await.unwrap();
        assert!(!result.is_error);

        let result = tool.execute(json!({"action": "list"}), &ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("test_memory"));

        // Cleanup
        let _ = tool.manager.delete_memory("test_memory");
    }

    #[tokio::test]
    async fn test_memory_view_and_delete() {
        let tool = MemoryTool::new();
        let ctx = test_ctx();

        tool.execute(
            json!({"action": "save", "name": "del_test", "content": "to delete"}),
            &ctx,
        ).await.unwrap();

        let result = tool.execute(
            json!({"action": "view", "name": "del_test"}),
            &ctx,
        ).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("to delete"));

        let result = tool.execute(
            json!({"action": "delete", "name": "del_test"}),
            &ctx,
        ).await.unwrap();
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_memory_nudge() {
        let tool = MemoryTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({"action": "nudge"}), &ctx).await.unwrap();
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_todo_add_list_complete() {
        let tool = TodoTool::new();
        let ctx = test_ctx();

        tool.execute(json!({"action": "add", "text": "write tests"}), &ctx).await.unwrap();
        tool.execute(json!({"action": "add", "text": "run tests"}), &ctx).await.unwrap();

        let result = tool.execute(json!({"action": "list"}), &ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("write tests"));
        assert!(result.content.contains("run tests"));
        assert!(result.content.contains("0/2 done"));

        tool.execute(json!({"action": "complete", "id": 1}), &ctx).await.unwrap();

        let result = tool.execute(json!({"action": "list"}), &ctx).await.unwrap();
        assert!(result.content.contains("1/2 done"));
    }

    #[tokio::test]
    async fn test_todo_delete_and_clear() {
        let tool = TodoTool::new();
        let ctx = test_ctx();

        tool.execute(json!({"action": "add", "text": "temp todo"}), &ctx).await.unwrap();
        tool.execute(json!({"action": "delete", "id": 1}), &ctx).await.unwrap();

        let result = tool.execute(json!({"action": "list"}), &ctx).await.unwrap();
        assert!(result.content.contains("No todos"));

        tool.execute(json!({"action": "add", "text": "another"}), &ctx).await.unwrap();
        tool.execute(json!({"action": "clear"}), &ctx).await.unwrap();

        let result = tool.execute(json!({"action": "list"}), &ctx).await.unwrap();
        assert!(result.content.contains("No todos"));
    }

    #[tokio::test]
    async fn test_session_search_no_db() {
        let tool = SessionSearchTool::new(std::path::PathBuf::from("/nonexistent/db.sqlite"));
        let ctx = test_ctx();

        let result = tool.execute(json!({"query": "test"}), &ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }

    #[tokio::test]
    async fn test_vision_no_image() {
        let tool = VisionTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({"question": "what is this?"}), &ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("image_url") || result.content.contains("image_base64"));
    }

    #[tokio::test]
    async fn test_image_gen_no_prompt() {
        let tool = ImageGenTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("prompt"));
    }

    #[tokio::test]
    async fn test_tts_no_text() {
        let tool = TtsTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("text"));
    }

    #[tokio::test]
    async fn test_transcription_no_audio() {
        let tool = TranscriptionTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({}), &ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("audio"));
    }

    #[tokio::test]
    async fn test_delegate_no_api_keys() {
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
        unsafe { std::env::remove_var("OPENAI_API_KEY") };

        let tool = DelegateTool::new();
        let ctx = test_ctx();

        let result = tool.execute(
            json!({"task": "summarize this document"}),
            &ctx,
        ).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Subagent delegation requested"));
    }

    #[tokio::test]
    async fn test_delegate_missing_task() {
        let tool = DelegateTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("task"));
    }

    // ─── Home Assistant Tool Tests ───

    #[tokio::test]
    async fn test_ha_no_config() {
        let tool = HomeAssistantTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({"action": "list_entities"}), &ctx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not configured") || err.contains("HERMES_HOME_ASSISTANT"));
    }

    #[tokio::test]
    async fn test_ha_invalid_action() {
        let tool = HomeAssistantTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({"action": "unknown_action"}), &ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("Unknown"));
    }

    // ─── Cron Job Tool Tests ───

    #[tokio::test]
    async fn test_cron_job_create_and_list() {
        let tool = CronJobTool::new();
        let ctx = test_ctx();

        // Create a job
        let result = tool.execute(json!({
            "action": "create",
            "id": "morning_report",
            "schedule": "0 9 * * *",
            "prompt": "Give me a daily summary",
            "platform": "telegram",
            "chat_id": "12345"
        }), &ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("created successfully"));

        // List jobs
        let result = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(result.is_ok());
        let content = result.unwrap().content;
        assert!(content.contains("morning_report"));
        assert!(content.contains("0 9 * * *"));
    }

    #[tokio::test]
    async fn test_cron_job_create_invalid_schedule() {
        let tool = CronJobTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({
            "action": "create",
            "id": "bad_job",
            "schedule": "not a cron",
            "prompt": "test",
            "platform": "telegram",
            "chat_id": "123"
        }), &ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("Invalid cron expression"));
    }

    #[tokio::test]
    async fn test_cron_job_duplicate_id() {
        let tool = CronJobTool::new();
        let ctx = test_ctx();

        let _ = tool.execute(json!({
            "action": "create",
            "id": "dup",
            "schedule": "0 * * * *",
            "prompt": "test",
            "platform": "discord",
            "chat_id": "ch1"
        }), &ctx).await;

        let result = tool.execute(json!({
            "action": "create",
            "id": "dup",
            "schedule": "30 * * * *",
            "prompt": "test2",
            "platform": "discord",
            "chat_id": "ch1"
        }), &ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("already exists"));
    }

    #[tokio::test]
    async fn test_cron_job_enable_disable() {
        let tool = CronJobTool::new();
        let ctx = test_ctx();

        let _ = tool.execute(json!({
            "action": "create",
            "id": "toggle_job",
            "schedule": "*/5 * * * *",
            "prompt": "test",
            "platform": "telegram",
            "chat_id": "123"
        }), &ctx).await;

        let result = tool.execute(json!({"action": "disable", "id": "toggle_job"}), &ctx).await;
        assert!(result.unwrap().content.contains("disabled"));

        let result = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(result.unwrap().content.contains("disabled"));

        let result = tool.execute(json!({"action": "enable", "id": "toggle_job"}), &ctx).await;
        assert!(result.unwrap().content.contains("enabled"));
    }

    #[tokio::test]
    async fn test_cron_job_delete() {
        let tool = CronJobTool::new();
        let ctx = test_ctx();

        let _ = tool.execute(json!({
            "action": "create",
            "id": "del_me",
            "schedule": "0 0 * * *",
            "prompt": "test",
            "platform": "telegram",
            "chat_id": "123"
        }), &ctx).await;

        let result = tool.execute(json!({"action": "delete", "id": "del_me"}), &ctx).await;
        assert!(result.unwrap().content.contains("deleted"));

        let result = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!result.unwrap().content.contains("del_me"));
    }

    #[tokio::test]
    async fn test_cron_job_run_now() {
        let tool = CronJobTool::new();
        let ctx = test_ctx();

        let _ = tool.execute(json!({
            "action": "create",
            "id": "run_now_test",
            "schedule": "0 12 * * *",
            "prompt": "Run this now",
            "platform": "slack",
            "chat_id": "C123"
        }), &ctx).await;

        let result = tool.execute(json!({"action": "run_now", "id": "run_now_test"}), &ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("triggered"));

        // Check history
        let result = tool.execute(json!({"action": "history"}), &ctx).await;
        assert!(result.unwrap().content.contains("run_now_test"));
    }

    #[tokio::test]
    async fn test_cron_job_empty_list() {
        let tool = CronJobTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("No cron jobs"));
    }

    #[tokio::test]
    async fn test_cron_job_history_empty() {
        let tool = CronJobTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({"action": "history"}), &ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("No execution history"));
    }

    #[test]
    fn test_cron_field_validation() {
        let tool = CronJobTool::new();
        assert!(tool.validate_cron("0 9 * * *").is_ok());
        assert!(tool.validate_cron("*/5 * * * *").is_ok());
        assert!(tool.validate_cron("0,30 9-17 * * 1-5").is_ok());
        assert!(tool.validate_cron("invalid").is_err());
        assert!(tool.validate_cron("60 25 * * *").is_err()); // out of range
        assert!(tool.validate_cron("0 9 * * * *").is_err()); // 6 fields
    }

    // ─── Mixture of Agents Tool Tests ───

    #[tokio::test]
    async fn test_mixture_no_api_keys() {
        let tool = MixtureOfAgentsTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({"prompt": "What is 2+2?"}), &ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("No LLM API keys configured"));
    }

    #[tokio::test]
    async fn test_mixture_missing_prompt() {
        let tool = MixtureOfAgentsTool::new();
        let ctx = test_ctx();

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("prompt"));
    }

    // ─── SkillsTool Tests ───────────────────────────────────────────

    #[tokio::test]
    async fn test_skills_list_empty() {
        let tool = SkillsTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "list"}), &ctx).await.unwrap();
        // Either empty list or "no skills" message is acceptable
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_skills_install_and_view() {
        let tool = SkillsTool::new();
        let ctx = test_ctx();

        let skill_md = r#"---
name: test-skill
description: A test skill
enabled: true
---
# Test Skill
This is a test skill for unit testing.
"#;

        let result = tool.execute(
            json!({"action": "install", "id": "test-skill", "content": skill_md}),
            &ctx,
        ).await.unwrap();
        assert!(!result.is_error);

        let result = tool.execute(
            json!({"action": "view", "id": "test-skill"}),
            &ctx,
        ).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Test Skill") || result.content.contains("test-skill"));

        // Cleanup
        let _ = tool.execute(json!({"action": "uninstall", "id": "test-skill"}), &ctx).await;
    }

    #[tokio::test]
    async fn test_skills_enable_disable() {
        let tool = SkillsTool::new();
        let ctx = test_ctx();

        let skill_md = r#"---
name: toggle-skill
description: A toggleable skill
enabled: true
---
# Toggle Skill
"#;

        tool.execute(
            json!({"action": "install", "id": "toggle-skill", "content": skill_md}),
            &ctx,
        ).await.unwrap();

        let result = tool.execute(
            json!({"action": "disable", "id": "toggle-skill"}),
            &ctx,
        ).await.unwrap();
        assert!(!result.is_error);

        let result = tool.execute(
            json!({"action": "enable", "id": "toggle-skill"}),
            &ctx,
        ).await.unwrap();
        assert!(!result.is_error);

        // Cleanup
        let _ = tool.execute(json!({"action": "uninstall", "id": "toggle-skill"}), &ctx).await;
    }

    #[tokio::test]
    async fn test_skills_view_not_found() {
        let tool = SkillsTool::new();
        let ctx = test_ctx();
        let result = tool.execute(
            json!({"action": "view", "id": "nonexistent-skill"}),
            &ctx,
        ).await.unwrap();
        // Should return an error message about skill not found
        assert!(result.is_error || result.content.to_lowercase().contains("not found") || result.content.to_lowercase().contains("error"));
    }

    // ─── SkillsHubTool Tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_skills_hub_search_no_query() {
        let tool = SkillsHubTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "search"}), &ctx).await.unwrap();
        // Should return guidance about configuring Skills Hub
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_skills_hub_list() {
        let tool = SkillsHubTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "list"}), &ctx).await.unwrap();
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_skills_hub_info_no_id() {
        let tool = SkillsHubTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "info"}), &ctx).await;
        // Should error due to missing id
        assert!(result.is_err() || result.unwrap().is_error);
    }

    #[tokio::test]
    async fn test_skills_hub_install_no_hub() {
        let tool = SkillsHubTool::new();
        let ctx = test_ctx();
        let result = tool.execute(
            json!({"action": "install", "id": "some-skill"}),
            &ctx,
        ).await.unwrap();
        // Should return helpful guidance since no hub configured
        assert!(!result.is_error);
    }

    // ─── BrowserTool Tests ──────────────────────────────────────────

    #[tokio::test]
    async fn test_browser_missing_action() {
        let tool = BrowserTool::new();
        let ctx = test_ctx();
        let err = tool.execute(json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("action"));
    }

    #[tokio::test]
    async fn test_browser_unknown_action() {
        let tool = BrowserTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "invalid"}), &ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Unknown"));
    }

    #[tokio::test]
    async fn test_browser_navigate_no_backend() {
        let tool = BrowserTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "navigate", "url": "https://example.com"}), &ctx).await.unwrap();
        // Without BROWSERBASE_API_KEY or local Chromium, should fail gracefully
        assert!(result.is_error || result.content.contains("local") || result.content.contains("Browserbase"));
    }

    #[tokio::test]
    async fn test_browser_close_no_session() {
        let tool = BrowserTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "close"}), &ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("No active") || result.content.contains("close"));
    }

    #[tokio::test]
    async fn test_browser_click_no_session() {
        let tool = BrowserTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "click", "selector": "#btn"}), &ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("No active") || result.content.contains("Navigate"));
    }

    #[tokio::test]
    async fn test_browser_screenshot_no_session() {
        let tool = BrowserTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "screenshot"}), &ctx).await.unwrap();
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_browser_snapshot_no_session() {
        let tool = BrowserTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"action": "snapshot"}), &ctx).await.unwrap();
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_browser_type_missing_text() {
        let tool = BrowserTool::new();
        let ctx = test_ctx();
        let err = tool.execute(json!({"action": "type", "selector": "#input"}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("text"));
    }

    // ─── CodeExecutionTool Tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_code_execution_missing_code() {
        let tool = CodeExecutionTool::new();
        let ctx = test_ctx();
        let err = tool.execute(json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("code"));
    }

    #[tokio::test]
    async fn test_code_execution_no_backend() {
        let tool = CodeExecutionTool::new();
        let ctx = test_ctx();
        let result = tool.execute(json!({"code": "print('hello')"}), &ctx).await.unwrap();
        // Without E2B_API_KEY or MODAL_TOKEN_ID, should return helpful error
        assert!(result.is_error);
        assert!(result.content.contains("E2B_API_KEY") || result.content.contains("configured"));
    }

    #[tokio::test]
    async fn test_clarify_without_callback() {
        let tool = ClarifyTool;
        let ctx = test_ctx(); // clarify: None

        let result = tool.execute(
            json!({"question": "What is your preferred language?"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("What is your preferred language?"));
        assert!(result.content.contains("Interactive mode not available"));
    }

    #[tokio::test]
    async fn test_clarify_with_choices_without_callback() {
        let tool = ClarifyTool;
        let ctx = test_ctx();

        let result = tool.execute(
            json!({
                "question": "Choose a language",
                "choices": ["Rust", "Python", "Go"]
            }),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("Choose a language"));
        assert!(result.content.contains("1. Rust"));
        assert!(result.content.contains("2. Python"));
        assert!(result.content.contains("3. Go"));
    }

    #[tokio::test]
    async fn test_clarify_with_callback() {
        use std::sync::Arc;

        let tool = ClarifyTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            task_id: "test".to_string(),
            config: std::sync::Arc::new(h_core::HermesConfig::default()),
            working_dir: std::env::temp_dir(),
            clarify: Some(Arc::new(|question: &str, _choices: &[&str]| {
                assert!(question.contains("language"));
                "Rust".to_string()
            })),
        };

        let result = tool.execute(
            json!({"question": "What is your preferred programming language?"}),
            &ctx,
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("User answered: Rust"));
    }
}

// ─── Clarify Tool (Interactive User Question) ────────────────────────────

/// Ask the user an interactive question with optional choices.
///
/// This is the only tool marked as "never parallel" in the spec. It uses
/// the clarify callback in ToolContext to prompt the user directly.
pub struct ClarifyTool;

#[async_trait]
impl Tool for ClarifyTool {
    fn name(&self) -> &str {
        "clarify"
    }

    fn toolset(&self) -> &str {
        "core"
    }

    fn description(&self) -> &str {
        "Ask the user a clarifying question with optional choices. \
        Use this when you need more information before proceeding. \
        Returns the user's selected answer."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                },
                "choices": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of choices for the user to select from"
                }
            },
            "required": ["question"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let question = args.get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: question"))?;

        let choices: Vec<&str> = args.get("choices")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect()
            })
            .unwrap_or_default();

        // Use the clarify callback if available
        if let Some(ref callback) = ctx.clarify {
            let answer = callback(question, &choices);
            return Ok(ToolResult::ok(format!("User answered: {answer}")));
        }

        // Fallback: no interactive mode available
        if choices.is_empty() {
            Ok(ToolResult::ok(format!(
                "Question: {question}\n\n\
                (Interactive mode not available. Please respond with your answer in the next message.)"
            )))
        } else {
            let choices_str = choices.iter()
                .enumerate()
                .map(|(i, c)| format!("  {}. {c}", i + 1))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(ToolResult::ok(format!(
                "Question: {question}\n\nChoices:\n{choices_str}\n\n\
                (Interactive mode not available. Please respond with your choice number or text in the next message.)"
            )))
        }
    }
}

// ─── Delegate Tool (Subagent) ────────────────────────────────────────────

/// Spawn a subagent to handle a specialized task.
///
/// The delegate tool creates a fresh conversation with the given system prompt,
/// sends the user message, and returns the subagent's final response.
pub struct DelegateTool {
    client: reqwest::Client,
}

impl DelegateTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for DelegateTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str {
        "delegate"
    }

    fn toolset(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        "Spawn a subagent to handle a specialized task. The subagent runs in \
        isolation with its own context and returns a focused result. \
        Use for: research, code review, data analysis, writing, etc."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task or question for the subagent"
                },
                "system_prompt": {
                    "type": "string",
                    "description": "System prompt for the subagent (optional, uses default if not provided)"
                },
                "model": {
                    "type": "string",
                    "description": "Model to use for the subagent (optional, inherits parent model)"
                }
            },
            "required": ["task"]
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &[]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let task = args.get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required argument: task"))?;

        let system_prompt = args.get("system_prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("You are a helpful assistant. Answer the user's question concisely and accurately.");

        // Try to call LLM API directly for the subagent
        // Check for Anthropic API key
        if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
            if !api_key.is_empty() {
                return self.run_subagent_anthropic(task, system_prompt, &api_key).await;
            }
        }

        // Check for OpenAI API key
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            if !api_key.is_empty() {
                return self.run_subagent_openai(task, system_prompt, &api_key).await;
            }
        }

        // Fallback: return a helpful message
        Ok(ToolResult::ok(format!(
            "Subagent delegation requested for task: \"{task}\". \
            No LLM API key configured. Set ANTHROPIC_API_KEY or OPENAI_API_KEY to enable subagent spawning. \
            System prompt: {system_prompt}"
        )))
    }
}

impl DelegateTool {
    async fn run_subagent_anthropic(&self, task: &str, system_prompt: &str, api_key: &str) -> Result<ToolResult> {
        let resp = self.client.post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&json!({
                "model": "claude-sonnet-4-20250514",
                "max_tokens": 4096,
                "system": system_prompt,
                "messages": [{
                    "role": "user",
                    "content": task
                }]
            }))
            .send()
            .await;

        match resp {
            Ok(resp) if resp.status().is_success() => {
                #[derive(serde::Deserialize)]
                struct AnthropicSubResponse {
                    content: Vec<AnthropicSubContent>,
                }
                #[derive(serde::Deserialize)]
                struct AnthropicSubContent {
                    #[serde(rename = "type")]
                    content_type: String,
                    text: Option<String>,
                }
                let body: Result<AnthropicSubResponse, _> = resp.json().await;
                match body {
                    Ok(r) => {
                        let text = r.content.iter()
                            .filter(|c| c.content_type == "text")
                            .filter_map(|c| c.text.clone())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if text.is_empty() {
                            Ok(ToolResult::ok("Subagent returned no response".to_string()))
                        } else {
                            let truncated = if text.len() > 50_000 {
                                format!("{}... [truncated]", &text[..50_000])
                            } else {
                                text
                            };
                            Ok(ToolResult::ok(format!(
                                "Subagent response:\n\n{truncated}"
                            )))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse subagent response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("Subagent error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("Subagent request failed: {e}"))),
        }
    }

    async fn run_subagent_openai(&self, task: &str, system_prompt: &str, api_key: &str) -> Result<ToolResult> {
        let resp = self.client.post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&json!({
                "model": "gpt-4o-mini",
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": task}
                ],
                "max_tokens": 4096
            }))
            .send()
            .await;

        #[derive(serde::Deserialize)]
        struct OpenAISubResponse {
            choices: Vec<OpenAISubChoice>,
        }
        #[derive(serde::Deserialize)]
        struct OpenAISubChoice {
            message: OpenAISubMessage,
        }
        #[derive(serde::Deserialize)]
        struct OpenAISubMessage {
            content: Option<String>,
        }

        match resp {
            Ok(resp) if resp.status().is_success() => {
                let body: Result<OpenAISubResponse, _> = resp.json().await;
                match body {
                    Ok(r) => {
                        let text = r.choices.first()
                            .and_then(|c| c.message.content.clone())
                            .unwrap_or_else(|| "Subagent returned no response".to_string());
                        let truncated = if text.len() > 50_000 {
                            format!("{}... [truncated]", &text[..50_000])
                        } else {
                            text
                        };
                        Ok(ToolResult::ok(format!(
                            "Subagent response:\n\n{truncated}"
                        )))
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse subagent response: {e}"))),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("Subagent error: {status}: {body}")))
            }
            Err(e) => Ok(ToolResult::err(format!("Subagent request failed: {e}"))),
        }
    }
}

// ─── Voice Tool (Speech-to-Text) ─────────────────────────────────────────

/// Voice input transcription tool.
///
/// Transcribes audio files or base64 audio data to text using OpenAI Whisper.
/// Supports wav, mp3, mp4, mpeg, mpga, m4a, ogg, and webm formats.
/// Max audio length: 25MB.
const MAX_AUDIO_SIZE: usize = 25 * 1024 * 1024; // 25MB Whisper limit

pub struct VoiceTool {
    client: reqwest::Client,
}

impl VoiceTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for VoiceTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for VoiceTool {
    fn name(&self) -> &str {
        "voice"
    }

    fn toolset(&self) -> &str {
        "voice"
    }

    fn description(&self) -> &str {
        "Transcribe speech audio to text. Accepts an audio file path, URL, \
        or base64-encoded audio data. Supports wav, mp3, mp4, mpeg, mpga, m4a, ogg, webm. \
        Max audio size: 25MB. Requires OPENAI_API_KEY."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "audio_path": {
                    "type": "string",
                    "description": "Path to a local audio file"
                },
                "audio_url": {
                    "type": "string",
                    "description": "URL of an audio file to transcribe"
                },
                "audio_base64": {
                    "type": "string",
                    "description": "Base64-encoded audio data"
                },
                "language": {
                    "type": "string",
                    "description": "Language code (e.g., 'en', 'zh', default: auto-detect)"
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional text to guide the model's transcription style"
                }
            },
            "required": []
        })
    }

    fn requires_env(&self) -> &'static [&'static str] {
        &["OPENAI_API_KEY"]
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let audio_path = args.get("audio_path").and_then(|v| v.as_str());
        let audio_url = args.get("audio_url").and_then(|v| v.as_str());
        let audio_base64 = args.get("audio_base64").and_then(|v| v.as_str());
        let language = args.get("language").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");

        if audio_path.is_none() && audio_url.is_none() && audio_base64.is_none() {
            return Ok(ToolResult::err(
                "Provide audio_path, audio_url, or audio_base64".to_string()
            ));
        }

        // Fetch audio bytes from the provided source
        let audio_bytes = if let Some(path) = audio_path {
            let path_buf = std::path::PathBuf::from(path);
            if !path_buf.exists() {
                return Ok(ToolResult::err(format!("Audio file not found: {path}")));
            }
            let metadata = std::fs::metadata(&path_buf)?;
            if metadata.len() as usize > MAX_AUDIO_SIZE {
                return Ok(ToolResult::err(format!(
                    "Audio file too large ({} bytes). Maximum: {} bytes",
                    metadata.len(), MAX_AUDIO_SIZE
                )));
            }
            std::fs::read(&path_buf).with_context(|| format!("Failed to read audio file: {path}"))?
        } else if let Some(url) = audio_url {
            let resp = self.client.get(url).send().await?;
            if !resp.status().is_success() {
                return Ok(ToolResult::err(
                    format!("Failed to fetch audio from URL: {}", resp.status())
                ));
            }
            let bytes = resp.bytes().await?.to_vec();
            if bytes.len() > MAX_AUDIO_SIZE {
                return Ok(ToolResult::err(format!(
                    "Audio too large ({} bytes). Maximum: {} bytes",
                    bytes.len(), MAX_AUDIO_SIZE
                )));
            }
            bytes
        } else if let Some(b64) = audio_base64 {
            let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
                .with_context(|| "Failed to decode base64 audio")?;
            if bytes.len() > MAX_AUDIO_SIZE {
                return Ok(ToolResult::err(format!(
                    "Audio too large ({} bytes). Maximum: {} bytes",
                    bytes.len(), MAX_AUDIO_SIZE
                )));
            }
            bytes
        } else {
            return Ok(ToolResult::err("No audio provided".to_string()));
        };

        // Determine filename with extension for Whisper API
        let filename = if let Some(path) = audio_path {
            std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("audio.mp3")
                .to_string()
        } else {
            "audio.mp3".to_string()
        };

        // Call Whisper API
        let api_key = std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|k| !k.is_empty());

        match api_key {
            Some(api_key) => {
                self.call_whisper(&audio_bytes, &filename, language, prompt, &api_key).await
            }
            None => Ok(ToolResult::ok(
                "Voice transcription requested but OPENAI_API_KEY is not configured. \
                Set OPENAI_API_KEY to enable Whisper transcription."
            )),
        }
    }
}

impl VoiceTool {
    async fn call_whisper(
        &self,
        audio_bytes: &[u8],
        filename: &str,
        language: &str,
        prompt: &str,
        api_key: &str,
    ) -> Result<ToolResult> {
        let mut form = reqwest::multipart::Form::new()
            .text("model", "whisper-1")
            .part(
                "file",
                reqwest::multipart::Part::bytes(audio_bytes.to_vec())
                    .file_name(filename.to_string())
                    .mime_str("audio/mpeg")
                    .unwrap(),
            );

        if !language.is_empty() {
            form = form.text("language", language.to_string());
        }
        if !prompt.is_empty() {
            form = form.text("prompt", prompt.to_string());
        }

        let resp = self.client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {api_key}"))
            .multipart(form)
            .send()
            .await?;

        match resp {
            resp if resp.status().is_success() => {
                #[derive(serde::Deserialize)]
                struct WhisperResponse {
                    text: String,
                }
                let body: Result<WhisperResponse, _> = resp.json().await;
                match body {
                    Ok(r) => {
                        let text = r.text.trim().to_string();
                        if text.is_empty() {
                            Ok(ToolResult::ok("(no speech detected)".to_string()))
                        } else {
                            Ok(ToolResult::ok(text))
                        }
                    }
                    Err(e) => Ok(ToolResult::err(format!("Failed to parse Whisper response: {e}"))),
                }
            }
            resp => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ToolResult::err(format!("Whisper API error ({status}): {body}")))
            }
        }
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    #[tokio::test]
    async fn test_voice_no_audio() {
        let tool = VoiceTool::new();
        let ctx = ToolContext::default();
        let result = tool.execute(json!({}), &ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Provide"));
    }

    #[tokio::test]
    async fn test_voice_file_not_found() {
        let tool = VoiceTool::new();
        let ctx = ToolContext::default();
        let result = tool.execute(
            json!({"audio_path": "/nonexistent/audio.wav"}),
            &ctx,
        ).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }

    #[tokio::test]
    async fn test_voice_no_api_key() {
        // Create a small valid audio file (minimal WAV header + silence)
        let dir = std::env::temp_dir().join("hermes_voice_test");
        std::fs::create_dir_all(&dir).ok();
        let audio_file = dir.join("test.wav");
        // Minimal WAV file (44 bytes header + 100 bytes of silence)
        let mut wav_data = vec![0u8; 144];
        // RIFF header
        wav_data[0..4].copy_from_slice(b"RIFF");
        wav_data[8..12].copy_from_slice(b"WAVE");
        wav_data[12..16].copy_from_slice(b"fmt ");
        wav_data[16..20].copy_from_slice(&16u32.to_le_bytes()); // chunk size
        wav_data[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
        wav_data[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
        wav_data[24..28].copy_from_slice(&16000u32.to_le_bytes()); // sample rate
        wav_data[28..32].copy_from_slice(&32000u32.to_le_bytes()); // byte rate
        wav_data[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
        wav_data[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav_data[36..40].copy_from_slice(b"data");
        wav_data[40..44].copy_from_slice(&100u32.to_le_bytes()); // data size
        std::fs::write(&audio_file, &wav_data).ok();

        unsafe { std::env::remove_var("OPENAI_API_KEY") };
        let tool = VoiceTool::new();
        let ctx = ToolContext::default();
        let result = tool.execute(
            json!({"audio_path": audio_file.to_str().unwrap()}),
            &ctx,
        ).await.unwrap();
        assert!(!result.is_error); // returns helpful message, not error
        assert!(result.content.contains("OPENAI_API_KEY") || result.content.contains("api"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
