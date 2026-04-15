# hermes-rs — Clean-Room Rust Reimplementation Plan

## 1. Overview

hermes-rs is a **from-scratch, clean-room Rust rewrite** of the Hermes Agent Python
codebase (`hermes-agent`, ~500K+ LOC across ~300 Python files). No Python source
code is referenced or copied — the implementation is driven entirely by the
architecture documented in the `spec/` directory, which was produced by reading
and analyzing the Python codebase separately.

The goal is to reproduce **all user-facing functionality** of Hermes Agent in
async Rust using the Tokio runtime, with the same tool semantics, same config
format (`~/.hermes/config.yaml`), same CLI commands, same platform integrations,
and same memory/skills systems — but with the performance, safety, and
compile-time guarantees of Rust.

**Key principle:** The Python codebase is never imported, copied, or read during
implementation. The spec documents in `spec/` are the sole source of truth for
what needs to be built. The Rust codebase should be idiomatic, not a line-by-line
translation.

**Scope boundaries:**
- **In scope:** Agent core loop, CLI TUI, tool system, session storage, memory,
  skills, MCP client, 6 terminal backends, 18+ messaging platform adapters, cron
  scheduler, ACP server, plugin system, model provider support, web UI.
- **Deferred (v2):** Research pipeline (Atropos RL, batch runner, trajectory
  compression), Mini SWE runner, Honcho dialectic user modeling, voice mode,
  Mixture of Agents tool, RL environments.

## 2. Architecture

### 2.1 Workspace Structure

```
hermes-rs/
├── Cargo.toml                  # Workspace root
└── crates/
    ├── core/       (h-core)       # Shared types, config, sessions, memory, skills
    ├── api/        (h-api)        # LLM API client + SSE streaming (multi-provider)
    ├── tools/      (h-tools)      # All tool implementations (~40 tools)
    ├── query/      (h-query)      # Agentic query loop, context compression
    ├── tui/        (h-tui)        # ratatui terminal UI
    ├── commands/   (h-commands)   # Slash command implementations (~60 commands)
    ├── mcp/        (h-mcp)        # MCP (Model Context Protocol) client
    ├── gateway/    (h-gateway)    # Messaging platform gateway (Telegram, Discord, ...)
    ├── acp/        (h-acp)        # ACP server (VS Code / Zed / JetBrains)
    ├── envs/       (h-envs)       # Terminal backends (Docker, SSH, Modal, ...)
    ├── web/        (h-web)        # Web UI server (FastAPI → Axum)
    ├── plugins/    (h-plugins)    # Plugin discovery and hook system
    └── cli/        (hermes-rs)    # Binary entry point
```

### 2.2 Dependency Flow

```
cli → query → tools → core
         ↓         ↗
        api   →  core
         ↓
       commands → core
         ↓
        tui   → core
         ↓
        mcp   → core
         ↓
       gateway → query, core
         ↓
        envs  → core
         ↓
        acp   → query, core
         ↓
        web   → query, core
         ↓
       plugins → core
```

### 2.3 High-Level Message Flow

```
User Input (CLI / Telegram / Discord / Web / ACP)
    |
    v
+--------------------------------------------------+
|  Entry Point Router                               |
|  cli binary dispatches to:                        |
|    - hermes (interactive TUI)                     |
|    - hermes gateway (messaging daemon)            |
|    - hermes-acp (IDE server)                      |
+--------------------------------------------------+
    |
    v
+--------------------------------------------------+
|  Query Loop (h-query)                             |
|  1. Load session from SQLite                      |
|  2. Build system prompt (identity + tools + mem)  |
|  3. Call LLM API (streaming)                      |
|  4. Handle tool_use → execute → feed back         |
|  5. Repeat until stop condition                   |
|  6. Save session, track cost                      |
+--------------------------------------------------+
    |
    v
+--------------------------------------------------+
|  Tool System (h-tools)                            |
|  ToolRegistry → 40+ tool implementations          |
|  Approval system → dangerous command detection    |
|  ProcessRegistry → background process management  |
+--------------------------------------------------+
    |
    v
+--------------------------------------------------+
|  Model Providers (h-api)                          |
|  ProviderRegistry → 15+ providers                 |
|  OpenRouter (200+ models)                         |
|  Anthropic, OpenAI, Nous Portal, Ollama, ...     |
+--------------------------------------------------+
```

### 2.4 Key Design Decisions

1. **ratatui + crossterm for TUI** — the Python codebase uses prompt_toolkit; in
   Rust we use the ratatui ecosystem which is the standard terminal UI framework.

2. **reqwest + eventsource-client for API** — the Python codebase uses the OpenAI
   SDK; in Rust we implement the OpenAI-compatible protocol directly with reqwest,
   supporting Chat Completions, Codex Responses, and Anthropic Messages APIs.

3. **rusqlite + fts5 for state** — the Python codebase uses sqlite3 with FTS5; we
   use rusqlite with the fts5 extension for identical full-text search.

4. **Tool auto-registration via `inventory` crate** — the Python codebase uses
   AST-based `registry.register()` at import time; in Rust we use the `inventory`
   crate for zero-boilerplate tool registration at compile time.

5. **Config compatibility** — `~/.hermes/config.yaml` and `~/.hermes/.env` formats
   are preserved exactly so users can migrate without reconfiguration.

6. **Shared query loop across all interfaces** — the CLI TUI, messaging gateway,
   web UI, and ACP server all use the same `h-query` query loop. Only the I/O
   layer differs.

## 3. Implementation Phases

### Phase 1: Workspace, Core Types & Configuration

**Goal:** Set up the Cargo workspace, define all core data structures, implement
config loading compatible with the Python codebase.

**Dependencies:** None (foundational phase).

#### Workspace `Cargo.toml`

```toml
[workspace]
resolver = "2"
members = [
    "crates/core",
    "crates/api",
    "crates/tools",
    "crates/query",
    "crates/tui",
    "crates/commands",
    "crates/mcp",
    "crates/gateway",
    "crates/acp",
    "crates/envs",
    "crates/web",
    "crates/plugins",
    "crates/cli",
]

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
reqwest = { version = "0.12", features = ["json", "stream", "rustls-tls"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
ratatui = "0.29"
crossterm = { version = "0.28", features = ["event-stream"] }
clap = { version = "4", features = ["derive", "env"] }
anyhow = "1"
thiserror = "2"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
rusqlite = { version = "0.32", features = ["bundled", "functions"] }
async-openai = "0.27"  # OpenAI-compatible API client
inventory = "0.3"       # Compile-time tool registration
futures = "0.3"
async-trait = "0.1"
url = "2"
regex = "1"
glob = "0.3"
walkdir = "2"
toml = "0.8"
dotenvy = "0.15"
```

#### Core Types (`crates/core/src/lib.rs`)

```rust
/// Provider identifier (e.g., "anthropic", "openrouter", "openai").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(String);

/// Model identifier (e.g., "claude-sonnet-4-6").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelId(String);

/// Combined provider + model reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: ProviderId,
    pub model: ModelId,
}

/// Tool definition in OpenAI-compatible format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub r#type: String,         // "function"
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,  // JSON Schema
}

/// Message in conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Option<Content>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role { System, User, Assistant, Tool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Multi(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,  // JSON string
}

/// Cost tracking for a session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostTracker {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub estimated_cost_usd: f64,
    pub api_call_count: u64,
}
```

#### Configuration (`crates/core/src/config.rs`)

