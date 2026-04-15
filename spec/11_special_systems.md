# Hermes Agent — Special Systems

This document covers the soul system, web UI, ACP adapter, plugin system, smart model routing, trajectory management, and other special systems.

---

## Table of Contents

1. [Soul System](#1-soul-system)
2. [Web UI Server](#2-web-ui-server)
3. [ACP Adapter](#3-acp-adapter)
4. [Plugin System](#4-plugin-system)
5. [Smart Model Routing](#5-smart-model-routing)
6. [Trajectory Management](#6-trajectory-management)
7. [Prompt Caching](#7-prompt-caching)
8. [Subdirectory Hints](#8-subdirectory-hints)
9. [Smart Model Routing](#9-smart-model-routing)
10. [Checkpoint Manager](#10-checkpoint-manager)
11. [Tool Result Storage](#11-tool-result-storage)
12. [Interrupt System](#12-interrupt-system)
13. [Transcription Tools](#13-transcription-tools)

---

## 1. Soul System

### Location

`hermes_cli/default_soul.py`, `agent/prompt_builder.py` — `load_soul_md()`

### Purpose

The soul system allows users to define a persistent personality/persona for the agent via a `SOUL.md` file.

### SOUL.md

Located at `~/.hermes/SOUL.md`, the soul file defines:
- Agent personality traits
- Communication style
- Behavioral guidelines
- Personal preferences

### Loading

```python
def load_soul_md() -> str:
    """Load SOUL.md from ~/.hermes/ for prompt injection."""
```

### Default Soul

`hermes_cli/default_soul.py` — Default soul content if no user SOUL.md exists.

---

## 2. Web UI Server

### Location

`hermes_cli/web_server.py` (~80K lines)

### Purpose

Web-based UI for interacting with Hermes Agent, as an alternative to CLI and messaging platforms.

### Architecture

- **FastAPI** backend
- **HTML/CSS/JS** frontend
- **Streaming** responses via SSE
- **Session management** per user

### Features

- Chat interface
- Session history
- Model switching
- Tool output display
- File attachment support
- Dark/light theme

### Configuration

```yaml
web:
  enabled: true
  host: "localhost"
  port: 8080
```

### API Endpoints

| Endpoint | Purpose |
|----------|---------|
| `POST /api/chat` | Send message |
| `GET /api/sessions` | List sessions |
| `GET /api/sessions/{id}` | Get session messages |
| `GET /api/models` | List available models |
| `GET /api/stream` | SSE stream for responses |

---

## 3. ACP Adapter

### Location

`acp_adapter/` — ACP (Agent Communication Protocol) server

### Purpose

VS Code / Zed / JetBrains IDE integration via the Agent Communication Protocol.

### Architecture

```
IDE Extension → ACP Protocol → Hermes Agent → Tool Execution
```

### Features

- IDE-native agent experience
- File context awareness
- Selection-based operations
- Terminal integration
- Git integration

### Entry Point

```python
[project.scripts]
hermes-acp = "acp_adapter.entry:main"
```

---

## 4. Plugin System

### Location

`hermes_cli/plugins.py` (~27K lines), `hermes_cli/plugins_cmd.py` (~40K lines), `plugins/`

### Purpose

Plugin system for extending Hermes with custom functionality.

### Plugin Types

| Type | Description |
|------|-------------|
| Commands | New slash commands |
| Hooks | Event listeners |
| Tools | New tool implementations |
| Skills | Procedural memory |

### Plugin Discovery

Plugins are discovered from:
- `plugins/` directory in Hermes home
- User-specified plugin paths

### Plugin Lifecycle

1. **Discovery** — Scan plugin directories
2. **Loading** — Import plugin modules
3. **Registration** — Register commands, hooks, tools
4. **Execution** — Invoke plugin handlers
5. **Cleanup** — Unregister on shutdown

### Plugin Hooks

| Hook | When Fired | Purpose |
|------|-----------|---------|
| `on_session_start` | New session created | Initialize session-scoped state |
| `pre_llm_call` | Before LLM API call | Inject context into user message |
| `post_llm_response` | After LLM response | Process response |
| `on_tool_call` | Before tool execution | Intercept/modify tool calls |
| `on_tool_result` | After tool execution | Process tool results |

### Plugin Commands

`hermes plugins` — Manage plugins (install, enable, disable, list)

---

## 5. Smart Model Routing

### Location

`agent/smart_model_routing.py` (~6K lines)

### Purpose

Intelligently route requests to the most appropriate model based on task complexity.

### Features

- Task complexity estimation
- Cost vs quality tradeoff
- Automatic model selection
- Fallback routing

---

## 6. Trajectory Management

### Location

`agent/trajectory.py` (~2K lines), `trajectory_compressor.py` (~63K lines)

### Purpose

Save and compress conversation trajectories for training data generation.

### Trajectory Format

JSONL format with:
- System prompt
- Messages (user, assistant, tool)
- Tool definitions
- Metadata (model, cost, tokens)

### Trajectory Features

- Think tag conversion (`convert_scratchpad_to_think`)
- Incomplete scratchpad detection
- Atomic JSON writes
- Per-trajectory file isolation

### Trajectory Compression

The trajectory compressor reduces trajectories for training:
- Removes redundant context
- Compresses tool results
- Optimizes for training efficiency
- Maintains conversation quality

---

## 7. Prompt Caching

### Location

`agent/prompt_caching.py` (~2K lines)

### Purpose

Apply Anthropic prompt cache control for reduced API costs.

### Features

- Cache control headers on system prompt
- Cache-eligible message marking
- Prefix caching support

---

## 8. Subdirectory Hints

### Location

`agent/subdirectory_hints.py` (~8K lines)

### Purpose

Track and inject subdirectory-specific context hints.

### Features

- Directory change detection
- Context file discovery per directory
- Hint injection into system prompt

---

## 9. Smart Model Routing

*(See §5 above — duplicate section number in original file)*

---

## 10. Checkpoint Manager

### Location

`tools/checkpoint_manager.py` (~23K lines)

### Purpose

Session checkpointing for save/restore conversation state.

### Features

- Per-turn snapshots
- Maximum snapshot limit (configurable, default 50)
- Restore from any checkpoint
- Automatic cleanup

---

## 11. Tool Result Storage

### Location

`tools/tool_result_storage.py` (~8K lines)

### Purpose

Persist long tool results across turns.

### Features

- Result size limiting
- Persistent result storage
- Turn-based budget enforcement
- Result retrieval

---

## 12. Interrupt System

### Location

`tools/interrupt.py` (~4K lines)

### Purpose

Thread-scoped interrupt signaling for tool execution.

### Features

- Thread ID tracking
- Signal-based interrupt
- Safe interrupt clearing
- Per-agent isolation

---

## 13. Transcription Tools

### Location

`tools/transcription_tools.py` (~27K lines)

### Purpose

Audio transcription for voice memos and voice input.

### Features

- Voice memo transcription
- Speech-to-text conversion
- Multi-format audio support
- Integration with messaging platforms

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Plugin hooks | 5 |
| Terminal backends | 6 |
| Soul file | SOUL.md |
| Web UI | FastAPI + HTML |
| ACP protocol | VS Code, Zed, JetBrains |
| Trajectory format | JSONL |

---

*Generated from source analysis of the Hermes Agent codebase.*
