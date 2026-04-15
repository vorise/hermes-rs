# Hermes Agent — Tools Reference

This document is an exhaustive reference for all tool implementations in Hermes Agent, derived from the source in `tools/registry.py` and individual tool files.

---

## Table of Contents

1. [Tool Registry Architecture](#1-tool-registry-architecture)
2. [Tool System Framework](#2-tool-system-framework)
3. [Toolsets](#3-toolsets)
4. [Individual Tool Reference](#4-individual-tool-reference)

---

## 1. Tool Registry Architecture

### Central Registry Pattern

Hermes uses a centralized `ToolRegistry` class with AST-based auto-discovery. Each tool self-registers at module load time.

### Location

`tools/registry.py` — classes `ToolEntry`, `ToolRegistry`, module `registry`

### Registration Flow

```
tools/registry.py (no deps — imported first)
       ↑
tools/*.py (each calls registry.register() at module level)
       ↑
model_tools.py (imports tools/registry + triggers tool discovery)
       ↑
run_agent.py, cli.py, batch_runner.py, environments/
```

### ToolEntry Structure

| Field | Type | Purpose |
|-------|------|---------|
| `name` | str | Tool name (unique) |
| `toolset` | str | Toolset membership |
| `schema` | dict | OpenAI-format tool schema |
| `handler` | Callable | Tool execution function |
| `check_fn` | Callable | Availability check (returns bool) |
| `requires_env` | list | Required environment variables |
| `is_async` | bool | Whether tool is async |
| `description` | str | Tool description |
| `emoji` | str | Display emoji |
| `max_result_size_chars` | int/float/None | Result size limit |

### Registry Methods

| Method | Purpose |
|--------|---------|
| `register()` | Register a tool at module load time |
| `deregister()` | Remove a tool (used by MCP dynamic discovery) |
| `get_definitions()` | Get OpenAI-format tool schemas for requested names |
| `get_handler()` | Get handler function for a tool name |
| `discover_builtin_tools()` | Import built-in self-registering tool modules |
| `resolve_toolset()` | Resolve toolset name (with alias support) |
| `register_toolset_alias()` | Register toolset alias |

### Shadow Prevention

The registry prevents built-in tools from being shadowed by plugins or MCP servers:
- MCP-to-MCP overwrites are allowed (legitimate server refresh)
- Built-in vs plugin/MCP shadowing is rejected with error

---

## 2. Tool System Framework

### Tool Execution Flow

```
1. AIAgent receives tool_use from LLM
2. model_tools.handle_function_call() called
3. ToolRegistry.get_handler(tool_name) → handler function
4. check_fn() validates tool availability
5. Requires_env checked → error if missing
6. Approval check for dangerous commands
7. Handler executed with tool arguments
8. Result returned to LLM (with size limits)
```

### Parallel Execution

Tools are classified for parallel safety:

| Classification | Tools | Parallel? |
|---------------|-------|-----------|
| Never parallel | `clarify` | No |
| Parallel-safe | Read-only tools (read_file, search_files, web_search, etc.) | Yes |
| Path-scoped | File tools (read_file, write_file, patch) | Yes, if paths don't overlap |
| Default | All other tools | No (sequential) |

### Approval System

`tools/approval.py` — heuristics for detecting dangerous terminal commands.

#### Dangerous Command Patterns

```python
_DESTRUCTIVE_PATTERNS = re.compile(r"""
    rm\s|rmdir\s|mv\s|sed\s+-i|truncate\s|dd\s|shred\s|
    git\s+(?:reset|clean|checkout)\s
""")
_REDIRECT_OVERWRITE = re.compile(r'[^>]>[^>]')
```

### Process Registry

`tools/process_registry.py` — manages background processes spawned by terminal tools.

#### Key Features

- Process tracking and lifecycle management
- Background process support
- Process output streaming
- Process cleanup on session end
- Resource limits

---

## 3. Toolsets

Toolsets are groups of tools that can be enabled/disabled together. Defined in `toolsets.py`.

### Core Toolsets

| Toolset | Tools | Purpose |
|---------|-------|---------|
| `file` | read_file, write_file, patch, search_files | File I/O operations |
| `terminal` | terminal, process management | Shell command execution |
| `web` | web_search, web_extract | Web search and extraction |
| `browser` | browser automation | Browser-based automation |
| `code_execution` | execute_code | Python code sandbox execution |
| `delegate` | delegate | Subagent delegation |
| `mcp` | MCP client tools | Model Context Protocol |
| `skills` | Skill execution tools | Procedural memory |
| `memory` | Memory operations | Persistent memory |
| `tts` | Text-to-speech | Audio output |
| `voice` | Voice input | Speech-to-text |
| `vision` | Image analysis | Vision/image processing |
| `homeassistant` | Home Assistant integration | Smart home control |
| `todo` | Todo management | Task tracking |
| `image_generation` | Image generation | DALL-E/FAL image creation |
| `session_search` | Session search | Past conversation search |
| `cronjob` | Cron job management | Scheduled automations |

### Toolset Configuration

Toolsets are configured per-platform in `~/.hermes/config.yaml`:
```yaml
platforms:
  cli:
    enabled_toolsets: [file, terminal, web, memory, skills]
  telegram:
    enabled_toolsets: [file, terminal, web, memory, skills, tts]
  discord:
    enabled_toolsets: [file, terminal, web, memory, skills, voice]
```

---

## 4. Individual Tool Reference

### Terminal Tool

**File:** `tools/terminal_tool.py` (~74K lines)
**Toolset:** `terminal`
**Handler:** `execute_terminal_tool()`

#### Purpose

Execute shell commands in the terminal. Supports multiple backends (local, Docker, SSH, Modal, Daytona, Singularity).

#### Key Features

- Multi-backend support (6 terminal backends)
- Process management (foreground/background)
- Output streaming
- Timeout support
- Working directory management
- Environment variable pass-through
- Destructive command detection (approval system)
- PTY support for interactive commands

#### Arguments

| Argument | Type | Required | Purpose |
|----------|------|----------|---------|
| `command` | str | Yes | Shell command to execute |
| `timeout` | int | No | Command timeout in seconds |
| `background` | bool | No | Run in background |

---

### File Tools

**File:** `tools/file_tools.py` (~48K lines)
**Toolset:** `file`
**Handler:** `execute_file_tool()`

#### Tools Provided

| Tool | Purpose |
|------|---------|
| `read_file` | Read file contents |
| `write_file` | Write/create file contents |
| `patch` | Apply patch to existing file |
| `search_files` | Search files by content/glob |

#### Key Features

- Path security validation
- Binary file detection
- Large file handling
- Atomic write support
- Patch parsing and application
- Glob pattern matching

---

### Web Tools

**File:** `tools/web_tools.py` (~87K lines)
**Toolset:** `web`
**Handler:** `execute_web_tool()`

#### Tools Provided

| Tool | Purpose |
|------|---------|
| `web_search` | Search the web (Exa, Parallel, etc.) |
| `web_extract` | Extract content from URLs (Firecrawl) |

#### Key Features

- Multiple search backends
- URL safety checking
- Content extraction and summarization
- Screenshot support
- Rate limit handling

---

### Browser Tool

**File:** `tools/browser_tool.py` (~94K lines)
**Toolset:** `browser`
**Handler:** `execute_browser_tool()`

#### Purpose

Browser automation for web interaction. Supports multiple backends: Browser Use (cloud), Browserbase (cloud), local Chromium.

#### Tools Provided

| Tool | Purpose |
|------|---------|
| `browser_navigate` | Navigate to URL |
| `browser_click` | Click element |
| `browser_type` | Type text |
| `browser_screenshot` | Take screenshot |
| `browser_snapshot` | Get page accessibility tree |
| `browser_close` | Close browser |

#### Key Features

- Accessibility tree snapshots (LLM-friendly)
- Element interaction via ref selectors
- Task-aware content extraction
- Session isolation per task ID
- Automatic cleanup

---

### Code Execution Tool

**File:** `tools/code_execution_tool.py` (~53K lines)
**Toolset:** `code_execution`
**Handler:** `execute_code_tool()`

#### Purpose

Execute Python code in a sandboxed environment via `execute_code` API.

#### Key Features

- Sandboxed code execution
- Package installation support
- Output capture
- Error handling
- Dependency management

---

### Delegate Tool

**File:** `tools/delegate_tool.py` (~48K lines)
**Toolset:** `delegate`
**Handler:** `execute_delegate_tool()`

#### Purpose

Spawn isolated subagents for parallel workstreams.

#### Key Features

- Subagent isolation (separate conversation)
- Configurable iteration budget
- Thread-safe execution (ThreadPoolExecutor)
- Context passing to subagent
- Result aggregation
- Error recovery

#### Arguments

| Argument | Type | Required | Purpose |
|----------|------|----------|---------|
| `description` | str | Yes | Task description for subagent |
| `prompt` | str | Yes | Detailed instructions |
| `model` | str | No | Override model for subagent |
| `max_iterations` | int | No | Override iteration limit |

---

### MCP Tool

**File:** `tools/mcp_tool.py` (~88K lines)
**Toolset:** `mcp`
**Handler:** `execute_mcp_tool()`

#### Purpose

MCP (Model Context Protocol) client. Connects to external MCP servers and exposes their tools.

#### Key Features

- MCP server management
- Dynamic tool discovery
- Stdio and SSE transports
- Resource support
- Prompt support
- Tool result size limiting
- Server lifecycle management

---

### Skills Tool

**File:** `tools/skills_tool.py` (~51K lines)
**Toolset:** `skills`
**Handler:** `execute_skills_tool()`

#### Purpose

Execute installed skills. Skills are procedural memory units that extend the agent's capabilities.

#### Key Features

- Skill execution with arguments
- Skill system prompt injection
- Skill version support
- Per-platform enable/disable

---

### Skills Hub Tool

**File:** `tools/skills_hub.py` (~112K lines)
**Toolset:** `skills`
**Handler:** `execute_skills_hub()`

#### Purpose

Search, browse, and install skills from the agentskills.io registry.

#### Key Features

- GitHub API integration
- Skill search and filtering
- Installation management
- Version checking
- Updates support

---

### Memory Tool

**File:** `tools/memory_tool.py` (~23K lines)
**Toolset:** `memory`
**Handler:** `execute_memory_tool()`

#### Purpose

Manage persistent memory. Create, update, delete, and query memory entries.

#### Key Features

- Memory CRUD operations
- Honcho integration
- Memory categorization
- Cross-session memory access

---

### Session Search Tool

**File:** `tools/session_search_tool.py` (~23K lines)
**Toolset:** `session_search`
**Handler:** `execute_session_search_tool()`

#### Purpose

Search past conversations using FTS5 full-text search in SQLite.

#### Key Features

- FTS5 search
- LLM-powered summarization
- Cross-session search
- Source filtering (cli, telegram, discord)
- Time-based filtering

---

### TTS Tool

**File:** `tools/tts_tool.py` (~41K lines)
**Toolset:** `tts`
**Handler:** `execute_tts_tool()`

#### Purpose

Text-to-speech output. Supports Edge TTS (free) and ElevenLabs (premium).

#### Key Features

- Edge TTS support (no API key needed)
- ElevenLabs integration
- Voice selection
- Audio output
- Streaming support

---

### Voice Mode

**File:** `tools/voice_mode.py` (~39K lines)
**Toolset:** `voice`
**Handler:** `execute_voice_mode()`

#### Purpose

Voice input mode with speech-to-text transcription.

#### Key Features

- Faster-whisper local transcription
- Sound device input
- Noise filtering
- Continuous listening mode

---

### Vision Tools

**File:** `tools/vision_tools.py` (~31K lines)
**Toolset:** `vision`
**Handler:** `execute_vision_tool()`

#### Purpose

Image analysis using LLM vision capabilities.

#### Key Features

- Image URL support
- Base64 image encoding
- Vision model routing
- Description generation

---

### Home Assistant Tool

**File:** `tools/homeassistant_tool.py` (~18K lines)
**Toolset:** `homeassistant`
**Handler:** `execute_ha_tool()`

#### Purpose

Home Assistant integration for smart home control.

#### Key Features

- Entity listing
- State queries
- Service calls
- Event subscriptions

---

### Image Generation Tool

**File:** `tools/image_generation_tool.py` (~27K lines)
**Toolset:** `image_generation`
**Handler:** `execute_image_tool()`

#### Purpose

Generate images using DALL-E or FAL.

#### Key Features

- Multiple backends (DALL-E, FAL)
- Style selection
- Size configuration
- Image URL return

---

### Todo Tool

**File:** `tools/todo_tool.py` (~10K lines)
**Toolset:** `todo`
**Handler:** `execute_todo_tool()`

#### Purpose

Task tracking and management.

#### Key Features

- Task creation, update, completion
- Task listing
- Priority support
- Status tracking

---

### Cron Job Tools

**File:** `tools/cronjob_tools.py` (~21K lines)
**Toolset:** `cronjob`
**Handler:** `execute_cronjob_tool()`

#### Purpose

Manage scheduled automations.

#### Key Features

- Cron job CRUD
- Schedule management
- Job execution history
- Delivery to any platform

---

### Mixture of Agents Tool

**File:** `tools/mixture_of_agents_tool.py` (~22K lines)
**Toolset:** `web`
**Handler:** `execute_mixture_of_agents_tool()`

#### Purpose

Run multiple models and aggregate their responses.

#### Key Features

- Parallel model execution
- Response aggregation
- Consensus building

---

### Approval Tool

**File:** `tools/approval.py` (~38K lines)
**Toolset:** N/A (utility)

#### Purpose

Detect dangerous commands before execution.

#### Key Features

- Pattern matching for destructive commands
- Output redirect detection
- Heuristic scoring

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Total tool files | 54 |
| Total toolsets | 17+ |
| Total individual tools | 40+ |
| Max tool workers | 8 |
| Max result size | Configurable per tool |

---

*Generated from source analysis of the Hermes Agent codebase.*
