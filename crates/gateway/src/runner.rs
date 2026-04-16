use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;

use crate::base::{IncomingMessage, PlatformAdapter, StreamConsumer};
use crate::config::GatewayConfig;
use crate::session::GatewaySessionStore;
use h_core::SessionDB;

/// Status of the gateway runner.
#[derive(Debug, Clone)]
pub enum GatewayStatus {
    Idle,
    Running { platforms_connected: usize },
    Stopping,
    Error(String),
}

impl std::fmt::Display for GatewayStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GatewayStatus::Idle => write!(f, "idle"),
            GatewayStatus::Running { platforms_connected } => {
                write!(f, "running ({platforms_connected} platform(s) connected)")
            }
            GatewayStatus::Stopping => write!(f, "stopping"),
            GatewayStatus::Error(msg) => write!(f, "error: {msg}"),
        }
    }
}

/// Manages the lifecycle of all connected platform adapters.
///
/// The GatewayRunner:
/// 1. Loads configuration and creates platform adapters
/// 2. Connects to all enabled platforms
/// 3. Routes incoming messages to the appropriate session
/// 4. Manages session state per platform-user pair
pub struct GatewayRunner {
    #[allow(dead_code)]
    config: GatewayConfig,
    session_store: GatewaySessionStore,
    platforms: Vec<Arc<dyn PlatformAdapter>>,
    status: std::sync::Mutex<GatewayStatus>,
    shutdown: Arc<Notify>,
}

impl GatewayRunner {
    /// Create a new GatewayRunner.
    pub fn new(config: GatewayConfig, db: Arc<SessionDB>) -> Self {
        Self {
            config,
            session_store: GatewaySessionStore::new(db),
            platforms: Vec::new(),
            status: std::sync::Mutex::new(GatewayStatus::Idle),
            shutdown: Arc::new(Notify::new()),
        }
    }

    /// Get the session store.
    pub fn session_store(&self) -> &GatewaySessionStore {
        &self.session_store
    }

    /// Get the current gateway status.
    pub fn status(&self) -> GatewayStatus {
        self.status.lock().unwrap().clone()
    }

    /// Register a platform adapter.
    pub fn register(&mut self, adapter: Arc<dyn PlatformAdapter>) {
        self.platforms.push(adapter);
    }

    /// Start all enabled platform adapters.
    pub async fn start(&self) -> Result<()> {
        tracing::info!("Gateway runner starting");
        let mut connected = 0;

        for platform in &self.platforms {
            match platform.connect().await {
                Ok(()) => {
                    connected += 1;
                    tracing::info!(platform = platform.name(), "Platform connected");
                }
                Err(e) => {
                    tracing::warn!(platform = platform.name(), error = %e, "Platform connection failed");
                }
            }
        }

        *self.status.lock().unwrap() = GatewayStatus::Running {
            platforms_connected: connected,
        };

        tracing::info!(connected, "Gateway runner started");
        Ok(())
    }

    /// Stop all platform adapters gracefully.
    pub async fn stop(&self) -> Result<()> {
        *self.status.lock().unwrap() = GatewayStatus::Stopping;
        self.shutdown.notify_waiters();

        for platform in &self.platforms {
            if let Err(e) = platform.disconnect().await {
                tracing::warn!(platform = platform.name(), error = %e, "Platform disconnect failed");
            }
        }

        *self.status.lock().unwrap() = GatewayStatus::Idle;
        tracing::info!("Gateway runner stopped");
        Ok(())
    }

    /// Wait for shutdown signal.
    pub async fn wait_for_shutdown(&self) {
        self.shutdown.notified().await;
    }

    /// Trigger shutdown.
    pub fn request_shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    /// Handle an incoming message from a platform adapter.
    ///
    /// This is called by platform adapters when a new message arrives.
    /// It creates/gets a session and dispatches the message for processing.
    pub async fn handle_message(
        &self,
        adapter: &dyn PlatformAdapter,
        msg: IncomingMessage,
    ) -> Result<()> {
        tracing::info!(
            platform = adapter.name(),
            user = %msg.user_id,
            chat = %msg.chat_id,
            "Received message"
        );

        // Get or create session for this platform-user pair
        let session = self
            .session_store
            .get_or_create(adapter.name(), &msg.user_id, &msg.chat_id)
            .await?;

        // Check if session is already active (prevents concurrent processing)
        if self.session_store.is_active(adapter.name(), &msg.user_id) {
            adapter.send_message(&msg.chat_id, "_Already processing your request, please wait._")
                .await?;
            return Ok(());
        }

        // Mark session as active
        self.session_store.set_active(adapter.name(), &msg.user_id, true);

        // Create stream consumer for this chat
        let _consumer = adapter.create_consumer(&msg.chat_id, msg.message_id);

        // Show typing indicator
        let _ = adapter.is_typing(&msg.chat_id).await;

        // In a real implementation, this would dispatch to the query loop.
        // For now, we acknowledge the message.
        tracing::info!(
            session_id = %session.session_id,
            "Dispatching message to query loop"
        );

        // Mark session as inactive after processing
        self.session_store.set_active(adapter.name(), &msg.user_id, false);

        Ok(())
    }

    /// Get the list of connected platform names.
    pub fn connected_platforms(&self) -> Vec<&str> {
        self.platforms.iter().map(|p| p.name()).collect()
    }

    /// Get the session store reference.
    pub fn db(&self) -> &Arc<SessionDB> {
        self.session_store.db()
    }
}