```rust
/// Full user configuration. Mirrors ~/.hermes/config.yaml from the Python codebase.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HermesConfig {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub personality: Option<String>,
    pub enabled_toolsets: Option<Vec<String>>,
    pub disabled_toolsets: Option<Vec<String>>,
    pub enabled_skills: Option<Vec<String>>,
    pub disabled_skills: Option<Vec<String>>,
    pub terminal: Option<TerminalConfig>,
    pub delegation: Option<DelegationConfig>,
    pub memory: Option<MemoryConfig>,
    pub platforms: Option<PlatformsConfig>,
    pub mcp: Option<McpConfig>,
    pub cron: Option<CronConfig>,
    pub web: Option<WebConfig>,
    pub skin: Option<SkinConfig>,
    pub plugins: Option<PluginsConfig>,
    pub profiles: Option<Vec<Profile>>,
}

/// Terminal backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    pub backend: TerminalBackend,  // local, docker, ssh, modal, daytona, singularity
    pub docker: Option<DockerOpts>,
    pub ssh: Option<SshOpts>,
    pub modal: Option<ModalOpts>,
    pub daytona: Option<DaytonaOpts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalBackend {
    Local, Docker, Ssh, Modal, Daytona, Singularity,
}
```

#### Config Loading

```
~/.hermes/config.yaml → serde_yaml::from_str → HermesConfig
~/.hermes/.env        → dotenvy               → std::env
CLI flags             → clap::Parser           → override above
```

#### Checklist

- [ ] Create workspace `Cargo.toml` with 13 member crates
- [ ] Define `core/src/lib.rs`: ProviderId, ModelId, ModelRef, ToolDefinition, Message, Role, Content, ToolCall, CostTracker
- [ ] Define `core/src/config.rs`: HermesConfig + all sub-configs
- [ ] Implement `core/src/config.rs`: `fn load() -> HermesConfig` from YAML + env + CLI
- [ ] Define `core/src/session.rs`: Session metadata struct
- [ ] Define `core/src/toolset.rs`: Toolset enum/registry
- [ ] Implement `core/src/home.rs`: `fn hermes_home() -> PathBuf` (~/.hermes with HERMES_HOME override)
- [ ] Implement `core/src/logging.rs`: tracing subscriber setup
- [ ] Unit tests: config YAML round-trip, env var override, CLI flag override

---

### Phase 2: LLM API Client & Provider Registry

**Goal:** Implement multi-provider LLM API client with streaming support.

**Dependencies:** Phase 1 (core types must exist).

#### Provider Registry (`crates/api/src/registry.rs`)

```rust
pub struct ProviderInfo {
    pub id: ProviderId,
    pub display_name: &'static str,
    pub default_base_url: &'static str,
    pub api_key_env: &'static str,
    pub default_model: &'static str,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_reasoning: bool,
}

pub struct ProviderRegistry {
    providers: HashMap<ProviderId, ProviderInfo>,
}

impl ProviderRegistry {
    pub fn register(&mut self, info: ProviderInfo) { ... }
    pub fn get(&self, id: &ProviderId) -> Option<&ProviderInfo> { ... }
    pub fn list(&self) -> Vec<&ProviderInfo> { ... }
}
```

#### API Client (`crates/api/src/client.rs`)

Supports three API modes matching the Python codebase:

| Mode | Protocol | Used By |
|------|----------|---------|
| `chat_completions` | OpenAI Chat Completions | Most providers |
| `codex_responses` | OpenAI Codex Responses | OpenAI Codex |
| `anthropic_messages` | Anthropic Messages API | Anthropic, /anthropic endpoints |

```rust
pub enum ApiMode {
    ChatCompletions,
    CodexResponses,
    AnthropicMessages,
}

pub struct ApiClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    mode: ApiMode,
    provider: ProviderId,
    model: ModelId,
}

impl ApiClient {
    pub fn new(config: ApiConfig) -> Result<Self> { ... }
    pub async fn chat(&self, messages: &[Message], tools: &[ToolDefinition])
        -> Result<ApiResponse> { ... }
    pub async fn chat_stream(&self, messages: &[Message], tools: &[ToolDefinition])
        -> Result<impl Stream<Item = Result<Delta>>> { ... }
}
```

#### Built-in Providers

```rust
fn register_builtin_providers(registry: &mut ProviderRegistry) {
    // Nous Portal, OpenRouter, Anthropic, OpenAI, Xiaomi MiMo,
    // z.ai/GLM, Kimi/Moonshot, MiniMax, HuggingFace, Ollama,
    // Mistral, and generic OpenAI-compatible endpoint.
}
```

#### Checklist

- [ ] Implement `ProviderRegistry` with 15+ built-in providers
- [ ] Implement `ApiClient` with auto-detection of API mode
- [ ] Implement Chat Completions streaming (SSE)
- [ ] Implement Anthropic Messages API streaming
- [ ] Implement Codex Responses API
- [ ] Implement credential resolution (env → .env → config)
- [ ] Implement token usage parsing from all response formats
- [ ] Implement error classification (rate limit, context length, auth failure)
- [ ] Unit tests: provider registration, API mode detection, streaming parse

---

### Phase 3: Tool Framework & Built-in Tools

**Goal:** Implement the central tool registry and all ~40 built-in tools.

**Dependencies:** Phase 1 (core types).

#### Tool Trait (`crates/tools/src/lib.rs`)

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn toolset(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;  // JSON Schema
    fn check_fn(&self) -> Option<fn() -> bool> { None }
    fn requires_env(&self) -> &'static [&'static str] { &[] }
    fn max_result_size_chars(&self) -> Option<usize> { None }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext)
        -> Result<ToolResult>;
}

pub struct ToolContext {
    pub session_id: String,
    pub task_id: String,
    pub config: Arc<HermesConfig>,
    pub process_registry: Arc<ProcessRegistry>,
    pub working_dir: PathBuf,
    // ... more context
}

pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    pub persisted_path: Option<PathBuf>,  // for oversized results
}
```

#### Compile-Time Registration (`inventory`)

```rust
// In each tool file:
inventory::submit!(ToolEntry {
    name: "terminal",
    toolset: "terminal",
    constructor: || Box::new(TerminalTool),
});

// In registry:
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn discover_all() -> Self {
        let mut registry = Self { tools: HashMap::new() };
        for entry in inventory::iter::<ToolEntry> {
            registry.tools.insert(entry.name.to_string(), (entry.constructor)());
        }
        registry
    }
}
```

#### Tool Categories

| Category | Tools | File |
|----------|-------|------|
| Terminal | `terminal` | `terminal_tool.rs` |
| File I/O | `read_file`, `write_file`, `patch`, `search_files` | `file_tools.rs` |
| Web | `web_search`, `web_extract` | `web_tools.rs` |
| Browser | `browser_navigate`, `browser_click`, `browser_type`, `browser_screenshot`, `browser_snapshot`, `browser_close` | `browser_tool.rs` |
| Code Execution | `execute_code` | `code_execution_tool.rs` |
| Delegation | `delegate` | `delegate_tool.rs` |
| MCP | MCP dynamic tools | `mcp_tool.rs` (in h-mcp crate) |
| Skills | `skill_view`, `skills_list`, `skill_manage` | `skills_tool.rs` |
| Memory | `memory` | `memory_tool.rs` |
| Session Search | `session_search` | `session_search_tool.rs` |
| TTS | `speak` | `tts_tool.rs` |
| Vision | `vision_analyze` | `vision_tools.rs` |
| Home Assistant | `ha_list_entities`, `ha_get_state`, `ha_call_service` | `homeassistant_tool.rs` |
| Todo | `todo_create`, `todo_list`, `todo_complete` | `todo_tool.rs` |
| Image Gen | `generate_image` | `image_generation_tool.rs` |
| Cron Job | `cronjob_create`, `cronjob_list`, `cronjob_delete` | `cronjob_tools.rs` |
| Transcription | `transcribe` | `transcription_tools.rs` |
| Approval | (utility, not a tool) | `approval.rs` |

#### Approval System (`crates/tools/src/approval.rs`)

```rust
/// Heuristic detection of dangerous terminal commands.
pub fn is_destructive_command(cmd: &str) -> bool {
    // Matches: rm, rmdir, mv, sed -i, truncate, dd, shred,
    // git reset/clean/checkout, output redirects (>)
}
```

#### Process Registry (`crates/tools/src/process_registry.rs`)

```rust
pub struct ProcessRegistry {
    processes: HashMap<String, ChildHandle>,
}

