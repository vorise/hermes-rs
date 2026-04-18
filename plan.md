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
- **In scope:** Agent core loop, CLI TUI, tool system, session storage, memory
  plugins (7 backends), skills (3-tier progressive disclosure), MCP client,
  6 terminal backends, 20+ messaging platform adapters, cron scheduler, ACP
  server, plugin system, model provider support (15+ providers), web UI,
  credential pool (4 strategies, OAuth auto-refresh), error classification
  (14 failover reasons), context compression (auxiliary LLM, 15+ tool-specific
  summaries), code execution sandbox (UDS + file RPC), TTS (6 providers,
  4000 char limit), browser tool (5 backends), checkpoint manager (shadow git),
  process registry (200KB buffer, 64 max concurrent), fuzzy match engine
  (9-strategy chain), V4A patch parser, send_message (18+ platforms),
  Tirith security scanner, RL training tool (9 tools, 3-process pipeline).
- **Deferred (v2):** Atropos RL environments (research-only), batch trajectory
  generation, trajectory compressor, Mini SWE runner, Honcho dialectic user
  modeling, voice mode (16kHz mono int16 audio pipeline), Mixture of Agents,
  Nous subscription prompts, smart model routing, toolset distributions,
  RL CLI, Gacha mechanics / Buddy system.

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
                    ┌──────┐
                    │ core │ ← config, types, session, memory, skills, pricing
                    └──┬───┘
           ┌───────┬───┴───┬────────┬────────┬────────┐
           ▼       ▼       ▼        ▼        ▼        ▼
        ┌────┐  ┌────┐  ┌─────┐  ┌──────┐  ┌───────┐ ┌────────┐
        │ api│  │tools│  │envs │  │plugins│  │commands│ │  mcp   │
        └──┬─┘  └──┬──┘  └─────┘  └──────┘  └───┬───┘ └───┬────┘
           │       │                              │        │
           ▼       ▼                              ▼        │
        ┌──────────────────┐                    ┌─────────┘
        │      query       │ ← conversation loop, prompt builder, compression
        └────────┬─────────┘
                 │
        ┌────────┼────────┬────────┬──────────────┐
        ▼        ▼        ▼        ▼              ▼
     ┌────┐  ┌─────┐  ┌─────┐  ┌─────┐  ┌────────────┐
     │ tui│  │gateway│ │ acp │  │ web │  │ cli binary │
     └────┘  └──────┘  └─────┘  └─────┘  └────────────┘
```

**Key relationships:**
- `core` — all crates depend on core (shared types, config, sessions)
- `api` — depends on core; called by query for LLM requests
- `tools` — depends on core + envs; called by query for tool execution
- `envs` — depends on core; used by tools (terminal_tool → environments)
- `query` — depends on api + tools + core + mcp; the shared conversation loop
- `mcp` — depends on core; tools discovered by query at runtime
- `commands` — depends on core; called by tui/gateway/acp for slash commands
- `tui`, `gateway`, `acp`, `web` — all depend on query + core (different I/O layers)
- `cli` binary — wires together tui + commands + query + api + tools + core
- `gateway` binary — wires together gateway + query + api + tools + envs + core
- `acp` binary — wires together acp + query + api + tools + core

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
   use rusqlite with the fts5 extension for identical full-text search. Memory
   plugins (Holographic) also use SQLite FTS5 for retrieval.

4. **Tool auto-registration via `inventory` crate** — the Python codebase uses
   AST-based `registry.register()` at import time; in Rust we use the `inventory`
   crate for zero-boilerplate tool registration at compile time.

5. **Config compatibility** — `~/.hermes/config.yaml` and `~/.hermes/.env` formats
   are preserved exactly so users can migrate without reconfiguration.

6. **Shared query loop across all interfaces** — the CLI TUI, messaging gateway,
   web UI, and ACP server all use the same `h-query` query loop. Only the I/O
   layer differs.

7. **Credential pool with 4 strategies** — fill_first, round_robin, random,
   least_used. OAuth credentials auto-refresh from external files. Exhaustion
   cooldown (1hr for 429, 5min default). Concurrent request tracking per
   credential.

8. **Error classification pipeline** — 14 `FailoverReason` enum values,
   priority-ordered classification, `ClassifiedError` with recovery hints
   (retryable, should_compress, should_rotate_credential, should_fallback).

9. **Context compression with auxiliary LLM** — 20% summary ratio, 2K-12K token
   range, protect first 3 + last 6 turns, tool-specific summaries for 15+ tool
   types, iterative updates, 600s failure cooldown.

10. **Code execution sandbox** — UDS transport (local) and file-based RPC
    (remote). 7-tool allow-list. `hermes_tools.py` module generator. Blocked
    terminal params: background, pty, notify_on_complete, watch_patterns.

11. **Checkpoint manager** — shadow git repos with GIT_DIR/GIT_WORK_TREE,
    20 exclude patterns, per-turn dedup, pre-rollback snapshots.

12. **BasePlatformAdapter ABC** — 2,087-line interface defining MessageEvent
    (9 message types), SendResult (with retryable flag), 16-step background
    processing pipeline, retry system (9 retryable error patterns), typing
    indicator (2s refresh), message truncation (UTF-16 for Telegram).

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
- [ ] Implement `ApiClient` with auto-detection of API mode (3 modes: chat_completions, codex_responses, anthropic_messages)
- [ ] Implement Chat Completions streaming (SSE)
- [ ] Implement Anthropic Messages API streaming (5 beta headers, per-model output limits: Opus=128K/Sonnet=64K, 4-level thinking budgets)
- [ ] Implement Codex Responses API
- [ ] Implement credential resolution (env → .env → config)
- [ ] Implement token usage parsing from all response formats
- [ ] Implement error classification (14 FailoverReasons: rate_limit, context_length, auth, billing, timeout, connection, etc.)
- [ ] Implement auxiliary client for context compression (5 backend types: OpenAI, Codex, Anthropic, Nous, Custom)
- [ ] Implement auxiliary client OpenAI-compatible shim (consumer calls `client.chat.completions.create()` regardless of backend)
- [ ] Implement Codex Responses adapter (content conversion, streaming event collection, backfill logic, response shaping)
- [ ] Implement Anthropic Messages adapter (build_anthropic_kwargs + normalize_anthropic_response, usage mapping)
- [ ] Implement URL rewriting (/anthropic → /v1 for providers with dual endpoints)
- [ ] Implement vision backend resolution (get_available_vision_backends)
- [ ] Implement text auxiliary client resolution (OpenRouter → Nous → Custom → Codex chain)
- [ ] Implement credential pool integration (PooledCredential, 4 strategies, OAuth auto-refresh, exhaustion cooldown)
- [ ] Implement rate limit header parsing (12 header types, RateLimitBucket/RateLimitState, 80% warning)
- [ ] Implement model metadata (64K min context, 128K fallback, 5 probe tiers, 1hr cache)
- [ ] Implement usage pricing (CanonicalUsage, per-million rates, CostResult with 4 status types)
- [ ] Implement insights engine (SQLite analysis, token/cost/tool tracking)
- [ ] Implement Copilot ACP client (JSON-RPC stdio, OAuth, 900s timeout)
- [ ] Unit tests: provider registration, API mode detection, streaming parse, error classification

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
/// Command guard system: Tirith scanner + dangerous command patterns.
pub fn check_all_command_guards(cmd: &str) -> ApprovalResult { ... }

/// Heuristic detection of dangerous terminal commands.
pub fn is_destructive_command(cmd: &str) -> bool {
    // Matches: rm, rmdir, mv, sed -i, truncate, dd, shred,
    // git reset/clean/checkout, output redirects (>)
}

/// Sudo rewriting: transforms unquoted `sudo` into `sudo -S` with password piped.
pub fn rewrite_sudo_invocations(cmd: &str) -> String { ... }
```

#### Checkpoint Manager (`crates/tools/src/checkpoint.rs`)

Shadow git repo for workspace state:
- `GIT_DIR` / `GIT_WORK_TREE` for isolated snapshots
- 20 exclude patterns (node_modules, .git, target/, etc.)
- Per-turn dedup (skip if no changes since last checkpoint)
- Pre-rollback snapshot (auto-save before undo)

#### Fuzzy Match Engine (`crates/tools/src/fuzzy_match.rs`)

9-strategy chain for patch block matching:
1. Exact match → 2. Anchor match (50%/70% thresholds) → 3. Context hint →
4. Whitespace-insensitive → 5. Unicode normalization (8 mappings) →
6. Line-number fallback → 7. Block search → 8. Fuzzy → 9. Best-effort

#### V4A Patch Parser (`crates/tools/src/patch_parser.rs`)

4 operation types: add, update, delete, move.
Two-phase validate-then-apply. Fuzzy match with context hints.

#### Process Registry (`crates/tools/src/process_registry.rs`)

- 200KB rolling output buffer per process
- 30min TTL for completed processes
- 64 max concurrent processes
- Watch patterns for completion detection
- Crash recovery with configurable restarts

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
- [ ] Implement approval system (command guards, Tirith scanner integration, dangerous command patterns)
- [ ] Implement sudo rewriting and password caching
- [ ] Implement process registry (200KB buffer, 30min TTL, 64 max concurrent, watch patterns)
- [ ] Implement checkpoint manager (shadow git, 20 exclude patterns, per-turn dedup)
- [ ] Implement fuzzy match engine (9-strategy chain, 8 Unicode mappings)
- [ ] Implement V4A patch parser (4 operations, validate-then-apply)
- [ ] Implement tool result size limiting and persistence
- [ ] Implement parallel tool execution with `_MAX_TOOL_WORKERS = 8`
- [ ] Implement parallel safety classification (never_parallel, parallel_safe, path_scoped)
- [ ] Unit tests: tool registration, approval detection, process lifecycle

---

### Phase 4: Query Loop & Context Compression

**Goal:** Implement the core agentic conversation loop.

**Dependencies:** Phase 1 (core types), Phase 2 (API client), Phase 3 (tools).

#### Query Loop (`crates/query/src/lib.rs`)

Two-level loop architecture (matching Python `run_conversation()`):

**Outer loop** (API call iterations):
- Condition: `(api_call_count < max_iterations && budget.remaining > 0) || budget_grace_call`
- Each iteration: check interrupt, consume budget, build API messages, call API with retry, process response
- Tool calls: execute, append results, continue outer loop
- `finish_reason == "stop"`: return final response

**Inner loop** (API retries, max 3):
- Build API kwargs, call streaming or non-streaming API
- On success: break inner loop, process response
- On failure: classify error, retry/backoff/fallback/compress

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
    threshold: u64,            // 75% of context window
    auxiliary_client: ApiClient,  // cheaper model for summarization
    summary_ratio: f64,        // 0.20 (20% of original size)
    token_range: (u64, u64),   // (2000, 12000)
    cooldown: Duration,        // 10min failure cooldown
}

