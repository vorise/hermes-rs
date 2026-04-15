# Hermes Agent — Master Architecture Overview

> **Repository:** `/Users/zugle/workspaces/github/hermes-agent`
> **Primary Language:** Python 3.11+ (~300+ files, ~500K+ LOC)
> **Package Manager:** uv (with pip fallback)
> **TUI Framework:** prompt_toolkit (interactive CLI)
> **Runtime Target:** Local Python / Cloud VMs (Modal, Daytona) / Docker / SSH / Termux

---

## 1. What Is Hermes Agent?

Hermes Agent is an AI-powered agentic assistant and coding companion built by Nous Research. It is a full-featured interactive application that:

- Embeds any LLM as an agentic assistant via OpenAI-compatible API protocol
- Runs in the terminal using a prompt_toolkit-based TUI with multiline editing, history, and autocomplete
- Executes tools (file I/O, terminal, web, browser, MCP, skills) with a permission/approval system
- Lives on messaging platforms (Telegram, Discord, Slack, WhatsApp, Signal, and 12+ more) via a gateway process
- Has a built-in learning loop: creates skills from experience, improves them during use, persists memory across sessions
- Searches its own past conversations via FTS5 full-text search in SQLite
- Delegates tasks to isolated subagents for parallel execution
- Runs on six terminal backends: local, Docker, SSH, Modal, Daytona, Singularity
- Includes a built-in cron scheduler for automations
- Supports research workflows: batch trajectory generation, Atropos RL environments, trajectory compression

---

## 2. Repository Structure