impl ProcessRegistry {
    pub fn spawn(&mut self, cmd: &str, background: bool) -> Result<ProcessHandle> { ... }
    pub fn get_output(&mut self, handle: &ProcessHandle) -> Result<String> { ... }
    pub fn kill(&mut self, handle: &ProcessHandle) -> Result<()> { ... }
    pub fn cleanup_all(&mut self) { ... }
}
```

#### Checklist

- [ ] Define `Tool` trait and `ToolContext` struct
- [ ] Implement `ToolRegistry` with `inventory`-based auto-registration
- [ ] Implement terminal tool with multi-backend support (defer env backends to Phase 8)
- [ ] Implement file tools (read_file, write_file, patch, search_files)
- [ ] Implement web tools (web_search via Exa/Parallel, web_extract via Firecrawl)
- [ ] Implement delegate tool (subagent spawning)
- [ ] Implement memory tool
- [ ] Implement session search tool
- [ ] Implement TTS tool (Edge TTS + ElevenLabs)
- [ ] Implement vision tool
- [ ] Implement Home Assistant tool
- [ ] Implement todo tool
- [ ] Implement image generation tool (DALL-E + FAL)
- [ ] Implement cron job tools
- [ ] Implement transcription tool
- [ ] Implement approval system (is_destructive_command)
- [ ] Implement process registry
- [ ] Implement tool result size limiting and persistence
- [ ] Implement parallel tool execution with `_MAX_TOOL_WORKERS = 8`
- [ ] Implement parallel safety classification (never_parallel, parallel_safe, path_scoped)
- [ ] Unit tests: tool registration, approval detection, process lifecycle

---

### Phase 4: Query Loop & Context Compression

**Goal:** Implement the core agentic conversation loop.

**Dependencies:** Phase 1 (core types), Phase 2 (API client), Phase 3 (tools).

#### Query Loop (`crates/query/src/lib.rs`)

```rust
pub struct QueryConfig {
    pub model: ModelRef,
    pub api_mode: ApiMode,
    pub tools: Vec<ToolDefinition>,
    pub system_prompt: String,
    pub messages: Vec<Message>,
    pub max_iterations: u32,       // default 90
    pub max_tokens: Option<u32>,
    pub reasoning_config: Option<ReasoningConfig>,
    pub request_overrides: Option<serde_json::Value>,
}

pub async fn run_query_loop(config: &mut QueryConfig) -> Result<QueryResult> {
    // 1. Load session from SQLite
    // 2. Build system prompt (if not cached)
    // 3. Preflight context compression check
    // 4. Main loop: while iterations remaining:
    //    a. Check interrupt
    //    b. Consume iteration budget
    //    c. Prepare messages (inject memory context, plugin context)
    //    d. Call LLM API (streaming)
    //    e. Handle response:
    //       - text → display/stream
    //       - tool_use → execute tools (parallel if safe)
    //       - feed tool results back
    //    f. Check stop conditions
    // 5. Save session, track cost
}
```

#### Iteration Budget

```rust
pub struct IterationBudget {
    max_total: u32,
    used: AtomicU32,
}

impl IterationBudget {
    pub fn consume(&self) -> bool { ... }   // thread-safe
    pub fn refund(&self) { ... }            // for execute_code turns
    pub fn remaining(&self) -> u32 { ... }
}
```

#### System Prompt Builder (`crates/query/src/prompt_builder.rs`)

Assembles system prompt from components (matching the Python codebase):

1. Agent identity (`DEFAULT_AGENT_IDENTITY`)
2. Personality (from config or SOUL.md)
3. Platform hints
4. Tool usage guidance
5. Memory guidance
6. Session search guidance
7. Skills guidance
8. Context files (AGENTS.md, .cursorrules)
9. Environment hints
10. Skills system prompt

The prompt is cached per session for Anthropic prefix caching.

#### Context Compressor (`crates/query/src/context_compressor.rs`)

```rust
pub struct ContextCompressor {
    threshold_tokens: u64,
    auxiliary_client: ApiClient,  // cheaper model for summarization
}

impl ContextCompressor {
    pub fn should_compress(&self, messages: &[Message], system_prompt: &str) -> bool { ... }
    pub async fn compress(&self, messages: &mut Vec<Message>) -> Result<()> {
        // 1. Identify messages above token threshold
        // 2. Use auxiliary LLM to summarize older turns
        // 3. Preserve recent messages (last N turns)
        // 4. Preserve tool results
        // 5. Replace original messages with compressed versions
    }
}
```

#### Parallel Tool Execution

```rust
fn should_parallelize(tool_calls: &[ToolCall], tool_registry: &ToolRegistry) -> bool {
    // Never parallelize: clarify
    // Parallel safe: read-only tools
    // Path-scoped: file tools targeting different paths
    // Default: sequential
}

async fn execute_tool_batch_parallel(
    tool_calls: &[ToolCall],
    registry: &ToolRegistry,
    ctx: &ToolContext,
) -> Vec<Result<ToolResult>> {
    // Spawn up to _MAX_TOOL_WORKERS (8) tasks
    // Path overlap detection for file tools
}
```

#### Checklist

- [ ] Implement `QueryConfig` struct
- [ ] Implement `run_query_loop()` with main conversation loop
- [ ] Implement `IterationBudget` (thread-safe)
- [ ] Implement system prompt builder with all 10 components
- [ ] Implement prompt caching (Anthropic cache control)
- [ ] Implement context compressor with auxiliary LLM
- [ ] Implement preflight context compression check
- [ ] Implement interrupt system (thread-scoped signal)
- [ ] Implement parallel tool execution with safety classification
- [ ] Implement surrogate character sanitization
- [ ] Implement non-ASCII stripping for ASCII-only encodings
- [ ] Implement error recovery (retry with backoff, failover)
- [ ] Implement dead connection detection and cleanup
- [ ] Implement session state saving to SQLite
- [ ] Implement cost tracking per API call
- [ ] Implement plugin hook integration (pre_llm_call, on_session_start)
- [ ] Unit tests: query loop iteration, budget consumption, parallel safety
- [ ] Unit tests: system prompt assembly, context compression trigger

---

### Phase 5: SQLite Session Store & FTS5 Search

**Goal:** Implement persistent session storage with full-text search.

**Dependencies:** Phase 1 (core types).

#### SessionDB (`crates/core/src/session_db.rs`)

```rust
pub struct SessionDB {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl SessionDB {
    pub fn open(path: &Path) -> Result<Self> {
        // WAL mode, schema creation, FTS5 setup
    }

