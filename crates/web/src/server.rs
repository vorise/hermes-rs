//! Web Server
//!
//! Axum-based HTTP server for Hermes Web UI.

use axum::Router;
use axum::routing::{get, post};
use tower_http::cors::{CorsLayer, Any};
use tower_http::services::ServeDir;
use std::sync::Arc;
use h_query::QueryConfig;

use super::routes::{
    chat_handler, sessions_handler, session_handler, models_handler,
    model_handler, stream_handler, health_handler,
};
use super::sse::SseBroadcaster;

/// Web server configuration.
#[derive(Debug, Clone)]
pub struct WebConfig {
    /// Server port.
    pub port: u16,

    /// Host address.
    pub host: String,

    /// Static files directory.
    pub static_dir: Option<String>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            host: "127.0.0.1".to_string(),
            static_dir: None,
        }
    }
}

/// Web server state.
pub struct WebState {
    /// Query configuration.
    pub query_config: Arc<QueryConfig>,

    /// SSE broadcaster for real-time updates.
    pub broadcaster: SseBroadcaster,

    /// Active sessions.
    pub sessions: Arc<std::sync::Mutex<Vec<SessionInfo>>>,
}

/// Session information for API.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    /// Session ID.
    pub id: String,

    /// Session title/name.
    pub title: String,

    /// Creation timestamp.
    pub created_at: u64,

    /// Message count.
    pub message_count: usize,

    /// Current model.
    pub model: String,
}

/// Web server.
pub struct WebServer {
    /// Configuration.
    config: WebConfig,

    /// Router.
    router: Router,

    /// State.
    state: Arc<WebState>,
}

impl WebServer {
    /// Create new web server.
    pub fn new(query_config: Arc<QueryConfig>) -> Self {
        Self::with_config(query_config, WebConfig::default())
    }

    /// Create with custom config.
    pub fn with_config(query_config: Arc<QueryConfig>, config: WebConfig) -> Self {
        let broadcaster = SseBroadcaster::new(100);
        let sessions = Arc::new(std::sync::Mutex::new(Vec::new()));

        let state = Arc::new(WebState {
            query_config,
            broadcaster,
            sessions,
        });

        let router = Self::build_router(state.clone(), &config);

        Self {
            config,
            router,
            state,
        }
    }

    /// Create a minimal server for testing without QueryConfig.
    pub fn new_test() -> Self {
        let config = WebConfig::default();
        let broadcaster = SseBroadcaster::new(100);
        let sessions = Arc::new(std::sync::Mutex::new(Vec::new()));

        // Create a minimal test API config
        let test_api_config = h_api::client::ResolvedApiConfig {
            provider: h_core::ProviderId::new("anthropic"),
            model: h_core::ModelId::new("claude-3"),
            base_url: "https://api.anthropic.com".to_string(),
            api_key: "test-key".to_string(),
            mode: h_api::client::ApiMode::ChatCompletions,
            timeout_seconds: 30,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
        };

        let router = Router::new()
            .route("/api/health", get(health_handler))
            .route("/api/models", get(models_handler));

        Self {
            config,
            router,
            state: Arc::new(WebState {
                query_config: Arc::new(QueryConfig::new(test_api_config)),
                broadcaster,
                sessions,
            }),
        }
    }

    /// Build the router.
    fn build_router(state: Arc<WebState>, config: &WebConfig) -> Router {
        let api_routes = Router::new()
            .route("/chat", post(chat_handler))
            .route("/sessions", get(sessions_handler))
            .route("/sessions/{id}", get(session_handler))
            .route("/models", get(models_handler))
            .route("/model", post(model_handler))
            .route("/stream", get(stream_handler))
            .route("/health", get(health_handler))
            .with_state(state.clone());

        let mut router = Router::new()
            .nest("/api", api_routes)
            .layer(CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any));

        // Add static file serving if configured
        if let Some(static_dir) = &config.static_dir {
            router = router
                .fallback_service(ServeDir::new(static_dir));
        }

        router
    }

    /// Get server address.
    pub fn address(&self) -> String {
        format!("{}:{}", self.config.host, self.config.port)
    }

    /// Get state reference.
    pub fn state(&self) -> Arc<WebState> {
        self.state.clone()
    }

    /// Run the server.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let addr = self.address();
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        tracing::info!("Web server listening on {}", addr);

        axum::serve(listener, self.router.clone()).await?;

        Ok(())
    }

    /// Add a session.
    pub fn add_session(&self, session: SessionInfo) {
        let mut sessions = self.state.sessions.lock().unwrap();
        sessions.push(session);
    }

    /// Remove a session.
    pub fn remove_session(&self, id: &str) {
        let mut sessions = self.state.sessions.lock().unwrap();
        sessions.retain(|s| s.id != id);
    }

    /// Update a session.
    pub fn update_session(&self, id: &str, update: SessionInfo) {
        let mut sessions = self.state.sessions.lock().unwrap();
        if let Some(session) = sessions.iter_mut().find(|s| s.id == id) {
            *session = update;
        }
    }

    /// Get all sessions.
    pub fn get_sessions(&self) -> Vec<SessionInfo> {
        self.state.sessions.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_config_default() {
        let config = WebConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "127.0.0.1");
    }

    #[test]
    fn test_web_server_new_test() {
        let server = WebServer::new_test();
        assert_eq!(server.address(), "127.0.0.1:8080");
    }

    #[test]
    fn test_session_info() {
        let session = SessionInfo {
            id: "session-1".to_string(),
            title: "Test Session".to_string(),
            created_at: 1000,
            message_count: 5,
            model: "claude-3".to_string(),
        };
        assert_eq!(session.id, "session-1");
    }

    #[test]
    fn test_add_session() {
        let server = WebServer::new_test();
        let session = SessionInfo {
            id: "session-1".to_string(),
            title: "Test".to_string(),
            created_at: 1000,
            message_count: 0,
            model: "claude-3".to_string(),
        };
        server.add_session(session.clone());
        let sessions = server.get_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, session.id);
    }

    #[test]
    fn test_remove_session() {
        let server = WebServer::new_test();
        server.add_session(SessionInfo {
            id: "session-1".to_string(),
            title: "Test".to_string(),
            created_at: 1000,
            message_count: 0,
            model: "claude-3".to_string(),
        });
        server.remove_session("session-1");
        assert_eq!(server.get_sessions().len(), 0);
    }
}