impl ContextCompressor {
    pub fn should_compress(&self, messages: &[Message], system_prompt: &str) -> bool { ... }
    pub async fn compress(&self, messages: &mut Vec<Message>) -> Result<()> {
        // 5-step algorithm:
        // 1. Count tokens, check threshold (75%)
        // 2. Identify middle turns for summarization
        // 3. Protect first 3 turns + last 6 turns
        // 4. Generate tool-specific summaries (15+ tool types)
        // 5. Iterative update until within 12K ceiling
    }
}
```

**Tool-specific summary strategies**:
| Tool Type | Summary Approach |
|-----------|-----------------|
| terminal | Command + exit code + first/last 10 lines |
| read_file | File path + line count + first 5 lines |
| write_file | File path + byte count |
| patch | File path + lines added/removed |
| web_search | Query + result count + top 3 titles |
| browser | URL + page title + snapshot summary |
| memory | Query + memory count + top matches |
| skill_view | Skill name + readiness status |
| MCP tools | Server + tool name + result preview |

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

#### Detailed Conversation Loop Behaviors (from Spec 15/40)

**Per-turn initialization** at start of each `run_conversation()`:
- Restore primary runtime if previous turn activated fallback
- Sanitize surrogates from user input (U+D800..U+DFFF → U+FFFD)
- Reset all retry counters (invalid_tool, invalid_json, empty_content, scratchpad, codex_incomplete, thinking_prefill, post_tool_empty, mute_post_response, unicode_sanitization_passes)
- Dead connection cleanup for non-anthropic modes
- Replay compression warning through status_callback
- Create fresh IterationBudget instance
- Hydrate todo store from conversation history if empty
- Increment `_user_turn_count`, check memory nudge trigger

**System prompt caching**:
1. First turn: check SQLite for stored prompt, reuse if found; otherwise build from scratch
2. Subsequent turns: reuse cached prompt to preserve Anthropic cache prefix
3. Only rebuilt after context compression
4. Plugin hook `on_session_start` fires once on brand-new session creation

**Preflight context compression** before entering main loop:
- Estimate token count including tool schema tokens (20-30K+ with many tools)
- Run up to 3 compression passes
- Reset retry counters after compression
- Re-estimate after each pass, break when under threshold

**Plugin context injection** (`pre_llm_call` hook):
- Plugins return context dict with `context` key or plain string
- All injected context appended to user message (NOT system prompt) — preserves prompt cache prefix
- All injected context is ephemeral (not persisted to session DB)

**External memory provider prefetch**:
- Call `prefetch_all(user_message)` once before tool loop
- Cache result in `_ext_prefetch_cache` — reused on every iteration
- Uses `original_user_message` (clean input)

**Message preparation for API**:
- Current-turn user message: inject memory prefetch context (fenced) + plugin context
- Assistant messages: copy `reasoning` to `reasoning_content`, then remove `reasoning` field
- Remove `finish_reason`, `_thinking_prefill` internal marker
- For strict providers: sanitize tool call fields (remove Codex-specific fields)
- Keep `reasoning_details` for OpenRouter multi-turn reasoning context

**System message assembly**: `effective_system = active_system_prompt + ephemeral_system_prompt`
- Ephemeral additions are API-call-time only
- External recall context goes into user message, not system prompt

**Prefill messages**:
- Inserted right after system prompt but before conversation history
- Never stored in messages list
- Automatically re-applied on every API call

**Message sanitization** before sending to API:
- Strip orphaned tool results / add stubs for missing results
- Normalize whitespace on all message content
- Normalize tool-call JSON: `json.dumps(args, separators=(",",":"), sort_keys=True)`

**Recovery decision tree** (10-step):
1. UnicodeEncodeError (surrogates/ASCII codec) → sanitize → retry (max 2 passes)
2. Credential pool rotation → if recovered, continue retry loop
3. Codex/Nous/Anthropic auth refresh (401) → refresh → retry (once each)
4. Thinking signature invalid → strip reasoning_details → retry (once)
5. Rate limit (429/billing) → check credential pool → if pool can recover, retry; else eagerly fallback
6. Payload too large (413) → compress context (max 3 attempts)
7. Context length error → parse actual limit → step down context_length OR reduce max_tokens → compress
8. Anthropic long-context tier (429) → reduce to 200K → compress
9. Non-retryable client error → try fallback → if no fallback, abort with error dump
10. Max retries exhausted → try transport recovery → try fallback → abort

**Output cap adjustment** when `max_tokens` too large:
- Parse `available_output_tokens` from error message
- Set `_ephemeral_max_output_tokens = available - 64`
- Retry without touching `context_length`

**Thinking budget exhaustion detection** when `finish_reason == "length"`:
- Check if response has think tags but no visible content after them
- Only flag when model produced reasoning blocks but no text
- Models without think tags treated as normal truncations

**Truncated tool call recovery** when `finish_reason == "length"` with tool calls:
- Retry API call once (don't append broken response)
- If still truncated → refuse to execute incomplete tool arguments

**Length continuation** when `finish_reason == "length"` without tool calls:
- Append partial assistant message, send continuation prompt
- Retry up to 3 times, then return partial response

**Transport recovery** before falling back on max retries:
- `_try_recover_primary_transport()` rebuilds HTTP client
- Cleans up dead connections in connection pool
- One-shot attempt per API call block

**Interrupt system**:
- `interrupt(message)` sets `_interrupt_requested = True`
- Propagates to all active child agents (subagent delegation)
- Checked at: outer loop start, retry backoff waits (every 200ms), API call (via InterruptedError), error handling
- `clear_interrupt()` resets flags at start of each `run_conversation()`

**Streaming**: Always prefers streaming
- 90s stale-stream detection, 60s read timeout
- Falls back to non-streaming if provider doesn't support it
- `on_first_delta` callback fires on first token

**Ollama context injection**:
- Detect via `is_local_endpoint(base_url)`
- Query `/api/show` for model's max context
- Pass `num_ctx` on every chat request
- User override: `model.ollama_num_ctx` in config.yaml

**Dual-path persistence**:
1. JSON log file: atomic write to session log (~/.hermes/sessions/session_<id>.json)
2. SQLite database: incremental message flush with dedup
- Guard: never overwrite larger log with fewer messages
- Skip persistence on context overflow (status 400 + large session)

**Context pressure tiered warnings**:
- At 85% of compaction threshold: first warning
- At 95% of compaction threshold: second warning

**Compression feasibility check** at init time:
- If aux context < threshold: warn with fix options
- Warning stored and replayed through status_callback on first run_conversation

**Context engine plugin system**:
- Config: `context.engine` in config.yaml (default: "compressor")
- Try plugins/context_engine/<name>/, then general plugin system, then fall back
- Lifecycle: `update_model()`, `get_tool_schemas()`, `on_session_start()`, `on_session_reset()`, `on_session_end()`

**Minimum context length**: Reject models with context window below 64K tokens

**Background review system**:
- Two triggers: memory review (every `_memory_nudge_interval` user turns) + skill review (every `_skill_nudge_interval` iterations)
- Forked agent: same model/provider/tools, max_iterations=8, quiet_mode=True
- Scans session messages for tool results with success: true
- Extracts action descriptions: "created", "updated", "added", "removed", "replaced"

**Stream consumer system**:
- Registered for: CLI TUI display (token-by-token), TTS pipeline, gateway platform callbacks
- `_has_stream_consumers()` affects thinking spinner, vprint suppression
- Stale stream detection: 90s without data

**Activity monitoring**:
- `_touch_activity(desc)` updates last activity timestamp (thread-safe)
- Called at: API call start, API call complete, error recovery, backoff waits
- `get_activity_summary()` returns: last_activity_ts, seconds_since_activity, current_tool, api_call_count, budget used/max

**Thinking block handling**:
- Extract reasoning from multiple provider formats: `message.reasoning`, `message.reasoning_content`, `message.reasoning_details`, inline think blocks
- `_strip_think_blocks()` removes all reasoning tag variants
- `_has_content_after_think_block()` checks for visible text after think blocks

**Context length resolution** priority: config.yaml > custom_providers per-model > auto-detection > default 128K

**Budget refund on compression**: When context compression triggers restart: `api_call_count -= 1`, `iteration_budget.refund()`, `retry_count += 1`

**Context compression configuration** (YAML):
```yaml
compression:
  enabled: true
  threshold: 0.50
  target_ratio: 0.20
  protect_last_n: 20