    pub fn create_session(&self, session: &Session) -> Result<()> { ... }
    pub fn add_message(&self, session_id: &str, message: &Message) -> Result<()> { ... }
    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>> { ... }
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<Message>> { ... }
    pub fn update_system_prompt(&self, session_id: &str, prompt: &str) -> Result<()> { ... }
    pub fn update_session_stats(&self, session_id: &str, stats: &CostTracker) -> Result<()> { ... }
    pub fn search_sessions(&self, query: &str, source: Option<&str>) -> Result<Vec<SearchResult>> { ... }
    pub fn get_session_summaries(&self, limit: usize) -> Result<Vec<SessionSummary>> { ... }
}
```

#### Schema (matching Python codebase exactly)

```sql
-- sessions table
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    user_id TEXT,
    model TEXT,
    model_config TEXT,
    system_prompt TEXT,
    parent_session_id TEXT REFERENCES sessions(id),
    started_at REAL NOT NULL,
    ended_at REAL,
    end_reason TEXT,
    message_count INTEGER DEFAULT 0,
    tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    billing_provider TEXT,
    billing_base_url TEXT,
    estimated_cost_usd REAL,
    title TEXT
);

-- messages table
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL,
    content TEXT,
    tool_call_id TEXT,
    tool_calls TEXT,
    tool_name TEXT,
    timestamp REAL NOT NULL,
    token_count INTEGER,
    finish_reason TEXT,
    reasoning TEXT,
    reasoning_details TEXT
);

-- FTS5 virtual table
CREATE VIRTUAL TABLE messages_fts USING fts5(
    content,
    content=messages,
    content_rowid=id
);
```

#### Checklist

- [ ] Implement `SessionDB` with rusqlite
- [ ] Create schema (sessions, messages, messages_fts)
- [ ] Enable WAL mode
- [ ] Implement FTS5 triggers (insert, update, delete)
- [ ] Implement all CRUD methods
- [ ] Implement search_sessions with FTS5
- [ ] Implement session splitting (parent_session_id chain for compression)
- [ ] Implement session summaries
- [ ] Implement source tagging (cli, telegram, discord, etc.)
- [ ] Unit tests: session create/read, message add/retrieve, FTS5 search
- [ ] Unit tests: schema migration from version N to N+1

---

### Phase 6: TUI (Terminal User Interface)

**Goal:** Implement the interactive CLI TUI using ratatui.

**Dependencies:** Phase 1 (core types), Phase 4 (query loop).

#### TUI Architecture

```
┌──────────────────────────────────────────────────────┐
│  Banner / Model Info / Status                         │
│  ──────────────────────────────────────────────────   │
│  User: hello!                                         │
│  Assistant: Hi! How can I help you today?            │
│  [tool execution output...]                          │
│                                                       │
├──────────────────────────────────────────────────────┤
│  > user input here...                    [⠋ spinner]  │
│                    Fixed Input Area                   │
└──────────────────────────────────────────────────────┘
```

#### Key Components (`crates/tui/src/`)

| File | Purpose |
|------|---------|
| `lib.rs` | App state, main render loop |
| `app.rs` | Application state management |
| `output.rs` | Scrolling output region |
| `input.rs` | Fixed input area with multiline |
| `completer.rs` | Slash command autocomplete |
| `spinner.rs` | Kawaii spinner animation |
| `keybindings.rs` | Keyboard shortcuts |
| `status.rs` | Status bar display |
| `skin_engine.rs` | Theme/skin customization |

#### Key Bindings

| Key | Action |
|-----|--------|
| Enter | Submit input |
| Shift+Enter | Newline in input |
| Up/Down | History navigation |
| Ctrl+C | Interrupt current work |
| Ctrl+D | Exit |
| Ctrl+L | Clear screen |
| Tab | Autocomplete |

#### Checklist

- [ ] Implement ratatui + crossterm app scaffold
- [ ] Implement fixed input area layout (HSplit)
- [ ] Implement scrolling output region
- [ ] Implement multiline input with Shift+Enter
- [ ] Implement slash command autocomplete widget
- [ ] Implement spinner animation (10 frames)
- [ ] Implement key bindings (Enter, Ctrl+C, Ctrl+D, Ctrl+L, Tab, Up/Down)
- [ ] Implement history navigation (up/down arrow)
- [ ] Implement interrupt handling (Ctrl+C during tool execution)
- [ ] Implement status bar (model, cost, iteration budget)
- [ ] Implement Skin Engine (theme customization)
- [ ] Implement banner display (ASCII art + version)
- [ ] Implement ANSI output handling
- [ ] Implement streaming response rendering
- [ ] Unit tests: completer fuzzy matching, skin loading

---

### Phase 7: Slash Commands

**Goal:** Implement all 60+ slash commands.

**Dependencies:** Phase 1 (core types), Phase 4 (query loop), Phase 6 (TUI).

#### Command Trait (`crates/commands/src/lib.rs`)

```rust
#[async_trait]
pub trait SlashCommand: Send + Sync {
    fn name(&self) -> &str;
    fn aliases(&self) -> Vec<&str> { vec![] }
    fn description(&self) -> &str;
    fn category(&self) -> &str { "general" }

    async fn execute(&self, args: &str, ctx: &CommandContext)
        -> Result<CommandResult>;
}

