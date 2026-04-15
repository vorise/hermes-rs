# Hermes Agent — Core Entry Points & Query System

## Table of Contents

1. [hermes — Shell Entry Point](#hermes--shell-entry-point)
2. [hermes_cli/main.py — CLI Entry Point & Subcommand Router](#hermes_climainpy--cli-entry-point--subcommand-router)
3. [hermes_cli/main.py — HermesCLI Class](#hermescli-class)
4. [cli.py — Interactive TUI](#clipy--interactive-tui)
5. [run_agent.py — AIAgent Class](#run_agentpy--aiagent-class)
6. [run_conversation() — Main Conversation Loop](#run_conversation--main-conversation-loop)
7. [model_tools.py — Tool Orchestration](#model_toolspy--tool-orchestration)
8. [IterationBudget — Thread-Safe Iteration Counter](#iterationbudget--thread-safe-iteration-counter)
9. [ContextCompressor — Auto Context Compression](#contextcompressor--auto-context-compression)
10. [PromptBuilder — System Prompt Assembly](#promptbuilder--system-prompt-assembly)
11. [CredentialPool — Provider Failover](#credentialpool--provider-failover)
12. [ErrorClassifier — API Error Classification](#errorclassifier--api-error-classification)
13. [SessionDB (hermes_state.py) — SQLite State Store](#sessiondb-hermes_statepy--sqlite-state-store)
14. [hermes_cli/commands.py — Slash Command System](#hermes_clicommandspy--slash-command-system)

---

## hermes — Shell Entry Point

### Purpose

The shell script that makes `hermes` available on PATH. Located at project root as `hermes`.

### Content

```bash
#!/bin/bash
exec python3 -m hermes_cli.main "$@"
```

Or installed via setuptools entry point:
```python
[project.scripts]
hermes = "hermes_cli.main:main"
```

### Entry Flow

```
hermes → hermes_cli.main:main() → argparse/fire → subcommand dispatch
```

---

## hermes_cli/main.py — CLI Entry Point & Subcommand Router

### Purpose

The main entry point for all `hermes` CLI subcommands. Uses the `fire` library for command dispatch. Routes to the interactive CLI, setup wizard, gateway management, tool/model configuration, and 50+ other subcommands.

### Key Subcommands

| Subcommand | Handler | Description |
|-----------|---------|-------------|
| (no args) | `HermesCLI().interact()` | Start interactive CLI session |
| `hermes setup` | `hermes_cli.setup:main()` | Run full setup wizard |
| `hermes model` | `hermes_cli.model_switch` | Switch model/provider |
| `hermes tools` | `hermes_cli.tools_config` | Enable/disable tools per platform |
| `hermes skills` | `hermes_cli.skills_config` | Enable/disable skills per platform |
| `hermes gateway` | `hermes_cli.gateway` | Gateway management (setup/start/stop) |
| `hermes config` | `hermes_cli.config` | Set/get config values |
| `hermes doctor` | `hermes_cli.doctor` | Diagnose issues |
| `hermes update` | `hermes_cli.setup:update()` | Update to latest version |
| `hermes claw` | `hermes_cli.claw` | OpenClaw migration |
| `hermes backup` | `hermes_cli.backup` | Backup/restore |
| `hermes plugins` | `hermes_cli.plugins_cmd` | Plugin management |
| `hermes profiles` | `hermes_cli.profiles` | Profile management |
| `hermes web` | `hermes_cli.web_server` | Start web UI server |
| `hermes status` | `hermes_cli.status` | Show status |
| `hermes logs` | `hermes_cli.logs` | View session logs |
| `hermes dump` | `hermes_cli.dump` | Dump session data |
| `hermes completion` | `hermes_cli.completion` | Shell completion setup |
| `hermes uninstall` | `hermes_cli.uninstall` | Uninstall Hermes |
| `hermes version` | `hermes_cli.banner` | Show version/banner |

### Architecture

```
hermes_cli/main.py
├── HermesCLI class (main orchestrator)
│   ├── interact() → HermesCLI TUI (cli.py)
│   ├── setup() → SetupWizard
│   ├── model() → model_switch pipeline
│   ├── tools() → tools_config
│   ├── skills() → skills_config + skills_hub
│   ├── gateway() → gateway management
│   ├── config() → config read/write
│   ├── doctor() → diagnostics
│   ├── update() → update/rollback
│   ├── claw migrate() → OpenClaw migration
│   ├── backup() → backup/restore
│   ├── plugins() → plugin management
│   ├── profiles() → profile management
│   └── ... (50+ more)
├── hermes_cli/config.py → DEFAULT_CONFIG, env vars, migration
├── hermes_cli/env_loader.py → .env loading from ~/.hermes/.env
└── hermes_logging.py → Logging configuration
```

### Feature Flags / Config Keys

The `hermes_cli/config.py` module defines `DEFAULT_CONFIG` with 200+ config keys covering:
- Provider/model settings
- Tool enable/disable per platform
- Skill enable/disable per platform
- Terminal backend configuration
- Delegation settings
- Memory settings
- Personality/soul settings
- Platform-specific settings
- Cron job settings

---

## HermesCLI Class

### Purpose

The main interactive CLI orchestrator. Handles the TUI, slash command dispatch, and session management for CLI mode.

### Location

`hermes_cli/main.py` — class `HermesCLI` (~250K file, main entry point)

### Key Methods

| Method | Purpose |
|--------|---------|
| `interact()` | Start interactive CLI session |
| `handle_command()` | Process slash commands |
| `run_query()` | Send message to agent |
| `load_session()` | Load session history from SQLite |
| `save_session()` | Save session to SQLite |
| `show_banner()` | Display version banner |

### Dependencies

- `cli.py` — HermesCLI TUI implementation
- `hermes_cli/commands.py` — Slash command definitions
- `hermes_cli/callbacks.py` — Terminal callbacks
- `hermes_cli/config.py` — Configuration
- `hermes_cli/models.py` — Model catalog
- `hermes_state.py` — SQLite session store

---

## cli.py — Interactive TUI

### Purpose

The interactive terminal UI implementation. Uses prompt_toolkit for a rich terminal experience with multiline editing, command autocomplete, history navigation, and streaming output.

### Location

`cli.py` (~447K lines)

### Key Features

- **Fixed input area** — prompt_toolkit `Application` with separate input/output regions
- **Slash command autocomplete** — `SlashCommandCompleter` with fuzzy matching
- **Conversation history** — Up/down arrow navigation through previous messages
- **Multiline editing** — Shift+Enter for newlines
- **Streaming output** — Tool results and model responses stream in real-time
- **Cursor control** — Block cursor, input area highlighting
- **Interrupt handling** — Ctrl+C to interrupt current tool execution
- **Spinner animations** — Kawaii spinner frames during tool execution
- **Rich formatting** — ANSI color support, markdown rendering

### Key Classes

| Class | Purpose |
|-------|---------|
| `HermesCLI` | Main CLI orchestrator |
| `SlashCommandCompleter` | Command autocomplete |

### Key Imports

```python
from prompt_toolkit import Application
from prompt_toolkit.layout import Layout, HSplit, Window
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.widgets import TextArea
from prompt_toolkit.completion import Completer, Completion
from prompt_toolkit.history import FileHistory
from prompt_toolkit.styles import Style
```

---

## run_agent.py — AIAgent Class

### Purpose

The core AI agent class. Manages the conversation loop, tool execution, and response handling for LLMs that support function/tool calling.

### Location

`run_agent.py` (~560K lines, the single largest file)

### Constructor Parameters

| Parameter | Type | Default | Purpose |
|-----------|------|---------|---------|
| `base_url` | str | None | API base URL |
| `api_key` | str | None | API key |
| `provider` | str | None | Provider identifier |
| `api_mode` | str | None | "chat_completions", "codex_responses", "anthropic_messages" |
| `model` | str | "" | Model name |
| `max_iterations` | int | 90 | Max tool-call iterations |
| `tool_delay` | float | 1.0 | Delay between tool calls |
| `enabled_toolsets` | List[str] | None | Filter by toolset |
| `disabled_toolsets` | List[str] | None | Disable toolsets |
| `save_trajectories` | bool | False | Save JSONL trajectories |
| `verbose_logging` | bool | False | Verbose debug logging |
| `quiet_mode` | bool | False | Suppress progress output |
| `session_id` | str | None | Session ID |
| `platform` | str | None | Platform identifier |
| `user_id` | str | None | Gateway user ID |
| `skip_context_files` | bool | False | Skip auto-context injection |
| `session_db` | SessionDB | None | SQLite session store |
| `iteration_budget` | IterationBudget | None | Shared budget counter |
| `credential_pool` | CredentialPool | None | Provider credential pool |
| `fallback_model` | Dict | None | Fallback model config |
| `checkpoints_enabled` | bool | False | Enable session checkpoints |
| `prefill_messages` | List[Dict] | None | Few-shot priming messages |
| `reasoning_config` | Dict | None | Reasoning configuration |
| `max_tokens` | int | None | Max response tokens |
| `request_overrides` | Dict | None | API request overrides |

### API Mode Detection

The agent auto-detects API mode based on provider and base_url:

| Condition | API Mode |
|-----------|----------|
| `provider == "openai-codex"` | `codex_responses` |
| `provider == "anthropic"` or `api.anthropic.com` in URL | `anthropic_messages` |
| URL ends in `/anthropic` | `anthropic_messages` |
| Default | `chat_completions` |

### Key Instance Attributes

| Attribute | Type | Purpose |
|-----------|------|---------|
| `model` | str | Current model name |
| `provider` | str | Current provider |
| `api_mode` | str | API protocol mode |
| `session_id` | str | Session UUID |
| `messages` | List[Dict] | Conversation history |
| `tools` | List[Dict] | Available tool schemas |
| `iteration_budget` | IterationBudget | Thread-safe iteration counter |
| `_cached_system_prompt` | str | Cached system prompt (for prefix caching) |
| `_session_db` | SessionDB | SQLite session store |
| `_credential_pool` | CredentialPool | Provider failover |
| `context_compressor` | ContextCompressor | Auto context compression |
| `_memory_manager` | MemoryManager | Memory management |
| `_checkpoint_mgr` | CheckpointManager | Session checkpoints |
| `_interrupt_requested` | bool | Interrupt flag |
| `_todo_store` | TodoStore | In-memory todo tracking |

### Callbacks

| Callback | Signature | Purpose |
|----------|-----------|---------|
| `tool_progress_callback` | `(tool_name, args_preview)` | Tool start notification |
| `tool_start_callback` | `(tool_name, args)` | Tool start |
| `tool_complete_callback` | `(tool_name, result)` | Tool complete |
| `thinking_callback` | `(text)` | Thinking text streaming |
| `clarify_callback` | `(question, choices) -> str` | Interactive user question |
| `step_callback` | `(api_call_count, prev_tools)` | Gateway step event |
| `stream_delta_callback` | `(delta)` | Streaming text delta |
| `interim_assistant_callback` | `(text)` | Interim assistant text |
| `tool_gen_callback` | `(tool_calls)` | Tool use generated |
| `status_callback` | `(status)` | Status updates |

---

## run_conversation() — Main Conversation Loop

### Purpose

The main conversation loop. Called once per user turn, it:

1. Sanitizes input (surrogates, non-ASCII)
2. Builds the system prompt (cached for prefix caching)
3. Runs preflight context compression if needed
4. Enters the tool-calling loop
5. Handles tool execution, parallel execution, error recovery
6. Manages iteration budget
7. Saves session state

### Main Loop Flow

```
while (api_call_count < max_iterations and budget.remaining > 0):
    1. Check for interrupt → break if requested
    2. Consume iteration budget → break if exhausted
    3. Fire step_callback (gateway hooks)
    4. Track tool iterations for skill nudge
    5. Prepare messages (inject ephemeral context, reasoning)
    6. Build system message (cached prompt + ephemeral)
    7. Call LLM API (streaming or non-streaming)
    8. Handle response:
       a. Text content → display/stream
       b. Tool use → execute tools (parallel if safe)
       c. Tool results → feed back into messages
    9. Check stop conditions
    10. Save session state
```

### Tool Execution Modes

| Mode | Condition | Behavior |
|------|-----------|----------|
| Sequential | Default, or unsafe batch | Execute tools one at a time |
| Parallel | Safe tool batch, no conflicts | Execute tools concurrently via ThreadPoolExecutor |
| Path-parallel | File tools targeting different paths | Execute concurrently with path overlap detection |

### Parallel Safety

```python
_NEVER_PARALLEL_TOOLS = frozenset({"clarify"})
_PARALLEL_SAFE_TOOLS = frozenset({
    "ha_get_state", "read_file", "search_files",
    "session_search", "skill_view", "skills_list",
    "vision_analyze", "web_extract", "web_search",
})
_PATH_SCOPED_TOOLS = frozenset({"read_file", "write_file", "patch"})
_MAX_TOOL_WORKERS = 8
```

### Stop Conditions

| Condition | Action |
|-----------|--------|
| No tool calls in response | Exit loop, return text |
| Iteration budget exhausted | Exit loop, print warning |
| User interrupt | Exit loop, print interrupt message |
| Max API calls reached | Exit loop |
| Budget grace call | One final call, then exit |

### Error Recovery

| Error Type | Recovery Strategy |
|------------|------------------|
| API rate limit | Retry with exponential backoff, failover to fallback model |
| Context length exceeded | Trigger context compression, retry |
| Invalid JSON response | Retry with sanitized input |
| Tool execution failure | Feed error back as tool result |
| Dead connection | Clean up TCP connection, retry |
| Surrogate characters | Sanitize and retry |
| Non-ASCII encoding | Strip non-ASCII and retry |

---

## model_tools.py — Tool Orchestration

### Purpose

Orchestrates tool definitions, discovery, and execution. Bridges the AIAgent with the ToolRegistry.

### Key Functions

| Function | Purpose |
|----------|---------|
| `get_tool_definitions()` | Get OpenAI-compatible tool schemas |
| `get_toolset_for_tool()` | Get toolset membership for a tool |
| `handle_function_call()` | Execute a tool call and return result |
| `check_toolset_requirements()` | Validate toolset requirements |
| `discover_builtin_tools()` | Import and discover self-registering tools |

### Dependencies

- `tools/registry.py` — Central tool registry
- All tool modules in `tools/`

---

## IterationBudget — Thread-Safe Iteration Counter

### Purpose

Thread-safe iteration counter for the agent. Each agent (parent or subagent) gets its own budget.

### Location

`run_agent.py` — class `IterationBudget`

### Methods

| Method | Purpose |
|--------|---------|
| `consume()` | Try to consume one iteration, returns True if allowed |
| `refund()` | Give back one iteration (for execute_code turns) |

### Properties

| Property | Type | Purpose |
|----------|------|---------|
| `max_total` | int | Maximum iterations |
| `used` | int | Iterations consumed |
| `remaining` | int | Remaining iterations |

---

## ContextCompressor — Auto Context Compression

### Purpose

Automatically compresses conversation context when approaching token limits. Uses an auxiliary LLM to summarize older turns while preserving recent context and tool results.

### Location

`agent/context_compressor.py` (~49K lines)

### Key Methods

| Method | Purpose |
|--------|---------|
| `compress()` | Compress conversation messages above threshold |
| `should_compress()` | Check if compression is needed |
| `get_compressed_messages()` | Get compressed message list |

### Compression Strategy

1. Identify messages above token threshold
2. Use auxiliary LLM to summarize older turns
3. Preserve recent messages (last N turns)
4. Preserve tool results and important context
5. Replace original messages with compressed versions
6. Split session in SQLite (parent_session_id chain)

---

## PromptBuilder — System Prompt Assembly

### Purpose

Assembles the system prompt from multiple components: identity, personality, tools, skills, memory, context files, and platform hints.

### Location

`agent/prompt_builder.py` (~46K lines)

### System Prompt Components

| Component | Source | Purpose |
|-----------|--------|---------|
| Identity | `DEFAULT_AGENT_IDENTITY` | Core agent identity |
| Personality | User config / soul | Agent personality |
| Tool usage guidance | `TOOL_USE_ENFORCEMENT_GUIDANCE` | How to use tools |
| Platform hints | `PLATFORM_HINTS` | Platform-specific formatting |
| Memory guidance | `MEMORY_GUIDANCE` | Memory system instructions |
| Session search guidance | `SESSION_SEARCH_GUIDANCE` | Search system hints |
| Skills guidance | `SKILLS_GUIDANCE` | Skills system hints |
| Context files | `AGENTS.md`, `.cursorrules` | Project-specific instructions |
| Environment hints | Terminal backend hints | Environment capabilities |
| Memory context | Memory manager prefetch | Retrieved memories |
| Nous subscription | `build_nous_subscription_prompt` | Nous Portal hints |
| Skills system | `build_skills_system_prompt` | Skills instructions |
| Soul | `load_soul_md` | User's SOUL.md personality |

### Caching

The system prompt is cached per session to enable Anthropic prefix caching. Only rebuilt after:
- Context compression events
- Memory changes (with cache invalidation)
- Session restart with different configuration

---

## CredentialPool — Provider Failover

### Purpose

Manages provider credentials with automatic failover. When one provider hits rate limits or goes down, the pool automatically tries the next available credential.

### Location

`agent/credential_pool.py` (~58K lines)

### Key Concepts

- **Credentials** — Each credential represents a provider + API key + base URL
- **Active set** — Subset of credentials currently available
- **Failover** — Automatic switching to next credential on failure
- **Recovery** — Credentials marked as unavailable are periodically re-tested
- **Thread-safe** — All operations are thread-safe for concurrent tool execution

---

## ErrorClassifier — API Error Classification

### Purpose

Classifies API errors to determine the appropriate recovery strategy (retry, failover, abort).

### Location

`agent/error_classifier.py` (~28K lines)

### Failover Reasons

| Reason | Recovery |
|--------|----------|
| Rate limit | Retry with backoff, failover if persistent |
| Context length | Trigger compression, retry |
| Invalid request | Abort, report to user |
| Network error | Retry with connection cleanup |
| Provider unavailable | Failover to next credential |
| Authentication failure | Report to user |

---

## SessionDB (hermes_state.py) — SQLite State Store

### Purpose

Persistent session storage with FTS5 full-text search. Replaces per-session JSONL files with a relational database.

### Location

`hermes_state.py` (~50K lines)

### Schema

| Table | Purpose |
|-------|---------|
| `sessions` | Session metadata (source, model, cost, tokens, title) |
| `messages` | Individual messages (role, content, tool calls, reasoning) |
| `messages_fts` | FTS5 virtual table for full-text search |
| `schema_version` | Schema migration tracking |

### Key Design Decisions

- **WAL mode** — Concurrent readers + one writer (gateway multi-platform)
- **FTS5** — Fast text search across all session messages
- **Session splitting** — Compression-triggered via `parent_session_id` chains
- **Source tagging** — 'cli', 'telegram', 'discord', etc. for filtering

### Key Methods

| Method | Purpose |
|--------|---------|
| `create_session()` | Create new session record |
| `add_message()` | Add message to session |
| `get_session()` | Get session metadata |
| `get_messages()` | Get messages for session |
| `update_system_prompt()` | Store system prompt snapshot |
| `search_sessions()` | FTS5 search across sessions |
| `get_session_summaries()` | Get recent sessions |

---

## hermes_cli/commands.py — Slash Command System

### Purpose

Defines all slash commands available in both CLI and messaging platforms. Includes the `SlashCommandCompleter` for autocomplete.

### Location

`hermes_cli/commands.py` (~49K lines)

### Key Commands

| Command | Purpose |
|---------|---------|
| `/new` or `/reset` | Start fresh conversation |
| `/model` | Switch model/provider |
| `/compress` | Compress context |
| `/usage` | Show token/cost usage |
| `/insights` | Show usage analytics |
| `/undo` | Undo last turn |
| `/retry` | Retry last turn |
| `/stop` | Interrupt current work |
| `/tools` | List/enable/disable tools |
| `/skills` | Browse/search skills |
| `/memory` | View/manage memory |
| `/personality` | Set personality |
| `/status` | Show status |
| `/export` | Export conversation |
| `/help` | Show help |
| `/title` | Set session title |
| `/summarize` | Summarize conversation |
| `/speak` | Text-to-speech |
| `/voice` | Voice mode |

### Shared vs Platform-Specific

Most commands are shared between CLI and messaging platforms. Some commands are platform-specific:
- `/platforms` — CLI-only (platform status)
- `/sethome` — Messaging-only (set home channel)

---

## Key Files by Importance

| Rank | File | Size | Role |
|------|------|------|------|
| 1 | `run_agent.py` | ~560K | Core AIAgent class, conversation loop |
| 2 | `cli.py` | ~447K | CLI orchestrator, prompt_toolkit TUI |
| 3 | `hermes_cli/main.py` | ~250K | Entry point, all `hermes` subcommands |
| 4 | `gateway/run.py` | ~443K | Gateway main loop, platform dispatch |
| 5 | `hermes_cli/config.py` | ~136K | Configuration schema, defaults, migration |
| 6 | `hermes_state.py` | ~50K | SQLite session store (FTS5) |
| 7 | `agent/credential_pool.py` | ~58K | Provider credential failover |
| 8 | `agent/prompt_builder.py` | ~46K | System prompt assembly |
| 9 | `agent/context_compressor.py` | ~49K | Auto context compression |
| 10 | `hermes_cli/commands.py` | ~49K | Slash command definitions |

---

*Generated from source analysis of the Hermes Agent codebase.*
