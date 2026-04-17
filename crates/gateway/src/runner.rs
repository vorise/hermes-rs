use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;

use crate::base::{IncomingMessage, PlatformAdapter, StreamConsumer};
use crate::config::GatewayConfig;
use crate::cron::{CronScheduler, SchedulerEvent};
use crate::dispatch::dispatch_message;
use crate::session::GatewaySessionStore;
use h_core::{HermesConfig, SessionDB};
use h_mcp::McpState;
use h_tools::Tool;

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
    hermes_config: HermesConfig,
    session_store: GatewaySessionStore,
    platforms: Vec<Arc<dyn PlatformAdapter>>,
    status: std::sync::Mutex<GatewayStatus>,
    shutdown: Arc<Notify>,
    all_tools: Vec<Arc<dyn Tool>>,
    mcp_state: Option<Arc<McpState>>,
    cron_scheduler: CronScheduler,
    cron_event_handle: Option<JoinHandle<()>>,
}

impl GatewayRunner {
    /// Create a new GatewayRunner.
    pub fn new(config: GatewayConfig, hermes_config: HermesConfig, db: Arc<SessionDB>, all_tools: Vec<Arc<dyn Tool>>) -> Self {
        Self {
            config,
            hermes_config,
            session_store: GatewaySessionStore::new(db),
            platforms: Vec::new(),
            status: std::sync::Mutex::new(GatewayStatus::Idle),
            shutdown: Arc::new(Notify::new()),
            all_tools,
            mcp_state: None,
            cron_scheduler: CronScheduler::new(),
            cron_event_handle: None,
        }
    }

    /// Create a new GatewayRunner with MCP state support.
    pub fn new_with_mcp(config: GatewayConfig, hermes_config: HermesConfig, db: Arc<SessionDB>, all_tools: Vec<Arc<dyn Tool>>, mcp_state: Arc<McpState>) -> Self {
        Self {
            config,
            hermes_config,
            session_store: GatewaySessionStore::new(db),
            platforms: Vec::new(),
            status: std::sync::Mutex::new(GatewayStatus::Idle),
            shutdown: Arc::new(Notify::new()),
            all_tools,
            mcp_state: Some(mcp_state),
            cron_scheduler: CronScheduler::new(),
            cron_event_handle: None,
        }
    }

    /// Get the MCP state, if configured.
    pub fn mcp_state(&self) -> Option<&Arc<McpState>> {
        self.mcp_state.as_ref()
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

    /// Start all enabled platform adapters and the cron scheduler.
    pub async fn start(&mut self) -> Result<()> {
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

        // Start cron scheduler with event handling
        self.start_cron_scheduler();

        *self.status.lock().unwrap() = GatewayStatus::Running {
            platforms_connected: connected,
        };

        tracing::info!(connected, "Gateway runner started");
        Ok(())
    }

    /// Start the cron scheduler and spawn an event handler task.
    fn start_cron_scheduler(&mut self) {
        if self.cron_scheduler.is_empty() {
            return;
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<SchedulerEvent>();
        self.cron_scheduler.start(tx);

        // Spawn event handler that processes job-due events
        let platforms = self.platforms.clone();
        let handle = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    SchedulerEvent::JobDue { job, scheduled_at: _ } => {
                        tracing::info!(job_id = %job.id, "Cron job due");
                        // Find the target platform and execute the job
                        for platform in &platforms {
                            if platform.name() == job.delivery.platform {
                                match crate::cron::execute_job(&job, platform).await {
                                    Ok(result) => {
                                        tracing::info!(job_id = %job.id, "Cron job completed: {}", result.chars().take(100).collect::<String>());
                                    }
                                    Err(e) => {
                                        tracing::warn!(job_id = %job.id, error = %e, "Cron job execution failed");
                                        let _ = platform.send_message(
                                            &job.delivery.chat_id,
                                            &format!("Cron job '{}' failed: {e}", job.id),
                                        ).await;
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });
        self.cron_event_handle = Some(handle);
    }

    /// Stop all platform adapters and the cron scheduler gracefully.
    pub async fn stop(&mut self) -> Result<()> {
        *self.status.lock().unwrap() = GatewayStatus::Stopping;
        self.shutdown.notify_waiters();

        // Stop cron scheduler
        self.cron_scheduler.stop();
        if let Some(handle) = self.cron_event_handle.take() {
            handle.abort();
        }

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
    /// It creates/gets a session and dispatches the message to the query loop.
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

        // Check if session is already active (prevents concurrent processing)
        if self.session_store.is_active(adapter.name(), &msg.user_id) {
            adapter.send_message(&msg.chat_id, "_Already processing your request, please wait._")
                .await?;
            return Ok(());
        }

        // Mark session as active
        self.session_store.set_active(adapter.name(), &msg.user_id, true);

        // Show typing indicator
        let _ = adapter.is_typing(&msg.chat_id).await;

        // Dispatch to query loop
        let result = dispatch_message(
            &msg,
            adapter,
            &self.session_store,
            &self.config,
            &self.hermes_config,
            &self.all_tools,
            self.mcp_state.as_ref(),
        ).await;

        // Mark session as inactive after processing
        self.session_store.set_active(adapter.name(), &msg.user_id, false);

        result
    }

    /// Get the list of connected platform names.
    pub fn connected_platforms(&self) -> Vec<&str> {
        self.platforms.iter().map(|p| p.name()).collect()
    }

    /// Get the session store reference.
    pub fn db(&self) -> &Arc<SessionDB> {
        self.session_store.db()
    }

    // ── Cron scheduler methods ───────────────────────────────────────

    /// Get a reference to the cron scheduler.
    pub fn cron_scheduler(&self) -> &CronScheduler {
        &self.cron_scheduler
    }

    /// Get a mutable reference to the cron scheduler.
    pub fn cron_scheduler_mut(&mut self) -> &mut CronScheduler {
        &mut self.cron_scheduler
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
        let hermes_config = HermesConfig::default();
        let mut runner = GatewayRunner::new(config, hermes_config, db, vec![]);

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
        let hermes_config = HermesConfig::default();
        let _runner = GatewayRunner::new(config, hermes_config, db, vec![]);
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