```

#### Checklist
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
- [ ] Implement per-turn initialization (retry counter reset, todo hydration, turn count)
- [ ] Implement system prompt caching (SQLite-stored, preserve Anthropic cache prefix)
- [ ] Implement preflight context compression (up to 3 passes before main loop)
- [ ] Implement external memory provider prefetch (cache for all iterations)
- [ ] Implement message preparation for API (reasoning copy, field sanitization, Codex cleanup)
- [ ] Implement prefill messages (never stored, auto-reapplied)
- [ ] Implement message sanitization (orphaned tool results, whitespace, tool-call JSON normalization)
- [ ] Implement recovery decision tree (10-step: surrogates, credentials, auth, thinking, rate limit, compression, context length, fallback, transport)
- [ ] Implement output cap adjustment (parse available_output_tokens from error)
- [ ] Implement thinking budget exhaustion detection
- [ ] Implement truncated tool call recovery
- [ ] Implement length continuation (3 retries)
- [ ] Implement transport recovery (HTTP client rebuild, dead connection cleanup)
- [ ] Implement interrupt propagation to subagents
- [ ] Implement streaming with stale stream detection (90s, 60s read timeout)
- [ ] Implement Ollama context injection (/api/show, num_ctx)
- [ ] Implement dual-path persistence (JSON log + SQLite flush with dedup)
- [ ] Implement context pressure tiered warnings (85%, 95%)
- [ ] Implement compression feasibility check
- [ ] Implement context engine plugin system
- [ ] Implement minimum context length check (64K)
- [ ] Implement background review system (memory + skill triggers)
- [ ] Implement stream consumer system (TUI, TTS, gateway)
- [ ] Implement activity monitoring (touch_activity, get_activity_summary)
- [ ] Implement thinking block handling (multi-format extraction, stripping, content check)
- [ ] Implement context length resolution (config > custom > auto-detect > 128K)
- [ ] Implement budget refund on compression
- [ ] Implement context compression YAML config (threshold, ratio, protect_last_n)
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

### Phase 8: Terminal Backends (Environments) & Code Execution Sandbox

**Goal:** Implement all 6 terminal backends, code execution sandbox, and process registry.

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
| Local | `local.rs` | Direct `subprocess.Popen` with PTY |
| Docker | `docker.rs` | Container lifecycle, volume mounts, CWD remapping |
| SSH | `ssh.rs` | paramiko SSH with persistent shell |
| Modal | `modal.rs` | Modal cloud sandbox, managed gateway support |
| Daytona | `daytona.rs` | Daytona sandbox API |
| Singularity | `singularity.rs` | Singularity/Apptainer exec with scratch dir |

#### Terminal Configuration (20+ env vars)

| Variable | Default | Purpose |
|----------|---------|---------|
| `TERMINAL_ENV` | `local` | Backend type |
| `TERMINAL_TIMEOUT` | `180` | Command timeout (seconds) |
| `TERMINAL_LIFETIME_SECONDS` | `300` | Sandbox lifetime |
| `TERMINAL_DOCKER_IMAGE` | `nikolaik/python-nodejs:python3.11-nodejs20` | Default container image |
| `TERMINAL_CONTAINER_CPU` | `1` | CPU allocation |
| `TERMINAL_CONTAINER_MEMORY` | `5120` | MB (5 GB) |
| `TERMINAL_CONTAINER_DISK` | `50200` | MB (50 GB) |
| `TERMINAL_MAX_FOREGROUND_TIMEOUT` | `600` | Hard cap (10 min) |
| `TERMINAL_DISK_WARNING_GB` | `500` | Disk usage warning |

#### CWD Handling
- **Local**: Host's current directory
- **SSH**: Starts in `~`
- **Containers/Modal**: Starts in `/root`
- Docker with `TERMINAL_DOCKER_MOUNT_CWD_TO_WORKSPACE=true`: Host path → `/workspace`
- Host/relative paths rejected for container backends

#### Code Execution Sandbox (`crates/tools/src/code_execution.rs`)

Two transports:
1. **Local (UDS)**: Unix domain socket RPC, `hermes_tools.py` stub module
2. **Remote (file-based)**: Request/response files, base64 encoding for shell safety

**Resource limits**:
| Limit | Value |
|-------|-------|
| Timeout | 300s (5 min) |
| Max tool calls | 50 per script |
| Max stdout | 50,000 bytes |
| Max stderr | 10,000 bytes |

**Allowed tools (7)**: web_search, web_extract, read_file, write_file, search_files, patch, terminal

**Blocked terminal params in sandbox**: background, pty, notify_on_complete, watch_patterns

#### Task Environment Overrides

`register_task_env_overrides(task_id, overrides)` allows per-task sandbox settings
(custom Dockerfile, image, cwd) before the agent loop starts.

#### Cleanup Thread

Background thread periodically checks `_last_activity`, reaps sandboxes past
`TERMINAL_LIFETIME_SECONDS`.

#### Disk Usage Warning

Scans `hermes-*` directories in scratch dir, warns when total exceeds
`TERMINAL_DISK_WARNING_GB` threshold.

#### File Sync (`crates/envs/src/file_sync.rs`)

```rust
pub async fn sync_up(env: &mut dyn Environment, local: &Path, remote: &Path) -> Result<()> { ... }
pub async fn sync_down(env: &mut dyn Environment, remote: &Path, local: &Path) -> Result<()> { ... }
```

Remote file shipping: `echo | base64 -d` (reliable across all backends including Modal).

#### Checklist

- [ ] Implement Local environment (direct process spawn with PTY, sudo rewriting)
- [ ] Implement Docker environment (container lifecycle, volume mounts, CWD remapping, env forwarding)
- [ ] Implement SSH environment (paramiko, persistent shell, SCP file sync)
- [ ] Implement Modal environment (cloud sandbox, managed gateway, backend resolution)
- [ ] Implement Daytona environment (Daytona sandbox API)
- [ ] Implement Singularity environment (Apptainer exec, scratch dir)
- [ ] Implement code execution sandbox (UDS transport for local, file-based RPC for remote)
- [ ] Implement `hermes_tools.py` module generator (7-tool stubs, transport header)
- [ ] Implement RPC server loop (UDS: allow-list, max calls, param stripping)
- [ ] Implement RPC poll loop (remote: file polling, base64 encoding, adaptive polling 50ms→250ms)
- [ ] Implement file shipping to remote (base64 encoding for Modal compatibility)
- [ ] Implement file sync (SCP/SFTP)
- [ ] Implement environment router (select backend from config)
- [ ] Implement cleanup_vm() for all backends
- [ ] Implement is_persistent_env() detection
- [ ] Implement task environment overrides registration
- [ ] Implement cleanup thread (sandbox lifetime enforcement)
- [ ] Implement disk usage warning (hermes-* directory scanning)
- [ ] Unit tests: local command execution, Docker container lifecycle, UDS RPC round-trip

---

### Phase 9: Memory System & Skills System

**Goal:** Implement persistent memory (7 plugin backends), skills (3-tier progressive disclosure), and Skills Hub.

**Dependencies:** Phase 1 (core types), Phase 4 (query loop), Phase 5 (session store).

#### Memory Provider ABC (`crates/core/src/memory.rs`)

```rust
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryResult>>;
    async fn save(&self, key: &str, content: &str, metadata: serde_json::Value) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn prefetch(&self, query: &str) -> Result<String>;
}
```

#### Memory Plugin Backends

| Backend | Retrieval Strategy | Notes |
|---------|-------------------|-------|
| Holographic | FTS5 + Jaccard + HRR | SQLite fact store, trust scoring +0.05/-0.10, temporal decay, memory banks |
| Honcho | Cross-session user modeling | 4 tools: profile/search/context/conclude |
| OpenViking | External API | |
| RetainDB | External API | |
| Supermemory | External API | |
| Hindsight | External API | |
| Byterover | External API | |

#### Holographic Memory Details

- SQLite with FTS5 full-text search
- Jaccard similarity for token overlap
- Hypercomplex Ring Retrieval (HRR) for semantic matching
- Trust scoring: +0.05 for confirmed, -0.10 for rejected
- Temporal decay: older memories score lower
- Memory banks: separate stores for different memory types

#### Memory File System (~/.hermes/memory/)

- `MEMORY.md` index file with frontmatter: name, description, type (user/feedback/project/reference)
- Semantic file organization: `user_*.md`, `feedback_*.md`, `project_*.md`, `reference_*.md`
- Two-step save: write memory file → update MEMORY.md index
- Max 200 lines in MEMORY.md (lines after 200 truncated)

#### Skills System (`crates/core/src/skills.rs`)

```rust
pub struct Skill {
    pub name: String,          // max 64 chars
    pub description: String,   // max 1024 chars
    pub version: String,       // optional
    pub instructions: String,  // SKILL.md body
    pub platforms: Vec<String>,  // optional: macos, linux, windows
    pub prerequisites: SkillPrerequisites,
    pub required_env_vars: Vec<EnvVarRequirement>,
    pub enabled: bool,
}
```

#### 3-Tier Progressive Disclosure

| Tier | Tool | Content | Token Cost |
|------|------|---------|------------|
| 1 | `skills_list` | name + description only | Minimal |
| 2 | `skill_view(skill)` | Full SKILL.md instructions | Moderate |
| 3 | `skill_view(skill, "references/file.md")` | Linked reference files | On demand |

#### SKILL.md Format

YAML frontmatter with: name, description, version, license, platforms, prerequisites,
compatibility, metadata, setup (help + collect_secrets), required_environment_variables.

Content directories: `references/`, `templates/`, `assets/`.

#### Platform Filtering

```rust
const PLATFORM_MAP: &[(&str, &str)] = &[
    ("macos", "darwin"),
    ("linux", "linux"),
    ("windows", "win32"),
];
```

Skills with `platforms` frontmatter that doesn't match are excluded from `skills_list`.

#### Skills Directory

Single source: `~/.hermes/skills/`. Bypasses `.git`, `.github`, `.hub` directories.
External skills dirs from `get_external_skills_dirs()`.

#### Prompt Injection Detection (9 patterns)

```rust
const INJECTION_PATTERNS: &[&str] = &[
    "ignore previous instructions",
    "ignore all previous",
    "you are now",
    "disregard your",
    "forget your instructions",
    "new instructions:",
    "system prompt:",
    "<system>",
    "]]>",
];
```

#### Secret Capture

`_secret_capture_callback` prompts for missing environment variables:
1. `_find_all_skills()` identifies skills with `required_environment_variables`
2. `skills_list()` checks which requirements are unsatisfied
3. If gateway surface: returns `gateway_setup_hint`
4. Otherwise: invokes callback for each missing secret

#### Disabled Skills

- `config.yaml` → `skills.disabled: ["skill-name"]` (global)
- `config.yaml` → `skills.platform_disabled.linux: ["skill-name"]` (per-platform)
- Excluded from `skills_list` but visible in `hermes skills` config UI

#### Skills Hub (`crates/commands/src/skills_hub.rs`)

4 source adapters: GitHub, WellKnown, SkillsSh, Official.
SkillMeta / SkillBundle data models. GitHubAuth with 4-method fallback.
HubLockFile for version tracking. Quarantine + audit for new installs.

#### Checklist

- [ ] Implement MemoryProvider trait and MemoryManager
- [ ] Implement Holographic memory (FTS5 + Jaccard + HRR, trust scoring, temporal decay)
- [ ] Implement memory file discovery and loading (~/.hermes/memory/)
- [ ] Implement MEMORY.md index parsing and update
- [ ] Implement memory prefetch with query relevance scoring
- [ ] Implement memory save/create/update/delete
- [ ] Implement Honcho integration stub (4 tools)
- [ ] Implement skill discovery (~/.hermes/skills/, external dirs)
- [ ] Implement SKILL.md parsing (YAML frontmatter, body, references)
- [ ] Implement 3-tier progressive disclosure (skills_list, skill_view, references)
- [ ] Implement platform filtering (macos/linux/windows)
- [ ] Implement prompt injection detection (9 patterns)
- [ ] Implement secret capture callback
- [ ] Implement disabled skills filtering (global + per-platform)
- [ ] Implement skill readiness status (available, setup_needed, unsupported)
- [ ] Implement Skills Hub search/install/list (4 source adapters)
- [ ] Implement skill versioning and quarantine
- [ ] Implement skills system prompt injection into query loop
- [ ] Unit tests: memory CRUD, skill loading, platform filtering, injection detection

---

### Phase 10: MCP Client

**Goal:** Implement MCP (Model Context Protocol) client with dynamic tool discovery, security scanning, and OAuth.

**Dependencies:** Phase 3 (tool framework).

#### MCP Client (`crates/mcp/src/lib.rs`)

```rust
pub struct McpClient {
    servers: HashMap<String, McpServer>,
}

