use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{State, Path},
    http::StatusCode,
    response::sse::{Event, Sse},
    routing::{get, post},
};
use futures_util::{stream::Stream, StreamExt};
use h_api::client::ApiClient;
use h_api::provider::ApiMode;
use h_core::{HermesConfig, Message, ModelId, ModelRef, ProviderId};
use h_query::{QueryConfig, QueryLoop, ToolRegistry};
use h_tools::Tool;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use h_core::SessionDB;
use h_api::ProviderRegistry;

/// Configuration for the web server.
#[derive(Debug, Clone)]
pub struct WebServerConfig {
    /// HTTP listen address.
    pub listen_addr: SocketAddr,
    /// Serve static frontend files.
    pub serve_static: bool,
    /// Hermes core config (model, tools, etc).
    pub hermes_config: HermesConfig,
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: ([0, 0, 0, 0], 8090).into(),
            serve_static: true,
            hermes_config: HermesConfig::default(),
        }
    }
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub session_db: Arc<SessionDB>,
    pub provider_registry: Arc<ProviderRegistry>,
    pub hermes_config: HermesConfig,
    pub all_tools: Vec<Arc<dyn Tool>>,
}

/// The web UI server.
pub struct WebServer {
    config: WebServerConfig,
    session_db: Arc<SessionDB>,
    provider_registry: Arc<ProviderRegistry>,
    all_tools: Vec<Arc<dyn Tool>>,
}

impl WebServer {
    /// Create a new web server.
    pub fn new(config: WebServerConfig, session_db: Arc<SessionDB>) -> Self {
        Self {
            config,
            session_db,
            provider_registry: Arc::new(ProviderRegistry::new()),
            all_tools: Vec::new(),
        }
    }

    /// Create with a custom provider registry.
    pub fn with_providers(mut self, registry: Arc<ProviderRegistry>) -> Self {
        self.provider_registry = registry;
        self
    }

    /// Set the available tools.
    pub fn with_tools(mut self, tools: Vec<Arc<dyn Tool>>) -> Self {
        self.all_tools = tools;
        self
    }

    /// Build the axum router.
    fn build_router(&self) -> Router<AppState> {
        let api = Router::new()
            .route("/chat", post(handle_chat))
            .route("/chat/stream", post(handle_chat_stream))
            .route("/sessions", get(handle_list_sessions))
            .route("/sessions/{id}", get(handle_get_session))
            .route("/stream", get(handle_sse_stream))
            .route("/models", get(handle_list_models))
            .route("/model", post(handle_switch_model));

        let router = Router::new().nest("/api", api);

        if self.config.serve_static {
            let static_r: Router<AppState> =
                super::static_files::static_router().with_state(());
            router.merge(static_r)
        } else {
            router
        }
    }