pub enum CommandResult {
    Message(String),
    ConfigChange(ConfigChangeMessage),
    Exit,
}
```

#### Command Registry

```rust
pub fn all_commands() -> Vec<Box<dyn SlashCommand>> {
    vec![
        Box::new(NewCommand),       // /new, /reset, /clear
        Box::new(ModelCommand),     // /model
        Box::new(CompressCommand),  // /compress
        Box::new(UsageCommand),     // /usage
        Box::new(UndoCommand),      // /undo
        Box::new(RetryCommand),     // /retry
        Box::new(StopCommand),      // /stop
        Box::new(ToolsCommand),     // /tools
        Box::new(SkillsCommand),    // /skills
        Box::new(MemoryCommand),    // /memory
        Box::new(HelpCommand),      // /help
        // ... 50+ more
    ]
}
```

#### Priority Commands

| Command | Implementation | Priority |
|---------|---------------|----------|
| `/new`, `/reset` | Clear session, create new | P0 |
| `/model [provider:model]` | Switch model/provider | P0 |
| `/compress` | Trigger context compression | P0 |
| `/usage` | Show token/cost usage | P1 |
| `/undo` | Undo last turn | P1 |
| `/retry` | Retry last turn | P1 |
| `/stop` | Interrupt current work | P0 |
| `/tools` | List/enable/disable tools | P1 |
| `/skills` | Browse/search skills | P1 |
| `/memory` | View/manage memory | P1 |
| `/personality [name]` | Set personality | P2 |
| `/status` | Show session status | P1 |
| `/help [command]` | Show help | P1 |
| `/title [text]` | Set session title | P2 |
| `/summarize` | Summarize conversation | P2 |
| `/export [format]` | Export conversation | P2 |
| `/speak [text]` | Text-to-speech | P2 |
| `/voice` | Toggle voice input | P2 |
| `/insights [--days N]` | Usage analytics | P2 |
| `/doctor` | Run diagnostics | P2 |

#### Checklist

- [ ] Define `SlashCommand` trait and `CommandContext`
- [ ] Implement command dispatcher with fuzzy matching
- [ ] Implement all P0 commands (new, model, compress, stop)
- [ ] Implement all P1 commands (usage, undo, retry, tools, skills, memory, status, help)
- [ ] Implement all P2 commands
- [ ] Implement autocomplete for command arguments
- [ ] Implement context-sensitive help
- [ ] Implement ConfigChangeMessage for live config updates
- [ ] Unit tests: command parsing, argument validation, autocomplete

---

### Phase 8: Terminal Backends (Environments)

**Goal:** Implement all 6 terminal backends.

**Dependencies:** Phase 3 (tool framework).

#### Environment Trait (`crates/envs/src/lib.rs`)

```rust
#[async_trait]
pub trait Environment: Send + Sync {
    async fn setup(&mut self, task_id: &str) -> Result<()>;
    async fn teardown(&mut self, task_id: &str) -> Result<()>;
    async fn run_command(&mut self, cmd: &str, timeout: Duration) -> Result<String>;
    async fn upload_file(&mut self, local: &Path, remote: &Path) -> Result<()>;
    async fn download_file(&mut self, remote: &Path, local: &Path) -> Result<()>;
    fn working_dir(&self) -> &Path;
    fn is_persistent(&self) -> bool;
}
```

#### Backends

| Backend | File | Implementation |
|---------|------|----------------|
| Local | `local.rs` | Direct process spawn |
| Docker | `docker.rs` | Docker container exec |
| SSH | `ssh.rs` | SSH connection via openssh crate |
| Modal | `modal.rs` | Modal sandbox API |
| Daytona | `daytona.rs` | Daytona sandbox API |
| Singularity | `singularity.rs` | Singularity/Apptainer exec |

#### File Sync (`crates/envs/src/file_sync.rs`)

```rust
pub async fn sync_up(env: &mut dyn Environment, local: &Path, remote: &Path) -> Result<()> { ... }
pub async fn sync_down(env: &mut dyn Environment, remote: &Path, local: &Path) -> Result<()> { ... }
```

#### Checklist

- [ ] Implement Local environment (direct process spawn with PTY)
- [ ] Implement Docker environment (container lifecycle, volume mounts)
- [ ] Implement SSH environment (openssh crate, SCP file sync)
- [ ] Implement Modal environment (Modal sandbox API, serverless persistence)
- [ ] Implement Daytona environment (Daytona sandbox API)
- [ ] Implement Singularity environment (Apptainer exec)
- [ ] Implement file sync (SCP/SFTP)
- [ ] Implement environment router (select backend from config)
- [ ] Implement cleanup_vm() for all backends
- [ ] Implement is_persistent_env() detection
- [ ] Unit tests: local command execution, Docker container lifecycle

---

### Phase 9: Memory System & Skills System

**Goal:** Implement persistent memory, skills, and Skills Hub.

**Dependencies:** Phase 1 (core types), Phase 4 (query loop), Phase 5 (session store).

#### Memory System (`crates/core/src/memory.rs`)

```rust
pub struct MemoryManager {
    memory_dir: PathBuf,  // ~/.hermes/memory/
    index: MemoryIndex,   // MEMORY.md parser
}

impl MemoryManager {
    pub fn prefetch_all(&self, query: &str) -> Result<String> { ... }
    pub fn save_memory(&self, name: &str, content: &str) -> Result<()> { ... }
    pub fn get_memories(&self) -> Result<Vec<MemoryEntry>> { ... }
    pub fn clear_memories(&self) -> Result<()> { ... }
}
```

#### Skills System (`crates/core/src/skills.rs`)

```rust
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub version: String,
    pub tool_requirements: Vec<String>,
    pub enabled: bool,
}

pub struct SkillRegistry {
    skills_dir: PathBuf,  // ~/.hermes/skills/
    skills: HashMap<String, Skill>,
}
```

#### Skills Hub (`crates/commands/src/skills_hub.rs`)

```rust
pub struct SkillsHub {
    github_client: Option<GitHubClient>,  // for agentskills.io registry
}

impl SkillsHub {
    pub async fn search(&self, query: &str) -> Result<Vec<SkillInfo>> { ... }
    pub async fn install(&self, repo: &str) -> Result<()> { ... }
    pub async fn update(&self, name: &str) -> Result<()> { ... }
    pub async fn list_installed(&self) -> Result<Vec<Skill>> { ... }
}
```

#### Checklist

- [ ] Implement memory file discovery and loading (~/.hermes/memory/)
- [ ] Implement MEMORY.md index parsing
- [ ] Implement memory prefetch with query relevance scoring
- [ ] Implement memory save/create/update/delete
- [ ] Implement memory nudge system (periodic reminders)
- [ ] Implement skill discovery (~/.hermes/skills/)
- [ ] Implement skill loading and validation
- [ ] Implement skill system prompt injection
- [ ] Implement skill nudge system (periodic skill creation reminders)
- [ ] Implement Skills Hub search/install/list via GitHub API
- [ ] Implement skill versioning and updates
- [ ] Implement per-platform skill enable/disable
- [ ] Implement skills guard (safety checks before execution)
- [ ] Implement Honcho integration stub (optional dependency)
- [ ] Unit tests: memory CRUD, skill loading, nudge timing

---

### Phase 10: MCP Client

**Goal:** Implement MCP (Model Context Protocol) client with dynamic tool discovery.

**Dependencies:** Phase 3 (tool framework).

#### MCP Client (`crates/mcp/src/lib.rs`)

```rust
pub struct McpClient {
    servers: HashMap<String, McpServer>,
}

impl McpClient {
    pub async fn connect(&mut self, name: &str, config: McpServerConfig) -> Result<()> { ... }
    pub async fn disconnect(&mut self, name: &str) -> Result<()> { ... }
    pub async fn list_tools(&self) -> Vec<ToolDefinition> { ... }
    pub async fn call_tool(&self, server: &str, name: &str, args: serde_json::Value) -> Result<ToolResult> { ... }
}

pub enum McpTransport {
    Stdio { command: String, args: Vec<String> },
    Sse { url: String },
}
```

#### Dynamic Tool Discovery

```
MCP server connects → tools registered via ToolRegistry.register()
Server sends tools/list_changed → old tools deregistered, new registered
Server disconnects → all tools deregistered
```

#### MCP Config CLI (`hermes mcp list|add|remove|status`)

#### Checklist

- [ ] Implement MCP Stdio transport (spawn process, communicate via stdin/stdout)
- [ ] Implement MCP SSE transport (HTTP Server-Sent Events)
- [ ] Implement tool list fetching and registration
- [ ] Implement tool calling via MCP
- [ ] Implement resource listing and reading
- [ ] Implement prompt support
- [ ] Implement dynamic tool discovery (notifications/tools/list_changed)
- [ ] Implement OAuth flow for authenticated MCP servers
- [ ] Implement MCP server lifecycle management
- [ ] Implement MCP config CLI (hermes mcp list/add/remove/status)
- [ ] Implement shadow prevention (MCP can't overwrite built-in tools)
- [ ] Unit tests: MCP server connection, tool call round-trip

---

### Phase 11: Messaging Gateway & Platform Adapters

**Goal:** Implement the gateway process and all 18+ platform adapters.

**Dependencies:** Phase 4 (query loop), Phase 5 (session store).

#### Gateway Architecture

```
┌────────────────────────────────────────────────────┐
│  Gateway Runner                                     │
│  Manages platform adapter lifecycle                 │
│  Routes messages → Query Loop → Response → Platform │
└────────────────────────────────────────────────────┘
       |              |              |
   Telegram       Discord        Slack       ...
