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
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: ([0, 0, 0, 0], 8090).into(),
            serve_static: true,
        }
    }
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub session_db: Arc<SessionDB>,
    pub provider_registry: Arc<ProviderRegistry>,
}

/// The web UI server.
pub struct WebServer {
    config: WebServerConfig,
    session_db: Arc<SessionDB>,
    provider_registry: Arc<ProviderRegistry>,
}

impl WebServer {
    /// Create a new web server.
    pub fn new(config: WebServerConfig, session_db: Arc<SessionDB>) -> Self {
        Self {
            config,
            session_db,
            provider_registry: Arc::new(ProviderRegistry::new()),
        }
    }

    /// Create with a custom provider registry.
    pub fn with_providers(mut self, registry: Arc<ProviderRegistry>) -> Self {
        self.provider_registry = registry;
        self
    }

    /// Build the axum router.
    fn build_router(&self) -> Router<AppState> {
        let api = Router::new()
            .route("/chat", post(handle_chat))
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
    State(_state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> (StatusCode, Json<ChatResponse>) {
    let session_id = req.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let response = format!("Echo: {}", req.message);
    (StatusCode::OK, Json(ChatResponse { response, session_id }))
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
    async fn test_chat_endpoint() {
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

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let resp: ChatResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(resp.response.contains("Echo"));
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
        let config = WebServerConfig::default();
        assert_eq!(config.listen_addr.port(), 8090);
        assert!(config.serve_static);
    }

    #[test]
    fn test_web_server_config_custom() {
        let config = WebServerConfig {
            listen_addr: ([127, 0, 0, 1], 3000).into(),
            serve_static: false,
        };
        assert_eq!(config.listen_addr.port(), 3000);
        assert!(!config.serve_static);
    }
}