    /// Start the server. Returns immediately after binding.
    pub async fn start(self) -> Result<()> {
        let addr = self.config.listen_addr;
        let state = AppState {
            session_db: self.session_db.clone(),
            provider_registry: self.provider_registry.clone(),
            hermes_config: self.config.hermes_config.clone(),
            all_tools: self.all_tools.clone(),
        };
        let router = self.build_router().with_state(state);
        tracing::info!("Web server starting on {addr}");
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, router).await?;
        Ok(())
    }

    /// Get the listen address.
    pub fn listen_addr(&self) -> SocketAddr {
        self.config.listen_addr
    }
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatResponse {
    response: String,
    session_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionListResponse {
    sessions: Vec<SessionSummaryDto>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionDetailResponse {
    session: SessionDto,
    messages: Vec<MessageDto>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionSummaryDto {
    id: String,
    title: Option<String>,
    model: Option<String>,
    message_count: i64,
    started_at: chrono::DateTime<chrono::Utc>,
    estimated_cost_usd: Option<f64>,
    source: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionDto {
    id: String,
    source: String,
    user_id: Option<String>,
    model: Option<String>,
    started_at: f64,
    ended_at: Option<f64>,
    message_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct MessageDto {
    id: i64,
    role: String,
    content: Option<String>,
    timestamp: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelListResponse {
    current_model: String,
    providers: Vec<ProviderDto>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProviderDto {
    id: String,
    display_name: String,
    default_model: String,
}

#[derive(Debug, Deserialize)]
struct SwitchModelRequest {
    provider: String,
    model: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SwitchModelResponse {
    provider: String,
    model: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> (StatusCode, Json<ChatResponse>) {
    let session_id = req.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Resolve model
    let provider = state.hermes_config.provider.as_deref().unwrap_or("anthropic");
    let model = state.hermes_config.model.as_deref().unwrap_or("claude-sonnet-4-6");
    let model_ref = ModelRef::new(ProviderId::new(provider), ModelId::new(model));

    // Build API client
    let api_config = match build_api_config(&state.hermes_config, &model_ref) {
        Ok(cfg) => cfg,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ChatResponse {
                response: format!("Failed to configure API: {e}"),
                session_id,
            }));
        }
    };
    let api_client = match ApiClient::new(api_config) {
        Ok(client) => client,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ChatResponse {
                response: format!("Failed to create API client: {e}"),
                session_id,
            }));
        }
    };

    // Build tool registry and tool definitions
    let tool_registry = ToolRegistry::new();
    let enabled_tools = filter_tools(&state.all_tools, &state.hermes_config);
    for tool in &enabled_tools {
        tool_registry.register(tool.clone());
    }
    let tool_defs: Vec<_> = enabled_tools.iter().map(|t| t.to_definition()).collect();

    // Load conversation history from session DB
    let messages: Vec<Message> = match state.session_db.get_messages(&session_id) {
        Ok(stored) => stored
            .into_iter()
            .filter_map(|m| m.to_message().ok())
            .collect(),
        Err(_) => vec![],
    };

    // Save and append the new user message
    let user_msg = Message::user(req.message.clone());
    if let Err(e) = state.session_db.add_message(&session_id, &user_msg) {
        tracing::warn!("Failed to save user message to session DB: {e}");
    }

    // Build system prompt
    let system_prompt = build_system_prompt(&state.hermes_config);

    // Build query config
    let mut query_config = QueryConfig::new(model_ref)
        .with_system_prompt(system_prompt)
        .with_max_iterations(90);

    for m in &messages {
        query_config = query_config.with_message(m.clone());
    }
    // Add the new user message
    query_config = query_config.with_message(user_msg);
    query_config = query_config.with_tools(tool_defs);

    // Run query loop
    let query_loop = QueryLoop::new(&query_config);
    let interrupt = Arc::new(tokio::sync::Notify::new());

    let result = match query_loop.run(&api_client, &tool_registry, interrupt).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ChatResponse {
                response: format!("Query failed: {e}"),
                session_id,
            }));
        }
    };

    // Save assistant response to session DB
    for m in &result.messages {
        if let Err(e) = state.session_db.add_message(&session_id, m) {
            tracing::warn!("Failed to save message to session DB: {e}");
        }
    }

    // Update session stats
    if let Err(e) = state.session_db.update_session_stats(&session_id, &result.cost) {
        tracing::warn!("Failed to update session stats: {e}");
    }

    (StatusCode::OK, Json(ChatResponse {
        response: result.final_text,
        session_id,
    }))
}