```

#### Platform Adapter Trait (`crates/gateway/src/platforms/base.rs`)

```rust
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    fn name(&self) -> &str;
    async fn connect(&self) -> Result<()>;
    async fn disconnect(&self) -> Result<()>;
    async fn send_message(&self, chat_id: &str, text: &str) -> Result<()>;
    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()>;
    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()>;
    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()>;
    async fn send_sticker(&self, chat_id: &str, sticker_id: &str) -> Result<()>;
    async fn edit_message(&self, chat_id: &str, message_id: &str, text: &str) -> Result<()>;
    async fn is_typing(&self, chat_id: &str) -> Result<()>;
    fn stream_consumer(&self) -> Arc<dyn StreamConsumer>;
}
```

#### Stream Consumer (`crates/gateway/src/stream_consumer.rs`)

```rust
#[async_trait]
pub trait StreamConsumer: Send + Sync {
    async fn on_text_delta(&self, delta: &str) -> Result<()>;
    async fn on_tool_start(&self, tool_name: &str, args_preview: &str) -> Result<()>;
    async fn on_tool_complete(&self, tool_name: &str, result: &str) -> Result<()>;
    async fn flush(&self) -> Result<()>;
}
```

#### Platform Adapters

| Platform | File | Library |
|----------|------|---------|
| Telegram | `telegram.rs` | teloxide or teloxide-derive |
| Telegram Network | `telegram_network.rs` | teloxide |
| Discord | `discord.rs` | serenity |
| Slack | `slack.rs` | slack-rust |
| WhatsApp | `whatsapp.rs` | HTTP bridge |
| Signal | `signal.rs` | signal-cli via subprocess |
| Matrix | `matrix.rs` | matrix-sdk |
| Home Assistant | `homeassistant.rs` | reqwest |
| Webhook | `webhook.rs` | axum |
| REST API | `api_server.rs` | axum |
| BlueBubbles | `bluebubbles.rs` | reqwest |
| DingTalk | `dingtalk.rs` | reqwest |
| Feishu | `feishu.rs` | reqwest |
| QQ Bot | `qqbot.rs` | reqwest |
| WeCom | `wecom.rs` | reqwest |
| WeChat | `weixin.rs` | reqwest |
| Mattermost | `mattermost.rs` | mattermost-rust |
| SMS | `sms.rs` | HTTP API |
| Email | `email.rs` | lettre |

#### Session Store (Gateway) (`crates/gateway/src/session.rs`)

```rust
pub struct GatewaySessionStore {
    db: Arc<SessionDB>,
    sessions: HashMap<String, GatewaySession>,  // platform-user → session
}
```

#### DM Pairing (`crates/gateway/src/pairing.rs`)

Pair code system for group/channel → DM routing.

#### Checklist

- [ ] Implement `PlatformAdapter` trait
- [ ] Implement `StreamConsumer` trait and `GatewayStreamConsumer`
- [ ] Implement `GatewayRunner` lifecycle management
- [ ] Implement gateway session store (per platform-user pair)
- [ ] Implement slash command routing in gateway
- [ ] Implement DM pairing system
- [ ] Implement message formatting per platform (Markdown, HTML, Mrkdwn)
- [ ] Implement Telegram adapter (teloxide, long polling + webhook)
- [ ] Implement Discord adapter (serenity)
- [ ] Implement Slack adapter (slack-rust)
- [ ] Implement WhatsApp adapter (HTTP bridge)
- [ ] Implement Signal adapter (signal-cli subprocess)
- [ ] Implement Matrix adapter (matrix-sdk)
- [ ] Implement Home Assistant adapter
- [ ] Implement Webhook adapter (generic HTTP)
- [ ] Implement REST API server (axum)
- [ ] Implement BlueBubbles, DingTalk, Feishu, QQ Bot, WeCom, WeChat adapters
- [ ] Implement Mattermost, SMS, Email adapters
- [ ] Implement channel directory tracking
- [ ] Implement display config per platform
- [ ] Implement gateway status reporting
- [ ] Implement gateway hooks (pre/post message)
- [ ] Unit tests: platform adapter mock, stream consumer buffering

---

### Phase 12: Cron Scheduler

**Goal:** Implement the cron scheduler for automations.

**Dependencies:** Phase 11 (gateway).

#### Cron Scheduler (`crates/gateway/src/cron/scheduler.rs`)

```rust
pub struct CronScheduler {
    jobs: Vec<CronJob>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl CronScheduler {
    pub fn add_job(&mut self, job: CronJob) { ... }
    pub fn remove_job(&mut self, id: &str) { ... }
    pub fn start(&mut self, gateway_tx: UnboundedSender<GatewayEvent>) { ... }
    pub fn stop(&mut self) { ... }
}

pub struct CronJob {
    pub id: String,
    pub schedule: CronSchedule,  // cron expression
    pub prompt: String,          // what to ask the agent
    pub delivery: DeliveryTarget, // where to send result
    pub enabled: bool,
}
```

#### Checklist

- [ ] Implement cron expression parser (croniter-compatible)
- [ ] Implement CronScheduler with tokio timer
- [ ] Implement CronJob CRUD
- [ ] Implement delivery to any platform adapter
- [ ] Implement job execution history
- [ ] Implement error handling and retry
- [ ] Implement job enable/disable
- [ ] Implement natural language scheduling ("every day at 9am" → cron expression)
- [ ] Unit tests: cron expression parsing, scheduler timing

---

### Phase 13: Web UI Server

**Goal:** Implement web-based UI as an alternative interface.

**Dependencies:** Phase 4 (query loop), Phase 5 (session store).

#### Web Server (`crates/web/src/lib.rs`)

```rust
pub struct WebServer {
    router: Router,  // axum
    query_config: QueryConfig,
}

// API endpoints:
// POST /api/chat          → send message, get response
// GET  /api/sessions      → list sessions
// GET  /api/sessions/{id} → get session messages
// GET  /api/models        → list available models
// GET  /api/stream        → SSE stream for responses
// POST /api/model         → switch model
```

#### Checklist

- [ ] Implement axum server scaffold
- [ ] Implement POST /api/chat endpoint
- [ ] Implement GET /api/sessions endpoint
- [ ] Implement SSE streaming endpoint for real-time responses
- [ ] Implement GET /api/models endpoint
- [ ] Implement POST /api/model (model switching)
- [ ] Serve static HTML/CSS/JS frontend
- [ ] Implement dark/light theme toggle
- [ ] Implement file attachment support
- [ ] Unit tests: API endpoint correctness, SSE streaming

---

### Phase 14: ACP Server

**Goal:** Implement ACP (Agent Communication Protocol) server for IDE integration.

**Dependencies:** Phase 4 (query loop).

#### ACP Server (`crates/acp/src/lib.rs`)

```rust
pub struct AcpServer {
    query_config: QueryConfig,
}

// ACP protocol:
// - session management
// - message exchange
// - tool execution
// - file context awareness
// - selection-based operations
```

#### Entry Point

```toml
[[bin]]
name = "hermes-acp"
path = "crates/cli/src/acp_main.rs"
```

#### Checklist

- [ ] Implement ACP protocol server
- [ ] Implement session management
- [ ] Implement message exchange
- [ ] Implement file context awareness
- [ ] Implement selection-based operations
- [ ] Implement git integration
- [ ] Implement terminal integration
- [ ] Unit tests: ACP protocol round-trip

---

### Phase 15: Plugin System

**Goal:** Implement plugin discovery, loading, and hook execution.

**Dependencies:** Phase 1 (core types), Phase 7 (commands).

#### Plugin System (`crates/plugins/src/lib.rs`)

```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn commands(&self) -> Vec<Box<dyn SlashCommand>> { vec![] }
    fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
}

pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn Plugin>>,
}

// Hook system
pub fn invoke_hook(hook: &str, args: HookArgs) -> Vec<serde_json::Value> { ... }
```

#### Hooks

| Hook | When Fired | Purpose |
|------|-----------|---------|
| `on_session_start` | New session created | Initialize session-scoped state |
| `pre_llm_call` | Before LLM API call | Inject context into user message |
| `post_llm_response` | After LLM response | Process response |
| `on_tool_call` | Before tool execution | Intercept/modify tool calls |
| `on_tool_result` | After tool execution | Process tool results |

#### Checklist

- [ ] Implement Plugin trait and PluginRegistry
- [ ] Implement plugin discovery from `plugins/` directory
- [ ] Implement plugin loading (dynamic library or directory-based)
- [ ] Implement hook system (on_session_start, pre_llm_call, etc.)
- [ ] Implement plugin command registration
- [ ] Implement plugin tool registration
- [ ] Implement `hermes plugins` CLI commands
- [ ] Unit tests: plugin discovery, hook invocation

---

### Phase 16: CLI Binary & Integration

**Goal:** Wire everything together into the `hermes` and `hermes-gateway` binaries.

**Dependencies:** All prior phases.

#### Binary Entry Points (`crates/cli/src/`)

| Binary | Entry Point | Purpose |
|--------|-------------|---------|
| `hermes` | `main.rs` | Interactive CLI TUI |
| `hermes-gateway` | `gateway_main.rs` | Messaging gateway daemon |
| `hermes-acp` | `acp_main.rs` | ACP server for IDEs |

#### CLI Argument Parsing

```rust
#[derive(Parser)]
#[command(name = "hermes", version, about)]
enum HermesCommand {
    /// Start interactive CLI session
    #[command(flatten)]
    Interact(InteractOpts),
    /// Run setup wizard
    Setup,
    /// Switch model/provider
    Model { spec: Option<String> },
    /// Configure tools
    Tools { action: Option<String>, name: Option<String> },
    /// Configure skills
    Skills { action: Option<String>, name: Option<String> },
    /// Gateway management
    Gateway { action: String },
    /// Configuration
    Config { action: String, key: Option<String>, value: Option<String> },
    /// Run diagnostics
    Doctor,
    /// Update to latest version
    Update,
    /// Backup/restore
    Backup { action: String },
    /// Plugin management
    Plugins { action: String },
    /// Profile management
    Profiles { action: String },
    /// Start web UI server
    Web,
    /// Show status
    Status,
    /// View logs
    Logs { session: Option<String> },
    /// Uninstall
    Uninstall,
    /// Show version/banner
    Version,
    /// MCP server management
    Mcp { action: String },
    /// OpenClaw migration
    Claw { action: String },
    /// Shell completion setup
    Completion,
}
```

#### Checklist

- [ ] Implement `hermes` binary with clap argument parsing
- [ ] Implement `hermes-gateway` binary
- [ ] Implement `hermes-acp` binary
- [ ] Implement setup wizard (interactive configuration)
- [ ] Implement `hermes doctor` diagnostics
- [ ] Implement `hermes update` (download latest binary)
- [ ] Implement `hermes backup` (export/import config + sessions)
- [ ] Implement `hermes logs` (session log viewing)
- [ ] Implement `hermes uninstall`
- [ ] Implement shell completion generation (bash, zsh, fish)
- [ ] Implement config migration (detect old config, auto-migrate)
- [ ] Implement environment variable loading (~/.hermes/.env)
- [ ] Integration test: full `hermes` CLI session
- [ ] Integration test: `hermes gateway start` with Telegram

---

## 4. Risk Assessment

### High Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| **API protocol diversity** — 15+ providers with subtly different response formats | Streaming parse failures, silent data loss | Strict schema validation per provider mode; extensive response parsing tests; fallback to non-streaming when streaming fails |
| **Terminal backend complexity** — 6 backends with different lifecycle models | Inconsistent tool execution behavior | Abstract `Environment` trait with thorough contract tests; shared test suite across all backends |
| **Platform adapter ecosystem** — 18+ adapters with different message formats, rate limits | Inconsistent user experience across platforms | `PlatformAdapter` trait with conformance tests; shared stream consumer; per-platform format normalization |
| **Credential management** — 20+ API keys across providers | Security vulnerability, credential leakage | Credentials never logged; env-var-only for sensitive keys; encrypted config option; `hermes doctor` credential validation |

### Medium Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Context compression quality** — Auxiliary LLM summarization loses important context | Agent loses track of conversation | Preserve last N turns uncompressed; preserve all tool results; compression quality metrics displayed to user |
| **FTS5 compatibility** — rusqlite FTS5 may differ from Python sqlite3 FTS5 | Search results differ between Python and Rust versions | Test against identical data; document any behavioral differences; use same tokenizer settings |
| **Tool parallelism bugs** — Race conditions in concurrent tool execution | Corrupted file state, interleaved output | Thread-safe ProcessRegistry; path overlap detection; sequential fallback for conflicting batches |
| **ratatui TUI limitations** — prompt_toolkit has richer input features | UX regression vs Python version | Prioritize core features (multiline, history, autocomplete); use ratatui's widget ecosystem for parity |
| **MCP dynamic tool conflicts** — Multiple servers with overlapping tool names | Tool registration failures | Shadow prevention already implemented in ToolRegistry; MCP-to-MCP overwrites allowed, built-in shadowing rejected |
| **Config migration** — Python YAML config → Rust struct deserialization | Config parse errors on migration | `#[serde(default)]` on all optional fields; graceful degradation with warnings; `hermes doctor` migration check |

### Low Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Skills Hub GitHub API rate limits** | Slow skill search/install | Cache search results; use GitHub App JWT auth for higher rate limits; fallback to local registry |
| **Memory file format changes** — Markdown files parsed differently | Memory parsing errors | Strict markdown parser; graceful handling of malformed files; MEMORY.md version tracking |
| **Binary size** — Rust static linking produces large binary | Distribution challenges | Use dynamic linking where possible; strip debug symbols; provide MUSL and glibc variants |

## 5. Cost Considerations

### Token Pricing

The Rust implementation uses the same pricing tables as the Python codebase
(`agent/usage_pricing.py`). All cost calculations remain server-side (provider
APIs report usage); the client only formats and displays.

### Estimated development cost per provider integration

| Provider | Effort | Notes |
|----------|--------|-------|
| OpenAI-compatible (standard) | ~2 hours | Reuse base client with different URL |
| Anthropic Messages API | ~4 hours | Different response format, streaming |
| OpenRouter (200+ models) | ~4 hours | Provider filtering, model metadata |
| Custom provider | ~2 hours | Base URL + API key = ready |

### Running cost per user interaction (reference pricing)

| Scenario | Tokens | Est. Cost (Sonnet via OpenRouter) |
|----------|--------|-----------------------------------|
| Simple question (no tools) | ~2K in / 500 out | ~$0.004 |
| File refactor (3 tool calls) | ~10K in / 3K out | ~$0.02 |
| Complex task (10 tool calls, parallel) | ~50K in / 10K out | ~$0.08 |
| Session with compression | ~100K in / 15K out | ~$0.15 |
| Subagent delegation (3 subagents) | ~150K in / 30K out | ~$0.30 |