```
hermes-agent/
├── run_agent.py              # AIAgent class — core conversation loop (~560K total with inline content)
├── model_tools.py            # Tool orchestration, discover_builtin_tools(), handle_function_call()
├── toolsets.py               # Toolset definitions, _HERMES_CORE_TOOLS list
├── cli.py                    # HermesCLI class — interactive CLI orchestrator
├── hermes_state.py           # SessionDB — SQLite session store (FTS5 search)
├── hermes_constants.py       # Constants, defaults, platform hints
├── hermes_logging.py         # Logging configuration
├── hermes_time.py            # Time utilities
├── hermes                    # Shell entry point script
├── utils.py                  # General utilities
│
├── agent/                    # Agent internals (30 files)
│   ├── prompt_builder.py         # System prompt assembly
│   ├── context_compressor.py     # Auto context compression
│   ├── prompt_caching.py         # Anthropic prompt caching
│   ├── auxiliary_client.py       # Auxiliary LLM client (vision, summarization)
│   ├── model_metadata.py         # Model context lengths, token estimation
│   ├── models_dev.py             # models.dev registry integration
│   ├── display.py                # KawaiiSpinner, tool preview formatting
│   ├── skill_commands.py         # Skill slash commands (shared CLI/gateway)
│   ├── skill_utils.py            # Skill utility functions
│   ├── credential_pool.py        # Provider credential pool with failover
│   ├── memory_manager.py         # Memory management (honcho integration)
│   ├── memory_provider.py        # Memory provider abstraction
│   ├── error_classifier.py       # API error classification & failover
│   ├── usage_pricing.py          # Usage/cost calculation
│   ├── rate_limit_tracker.py     # Rate limit tracking
│   ├── title_generator.py        # Session title generation
│   ├── insights.py               # Usage insights and analytics
│   ├── trajectory.py             # Trajectory saving helpers
│   ├── smart_model_routing.py    # Smart model routing
│   └── ...
│
├── tools/                    # Tool implementations (55+ files)
│   ├── registry.py               # Central tool registry
│   ├── approval.py               # Dangerous command detection
│   ├── terminal_tool.py          # Terminal orchestration
│   ├── process_registry.py       # Background process management
│   ├── file_tools.py             # File read/write/search/patch
│   ├── web_tools.py              # Web search/extract
│   ├── browser_tool.py           # Browser automation
│   ├── code_execution_tool.py    # execute_code sandbox
│   ├── delegate_tool.py          # Subagent delegation
│   ├── mcp_tool.py               # MCP client
│   ├── skills_tool.py            # Skills execution
│   ├── skills_hub.py             # Skills Hub (search/browse/install)
│   ├── memory_tool.py            # Memory operations
│   ├── tts_tool.py               # Text-to-speech
│   ├── voice_mode.py             # Voice mode
│   ├── vision_tools.py           # Vision/image analysis
│   ├── homeassistant_tool.py     # Home Assistant integration
│   ├── cronjob_tools.py          # Cron job management
│   ├── skill_manager_tool.py     # Skill management
│   ├── session_search_tool.py    # Session search
│   ├── todo_tool.py              # Todo management
│   ├── image_generation_tool.py  # Image generation
│   ├── mixture_of_agents_tool.py # Mixture of agents
│   ├── environments/             # Terminal backends (12 files)
│   └── ...
│
├── hermes_cli/               # CLI subcommands and setup (50+ files)
│   ├── main.py                   # Entry point — all `hermes` subcommands
│   ├── config.py                 # DEFAULT_CONFIG, env vars, migration
│   ├── commands.py               # Slash command definitions + completer
│   ├── callbacks.py              # Terminal callbacks (clarify, sudo, approval)
│   ├── setup.py                  # Interactive setup wizard
│   ├── skin_engine.py            # Skin/theme engine
│   ├── skills_config.py          # Skills enable/disable per platform
│   ├── tools_config.py           # Tools enable/disable per platform
│   ├── skills_hub.py             # `/skills` slash command
│   ├── models.py                 # Model catalog, provider model lists
│   ├── model_switch.py           # Shared /model switch pipeline
│   ├── auth.py                   # Provider credential resolution
│   ├── gateway.py                # Gateway management CLI
│   ├── web_server.py             # Web UI server
│   ├── plugins.py                # Plugin system
│   ├── plugins_cmd.py            # Plugin CLI commands
│   ├── profiles.py               # Profile system
│   ├── runtime_provider.py       # Runtime provider management
│   ├── banner.py                 # Banner/version display
│   ├── doctor.py                 # Diagnostics
│   ├── backup.py                 # Backup system
│   ├── mcp_config.py             # MCP configuration CLI
│   ├── memory_setup.py           # Memory setup
│   └── ...
│
├── gateway/                  # Messaging platform gateway (19 files)
│   ├── run.py                    # Main loop, slash commands, message dispatch
│   ├── session.py                # SessionStore — conversation persistence
│   ├── config.py                 # Gateway configuration
│   ├── stream_consumer.py        # Stream consumer for SSE-like platforms
│   ├── status.py                 # Gateway status reporting
│   ├── delivery.py               # Message delivery abstraction
│   ├── pairing.py                # DM pairing system
│   ├── session_context.py        # Session context tracking
│   ├── hooks.py                  # Gateway hooks
│   └── platforms/                # Platform adapters (25 files)
│       ├── base.py                   # Base platform adapter
│       ├── telegram.py               # Telegram adapter
│       ├── telegram_network.py       # Telegram Network (TgNet) adapter
│       ├── discord.py                # Discord adapter
│       ├── slack.py                  # Slack adapter
│       ├── whatsapp.py               # WhatsApp adapter
│       ├── signal.py                 # Signal adapter
│       ├── matrix.py                 # Matrix adapter
│       ├── homeassistant.py          # Home Assistant adapter
│       ├── webhook.py                # Generic webhook adapter
│       ├── api_server.py             # REST API server
│       ├── bluebubbles.py            # BlueBubbles (iMessage) adapter
│       ├── dingtalk.py               # DingTalk adapter
│       ├── feishu.py                 # Feishu adapter
│       ├── qqbot.py                  # QQ Bot adapter
│       ├── wecom_*.py                # WeCom (WeChat Work) adapters
│       ├── weixin.py                 # WeChat adapter
│       ├── mattermost.py             # Mattermost adapter
│       ├── sms.py                    # SMS adapter
│       └── email.py                  # Email adapter
│
├── acp_adapter/              # ACP server (VS Code / Zed / JetBrains integration)
├── cron/                     # Scheduler (jobs, scheduler)
├── environments/             # RL training environments (Atropos)
├── plugins/                  # Plugin system
├── skills/                   # Built-in skills
├── optional-skills/          # Optional skills
├── tests/                    # Pytest suite (~100+ files, ~3,000 tests)
├── web/                      # Web UI frontend
├── website/                  # Documentation website
├── landingpage/              # Landing page
├── scripts/                  # Build/install scripts
├── docker/                   # Docker configuration
├── nix/                      # Nix packaging
│
├── batch_runner.py           # Parallel batch processing
├── trajectory_compressor.py  # Trajectory compression for training
├── toolset_distributions.py  # Toolset distribution utilities
├── rl_cli.py                 # RL training CLI
├── mini_swe_runner.py        # Mini SWE agent runner
├── mcp_serve.py              # MCP server standalone
└── package.json              # Web UI dependencies
```