pub enum McpTransport {
    Stdio { command: String, args: Vec<String>, env: HashMap<String, String> },
    Http { url: String },
}
```

#### Transports

- **Stdio**: Spawn process, communicate via stdin/stdout (JSON-RPC)
- **HTTP**: Server-Sent Events for notifications, POST for requests

#### Sampling (LLM calls from MCP servers)

- Rate limiting: 10 RPM default
- Model allowlist: only approved models can be used
- User approval required for sampling requests

#### Dynamic Tool Discovery

- MCP servers register tools via `tools/list`
- `notifications/tools/list_changed` → refresh tool list
- Server disconnect → deregister all tools from that server
- Shadow prevention: MCP tools cannot overwrite built-in tool names

#### Security

- **Environment filtering**: strip sensitive env vars from MCP server environment
- **Credential stripping**: remove API keys, tokens from MCP config
- **Injection scanning**: 10 injection patterns scanned from MCP tool descriptions
- **OSV malware check**: npm/PyPI package scanning, 10s timeout, fail-open

#### OAuth 2.1 PKCE

- Authorization Code Flow with PKCE for authenticated MCP servers
- Token refresh and storage

#### Retry Behavior

- 3 initial retries for failed requests
- 5 reconnect retries for dropped connections

#### MCP Config CLI (`hermes mcp list|add|remove|status`)

#### Checklist

- [ ] Implement MCP Stdio transport (spawn process, JSON-RPC via stdin/stdout)
- [ ] Implement MCP HTTP transport (SSE + POST)
- [ ] Implement tool list fetching and registration
- [ ] Implement tool calling via MCP
- [ ] Implement resource listing and reading
- [ ] Implement prompt support
- [ ] Implement dynamic tool discovery (notifications/tools/list_changed)
- [ ] Implement shadow prevention (MCP can't overwrite built-in tools)
- [ ] Implement sampling with rate limiting (10 RPM, model allowlist)
- [ ] Implement security: env filtering, credential stripping, injection scanning
- [ ] Implement OSV malware check (npm/PyPI, 10s timeout, fail-open)
- [ ] Implement OAuth 2.1 PKCE flow for authenticated MCP servers
- [ ] Implement MCP server lifecycle management (3 initial retries, 5 reconnect retries)
- [ ] Implement MCP config CLI (hermes mcp list/add/remove/status)
- [ ] Unit tests: MCP server connection, tool call round-trip, dynamic discovery

---

### Phase 11: Messaging Gateway & Platform Adapters

**Goal:** Implement the gateway process (GatewayRunner), session management, and all 20+ platform adapters.

**Dependencies:** Phase 4 (query loop), Phase 5 (session store).

#### GatewayRunner (`crates/gateway/src/runner.rs`)

Core state:
- `_running_agents`: active agent sessions
- `_agent_cache`: session_key → (AIAgent, config_signature)
- `_session_model_overrides`: per-session model overrides
- `_pending_approvals`: approval waiting queue

**Startup lifecycle** (15 steps):
1. SSL cert auto-detection
2. Config bridging (platform env vars → config)
3. Platform platform lock acquisition
4. Adapter initialization sequence
5. Session expiry watcher (300s)
6. Platform reconnect watcher (10s)
7. BOOT.md hook execution
8. PID file with scoped locks

**Message processing pipeline**:
1. Authorization check
2. Staleness eviction
3. Running agent intercept (spawn new query loop or queue)
4. Command dispatch (approve/deny/status/stop/new/reset/background/brestart bypass active-session guard)

**Agent caching**: `session_key → (AIAgent, config_signature)`. Cache hit avoids re-initialization.

**Busy input modes (3)**: interrupt, queue, reject.

**Session suspension**: pause active session, resume later.

**Stuck-loop detection**: 3 restarts → auto-stop.

**Drain timeout**: 30s graceful shutdown.

**Exit code 42**: signals service restart.

**ContextVars (7 task-local variables)**: current session, platform, chat_id, etc.

**Session mirroring**: mirror sessions across platforms for cross-platform continuity.

#### Platform Adapter Trait (`crates/gateway/src/platforms/base.rs`)

```rust
#[derive(Debug, Clone)]
pub enum MessageType {
    Text, Location, Photo, Video, Audio, Voice, Document, Sticker, Command,
}

