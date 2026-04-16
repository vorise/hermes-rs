//! API Routes
//!
//! HTTP API endpoints for Hermes Web UI.

use axum::{
    extract::{Path, State, Json},
    http::{StatusCode, Response},
    body::Body,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::server::{WebState, SessionInfo};

/// Chat request.
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    /// Session ID (optional, will create new if not provided).
    pub session_id: Option<String>,

    /// User message.
    pub message: String,

    /// Model override (optional).
    pub model: Option<String>,
}

/// Chat response.
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    /// Session ID.
    pub session_id: String,

    /// Response message ID.
    pub message_id: String,

    /// Response text.
    pub response: String,

    /// Whether response is streaming.
    pub streaming: bool,
}

/// Model switch request.
#[derive(Debug, Deserialize)]
pub struct ModelRequest {
    /// Session ID.
    pub session_id: String,

    /// Model to switch to.
    pub model: String,
}

/// Model info.
#[derive(Debug, Serialize)]
pub struct ModelInfo {
    /// Model ID.
    pub id: String,

    /// Model name.
    pub name: String,

    /// Provider.
    pub provider: String,

    /// Is default.
    pub is_default: bool,
}

/// Session detail.
#[derive(Debug, Serialize)]
pub struct SessionDetail {
    /// Session info.
    #[serde(flatten)]
    pub info: SessionInfo,

    /// Messages in session.
    pub messages: Vec<MessageInfo>,
}

/// Message info.
#[derive(Debug, Serialize)]
pub struct MessageInfo {
    /// Message ID.
    pub id: String,

    /// Role (user/assistant/system).
    pub role: String,

    /// Content.
    pub content: String,

    /// Timestamp.
    pub timestamp: u64,
}

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Status.
    pub status: String,

    /// Version.
    pub version: String,
}

/// Chat endpoint handler.
pub async fn chat_handler(
    State(state): State<Arc<WebState>>,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    // In a real implementation, this would use the query_config to process the message
    // For now, we return a placeholder response

    let session_id = request.session_id.unwrap_or_else(|| {
        uuid::Uuid::new_v4().to_string()
    });

    let response = ChatResponse {
        session_id: session_id.clone(),
        message_id: uuid::Uuid::new_v4().to_string(),
        response: "Response placeholder".to_string(),
        streaming: true,
    };

    // Add/update session
    {
        let mut sessions = state.sessions.lock().unwrap();
        if !sessions.iter().any(|s| s.id == session_id) {
            sessions.push(SessionInfo {
                id: session_id.clone(),
                title: request.message.chars().take(50).collect(),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                message_count: 1,
                model: request.model.unwrap_or_else(|| "default".to_string()),
            });
        }
    }

    // Broadcast SSE event
    state.broadcaster.broadcast("chat", &serde_json::to_string(&response).unwrap_or_default());

    Ok(Json(response))
}

/// Sessions list handler.
pub async fn sessions_handler(
    State(state): State<Arc<WebState>>,
) -> Json<Vec<SessionInfo>> {
    let sessions = state.sessions.lock().unwrap().clone();
    Json(sessions)
}

/// Single session handler.
pub async fn session_handler(
    State(state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetail>, StatusCode> {
    let sessions = state.sessions.lock().unwrap();

    let session = sessions.iter()
        .find(|s| s.id == id)
        .cloned();

    match session {
        Some(info) => {
            let detail = SessionDetail {
                info,
                messages: vec![],  // Placeholder
            };
            Ok(Json(detail))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Models list handler.
pub async fn models_handler() -> Json<Vec<ModelInfo>> {
    // Placeholder list of models
    let models = vec![
        ModelInfo {
            id: "claude-3-opus".to_string(),
            name: "Claude 3 Opus".to_string(),
            provider: "anthropic".to_string(),
            is_default: true,
        },
        ModelInfo {
            id: "claude-3-sonnet".to_string(),
            name: "Claude 3 Sonnet".to_string(),
            provider: "anthropic".to_string(),
            is_default: false,
        },
        ModelInfo {
            id: "gpt-4".to_string(),
            name: "GPT-4".to_string(),
            provider: "openai".to_string(),
            is_default: false,
        },
    ];

    Json(models)
}

/// Model switch handler.
pub async fn model_handler(
    State(state): State<Arc<WebState>>,
    Json(request): Json<ModelRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut sessions = state.sessions.lock().unwrap();

    let session = sessions.iter_mut()
        .find(|s| s.id == request.session_id);

    match session {
        Some(s) => {
            s.model = request.model;
            Ok(StatusCode::OK)
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Health check handler.
pub async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// SSE stream handler - returns SSE stream placeholder.
pub async fn stream_handler(
    State(_state): State<Arc<WebState>>,
) -> Response<Body> {
    // In a real implementation, this would return an SSE stream
    // using axum's SSE support

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .body(Body::from("data: connected\n\n"))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_request_deserialize() {
        let json = r#"{"session_id":"s1","message":"hello","model":"claude"}"#;
        let req: ChatRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.session_id, Some("s1".to_string()));
        assert_eq!(req.message, "hello");
    }

    #[test]
    fn test_chat_response_serialize() {
        let resp = ChatResponse {
            session_id: "s1".to_string(),
            message_id: "m1".to_string(),
            response: "Hi there".to_string(),
            streaming: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("session_id"));
    }

    #[test]
    fn test_model_info() {
        let model = ModelInfo {
            id: "claude-3".to_string(),
            name: "Claude 3".to_string(),
            provider: "anthropic".to_string(),
            is_default: true,
        };
        assert!(model.is_default);
    }

    #[test]
    fn test_health_response() {
        let health = HealthResponse {
            status: "ok".to_string(),
            version: "0.1.0".to_string(),
        };
        assert_eq!(health.status, "ok");
    }
}