**User config:** `~/.hermes/config.yaml` (settings), `~/.hermes/.env` (API keys)

---

## 3. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      USER INTERFACES                              │
│  CLI TUI (prompt_toolkit)  │  Messaging (Telegram, Discord...)  │
│  Web UI (FastAPI + HTML)   │  ACP (VS Code, Zed, JetBrains)     │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│                    ENTRY POINTS / ORCHESTRATION                   │
│  hermes (cli) → hermes_cli.main → HermesCLI                     │
│  hermes gateway → gateway.run → Gateway main loop               │
│  hermes-agent → run_agent → AIAgent.run_conversation()          │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│                      AGENT CORE                                   │
│  run_agent.py → conversation loop → model_tools.handle_call()   │
│  ContextCompressor → PromptBuilder → CredentialPool             │
│  IterationBudget → ErrorClassifier → SmartModelRouting          │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│                      TOOL SYSTEM (40+ tools)                      │
│  ToolRegistry → file_tools, terminal_tool, web_tools,            │
│  browser_tool, mcp_tool, delegate_tool, skills_tool,             │
│  code_execution_tool, memory_tool, tts_tool, vision_tools, ...   │
│  Approval system → ProcessRegistry → Environment backends       │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│                      SERVICES LAYER                               │
│  SQLite State Store (FTS5) → Memory Manager → Skills System     │
│  Model Metadata → Usage Pricing → Rate Limit Tracker            │
│  Auxiliary Client (vision, summarization)                        │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│                      MODEL PROVIDERS                              │
│  Anthropic │ OpenRouter │ Nous Portal │ OpenAI │ Xiaomi MiMo    │
│  z.ai/GLM │ Kimi/Moonshot │ MiniMax │ HuggingFace │ Ollama      │
│  Mistral │ Any OpenAI-compatible endpoint                        │
└──────────────────────────────────────────────────────────────────┘
```

---

## 4. Core Subsystems

### 4.1 Agent Core / Conversation Loop (`run_agent.py`, `model_tools.py`)
The core loop that:
1. Takes user input (from CLI, messaging platform, or ACP)
2. Builds the API request (system prompt + history + tools + memory context)
3. Streams the response from the LLM API
4. Handles tool use (executes tools, feeds results back)
5. Manages context compression when approaching token limits
6. Tracks cost and iterations

### 4.2 Tool Framework (`tools/registry.py`, `tools/`)
- Central `ToolRegistry` class with AST-based auto-discovery
- Each tool self-registers via `registry.register()` at module load time
- OpenAI-compatible tool schema format
- 40+ tool implementations across file I/O, terminal, web, browser, MCP, delegation, skills, TTS, vision, Home Assistant, and more
- Approval system for dangerous commands
- Process registry for background tool management
- Tool result persistence for long-running operations

### 4.3 Terminal UI (`cli.py`, `hermes_cli/`)
- prompt_toolkit-based TUI with multiline editing
- Slash command autocomplete (SlashCommandCompleter)
- Conversation history navigation
- Cursor control and interrupt handling
- Skin Engine for theming/customization
- Spinner animations for tool execution

### 4.4 Commands System (`hermes_cli/commands.py`)
- 60+ slash commands (e.g., `/model`, `/skills`, `/tools`, `/compress`, `/memory`)
- Shared between CLI and messaging platforms
- Command completion with fuzzy matching
- Context-sensitive help

### 4.5 Messaging Gateway (`gateway/run.py`, `gateway/platforms/`)
- Single gateway process connects to multiple messaging platforms simultaneously
- 18+ platform adapters (Telegram, Discord, Slack, WhatsApp, Signal, Matrix, etc.)
- Session routing and management per platform
- Cross-platform conversation continuity
- Built-in slash command dispatch
- Stream consumer for long-running responses

### 4.6 Multi-Agent System (`tools/delegate_tool.py`)
- Spawn subagents as isolated AIAgent instances in ThreadPoolExecutor
- Configurable per-subagent iteration budget
- Context isolation (subagent gets its own conversation)
- Parallel execution with thread safety
- Result aggregation and error recovery

### 4.7 Memory System (`hermes_state.py`, `agent/memory_manager.py`)
- Short-term: SQLite session store with full message history
- Long-term: Memory files (markdown) with periodic consolidation
- FTS5 full-text search across all session messages
- LLM-powered search summarization for cross-session recall
- Honcho dialectic user modeling integration
- Memory provider abstraction for pluggable backends

### 4.8 Skills System (`tools/skills_tool.py`, `tools/skills_hub.py`)
- Procedural memory: skills created from complex tasks
- Skills self-improve during use
- Skills Hub: search, browse, install skills from agentskills.io
- Compatible with the agentskills.io open standard
- Per-platform skill enable/disable
- Skill versioning and updates

### 4.9 MCP Integration (`tools/mcp_tool.py`, `hermes_cli/mcp_config.py`)
- Model Context Protocol client support
- Dynamic tool registration from MCP servers
- MCP server configuration via CLI
- Resource and prompt support
- Stdio and SSE transports

### 4.10 Plugin System (`hermes_cli/plugins.py`, `plugins/`)
- Built-in plugins
- Plugin discovery and loading
- Plugin commands and skills
- Plugin toolsets

---

## 5. Data Flow: A User Turn

```
1. User types message (CLI TUI, Telegram, Discord, etc.)
2. Input routed to appropriate handler (HermesCLI or Gateway)
3. If slash command: dispatched to command handler
4. If regular message: sent to AIAgent.run_conversation()
5. PromptBuilder assembles API request:
   - System prompt (identity, personality, tools, skills, memory)
   - Message history (from SQLite SessionDB)
   - Available tools (filtered by toolset and platform)
   - Context files and environment hints
   - Memory context (from memory_manager)