/// Buffering stream consumer that accumulates text and sends a final message.
///
/// This is the default consumer used when a platform doesn't support
/// incremental message editing for streaming.
pub struct BufferingConsumer {
    chat_id: String,
    adapter: Arc<dyn PlatformAdapter>,
    buffer: Arc<parking_lot::Mutex<String>>,
    message_id: Arc<parking_lot::Mutex<Option<String>>>,
}

impl BufferingConsumer {
    /// Create a new buffering consumer.
    pub fn new(chat_id: String, adapter: Arc<dyn PlatformAdapter>) -> Self {
        Self {
            chat_id,
            adapter,
            buffer: Arc::new(parking_lot::Mutex::new(String::new())),
            message_id: Arc::new(parking_lot::Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl StreamConsumer for BufferingConsumer {
    async fn on_text_delta(&self, delta: &str) -> Result<()> {
        let mut buf = self.buffer.lock();
        buf.push_str(delta);
        Ok(())
    }

    async fn on_tool_start(&self, tool_name: &str, args_preview: &str) -> Result<()> {
        let mut buf = self.buffer.lock();
        buf.push_str(&format!("\n[_Running tool: {tool_name}_]\n"));
        if !args_preview.is_empty() {
            buf.push_str(&format!("  args: {args_preview}\n"));
        }
        Ok(())
    }

    async fn on_tool_complete(&self, tool_name: &str, result_preview: &str) -> Result<()> {
        let mut buf = self.buffer.lock();
        buf.push_str(&format!("\n[_Tool {tool_name} completed_]\n"));
        if !result_preview.is_empty() {
            buf.push_str(&format!("  result: {result_preview}\n"));
        }
        Ok(())
    }

    async fn on_tool_error(&self, tool_name: &str, error: &str) -> Result<()> {
        let mut buf = self.buffer.lock();
        buf.push_str(&format!("\n[_Tool {tool_name} failed: {error}_]\n"));
        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        let content = self.buffer.lock().clone();
        if content.is_empty() {
            return Ok(());
        }

        let supports_edit = self.adapter.supports_edit_streaming();
        let msg_id = self.message_id.lock().take();

        if supports_edit && msg_id.is_some() {
            self.adapter
                .edit_message(&self.chat_id, &msg_id.unwrap(), &content)
                .await?;
        } else {
            self.adapter.send_message(&self.chat_id, &content).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // Mock platform for testing
    struct MockPlatform {
        name: String,
        sent_messages: Arc<parking_lot::Mutex<Vec<(String, String)>>>,
    }

    #[async_trait::async_trait]
    impl PlatformAdapter for MockPlatform {
        fn name(&self) -> &str { &self.name }
        async fn connect(&self) -> Result<()> { Ok(()) }
        async fn disconnect(&self) -> Result<()> { Ok(()) }
        async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
            self.sent_messages.lock().push((chat_id.to_string(), text.to_string()));
            Ok("mock_msg_id".to_string())
        }
        async fn send_file(&self, _chat_id: &str, _path: &Path) -> Result<()> { Ok(()) }
        async fn send_animation(&self, _chat_id: &str, _path: &Path) -> Result<()> { Ok(()) }
        async fn send_voice(&self, _chat_id: &str, _path: &Path) -> Result<()> { Ok(()) }
        async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> { Ok(()) }
        async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> { Ok(()) }
        async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> { Ok(()) }
        async fn is_typing(&self, _chat_id: &str) -> Result<()> { Ok(()) }
        fn create_consumer(&self, chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
            Box::new(BufferingConsumer::new(chat_id.to_string(), Arc::new(MockPlatform {
                name: self.name.clone(),
                sent_messages: self.sent_messages.clone(),
            })))
        }
    }

    #[tokio::test]
    async fn test_gateway_runner_start_stop() {
        let db = Arc::new(SessionDB::new_in_memory().unwrap());
        let config = GatewayConfig::default();
        let mut runner = GatewayRunner::new(config, db);

        let sent = Arc::new(parking_lot::Mutex::new(Vec::new()));
        runner.register(Arc::new(MockPlatform {
            name: "mock".to_string(),
            sent_messages: sent.clone(),
        }));

        runner.start().await.unwrap();
        if let GatewayStatus::Running { platforms_connected } = runner.status() {
            assert_eq!(platforms_connected, 1);
        } else {
            panic!("Expected Running status");
        }

        runner.stop().await.unwrap();
        assert!(matches!(runner.status(), GatewayStatus::Idle));
    }

    #[tokio::test]
    async fn test_buffering_consumer() {
        let db = Arc::new(SessionDB::new_in_memory().unwrap());
        let config = GatewayConfig::default();
        let _runner = GatewayRunner::new(config, db);
        let sent = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let platform = Arc::new(MockPlatform {
            name: "mock".to_string(),
            sent_messages: sent.clone(),
        });

        let consumer = BufferingConsumer::new("chat1".to_string(), platform.clone());
        consumer.on_text_delta("Hello ").await.unwrap();
        consumer.on_text_delta("World").await.unwrap();
        consumer.on_tool_start("test_tool", "{\"arg\": 1}").await.unwrap();
        consumer.on_tool_complete("test_tool", "success").await.unwrap();
        consumer.flush().await.unwrap();

        let messages = sent.lock();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].1.contains("Hello World"));
        assert!(messages[0].1.contains("test_tool"));
    }
}