## 6. Provider Compatibility Matrix

Authentication must be verified before a provider can be used.

| Provider | Auth Method | Chat Completions | Streaming | Tools | Vision | Reasoning |
|----------|-------------|-----------------|-----------|-------|--------|-----------|
| Nous Portal | API key | Yes | Yes | Yes | Yes | Yes |
| OpenRouter | API key | Yes | Yes | Yes | Yes | Yes |
| Anthropic | API key | Via /anthropic | Yes | Yes | Yes | Yes |
| OpenAI | API key | Yes | Yes | Yes | Yes | Yes |
| Xiaomi MiMo | API key | Yes | Yes | Yes | Yes | No |
| z.ai/GLM | API key | Yes | Yes | Yes | Yes | Yes |
| Kimi/Moonshot | API key | Yes | Yes | Yes | No | No |
| MiniMax | API key | Yes | Yes | Yes | No | No |
| HuggingFace | API key | Yes | Partial | Yes | Yes | No |
| Ollama | None (local) | Yes | Yes | Yes | Varies | Varies |
| Mistral | API key | Yes | Yes | Yes | Yes | No |
| Local endpoint | Any | Yes | Varies | Varies | Varies | Varies |

### Cross-provider constraints

- All providers must resolve credentials before use
- Model names must be resolvable via `ProviderId/ModelId` format
- Streaming format differences are abstracted by the `ApiMode` enum
- Tool schema compatibility is validated per provider at registration time

## 7. File Change Summary

| Crate | Phase | Files (new) | Files (modify) | Description |
|-------|-------|-------------|----------------|-------------|
| `core` | 1, 5, 9 | 8 | — | Core types, config, session DB, memory, skills |
| `api` | 2 | 5 | — | Provider registry, API client, streaming |
| `tools` | 3 | 20 | — | Tool trait, registry, ~40 tool implementations |
| `query` | 4 | 5 | — | Query loop, prompt builder, context compressor |
| `tui` | 6 | 10 | — | ratatui TUI, input, output, completer, spinner |
| `commands` | 7, 9 | 20 | — | Slash commands, skills hub |
| `mcp` | 10 | 5 | — | MCP client, OAuth, dynamic discovery |
| `gateway` | 11, 12 | 25 | — | Gateway runner, 18+ platform adapters, cron |
| `acp` | 14 | 5 | — | ACP protocol server |
| `envs` | 8 | 8 | — | 6 terminal backends, file sync |
| `web` | 13 | 5 | — | Axum web server, SSE streaming |
| `plugins` | 15 | 5 | — | Plugin discovery, hooks |
| `cli` | 16 | 5 | — | Binary entry points, setup wizard |

**Total: ~126 new files across 13 crates.**

## 8. Crate Dependency Graph

```
                        ┌─────────┐
                        │  core   │
                        └────┬────┘
              ┌──────────────┼──────────────┐
              │              │              │
         ┌────▼────┐   ┌────▼────┐   ┌────▼────┐
         │   api   │   │  tools  │   │ plugins │
         └────┬────┘   └────┬────┘   └─────────┘
              │              │
         ┌────▼────┐   ┌────▼────┐
         │  query  │◄──┤   mcp   │
         └────┬────┘   └─────────┘
              │
    ┌─────────┼─────────┬──────────┬──────────┐
    │         │         │          │          │
┌───▼───┐ ┌──▼──┐ ┌───▼───┐ ┌───▼───┐ ┌───▼───┐
│  tui  │ │cmds │ │gateway│ │ envs  │ │  web  │
└───────┘ └─────┘ └───┬───┘ └───────┘ └───────┘
                      │
                  ┌───▼───┐
                  │  acp  │
                  └───────┘

cli binary → tui, commands, query, api, tools, core
gateway binary → gateway, query, api, tools, core
acp binary → acp, query, api, tools, core
```

## 9. Implementation Order and Timeline Estimate

```
Phase 1  (Workspace + Core)       ████████████████████░░  ~3 days
Phase 2  (API Client)             ░░░░████████████████░░  ~3 days  (depends on P1)
Phase 3  (Tool Framework)         ░░░░░░░░██████████████  ~5 days  (depends on P1)
Phase 4  (Query Loop)             ░░░░░░░░░░░░██████████  ~4 days  (depends on P1-3)
Phase 5  (SQLite Store)           ░░░░████████████████░░  ~2 days  (depends on P1)
Phase 6  (TUI)                    ░░░░░░░░░░░░░░░░██████  ~4 days  (depends on P4)
Phase 7  (Commands)               ░░░░░░░░░░░░░░░░░░████  ~3 days  (depends on P4,6)
Phase 8  (Terminal Backends)      ░░░░░░░░░░░░██████░░░░  ~4 days  (depends on P3)
Phase 9  (Memory + Skills)        ░░░░░░░░░░░░░░░░░░████  ~3 days  (depends on P4,5)
Phase 10 (MCP Client)             ░░░░░░░░░░░░░░░░░░████  ~3 days  (depends on P3)
Phase 11 (Gateway + Platforms)    ░░░░░░░░░░░░░░░░░░░███  ~8 days  (depends on P4,5)
Phase 12 (Cron Scheduler)         ░░░░░░░░░░░░░░░░░░░░░░  ~2 days  (depends on P11)
Phase 13 (Web UI)                 ░░░░░░░░░░░░░░░░░░░░░░  ~2 days  (depends on P4)
Phase 14 (ACP Server)             ░░░░░░░░░░░░░░░░░░░░░░  ~2 days  (depends on P4)
Phase 15 (Plugin System)          ░░░░░░░░░░░░░░░░░░░░░░  ~2 days  (depends on P1,7)
Phase 16 (CLI Binary + Integration)░░░░░░░░░░░░░░░░░░░░░  ~3 days  (depends on ALL)
```

Phases 5, 8, 9, 10 can run in parallel after Phase 3/4.
Phases 13, 14, 15 can run in parallel after Phase 4.

**Total estimate: ~35-50 working days** for a single developer, with parallelizable
phases reducing wall-clock time to approximately **4-6 weeks** with 2 developers.

## 10. Out of Scope (Deferred to v2)

These features exist in the Python codebase but are deferred to reduce initial
implementation scope. They can be added after the core system is functional.

| Feature | Python Source | Reason for Deferral |
|---------|--------------|---------------------|
| Atropos RL environments | `environments/` | Research-only, not user-facing |
| Batch trajectory generation | `batch_runner.py` | Data generation pipeline |
| Trajectory compression | `trajectory_compressor.py` | Training data tool |
| Mini SWE runner | `mini_swe_runner.py` | Benchmarking only |
| Honcho dialectic user modeling | `agent/memory_manager.py` | External service, optional |
| Voice mode (faster-whisper) | `tools/voice_mode.py` | Complex audio pipeline |
| Mixture of Agents | `tools/mixture_of_agents_tool.py` | Niche research feature |
| Nous subscription prompts | `agent/prompt_builder.py` | Partner-specific |
| Smart model routing | `agent/smart_model_routing.py` | Auto-optimization, not core |
| Skin sync | `tools/skills_sync.py` | Convenience feature |
| Toolset distributions | `toolset_distributions.py` | Research utility |
| RL CLI | `rl_cli.py` | Research utility |
| Gacha mechanics / Buddy system | (if present) | Gamification, not core |