6. Stream response from the LLM API (via OpenAI client)
7. For each content block:
   - text → render/display
   - tool_use → execute tool (with approval check if needed)
8. Tool results fed back into next API request
9. Loop until stop condition (no more tool use, iterations exceeded, budget exceeded)
10. Response saved to SQLite, cost tracked, skills/memory updated
```

---

## 6. Key Files by Importance

| Rank | File | Size | Role |
|------|------|------|------|
| 1 | `run_agent.py` | ~560K | Core AIAgent class, conversation loop |
| 2 | `cli.py` | ~447K | CLI orchestrator, prompt_toolkit TUI |
| 3 | `hermes_cli/main.py` | ~250K | Entry point, all `hermes` subcommands |
| 4 | `gateway/run.py` | ~443K | Gateway main loop, platform dispatch |
| 5 | `hermes_cli/config.py` | ~136K | Configuration schema, defaults, migration |
| 6 | `hermes_cli/setup.py` | ~127K | Interactive setup wizard |
| 7 | `hermes_cli/auth.py` | ~126K | Provider credential resolution |
| 8 | `hermes_cli/gateway.py` | ~132K | Gateway management CLI |
| 9 | `hermes_cli/tools_config.py` | ~72K | Tools enable/disable per platform |
| 10 | `hermes_state.py` | ~50K | SQLite session store (FTS5) |

---

## 7. Credential Model

Hermes uses a layered credential system:

1. **Environment variables** — API keys in `~/.hermes/.env` or system environment
2. **Config file** — Provider settings in `~/.hermes/config.yaml`
3. **Credential pool** — Automatic failover between multiple providers (agent/credential_pool.py)

Credential resolution:
- Each provider has specific env vars (e.g., `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`)
- The `auth.py` module resolves credentials from env → config → defaults
- The credential pool maintains a list of active credentials with failover on rate limits

Provider authentication methods:
- API key (most providers)
- OAuth (some providers)
- JWT (Skills Hub GitHub App)

---

## 8. Settings System

Layered settings (in priority order):
1. **Command-line flags** — Override everything for the current session
2. **Environment variables** — `HERMES_*` prefixed vars
3. **User config** — `~/.hermes/config.yaml`
4. **Project config** — `.hermes.yaml` in project root
5. **Defaults** — Built-in defaults in `hermes_cli/config.py`

Settings include: model/provider selection, tool enable/disable, skill enable/disable, personality, terminal backend, delegation settings, memory settings, platform-specific config, cron jobs.

---

## 9. Model Support

Hermes supports 15+ model providers with 200+ models via OpenRouter alone:

| Provider | Endpoint | Models |
|----------|----------|--------|
| Nous Portal | `api.nousresearch.com` | Hermes 3, Hermes 2.5, etc. |
| OpenRouter | `openrouter.ai` | 200+ models |
| Anthropic | `api.anthropic.com` | Claude 3/4 family |
| OpenAI | `api.openai.com` | GPT-4/4o/o-series |
| Xiaomi MiMo | `platform.xiaomimimo.com` | MiMo models |
| z.ai/GLM | `open.bigmodel.cn` | GLM models |
| Kimi/Moonshot | `platform.moonshot.ai` | Kimi models |
| MiniMax | `api.minimax.chat` | MiniMax models |
| HuggingFace | `huggingface.co` | Various models |
| Ollama | `localhost:11434` | Local models |
| Mistral | `api.mistral.ai` | Mistral models |
| Local endpoint | Any OpenAI-compatible URL | Custom models |

Model switching: `hermes model` or `/model [provider:model]` — no code changes.

---

## 10. File Dependency Chain

```
tools/registry.py  (no deps — imported by all tool files)
       ↑