/// Streaming chat endpoint — streams LLM response via SSE.
async fn handle_chat_stream(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let session_id = req.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let (tx, rx) = mpsc::channel::<String>(32);

    // Clone state for the spawned task
    let state_clone = state.clone();
    let message = req.message.clone();

    tokio::spawn(async move {
        // Resolve model
        let provider = state_clone.hermes_config.provider.as_deref().unwrap_or("anthropic");
        let model = state_clone.hermes_config.model.as_deref().unwrap_or("claude-sonnet-4-6");
        let model_ref = ModelRef::new(ProviderId::new(provider), ModelId::new(model));

        // Build API client
        let api_config = match build_api_config(&state_clone.hermes_config, &model_ref) {
            Ok(cfg) => cfg,
            Err(e) => {
                let _ = tx.send(format!("error: {e}")).await;
                let _ = tx.send("[DONE]".to_string()).await;
                return;
            }
        };
        let api_client = match ApiClient::new(api_config) {
            Ok(client) => client,
            Err(e) => {
                let _ = tx.send(format!("error: {e}")).await;
                let _ = tx.send("[DONE]".to_string()).await;
                return;
            }
        };

        // Build tool registry and tool definitions
        let tool_registry = ToolRegistry::new();
        let enabled_tools = filter_tools(&state_clone.all_tools, &state_clone.hermes_config);
        for tool in &enabled_tools {
            tool_registry.register(tool.clone());
        }
        let tool_defs: Vec<_> = enabled_tools.iter().map(|t| t.to_definition()).collect();

        // Load conversation history
        let messages: Vec<Message> = match state_clone.session_db.get_messages(&session_id) {
            Ok(stored) => stored.into_iter().filter_map(|m| m.to_message().ok()).collect(),
            Err(_) => vec![],
        };

        // Save user message
        let user_msg = Message::user(message.clone());
        let _ = state_clone.session_db.add_message(&session_id, &user_msg);

        // Build system prompt
        let system_prompt = build_system_prompt(&state_clone.hermes_config);

        // Prepare messages
        let mut api_messages = vec![Message::system(&system_prompt)];
        api_messages.extend(messages);
        api_messages.push(user_msg);

        // Start streaming
        let mut full_text = String::new();

        match api_client.chat_stream(&api_messages, &tool_defs).await {
            Ok(mut stream) => {
                while let Some(delta_result) = stream.next().await {
                    match delta_result {
                        Ok(delta) => {
                            // Send text deltas
                            if let Some(ref text) = delta.content {
                                if !text.is_empty() {
                                    full_text.push_str(text);
                                    let _ = tx.send(format!("text: {text}")).await;
                                }
                            }

                            // Send reasoning deltas (if any)
                            if let Some(ref reasoning) = delta.reasoning {
                                if !reasoning.is_empty() {
                                    let _ = tx.send(format!("reasoning: {reasoning}")).await;
                                }
                            }

                            // Track tool calls
                            for tc in &delta.tool_calls {
                                if let Some(ref name) = tc.name {
                                    let _ = tx.send(format!("tool_start: {name}")).await;
                                }
                            }

                            // Check for finish
                            if let Some(ref reason) = delta.finish_reason {
                                if reason == "stop" || reason == "tool_calls" {
                                    let _ = tx.send(format!("finish: {reason}")).await;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(format!("error: {e}")).await;
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(format!("error: {e}")).await;
            }
        }

        // Save assistant response
        if !full_text.is_empty() {
            let assistant_msg = Message::assistant(full_text.clone());
            let _ = state_clone.session_db.add_message(&session_id, &assistant_msg);
        }

        let _ = tx.send(format!("session: {session_id}")).await;
        let _ = tx.send("[DONE]".to_string()).await;
    });

    let stream = ReceiverStream::new(rx).map(|chunk| {
        Ok(Event::default().data(chunk))
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(tokio::time::Duration::from_secs(15))
            .text("keep-alive-text"),
    )
}

async fn handle_list_sessions(
    State(state): State<AppState>,
) -> Result<Json<SessionListResponse>, (StatusCode, String)> {
    let summaries = state.session_db
        .get_session_summaries(100)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let sessions = summaries.into_iter().map(summary_to_dto).collect();
    Ok(Json(SessionListResponse { sessions }))
}

async fn handle_get_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionDetailResponse>, (StatusCode, String)> {
    let session = state.session_db
        .get_session(&session_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let messages = state.session_db
        .get_messages(&session_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let session_dto = SessionDto {
        id: session.id.clone(),
        source: session.source,
        user_id: session.user_id,
        model: session.model,
        started_at: session.started_at,
        ended_at: session.ended_at,
        message_count: session.message_count,
    };

    let message_dtos = messages.into_iter().map(|m| MessageDto {
        id: m.id,
        role: m.role,
        content: m.content,
        timestamp: m.timestamp,
    }).collect();

    Ok(Json(SessionDetailResponse {
        session: session_dto,
        messages: message_dtos,
    }))
}

async fn handle_sse_stream(
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let (tx, rx) = mpsc::channel::<String>(32);

    tokio::spawn(async move {
        for chunk in ["Hello", ", ", "this ", "is ", "a ", "streaming ", "response."] {
            if tx.send(chunk.to_string()).await.is_err() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
        tx.send("[DONE]".to_string()).await.ok();
    });

    let stream = ReceiverStream::new(rx).map(|chunk| {
        Ok(Event::default().data(chunk))
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(tokio::time::Duration::from_secs(15))
            .text("keep-alive-text"),
    )
}

async fn handle_list_models(
    State(state): State<AppState>,
) -> Json<ModelListResponse> {
    let providers = state.provider_registry.list();
    let provider_dtos = providers.iter().map(|p| ProviderDto {
        id: format!("{:?}", p.id),
        display_name: p.display_name.to_string(),
        default_model: p.default_model.to_string(),
    }).collect();

    let current_model = providers.first()
        .map(|p| p.default_model.to_string())
        .unwrap_or_default();

    Json(ModelListResponse {
        current_model,
        providers: provider_dtos,
    })
}

async fn handle_switch_model(
    State(_state): State<AppState>,
    Json(req): Json<SwitchModelRequest>,
) -> Json<SwitchModelResponse> {
    Json(SwitchModelResponse {
        provider: req.provider,
        model: req.model,
    })
}

// ---------------------------------------------------------------------------
// DTO conversions
// ---------------------------------------------------------------------------

fn summary_to_dto(s: h_core::session::SessionSummary) -> SessionSummaryDto {
    SessionSummaryDto {
        id: s.id,
        title: s.title,
        model: s.model,
        message_count: s.message_count,
        started_at: s.started_at,
        estimated_cost_usd: s.estimated_cost_usd,
        source: s.source,
    }
}

// ---------------------------------------------------------------------------
// Helpers (mirrors gateway dispatch logic)
// ---------------------------------------------------------------------------

fn build_api_config(hermes: &HermesConfig, model_ref: &ModelRef) -> Result<h_api::client::ApiConfig> {
    let provider = &model_ref.provider;
    let model = &model_ref.model;

    let env_var = match provider.as_str() {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" | "openrouter" => "OPENAI_API_KEY",
        "nous" => "NOUS_API_KEY",
        _ => "API_KEY",
    };

    let api_key = std::env::var(env_var)
        .map_err(|_| anyhow::anyhow!("API key not found in environment: {env_var}"))?;

    let base_url = hermes.base_url.clone().unwrap_or_else(|| match provider.as_str() {
        "anthropic" => "https://api.anthropic.com".to_string(),
        "openai" => "https://api.openai.com/v1".to_string(),
        "openrouter" => "https://openrouter.ai/api/v1".to_string(),
        "nous" => "https://api.nousresearch.com/v1".to_string(),
        _ => "https://api.openai.com/v1".to_string(),
    });

    let api_mode = match provider.as_str() {
        "anthropic" => ApiMode::AnthropicMessages,
        _ => ApiMode::ChatCompletions,
    };

    Ok(h_api::client::ApiConfig {
        provider: provider.clone(),
        model: model.clone(),
        base_url,
        api_key,
        api_mode,
        max_tokens: Some(4096),
        temperature: Some(0.7),
        reasoning_effort: None,
    })
}

fn filter_tools(
    all_tools: &[Arc<dyn Tool>],
    config: &HermesConfig,
) -> Vec<Arc<dyn Tool>> {
    if let Some(ref disabled) = config.disabled_toolsets {
        all_tools
            .iter()
            .filter(|t| !disabled.contains(&t.toolset().to_string()))
            .cloned()
            .collect()
    } else if let Some(ref enabled) = config.enabled_toolsets {
        all_tools
            .iter()
            .filter(|t| enabled.contains(&t.toolset().to_string()))
            .cloned()
            .collect()
    } else {
        all_tools.to_vec()
    }
}

fn build_system_prompt(config: &HermesConfig) -> String {
    let mut prompt = h_query::PromptBuilder::build_cli();
    if let Some(ref personality) = config.personality {
        prompt = format!("{personality}\n\n{prompt}");
    }
    prompt
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn make_test_state() -> AppState {
        let db = Arc::new(SessionDB::new_in_memory().unwrap());
        AppState {
            session_db: db,
            provider_registry: Arc::new(ProviderRegistry::new()),
            hermes_config: HermesConfig::default(),
            all_tools: vec![],
        }
    }

    fn make_router() -> Router {
        Router::new()
            .route("/api/chat", post(handle_chat))
            .route("/api/sessions", get(handle_list_sessions))
            .route("/api/sessions/{id}", get(handle_get_session))
            .route("/api/models", get(handle_list_models))
            .route("/api/model", post(handle_switch_model))
            .with_state(make_test_state())
    }

    #[tokio::test]
    async fn test_chat_endpoint_no_api_key() {
        let router = make_router();
        let body = serde_json::json!({ "message": "hello world" });

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Without API key, should return 500 with error message
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let resp: ChatResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(resp.response.contains("API key not found"));
    }

    #[tokio::test]
    async fn test_list_sessions_empty() {
        let router = make_router();

        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let resp: SessionListResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(resp.sessions.is_empty());
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let router = make_router();

        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/sessions/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_models() {
        let router = make_router();

        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let resp: ModelListResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(!resp.providers.is_empty());
    }

    #[tokio::test]
    async fn test_switch_model() {
        let router = make_router();
        let body = serde_json::json!({ "provider": "anthropic", "model": "claude-sonnet-4-6" });

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/model")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let resp: SwitchModelResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(resp.provider, "anthropic");
        assert_eq!(resp.model, "claude-sonnet-4-6");
    }

    #[test]
    fn test_web_server_config_default() {
        let hermes_config = HermesConfig::default();
        let config = WebServerConfig {
            listen_addr: ([0, 0, 0, 0], 8090).into(),
            serve_static: true,
            hermes_config,
        };
        assert_eq!(config.listen_addr.port(), 8090);
        assert!(config.serve_static);
    }

    #[test]
    fn test_web_server_config_custom() {
        let hermes_config = HermesConfig::default();
        let config = WebServerConfig {
            listen_addr: ([127, 0, 0, 1], 3000).into(),
            serve_static: false,
            hermes_config,
        };
        assert_eq!(config.listen_addr.port(), 3000);
        assert!(!config.serve_static);
    }
}