#[derive(Debug, Clone)]
pub struct MessageEvent {
    pub text: String,
    pub message_type: MessageType,
    pub source: SessionSource,
    pub media_urls: Vec<String>,   // local file paths
    pub media_types: Vec<String>,
    pub reply_to_message_id: Option<String>,
    pub reply_to_text: Option<String>,
    pub auto_skill: Option<String>,
    pub internal: bool,            // synthetic events bypass auth
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct SendResult {
    pub success: bool,
    pub message_id: Option<String>,
    pub error: Option<String>,
    pub retryable: bool,          // transient failure → auto-retry
}
```

#### Base Platform Adapter ABC

**Abstract methods**: connect, disconnect, send, get_chat_info

**Optional overrides**: edit_message, send_typing, stop_typing, send_image, send_animation, send_voice, play_tts, send_video, send_document, send_image_file

#### Background Processing Pipeline (16 steps)

1. Start continuous typing indicator (`_keep_typing` — refreshes every 2s)
2. Fire `on_processing_start` hook
3. Call `_message_handler(event)` → response string
4. Extract `MEDIA:<path>` tags (from TTS tool)
5. Extract image URLs (markdown `![alt](url)` and `<img src="...">`)
6. Extract local file paths (bare `/path/to/image.png`)
7. Auto-TTS for voice input (if not disabled via `/voice off`)
8. Play TTS audio before text (voice-first experience)
9. Send text with retry (`_send_with_retry`)
10. Human-like pacing delay between text and media
11. Send images/animations natively
12. Send media files (routed by extension)
13. Send local files natively
14. Fire `on_processing_complete` hook
15. Check for pending messages from interrupt → recurse
16. Cancel typing indicator in `finally`

#### Retry System

- Max retries: 2, base delay: 2.0s with exponential backoff + jitter
- **Retryable errors** (9 patterns): connecterror, connectionerror, connectionreset, connectionrefused, connecttimeout, network, brokenpipe, remotedisconnected, eoferror
- **NOT retryable**: timed out, readtimeout, writetimeout (retry risks duplicate delivery)
- Fallback: on formatting failure, send plain-text version with prefix
- Delivery failure notice after all retries exhausted

#### Message Truncation

- Preserves fenced code block boundaries
- Reopens code fences with original language tag in next chunk
- Multi-chunk responses get `(1/3)` indicators
- UTF-16 length for Telegram (emoji = 2 units)
- Avoids splitting inside inline code spans

#### Human-Like Pacing

- `HERMES_HUMAN_DELAY_MODE`: off (default) | natural | custom
- Natural range: 800-2500ms
- Custom: `HERMES_HUMAN_DELAY_MIN_MS` / `HERMES_HUMAN_DELAY_MAX_MS`

#### Media Cache

- **Directories**: `~/.hermes/cache/images/`, `cache/audio/`, `cache/documents/`
- **Safety**: SSRF protection, redirect guard re-validates each 302, magic-byte validation, path traversal protection
- **Auto-cleanup**: 24h TTL
- **Retry**: exponential backoff on transient CDN failures

#### Proxy Support

Check order: platform-specific env var → HTTPS_PROXY/HTTP_PROXY/ALL_PROXY → macOS system proxy via `scutil --proxy`
SOCKS support with `rdns=true` for remote DNS resolution.

#### Event Hooks (7 events + wildcard)

`on_session_start`, `on_session_end`, `on_processing_start`, `on_processing_complete`, `on_message_received`, `on_message_sent`, `on_error` + wildcard `*`.

#### Delivery Router

Targets: `origin`, `local`, `platform:reference[:thread_id]`

#### Session Store (Gateway)

- SessionSource, SessionContext
- PII redaction for 4 safe platforms
- Voice mode persistence (per-chat voice on/off)
- Pre-reset memory flush

#### Platform Adapters

| Platform | Library | Notes |
|----------|---------|-------|
| Telegram | teloxide / tgram | MarkdownV2, media batch delays (0.8s photo, 0.6s text), forum topics, IP fallback transport |
| Discord | serenity | Voice receiver (RTP decryption, Opus decoding, 48kHz stereo), thread management, message dedup |
| Slack | slack-rust | Bolt framework, Socket Mode, mrkdwn conversion, approval buttons, multi-workspace |
| WhatsApp | HTTP bridge (Node.js) | HTTP polling, 100MB attachment |
| Signal | signal-cli subprocess | SSE + JSON-RPC, 100MB attachment |
| Matrix | matrix-sdk | E2EE with Olm/Megolm, pending_megolm buffer (100 entries/300s TTL) |
| Feishu/Lark | reqwest | WebSocket + Webhook, 10+ message types, per-chat serial processing, interactive approval cards, 3-layer webhook security |
| QQBot | reqwest | Op-code protocol, SILK audio decoding, 3-STT config sources |
| DingTalk | reqwest | Stream Mode SDK |
| Mattermost | aiohttp | Pure WebSocket, mention gating |
| WeCom | reqwest | Subscribe/callback/send protocol, chunked upload (512KB) |
| Weixin | reqwest | iLink API, AES-128-ECB CDN, QR login, markdown normalization |
| BlueBubbles | reqwest | Webhook + REST, Tapback reactions |
| Email | lettre | IMAP/SMTP polling, UID tracking (2000 max) |
| Home Assistant | reqwest | WebSocket state events, cooldown 30s |
| Webhook | axum | HMAC validation, 1MB body limit, 30/min rate limit |
| SMS | HTTP API | Twilio REST, HMAC-SHA1 signature, 1600 char limit |
| REST API | axum | Streaming responses |

#### DM Pairing (`crates/gateway/src/pairing.rs`)

Pair code system for group/channel → DM routing.

#### Gateway Service Management (`crates/cli/src/gateway.rs`)

Commands: `run` (foreground), `start` (background service), `stop`, `restart`, `status`, `install` (systemd/launchd), `uninstall`, `setup`.

| Platform | Service Manager | Label |
|----------|----------------|-------|
| Linux (systemd) | `systemctl --user` | `hermes-gateway.service` |
| macOS (launchd) | `launchctl` | `com.nousresearch.hermes-gateway` |
| Windows | Manual PID tracking | — |

**Restart with drain** (30s `DEFAULT_GATEWAY_RESTART_DRAIN_TIMEOUT`):
1. Signal existing gateway to stop accepting new messages
2. Wait for in-progress sessions to complete
3. Kill remaining processes
4. Start new gateway (exit code `GATEWAY_SERVICE_RESTART_EXIT_CODE = 42`)

**Process sweeping**: After service restart, sweeps for stale manual gateway processes, excluding service-managed PIDs.

#### Batch Runner (`crates/gateway/src/batch_runner.rs`)

Parallel batch processing of agent across multiple prompts from a dataset:
- `multiprocessing.Pool → AIAgent × N workers`
- Rich progress bar
- JSONL trajectory output
- `--distribution` flag for toolset distributions

#### Trajectory Compressor (`crates/gateway/src/trajectory.rs`)

Compresses trajectory JSONL files for training data:
- Sliding window compression with protected last 4 turns
- Uses OpenRouter with `google/gemini-3-flash-preview` for summarization
- Target: 15,250 max tokens, 750 summary tokens
- Sampling support (`--sample_percent`)
- Rich progress bars with spinner, bar, time remaining

#### MCP Serve (`crates/mcp/src/serve.rs`)

MCP server exposing Hermes tools via stdio transport:
- JSON-RPC 2.0 messages over stdin/stdout
- Tool definitions via `tools/list`
- Tool execution via `tools/call`
- Exposes: file tools, terminal, web search/extract, memory

#### Toolset Distributions (`crates/core/src/toolset_distributions.rs`)

Probabilistic toolset sampling for RL training:
```rust
// DISTRIBUTIONS: "default", "image_gen", "web_research", etc.
pub fn sample_toolsets(distribution: &str) -> Vec<String>;
pub fn list_distributions() -> Vec<String>;
```

Used by batch runner (`--distribution` flag), RL environments, and `run_agent` (`enabled_toolsets`).

#### Hermes Logging (`crates/core/src/logging.rs`)

Centralized logging:
- `agent.log` (INFO+) — full conversation trace
- `errors.log` (WARNING+) — errors only
- Session context filtering: `hermes logs --session <id>`
- Thread-safe with session ID tagging via `set_session_context()`

#### Checklist

- [ ] Implement `PlatformAdapter` trait (connect, disconnect, send, get_chat_info)
- [ ] Implement `MessageEvent`, `MessageType` (9 types), `SendResult`
- [ ] Implement `GatewayRunner` lifecycle management (15-step startup, PID file, scoped locks)
- [ ] Implement agent caching (session_key → (AIAgent, config_signature))
- [ ] Implement message processing pipeline (authorization, staleness, intercept, dispatch)
- [ ] Implement 16-step background processing pipeline
- [ ] Implement retry system (2 retries, exponential backoff, 9 retryable patterns)
- [ ] Implement typing indicator (2s refresh, pause during approval waits)
- [ ] Implement message truncation (code block preservation, UTF-16 length)
- [ ] Implement media cache (3 types, SSRF protection, 24h TTL)
- [ ] Implement media extraction (MEDIA: tags, image URLs, local files)
- [ ] Implement proxy support (env vars, SOCKS rdns, macOS system proxy)
- [ ] Implement human-like pacing (800-2500ms)
- [ ] Implement event hooks (7 events + wildcard)
- [ ] Implement delivery router (origin/local/platform targets)
- [ ] Implement session store (SessionSource, SessionContext, PII redaction)
- [ ] Implement busy input modes (3: interrupt, queue, reject)
- [ ] Implement stuck-loop detection (3 restarts → auto-stop)
- [ ] Implement drain sequence (30s graceful shutdown, exit code 42)
- [ ] Implement ContextVars (7 task-local variables)
- [ ] Implement session mirroring
- [ ] Implement BOOT.md hook
- [ ] Implement DM pairing system
- [ ] Implement Telegram adapter (MarkdownV2, media batch delays, forum topics, IP fallback)
- [ ] Implement Discord adapter (voice receiver, RTP decryption, Opus decoding, thread management)
- [ ] Implement Slack adapter (Socket Mode, mrkdwn, approval buttons)
- [ ] Implement WhatsApp adapter (HTTP bridge)
- [ ] Implement Signal adapter (signal-cli, SSE + JSON-RPC)
- [ ] Implement Matrix adapter (E2EE, Olm/Megolm, pending buffer)
- [ ] Implement Feishu/Lark adapter (WebSocket + Webhook, approval cards)
- [ ] Implement QQBot adapter (op-code, SILK audio)
- [ ] Implement DingTalk adapter (Stream Mode)
- [ ] Implement Mattermost adapter (WebSocket, mention gating)
- [ ] Implement WeCom adapter (chunked upload)
- [ ] Implement Weixin adapter (iLink API, AES-128-ECB)
- [ ] Implement BlueBubbles adapter (Tapback reactions)
- [ ] Implement Email adapter (IMAP/SMTP, UID tracking)
- [ ] Implement Home Assistant adapter (WebSocket, cooldown)
- [ ] Implement Webhook adapter (HMAC, rate limit)
- [ ] Implement SMS adapter (Twilio, HMAC-SHA1)
- [ ] Implement REST API server (axum, streaming)
- [ ] Implement channel directory tracking
- [ ] Implement display config per platform
- [ ] Implement gateway status reporting
- [ ] Implement gateway hooks (pre/post message)
- [ ] Implement platform lock (prevent multiple instances with same bot token)
- [ ] Implement fatal error tracking and health monitoring
- [ ] Implement message deduplicator (2000 entries/300s TTL)
- [ ] Implement text batch aggregator (0.6s/2.0s delays)
- [ ] Implement gateway service management (systemd/launchd, restart with drain, exit code 42)
- [ ] Implement process sweeping (stale process cleanup)
- [ ] Implement batch runner (parallel AIAgent workers, JSONL output, distribution flag)
- [ ] Implement trajectory compressor (sliding window, gemini-3-flash, 15,250 target tokens)
- [ ] Implement MCP serve (stdio transport, JSON-RPC, tool exposure)
- [ ] Implement toolset distributions (probabilistic sampling, named distributions)
- [ ] Implement hermes logging (agent.log + errors.log, session context filtering)
- [ ] Unit tests: platform adapter mock, stream consumer buffering, retry logic

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

### Phase 17: Browser Tool, TTS, Web Tools & send_message

**Goal:** Implement browser tool (5 backends), TTS (6 providers), web tools (4 backends), and cross-platform send_message.

**Dependencies:** Phase 3 (tool framework), Phase 11 (gateway).

#### Browser Tool (`crates/tools/src/browser.rs`)

5 backends:
| Backend | Notes |
|---------|-------|
| Local Chromium | puppeteer-controlled browser |
| Browserbase | Cloud browser platform |
| Browser Use | Open-source browser automation |
| Firecrawl | Web scraping + extraction API |
| Camofox | REST API on port 9377, VNC integration, SSRF protection |

10 tool schemas: browser_navigate, browser_click, browser_type, browser_screenshot, browser_snapshot, browser_close, browser_scroll, browser_evaluate, browser_vision, browser_hover.

**Features**:
- Accessibility tree snapshots (8000 char summarize threshold)
- SSRF protection (URL secret exfiltration blocking)
- browser_vision with annotation support
- Website policy: TTL cache 30s, fnmatch wildcard matching

#### TTS Tool (`crates/tools/src/tts.rs`)

6 providers:
| Provider | Notes |
|----------|-------|
| Edge TTS | Free, no API key |
| ElevenLabs | High quality, API key required |
| OpenAI TTS | API key required |
| MiniMax | Chinese-focused |
| Mistral TTS | API key required |
| NeuTTS | Local, fast |

**Features**:
- 4000 char limit per TTS call
- `MEDIA:<path>` tags in response for media delivery
- `[[audio_as_voice]]` directive for voice-first platforms
- Streaming TTS pipeline
- Voice mode CLI: 16kHz mono int16 audio

#### Web Tools (`crates/tools/src/web_tools.rs`)

4 backends:
| Backend | Purpose |
|---------|---------|
| Firecrawl | Web scraping + crawling |
| Exa | Semantic search |
| Parallel | Parallel web search |
| Tavily | AI-optimized search |

**Features**:
- LLM summarization with chunked processing (>500K chars)
- 2M char refusal limit
- SSRF + secret protection
- Managed tool gateway (Nous proxy, OAuth token resolution)

#### send_message (`crates/tools/src/send_message.rs`)

Cross-platform message delivery to 18+ platforms:
- Target format: `platform:reference[:thread_id]`
- Cron duplicate skip
- Session mirroring (send to all platforms in session)
- 18+ platform targets via gateway delivery router

#### Tirith Scanner (`crates/tools/src/tirith.rs`)

Rust CLI security scanner:
- Auto-install with cosign + SHA-256 verification
- 3 exit codes: allow (0), block (1), warn (2)
- 5s timeout
- Integrated into approval system as command guard

#### Checklist

- [ ] Implement browser tool trait and registry
- [ ] Implement Local Chromium backend
- [ ] Implement Browserbase backend
- [ ] Implement Browser Use backend
- [ ] Implement Firecrawl backend (web scraping)
- [ ] Implement Camofox backend (REST API, VNC)
- [ ] Implement accessibility tree snapshot (8000 char threshold)
- [ ] Implement SSRF protection and URL secret exfiltration blocking
- [ ] Implement browser_vision with annotation
- [ ] Implement website policy (TTL cache, fnmatch wildcard)
- [ ] Implement TTS tool trait
- [ ] Implement Edge TTS provider
- [ ] Implement ElevenLabs provider
- [ ] Implement OpenAI TTS provider
- [ ] Implement MiniMax, Mistral, NeuTTS providers
- [ ] Implement MEDIA: tag extraction and delivery
- [ ] Implement streaming TTS pipeline
- [ ] Implement voice mode CLI (16kHz mono int16)
- [ ] Implement web_search tool (Exa, Parallel, Tavily backends)
- [ ] Implement web_extract tool (Firecrawl backend)
- [ ] Implement LLM summarization for web results (chunked, 2M limit)
- [ ] Implement send_message tool (18+ platform targets)
- [ ] Implement Tirith scanner integration (auto-install, 3 exit codes)
- [ ] Unit tests: browser navigation, TTS generation, web search, send_message routing

---

### Phase 18: CLI Setup, Auth, Skin Engine & Configuration

**Goal:** Implement setup wizard, auth system (25+ providers), skin engine (10 built-in skins), and tools config.

**Dependencies:** Phase 1 (core types), Phase 7 (commands).

#### Setup Wizard (`crates/cli/src/setup.rs`)

6 sections, 5 modes. Two-phase OpenClaw migration:
1. Scan existing config
2. Apply migration with backup

#### Auth System (`crates/cli/src/auth.rs`)

25+ providers, 4 auth types:
| Auth Type | Providers |
|-----------|-----------|
| oauth_device | Anthropic, OpenAI, etc. |
| oauth_external | External OAuth providers |
| api_key | Direct API key input |
| external_process | Credential from external process |

**Features**:
- OAuth device code flow
- Auth store: cross-process fcntl locking, atomic writes with chmod 600
- Credential pool integration
- Endpoint probing (Z.AI 4 candidates, Kimi key routing)
- Auth error system
- Secret validation

#### Skin Engine (`crates/tui/src/skin_engine.rs`)

10 built-in skins (poseidon, sisyphus, charizard, etc.).
YAML schema with 20+ color keys.
prompt_toolkit 30+ style classes (mapped to ratatui theming).
Inheritance from default skin.

#### Runtime Provider Resolution

8-step pipeline:
1. API mode detection (3 modes)
2. Custom provider resolution
3. Nous credential resolution (configurable TTLs)
4. Account tier detection (180s TTL cache)
5. Canonical provider registry (25 providers, 50+ aliases)
6. Model catalog loading
7. Free-model filtering (Nous)
8. Smart model routing (40+ complexity keywords, 5 simple message criteria)

#### Tools Config (`crates/cli/src/tools_config.rs`)

18 configurable toolsets, 5 tool categories with providers.
Post-setup hooks. Platform toolset resolution.

#### Command Registry (`crates/commands/src/registry.rs`)

CommandDef dataclass, 5 categories.
Subcommand auto-extraction.
Gateway helpers.
Telegram/Discord 32-char command name limit, 100 max Telegram menu commands.

#### Model Catalogs (`crates/cli/src/model_catalogs.rs`)

Provider model catalogs. Nous free-model filtering. Account tier detection. Canonical provider registry.

#### Context References (`crates/core/src/context_refs.rs`)

6 types: @file, @folder, @git, @url, @diff, @staged.
Line range parsing. Security guards for 7 dirs + 12 files.

#### Checklist

- [ ] Implement setup wizard (6 sections, 5 modes)
- [ ] Implement OpenClaw migration (two-phase)
- [ ] Implement auth system (25+ providers, 4 auth types)
- [ ] Implement OAuth device code flow
- [ ] Implement auth store (fcntl locking, atomic writes, chmod 600)
- [ ] Implement endpoint probing (Z.AI, Kimi)
- [ ] Implement auth error system
- [ ] Implement secret validation
- [ ] Implement skin engine (10 built-in skins, YAML schema)
- [ ] Implement runtime provider resolution (8-step pipeline)
- [ ] Implement model catalogs (25 providers, 50+ aliases)
- [ ] Implement account tier detection (180s TTL)
- [ ] Implement smart model routing (40+ keywords)
- [ ] Implement tools config (18 toolsets, 5 categories)
- [ ] Implement command registry (CommandDef, 5 categories)
- [ ] Implement context references (6 types, security guards)
- [ ] Unit tests: auth flow, skin loading, model resolution

---

### Phase 19: Prompt Builder, Display, Insights & Rate Limiting

**Goal:** Implement system prompt assembly, display system, usage insights, and rate limit tracking.

**Dependencies:** Phase 4 (query loop), Phase 7 (commands).

#### Prompt Builder (`crates/query/src/prompt_builder.rs`)

System prompt assembly in 9-step order:
1. Agent identity (DEFAULT_AGENT_IDENTITY)
2. Personality (config or SOUL.md)
3. Platform hints
4. Tool usage guidance
5. Memory guidance
6. Session search guidance
7. Skills guidance
8. Context files (AGENTS.md, .cursorrules)
9. Skills system prompt

**Context threat scanning**: 10 patterns + 9 invisible Unicode chars.
**HERMES.md discovery**: scan for project-specific instructions.
**YAML frontmatter stripping**: remove metadata from context files.

#### Display System (`crates/core/src/display.rs`)

- Diff display (6 files max, 80 lines max per file)
- Tool preview
- Skin-aware theming
- Kawaii faces (status-dependent)

#### Usage Pricing (`crates/core/src/pricing.rs`)

CanonicalUsage dataclass with per-model pricing.
Per-million rates. CostResult with 4 status types (ok, estimated, unknown, error).

#### Insights Engine (`crates/commands/src/insights.rs`)

SQLite analysis. Token/cost/tool tracking.
Tool usage from two sources (session DB + API response).
Cost estimation from canonical usage data.

#### Model Metadata (`crates/api/src/model_metadata.rs`)

64K min context, 128K fallback.
5 probe tiers for context window detection.
1hr cache for metadata lookups.

#### Rate Limit Tracker (`crates/api/src/rate_limit.rs`)

12 header types parsed.
RateLimitBucket / RateLimitState dataclasses.
80% warning threshold.

#### Copilot ACP Client (`crates/acp/src/copilot.rs`)

JSON-RPC stdio. OAuth token management. 900s timeout.

#### Checklist

- [ ] Implement prompt builder (9-step assembly)
- [ ] Implement context threat scanning (10 patterns + 9 Unicode chars)
- [ ] Implement HERMES.md discovery
- [ ] Implement display system (diff, tool preview, skin theming)
- [ ] Implement usage pricing (CanonicalUsage, per-million rates)
- [ ] Implement insights engine (SQLite analysis, /insights command)
- [ ] Implement model metadata (5 probe tiers, 1hr cache)
- [ ] Implement rate limit tracker (12 headers, 80% warning)
- [ ] Implement Copilot ACP client (JSON-RPC, OAuth, 900s timeout)
- [ ] Unit tests: prompt assembly, pricing calculation, rate limit parsing

---

### Phase 20: RL Training Tool (Deferred)

**Goal:** Implement RL training management via Tinker-Atropos. Deferred to v2 as research-only feature.

**Dependencies:** Phase 3 (tool framework), Phase 8 (terminal backends).

#### RL Training Tool (`crates/tools/src/rl_training.rs`)

9 RL tools:
| Tool | Purpose |
|------|---------|
| `rl_list_environments` | Discover BaseEnv subclasses via AST scanning |
| `rl_select_environment` | Select env, load config fields |
| `rl_get_current_config` | Show configurable + locked fields |
| `rl_edit_config(field, value)` | Update one configurable field |
| `rl_start_training` | Spawn 3-process training pipeline |
| `rl_check_status` | Check training status (rate-limited 30 min) |
| `rl_stop_training` | Gracefully stop all 3 processes |
| `rl_get_results` | Fetch training results and metrics |

#### Environment Discovery

AST-based scanning of `.py` files in `environments/`:
- Parse each file, find classes inheriting `BaseEnv`
- Extract `name` class attribute and docstring
- Does NOT import module (avoids side effects)

#### 3-Process Training Pipeline

1. **Atropos API server** (`run-api`): Starts first, waits 5s
2. **Tinker trainer** (`launch_training.py --config`): Starts second, waits 30s for inference server on port 8001
3. **Environment** (`environment.py serve`): Starts third, waits 90s

Total startup delay: 125s.

#### Locked Configuration (18 fields)

Fields that cannot be changed by the model:
- `env.tokenizer_name` = `Qwen/Qwen3-8B`
- `env.rollout_server_url` = `http://localhost:8000`
- `env.use_wandb` = true
- `env.max_token_length` = 8192
- `env.max_num_workers` = 2048
- `env.worker_timeout` = 3600
- `env.total_steps` = 2500
- `env.steps_per_eval` = 25
- `env.max_batches_offpolicy` = 3
- `env.inference_weight` = 1.0
- `env.eval_limit_ratio` = 0.1
- `tinker.lora_rank` = 32
- `tinker.learning_rate` = 0.00004
- `tinker.max_token_trainer_length` = 9000
- `tinker.checkpoint_dir` = `./temp/`
- `tinker.save_checkpoint_interval` = 25
- `openai[0].model_name` = `Qwen/Qwen3-8B`
- `openai[0].base_url` = `http://localhost:8001/v1`
- `openai[0].server_type` = `sglang`

#### WandB Integration

- Auto-generates `wandb_name` as `{env_name}-{DATETIME}`
- Requires `WANDB_API_KEY` environment variable
- Monitors training metrics via WandB API

#### Required Environment Variables

| Variable | Purpose |
|----------|---------|
| `TINKER_API_KEY` | API key for Tinker service |
| `WANDB_API_KEY` | API key for Weights & Biases |

#### Checklist

- [ ] Implement RL tool trait and 9 tool schemas
- [ ] Implement AST-based environment discovery
- [ ] Implement config field introspection (Pydantic model_fields)
- [ ] Implement locked configuration enforcement (18 fields)
- [ ] Implement 3-process training pipeline (125s startup sequence)
- [ ] Implement status check rate limiting (30 min interval)
- [ ] Implement WandB integration
- [ ] Implement process cleanup (stop all 3 subprocesses)
- [ ] Unit tests: AST scanning, config validation, process lifecycle

---

## 4. Risk Assessment

### High Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| **API protocol diversity** — 15+ providers with subtly different response formats | Streaming parse failures, silent data loss | Strict schema validation per provider mode; extensive response parsing tests; fallback to non-streaming when streaming fails |
| **Terminal backend complexity** — 6 backends with different lifecycle models | Inconsistent tool execution behavior | Abstract `Environment` trait with thorough contract tests; shared test suite across all backends |
| **Platform adapter ecosystem** — 20+ adapters with different message formats, rate limits | Inconsistent user experience across platforms | `PlatformAdapter` trait with conformance tests; shared stream consumer; per-platform format normalization |
| **Credential management** — 25+ API keys across providers, OAuth flows | Security vulnerability, credential leakage | Credentials never logged; env-var-only for sensitive keys; encrypted config option; `hermes doctor` credential validation; atomic writes with chmod 600 |
| **Code execution sandbox security** — UDS/file RPC with 7-tool allowlist | Potential sandbox escape | Strict allow-list enforcement, blocked terminal param stripping, environment filtering, output size limits |

### Medium Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Context compression quality** — Auxiliary LLM summarization loses important context | Agent loses track of conversation | Preserve first 3 + last 6 turns uncompressed; preserve all tool results; tool-specific summaries for 15+ tool types; 10min failure cooldown |
| **FTS5 compatibility** — rusqlite FTS5 may differ from Python sqlite3 FTS5 | Search results differ between Python and Rust versions | Test against identical data; document any behavioral differences; use same tokenizer settings |
| **Tool parallelism bugs** — Race conditions in concurrent tool execution | Corrupted file state, interleaved output | Thread-safe ProcessRegistry; path overlap detection; sequential fallback for conflicting batches |
| **ratatui TUI limitations** — prompt_toolkit has richer input features | UX regression vs Python version | Prioritize core features (multiline, history, autocomplete); use ratatui's widget ecosystem for parity |
| **MCP dynamic tool conflicts** — Multiple servers with overlapping tool names | Tool registration failures | Shadow prevention already implemented in ToolRegistry; MCP-to-MCP overwrites allowed, built-in shadowing rejected |
| **Config migration** — Python YAML config → Rust struct deserialization | Config parse errors on migration | `#[serde(default)]` on all optional fields; graceful degradation with warnings; `hermes doctor` migration check |
| **MCP security** — Malicious MCP server injecting harmful tool descriptions | Credential exfiltration, code execution | 10 injection pattern scanning, OSV malware check, env filtering, credential stripping |
| **Browser SSRF** — Browser tool accessing internal network URLs | Internal network reconnaissance | SSRF protection, URL secret exfiltration blocking, website policy TTL cache |
| **Prompt injection via skills** — Malicious SKILL.md with injection patterns | Agent behavior manipulation | 9-pattern injection detection, skill readiness checks, disabled skill enforcement |

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
| Anthropic | API key / OAuth | Via /anthropic | Yes | Yes | Yes | Yes |
| OpenAI | API key | Yes | Yes | Yes | Yes | Yes |
| Xiaomi MiMo | API key | Yes | Yes | Yes | Yes | No |
| z.ai/GLM | API key | Yes | Yes | Yes | Yes | Yes |
| Kimi/Moonshot | API key | Yes | Yes | Yes | No | No |
| MiniMax | API key | Yes | Yes | Yes | No | No |
| HuggingFace | API key | Yes | Partial | Yes | Yes | No |
| Ollama | None (local) | Yes | Yes | Yes | Varies | Varies |
| Mistral | API key | Yes | Yes | Yes | Yes | No |
| Local endpoint | Any | Yes | Varies | Varies | Varies | Varies |

### Anthropic Adapter Details

- **OAuth/setup token detection**: auto-detects OAuth vs API key auth
- **5 beta headers**: prompt caching, max tokens override, etc.
- **Claude Code identity fingerprinting**: identifies as Claude Code to Anthropic API
- **Per-model output limits**: Opus=128K, Sonnet=64K
- **Thinking budgets (4 levels)**: configurable reasoning token budgets
- **Third-party endpoint detection**: adjusts behavior for non-Anthropic endpoints
- **Bearer-auth providers**: supports providers using Bearer token auth
- **Claude Code credentials**: reads from `~/.claude/.credentials.json`

### Credential Pool Details

- **PooledCredential** dataclass with 30+ fields
- **4 strategies**: fill_first, round_robin, random, least_used
- **OAuth auto-refresh**: syncs from 2 external files (Anthropic, Codex, Nous)
- **Exhaustion cooldown**: parses `reset_at` timestamp, 1hr for 429, 5min default
- **Concurrent request tracking**: per-credential in-flight count
- **Custom provider pools**: extensible pool registration

### Cross-provider constraints

- All providers must resolve credentials before use
- Model names must be resolvable via `ProviderId/ModelId` format
- Streaming format differences are abstracted by the `ApiMode` enum
- Tool schema compatibility is validated per provider at registration time
- 25 canonical providers with 50+ aliases

## 7. File Change Summary

| Crate | Phase | Files (new) | Files (modify) | Description |
|-------|-------|-------------|----------------|-------------|
| `core` | 1, 5, 9, 19 | 12 | — | Core types, config, session DB, memory (7 providers), skills, pricing, display, context refs |
| `api` | 2, 19 | 8 | — | Provider registry, API client (3 modes), streaming, model metadata, rate limit tracker |
| `tools` | 3, 8, 17 | 30 | — | Tool trait, registry, ~40 tools, approval, checkpoint, fuzzy match, patch parser, browser, TTS, web, send_message, Tirith |
| `query` | 4, 19 | 6 | — | Query loop, prompt builder (9-step), context compressor (15+ tool summaries) |
| `tui` | 6, 17 | 12 | — | ratatui TUI, skin engine (10 skins), input, output, completer, spinner, voice mode |
| `commands` | 7, 9, 19 | 25 | — | 60+ slash commands, skills hub (4 sources), insights engine |
| `mcp` | 10 | 6 | — | MCP client (2 transports), OAuth 2.1 PKCE, dynamic discovery, security scanning |
| `gateway` | 11, 12 | 30 | — | GatewayRunner, 20+ platform adapters, cron, session store, delivery router, hooks |
| `acp` | 14, 19 | 6 | — | ACP protocol server, Copilot ACP client |
| `envs` | 8 | 8 | — | 6 terminal backends, file sync, task env overrides |
| `web` | 13 | 5 | — | Axum web server, SSE streaming |
| `plugins` | 15 | 5 | — | Plugin discovery, hook system |
| `cli` | 16, 18 | 8 | — | Binary entry points, setup wizard, auth (25+ providers), model catalogs, tools config |

**Total: ~161 new files across 13 crates.**

## 8. Crate Dependency Graph

```
                              ┌─────────┐
                              │  core   │  (types, config, session, memory, skills)
                              └────┬────┘
           ┌──────────────┬───────┬┴──────┬────────┬────────┐
           ▼              ▼       ▼       ▼        ▼        ▼
      ┌────────┐    ┌────────┐ ┌─────┐ ┌────────┐ ┌────────┐ ┌────────┐
      │  api   │    │ tools  │ │envs │ │plugins │ │commands│ │  mcp   │
      │(clients)│    │(40+   │ │(6   │ │(hooks) │ │(60+   │ │(dynamic│
      │        │    │ tools)│ │back)│ │        │ │ cmds) │ │ tools) │
      └───┬────┘    └───┬────┘ └──┬──┘ └────────┘ └────────┘ └───┬────┘
          │             │         │                               │
          │         ┌───┘         │                               │
          │         │ (terminal   │                               │
          │         │  tool)      │                               │
          ▼         ▼             ▼                               ▼
      ┌────────────────────────────────────────────────────────────────┐
      │                           query                                │
      │  (conversation loop, prompt builder, context compression)      │
      └───────────────────────────┬────────────────────────────────────┘
                                  │
              ┌───────────────────┼───────────────┬──────────────┐
              ▼                   ▼               ▼              ▼
         ┌────────┐        ┌─────────────┐  ┌─────────┐  ┌─────────┐
         │  tui   │        │  gateway    │  │   acp   │  │   web   │
         │(ratatui)│       │(20+ platforms│  │(IDE srv)│  │(Axum)  │
         └────────┘        └─────────────┘  └─────────┘  └─────────┘

  ─────────────────────────────────────────────────────────────────
  Binary composition:
    hermes         → cli + tui + commands + query + api + tools + mcp + core
    hermes-gateway → cli + gateway + query + api + tools + envs + mcp + core
    hermes-acp     → cli + acp + query + api + tools + core
    hermes-web     → cli + web + query + api + tools + core
```

## 9. Implementation Order and Timeline Estimate

```
Phase 1  (Workspace + Core)          ████████████████████░░  ~3 days
Phase 2  (API Client + Credentials)  ░░░░████████████████░░  ~4 days  (depends on P1)
Phase 3  (Tool Framework + Approval) ░░░░░░░░██████████████  ~5 days  (depends on P1)
Phase 4  (Query Loop + Compression)  ░░░░░░░░░░░░██████████  ~4 days  (depends on P1-3)
Phase 5  (SQLite Store)              ░░░░████████████████░░  ~2 days  (depends on P1)
Phase 6  (TUI + Skins)               ░░░░░░░░░░░░░░░░██████  ~4 days  (depends on P4)
Phase 7  (Commands)                  ░░░░░░░░░░░░░░░░░░████  ~3 days  (depends on P4,6)
Phase 8  (Terminal + Code Exec)      ░░░░░░░░░░░░██████░░░░  ~5 days  (depends on P3)
Phase 9  (Memory + Skills + Hub)     ░░░░░░░░░░░░░░░░░░████  ~4 days  (depends on P4,5)
Phase 10 (MCP Client + Security)     ░░░░░░░░░░░░░░░░░░████  ~3 days  (depends on P3)
Phase 11 (Gateway + 20+ Platforms)   ░░░░░░░░░░░░░░░░░░░███  ~10 days (depends on P4,5)
Phase 12 (Cron Scheduler)            ░░░░░░░░░░░░░░░░░░░░░░  ~2 days  (depends on P11)
Phase 13 (Web UI)                    ░░░░░░░░░░░░░░░░░░░░░░  ~2 days  (depends on P4)
Phase 14 (ACP Server)                ░░░░░░░░░░░░░░░░░░░░░░  ~2 days  (depends on P4)
Phase 15 (Plugin System)             ░░░░░░░░░░░░░░░░░░░░░░  ~2 days  (depends on P1,7)
Phase 16 (CLI Binary + Integration)  ░░░░░░░░░░░░░░░░░░░░░░  ~3 days  (depends on ALL)
Phase 17 (Browser + TTS + Web)       ░░░░░░░░░░░░░░░░░░░░░░  ~4 days  (depends on P3,11)
Phase 18 (Setup + Auth + Skins)      ░░░░░░░░░░░░░░░░░░░░░░  ~3 days  (depends on P1,7)
Phase 19 (Prompt + Display + Pricing)░░░░░░░░░░░░░░░░░░░░░░  ~2 days  (depends on P4,7)
Phase 20 (RL Training - Deferred)    ░░░░░░░░░░░░░░░░░░░░░░  ~3 days  (v2)
```

Phases 5, 8, 9, 10 can run in parallel after Phase 3/4.
Phases 13, 14, 15 can run in parallel after Phase 4.
Phases 17, 18, 19 can run in parallel after Phase 11.

**Total estimate: ~50-70 working days** for a single developer, with parallelizable
phases reducing wall-clock time to approximately **6-8 weeks** with 2 developers.

## 10. Out of Scope (Deferred to v2)

These features exist in the Python codebase but are deferred to reduce initial
implementation scope. They can be added after the core system is functional.

| Feature | Python Source | Lines | Reason for Deferral |
|---------|--------------|-------|---------------------|
| Atropos RL environments | `environments/` | ~500+ | Research-only, not user-facing |
| Batch trajectory generation | `batch_runner.py` | ~391 | Data generation pipeline |
| Trajectory compression | `trajectory_compressor.py` | ~400 | Training data tool (gemini-3-flash, 15,250 target tokens) |
| Mini SWE runner | `mini_swe_runner.py` | ~709 | Benchmarking only |
| Honcho dialectic user modeling | `agent/memory_manager.py` | ~350 | External service, optional |
| Voice mode (faster-whisper) | `tools/voice_mode.py` | ~200 | Complex audio pipeline (16kHz mono int16) |
| Mixture of Agents | `tools/mixture_of_agents_tool.py` | ~150 | Niche research feature |
| Nous subscription prompts | `agent/prompt_builder.py` | ~100 | Partner-specific |
| Smart model routing | `agent/smart_model_routing.py` | ~200 | Auto-optimization, not core (40+ complexity keywords) |
| Skin sync | `tools/skills_sync.py` | ~100 | Convenience feature |
| Toolset distributions | `toolset_distributions.py` | ~200 | Research utility (probabilistic sampling) |
| RL CLI | `rl_cli.py` | ~446 | Research utility |
| Gacha mechanics / Buddy system | (if present) | — | Gamification, not core |
| Agent Loop Engine | `environments/` | ~500 | Research thread pool (128 threads) |
| Agentic OPD | `environments/` | ~200 | Token-level advantage for RL training |
| Web Research (FRAMES) | `environments/` | ~300 | Benchmark environment |
| TerminalBench2 / YCBench | `environments/` | ~400 | Benchmark environments |
| Tool call parsers (12 types) | `environments/` | ~300 | Model family-specific parsing for RL |

## 11. Key Numbers Reference

| Metric | Value | Source |
|--------|-------|--------|
| Python codebase LOC | ~500K+ across ~300 files | |
| Rust workspace crates | 13 | |
| Estimated new Rust files | ~161 | |
| LLM providers | 15+ (25 canonical, 50+ aliases) | Spec 45 |
| API modes | 3 (chat_completions, codex_responses, anthropic_messages) | Spec 46 |
| Terminal backends | 6 (local, docker, ssh, modal, singularity, daytona) | Spec 49 |
| Terminal env vars | 20+ | Spec 49 |
| Platform adapters | 20+ | Specs 27, 28, 30, 44 |
| Message types | 9 | Spec 44 |
| Retryable error patterns | 9 | Spec 44 |
| Command bypass commands | 8 | Spec 44 |
| Failover reasons | 14 | Specs 26, 40 |
| Context compression: summary ratio | 20% | Spec 47 |
| Context compression: token range | 2K-12K | Spec 47 |
| Context compression: protect turns | First 3 + last 6 | Spec 40 |
| Tool-specific summaries | 15+ tool types | Spec 47 |
| Compression failure cooldown | 10 min | Spec 47 |
| Credential pool strategies | 4 (fill_first, round_robin, random, least_used) | Spec 46 |
| Credential pool fields | 30+ | Spec 46 |
| OAuth external file sources | 2 | Spec 46 |
| Sandbox allowed tools | 7 | Spec 49 |
| Max tool calls per script | 50 | Spec 49 |
| Script timeout | 300s | Spec 49 |
| Max stdout | 50,000 bytes | Spec 49 |
| Max stderr | 10,000 bytes | Spec 49 |
| Parallel tool workers | 8 | Spec 38 |
| Prompt injection patterns | 9 | Spec 49 |
| Prompt builder steps | 9 | Spec 47 |
| Context threat patterns | 10 patterns + 9 Unicode chars | Spec 47 |
| MCP injection patterns | 10 | Spec 48 |
| MCP rate limit | 10 RPM | Spec 48 |
| MCP retries | 3 initial + 5 reconnect | Spec 48 |
| OSV malware check timeout | 10s | Spec 37 |
| Tirith scanner timeout | 5s | Spec 49 |
| Memory plugin backends | 7 | Spec 39 |
| Skill name max length | 64 chars | Spec 49 |
| Skill description max length | 1,024 chars | Spec 49 |
| Skills Hub source adapters | 4 | Spec 48 |
| TTS providers | 6 | Spec 49 |
| TTS char limit | 4,000 | Spec 49 |
| Browser backends | 5 | Spec 49 |
| Browser tool schemas | 10 | Spec 49 |
| Browser snapshot summarize | 8,000 chars | Spec 49 |
| Web backends | 4 | Spec 49 |
| Web refusal limit | 2M chars | Spec 49 |
| Web chunked processing | 500K chars | Spec 49 |
| Web processing threshold | 5K chars | Spec 49 |
| RL training tools | 9 | Spec 49 |
| RL locked config fields | 18 | Spec 49 |
| RL training processes | 3 (API, trainer, env) | Spec 49 |
| RL startup delay | 125s (5 + 30 + 90) | Spec 49 |
| RL status check interval | 30 min | Spec 49 |
| RL default total steps | 2,500 | Spec 49 |
| RL default max workers | 2,048 | Spec 49 |
| RL LoRA rank | 32 | Spec 49 |
| RL learning rate | 0.00004 | Spec 49 |
| RL tokenizer | Qwen/Qwen3-8B | Spec 49 |
| RL trainer | sglang | Spec 49 |
| Auth providers | 25+ | Spec 43 |
| Auth types | 4 | Spec 43 |
| Built-in skins | 10 | Spec 43 |
| Skin color keys | 20+ | Spec 43 |
| prompt_toolkit style classes | 30+ | Spec 43 |
| Toolsets configurable | 18 | Spec 45 |
| Tool categories | 5 | Spec 45 |
| CLI commands | 60+ | Spec 35 |
| Slash command categories | 5 | Spec 45 |
| Telegram max menu commands | 100 | Spec 45 |
| Telegram/Discord command name limit | 32 chars | Spec 45 |
| Main.py lines (Python) | 6,121 | Spec 43 |
| Gateway runner lines (Python) | 9,646 | Spec 42 |
| AIAgent lines (Python) | 11,024 | Spec 38 |
| Base adapter lines (Python) | 2,087 | Spec 44 |
| Total platforms/ lines (Python) | ~30,299 | Spec 44 |
| Total gateway/ lines (Python) | ~45,107 | Spec 42 |
| Message deduplicator TTL | 2000 entries / 300s | Spec 27 |
| Text batch delays | 0.6s / 2.0s | Spec 27 |
| Typing refresh interval | 2s | Spec 44 |
| Human delay range | 800-2500ms | Spec 44 |
| Media cache directories | 3 (images, audio, documents) | Spec 44 |
| Media cache TTL | 24 hours | Spec 44 |
| Process registry buffer | 200KB rolling | Spec 38 |
| Process registry TTL | 30 min | Spec 38 |
| Process registry max concurrent | 64 | Spec 38 |
| Checkpoint exclude patterns | 20 | Spec 33 |
| Fuzzy match strategies | 9 | Spec 33 |
| Unicode mappings | 8 | Spec 33 |
| send_message platforms | 18+ | Spec 33 |
| Diff display max files | 6 | Spec 47 |
| Diff display max lines | 80 per file | Spec 47 |
| Model metadata min context | 64K | Spec 47 |
| Model metadata fallback | 128K | Spec 47 |
| Model metadata probe tiers | 5 | Spec 47 |
| Model metadata cache TTL | 1 hour | Spec 47 |
| Rate limit header types | 12 | Spec 45 |
| Rate limit warning threshold | 80% | Spec 45 |
| Copilot ACP timeout | 900s | Spec 47 |
| Gateway drain timeout | 30s | Spec 42 |
| Gateway stuck-loop restarts | 3 | Spec 42 |
| Gateway exit code (restart) | 42 | Spec 42 |
| Gateway session expiry | 300s | Spec 42 |
| Gateway platform reconnect | 10s | Spec 42 |
| ContextVars (task-local) | 7 | Spec 29 |
| Event hooks | 7 + wildcard | Spec 29 |
| Container CPU default | 1 | Spec 49 |
| Container memory default | 5,120 MB (5 GB) | Spec 49 |
| Container disk default | 50,200 MB (50 GB) | Spec 49 |
| Sandbox lifetime default | 300s (5 min) | Spec 49 |
| Foreground max timeout | 600s (10 min) | Spec 49 |
| Disk warning threshold | 500 GB | Spec 49 |
| Sudo prompt timeout | 45s | Spec 49 |
| Gateway service management | systemd/launchd | Spec 41 |
| Batch runner output | JSONL | Spec 41 |
| Trajectory target tokens | 15,250 | Spec 41 |
| Trajectory summarizer | gemini-3-flash-preview | Spec 41 |
| Agent Loop thread pool | 128 | Spec 36 |
| Tool call parser types | 12 | Spec 36 |
| Security guard dirs | 7 | Spec 45 |
| Security guard files | 12 | Spec 45 |
| Account tier detection TTL | 180s | Spec 45 |
| Config validation env regex | ^[A-Za-z_][A-Za-z0-9_]*$ | Spec 49 |
| AIAgent init parameters | 50+ | Spec 40 |
| Default max iterations | 90 | Spec 40 |
| Default subagent iterations | 50 | Spec 40 |
| Parallel-safe tools | 12 | Spec 40 |
| Never-parallel tools | 1 (clarify) | Spec 40 |
| Path-scoped tools | 3 (read_file, write_file, patch) | Spec 40 |
| Surrogate code point range | U+D800–U+DFFF | Spec 40 |
| Stream stale detection | 90s | Spec 40 |
| Stream read timeout | 60s | Spec 40 |
| Prompt cache TTL | 5 minutes | Spec 40 |
| Compression passes max | 3 | Spec 15 |
| Compression threshold | 75% of context window | Spec 40 |
| Context pressure warnings | 85%, 95% thresholds | Spec 40 |
| Minimum context length | 64K tokens | Spec 40 |
| Grace call pattern | 1 extra iteration | Spec 40 |
| Retry counters reset | 10+ counters per turn | Spec 15 |
| Memory nudge interval | 10 user turns (default) | Spec 40 |
| Skill nudge interval | 10 iterations (default) | Spec 40 |
| Background review max iterations | 8 | Spec 15 |
| Content normalization fallback | json.dumps / str() | Spec 40 |
| Persistence skip threshold | 50K tokens or 80 messages | Spec 15 |
| CLI TUI lines (Python) | 10,024 | Spec 40 |
| Model tools dispatch lines | 562 | Spec 40 |
| Gateway service lines (Python) | 3,161 | Spec 41 |
| Trajectory compressor lines | 1,462 | Spec 41 |
| MCP serve lines | 867 | Spec 41 |
| Toolsets definitions lines | 702 | Spec 41 |
| Auxiliary client lines | 2,615 | Spec 46 |
| Anthropic adapter lines | 1,411 | Spec 46 |
| Credential pool lines | 1,416 | Spec 46 |
| Context compressor lines | 1,091 | Spec 47 |
| Prompt builder lines | 1,043 | Spec 47 |
| Display module lines | 1,037 | Spec 47 |
| Error classifier lines | 820 | Spec 47 |
| Insights engine lines | 789 | Spec 47 |
| Model metadata lines | 1,102 | Spec 47 |
| Usage pricing lines | 613 | Spec 47 |
| Copilot ACP client lines | 570 | Spec 47 |
| Context references lines | 520 | Spec 47 |
| Hermes state lines | 1,238 | Spec 41 |
| Hermes logging lines | 390 | Spec 41 |
| RL CLI lines | 446 | Spec 41 |
| Mini SWE runner lines | 709 | Spec 41 |
| Log files | 2 (agent.log, errors.log) | Spec 41 |
| Summary target tokens | 750 | Spec 41 |
| Protected last N turns (compressor) | 4 | Spec 41 |
| Context pressure cooldown | 300 seconds | Spec 41 |
| Fine-grained tool streaming header | x-anthropic-beta: fine-grained-tool-streaming-2025-05-14 | Spec 40 |
| Runtime snapshot fields | model, provider, base_url, api_mode, api_key, compressor state | Spec 40 |
| Fallback chain format | List of (model, provider, api_key, base_url) tuples | Spec 40 |
| Thinking tag variants | think, thinking, THINKING, reasoning, REASONING_SCRATCHPAD, thought | Spec 40 |
| Spinner frame count | 10 | Spec 40 |
| Spinner types | 5 (brain, sparkle, pulse, moon, star) | Spec 40 |
| Status callback types | 5 (lifecycle, step, tool_progress, background_review, thinking) | Spec 40 |
| Tool-to-toolset map | TOOL_TO_TOOLSET_MAP | Spec 40 |