tools/*.py  (each calls registry.register() at import time)
       ↑
model_tools.py  (imports tools/registry + triggers tool discovery)
       ↑
run_agent.py, cli.py, batch_runner.py, environments/
```

---

## 11. Spec Document Index

| File | Contents |
|------|----------|
| `00_overview.md` | This file — master architecture overview |
| `01_core_entry_query.md` | Entry points, agent loop, CLI, context compression |
| `02_commands.md` | All 60+ slash commands |
| `03_tools.md` | All 40+ tool implementations |
| `04_components_cli_tui.md` | CLI TUI components, Skin Engine |
| `05_components_messaging_platforms.md` | All 18+ platform adapters |
| `06_services_context_state.md` | SQLite state, memory, prompt builder, model metadata |
| `07_gateway_cron.md` | Gateway main loop, cron scheduler, session store |
| `08_environments_terminals.md` | Terminal backends |
| `09_auth_providers.md` | Provider credential resolution, model switching |
| `10_utils_memory_skills.md` | Memory system, skills system |
| `11_special_systems.md` | Soul system, web UI, ACP, plugins |
| `12_constants_types.md` | All constants, system prompts, model catalog |
| `13_research_rl.md` | RL environments, batch runner, trajectory compression |
| `14_mcp_acp.md` | MCP client, ACP server |
| `INDEX.md` | Quick-reference index |

---

*Generated from source analysis of the Hermes Agent codebase. ~300+ Python files, ~500K+ lines of code.*
