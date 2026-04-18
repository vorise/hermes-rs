# Hermes Agent — Delegation, Browser & MCP Systems

This document covers the delegate tool (subagent architecture), thread-scoped interrupt system, environment variable passthrough registry, browser automation tool, and MCP client.

---

## Table of Contents

1. [Delegate Tool](#1-delegate-tool)
2. [Interrupt System](#2-interrupt-system)
3. [Environment Passthrough Registry](#3-environment-passthrough-registry)
4. [Browser Tool](#4-browser-tool)
5. [MCP Client](#5-mcp-client)

---

## 1. Delegate Tool

### Location

`tools/delegate_tool.py` (~1100 lines)

### Purpose

Spawns child AIAgent instances with isolated context, restricted toolsets, and their own terminal sessions. Collapses multi-step reasoning into a single parent turn — the parent sees only the delegation call and the summary result, never the child's intermediate tool calls or reasoning.

### 1.1 Architecture

```
Parent AIAgent
├── delegate_task(goal="Research X")
│   └── Child AIAgent (isolated)
│       ├── Fresh conversation (no parent history)
│       ├── Own task_id (own terminal session, file ops cache)
│       ├── Restricted toolset
│       └── Focused system prompt
└── Receives summary result only
```

### 1.2 Blocked Tools for Children

Children must never have access to:

| Blocked Tool | Reason |
|-------------|--------|
| `delegate_task` | No recursive delegation (depth limit also enforced) |
| `clarify` | No user interaction from subagents |
| `memory` | No writes to shared MEMORY.md |
| `send_message` | No cross-platform side effects |
| `execute_code` | Children should reason step-by-step, not write scripts |

### 1.3 Depth Limit

`MAX_DEPTH = 2` — parent (0) → child (1) → grandchild rejected (2). Enforced in `delegate_task()`:

```python
depth = getattr(parent_agent, '_delegate_depth', 0)
if depth >= MAX_DEPTH:
    return tool_error(f"Delegation depth limit reached ({MAX_DEPTH}). ...")
```

### 1.4 Concurrency Control

| Parameter | Default | Config Key / Env Var |
|-----------|---------|---------------------|
| Max concurrent children | 3 | `delegation.max_concurrent_children` / `DELEGATION_MAX_CONCURRENT_CHILDREN` |
| Max iterations per child | 50 | `delegation.max_iterations` |
| Heartbeat interval | 30s | `_HEARTBEAT_INTERVAL` (hardcoded) |

### 1.5 Two Operation Modes

**Single task**:
```python
delegate_task(goal="Debug the login flow", context="Error: 500 on POST /login")
```

**Batch (parallel)**:
```python
delegate_task(tasks=[
    {"goal": "Research X framework"},
    {"goal": "Research Y framework"},
    {"goal": "Compare benchmarks"},
])
```

Batch mode uses `ThreadPoolExecutor(max_workers=max_children)` with `as_completed()` for result ordering.

### 1.6 Child Agent Construction

Built on the main thread (thread-safe construction), then run in background threads:

```python
child = AIAgent(
    base_url=effective_base_url,
    api_key=effective_api_key,
    model=effective_model,
    provider=effective_provider,
    max_iterations=max_iterations,
    reasoning_config=child_reasoning,
    enabled_toolsets=child_toolsets,    # stripped of blocked tools
    quiet_mode=True,
    ephemeral_system_prompt=child_prompt,
    log_prefix=f"[subagent-{task_index}]",
    skip_context_files=True,            # no CLAUDE.md / AGENTS.md
    skip_memory=True,                   # no memory system
    clarify_callback=None,              # no user interaction
    iteration_budget=None,              # fresh budget per subagent
    session_db=parent_session_db,       # shared DB for persistence
    parent_session_id=parent_session_id,
)
child._delegate_depth = parent_depth + 1
```

### 1.7 Credential Resolution

Three paths for child credentials (in priority order):

1. **`delegation.base_url` configured** — direct OpenAI-compatible endpoint with API key
2. **`delegation.provider` configured** — resolved via `resolve_runtime_provider()` (same path as CLI/gateway startup)
3. **Neither configured** — child inherits everything from parent

Special URL detection:
- `chatgpt.com/backend-api/codex` → provider=`openai-codex`, mode=`codex_responses`
- `api.anthropic.com` → provider=`anthropic`, mode=`anthropic_messages`

### 1.8 Toolset Inheritance

When no explicit toolsets given, child inherits from parent's enabled toolsets. Subagent must not gain tools the parent lacks (intersection enforced).

Excluded toolset names from delegation: `debugging`, `safe`, `delegation`, `moa`, `rl` — composite/platform/scenario toolsets.

### 1.9 Child System Prompt

Built from three components:
1. **Goal** — `YOUR TASK:\n{goal}`
2. **Context** — optional background info
3. **Workspace path** — resolved from parent's `TERMINAL_CWD`, subdirectory hints, or current working directory

The prompt instructs children to:
- Be thorough but concise
- Report what they did, found, created, and any issues
- Never assume container-style paths (`/workspace/...`) unless explicitly given
- Discover local paths before issuing git/workdir commands

### 1.10 Progress Display

Two display paths for child activity:

**CLI**: Prints tree-view lines above the parent's delegation spinner
```
[subagent] ├─ read_file  "src/main.py"
[subagent] ├─ terminal   "git log --oneline -5"
```

**Gateway**: Batches tool names (5 at a time) and relays to parent's progress callback
```
🔀 read_file, terminal, write_file, search_files, patch
```

### 1.11 Heartbeat System

Prevents gateway inactivity timeout from firing while subagent works. Without heartbeat, the parent's `_last_activity_ts` freezes when `delegate_task` starts and the gateway kills the agent.

```python
def _heartbeat_loop():
    while not _heartbeat_stop.wait(_HEARTBEAT_INTERVAL):
        touch(f"delegate_task: subagent {task_index} running {tool} (iteration {i}/{max})")
```

Heartbeat pulls detail from child's activity tracker (`get_activity_summary()`) to show current tool and iteration count.

### 1.12 Credential Pool Sharing

Children share the parent's credential pool when using the same provider, so cooldown state and rotation stay synchronized. Different provider children load their own pool.

```python
def _resolve_child_credential_pool(effective_provider, parent_agent):
    if effective_provider == parent_provider:
        return parent_pool  # share pool
    return load_pool(effective_provider)  # load own pool
```

### 1.13 Interrupt Propagation

Children register with parent's `_active_children` list for interrupt propagation. When user sends a new message, interrupt propagates to all active children.

### 1.14 Resource Cleanup

On child completion (success or failure):
1. Stop heartbeat thread (5s join timeout)
2. Release credential lease
3. Restore parent's tool names (global state fixup)
4. Remove child from `_active_children`
5. Close child agent resources (terminal sandboxes, browser daemons, background processes, httpx clients)

### 1.15 Tool Trace Extraction

Builds a tool execution trace from child's conversation messages:
- Pairs tool calls with results via `tool_call_id` (correctly handles parallel tool calls)
- Records tool name, args size, result size, and status (ok/error)
- Falls back to last-entry matching when `tool_call_id` is missing

### 1.16 Result Format

```json
{
  "results": [
    {
      "task_index": 0,
      "status": "completed",
      "summary": "...",
      "api_calls": 12,
      "duration_seconds": 45.3,
      "model": "claude-sonnet-4-6",
      "exit_reason": "completed",
      "tokens": {"input": 15000, "output": 3000},
      "tool_trace": [
        {"tool": "read_file", "args_bytes": 20, "result_bytes": 5000, "status": "ok"}
      ]
    }
  ],
  "total_duration_seconds": 48.7
}
```

Status values: `completed`, `failed`, `error`, `interrupted`
Exit reasons: `completed`, `max_iterations`, `interrupted`

### 1.17 Memory Notification

Parent's memory provider is notified of delegation outcomes via `memory_manager.on_delegation(task, result, child_session_id)` — allows the memory system to learn from subagent work.

### 1.18 ACP Subagent Support

Children can use ACP subprocess transport (e.g., Claude Code) instead of inheriting parent's transport:

```python
delegate_task(
    goal="Refactor auth module",
    acp_command="claude",
    acp_args=["--acp", "--stdio", "--model", "claude-opus-4-6"]
)
```

Per-task overrides available in batch mode via `task.acp_command` and `task.acp_args`.

---

## 2. Interrupt System

### Location

`tools/interrupt.py` (77 lines)

### Purpose

Thread-scoped interrupt signaling so that interrupting one agent session does not kill tools running in other sessions. Critical in the gateway where multiple agents run concurrently in the same process.

### 2.1 Architecture

```python
_interrupted_threads: set[int] = set()   # thread idents with pending interrupt
_lock = threading.Lock()

def set_interrupt(active: bool, thread_id: int | None = None):
    """Set or clear interrupt flag for a specific thread."""

def is_interrupted() -> bool:
    """Check if current thread has a pending interrupt."""
```

### 2.2 Thread Scoping

The agent stores its execution thread ID at the start of `run_conversation()` and passes it to `set_interrupt()`/`clear_interrupt()`. Tools call `is_interrupted()` which checks the CURRENT thread — no argument needed.

### 2.3 Usage Pattern in Tools

```python
from tools.interrupt import is_interrupted

def some_tool():
    for step in work_items:
        if is_interrupted():
            return {"output": "[interrupted]", "returncode": 130}
        # ... do work ...
```

### 2.4 Backward-Compatible Proxy

`_interrupt_event` — a `_ThreadAwareEventProxy` that implements `threading.Event` methods (`is_set`, `set`, `clear`, `wait`) mapped to per-thread state. Allows legacy code that imports `_interrupt_event` directly to continue working.

### 2.5 Exit Code Convention

Interrupted processes return exit code `130` (standard SIGINT exit code).

---

## 3. Environment Passthrough Registry

### Location

`tools/env_passthrough.py` (102 lines)

### Purpose

Session-scoped allowlist of environment variables that pass through to sandboxed execution environments (execute_code, terminal). By default both sandboxes strip secrets from the child process environment for security.

### 3.1 Two Sources

1. **Skill declarations** — when a skill is loaded via `skill_view`, its `required_environment_variables` are registered automatically
2. **User config** — `terminal.env_passthrough` in config.yaml for non-skill use cases

### 3.2 Session Isolation

```python
_allowed_env_vars_var: ContextVar[set[str]] = ContextVar("_allowed_env_vars")
```

Backed by `ContextVar` to prevent cross-session data bleed in the gateway pipeline.

### 3.3 API

| Function | Purpose |
|----------|---------|
| `register_env_passthrough(var_names)` | Register env vars (called when skill loads) |
| `is_env_passthrough(var_name)` | Check if var is allowed in sandbox |
| `get_all_passthrough()` | Union of skill-registered and config-based vars |
| `clear_env_passthrough()` | Reset skill-scoped allowlist (session reset) |

### 3.4 Config Loading

```python
# From config.yaml:
# terminal:
#   env_passthrough:
#     - CUSTOM_API_KEY
#     - DATABASE_URL
```

Loaded once per process (cached in `_config_passthrough` global).

### 3.5 Consumer Sites

Both `code_execution_tool.py` and `tools/environments/local.py` consult `is_env_passthrough()` before stripping a variable from the child environment.

---

## 4. Browser Tool

### Location

`tools/browser_tool.py` (large file with multiple backend providers)

### Purpose

Browser automation via `agent-browser` CLI. Supports multiple backends with identical agent-facing behavior — the backend is auto-detected from config and available credentials.

### 4.1 Backend Architecture

Three backends, auto-detected:

| Backend | Type | Setup |
|---------|------|-------|
| **Local Chromium** | Zero-cost headless | `agent-browser install` |
| **Browserbase** | Cloud | `BROWSERBASE_API_KEY` + `BROWSERBASE_PROJECT_ID` |
| **Browser Use** | Cloud (Nous subscribers) | `BROWSER_USE_API_KEY` |
| **Camofox** | Local anti-detection | `CAMOFOX_URL` REST API |
| **Firecrawl** | Cloud scraping | Firecrawl API key |

### 4.2 Provider Architecture

Pluggable provider system in `tools/browser_providers/`:
- `BrowserbaseProvider` — direct Browserbase cloud
- `BrowserUseProvider` — Browser Use cloud
- `FirecrawlProvider` — Firecrawl API
- `CloudBrowserProvider` — base class for cloud providers

Provider normalization via `normalize_browser_cloud_provider()`.

### 4.3 Page Representation

Uses `agent-browser`'s accessibility tree (ariaSnapshot) for text-based page representation — ideal for LLM agents without vision capabilities.

Element interaction via ref selectors: `@e1`, `@e2`, etc.

### 4.4 Key Operations

| Function | Purpose |
|----------|---------|
| `browser_navigate(url, task_id)` | Navigate to URL |
| `browser_snapshot(task_id)` | Get page accessibility tree snapshot |
| `browser_click(ref, task_id)` | Click element by ref selector |
| `browser_type(ref, text, task_id)` | Type text into element |
| `browser_scroll(task_id)` | Scroll page |
| `browser_close(task_id)` | Close browser session |

### 4.5 Session Isolation

Each task ID gets its own browser session — prevents cross-session state leakage.

### 4.6 Content Summarization

Snapshot content exceeding `SNAPSHOT_SUMMARIZE_THRESHOLD` (8000 tokens) is summarized via auxiliary LLM (`call_llm()`) for task-aware extraction.

### 4.7 Website Policy & URL Safety

Two optional policy modules consulted before browser operations:
- `tools.website_policy.check_website_access(url)` — fail-open if unavailable
- `tools.url_safety.is_safe_url(url)` — fail-closed: block all if unavailable

### 4.8 PATH Discovery

Handles minimal PATH environments (e.g., systemd services, Termux) with:
- Standard PATH directories (`_SANE_PATH_DIRS`)
- Homebrew Node.js discovery (`node@20`, `node@24`, etc.)
- Hermes-managed Node bin directory (`~/.hermes/node/bin`)

### 4.9 Configuration

```yaml
browser:
  command_timeout: 30  # seconds, floor at 5s
```

Environment variables:

| Variable | Purpose | Default |
|----------|---------|---------|
| `BROWSERBASE_API_KEY` | Browserbase auth | — |
| `BROWSERBASE_PROJECT_ID` | Browserbase project | — |
| `BROWSER_USE_API_KEY` | Browser Use auth | — |
| `BROWSERBASE_PROXIES` | Residential proxies | `true` |
| `BROWSERBASE_ADVANCED_STEALTH` | Custom Chromium | `false` |
| `BROWSERBASE_KEEP_ALIVE` | Session reconnection | `true` |
| `BROWSERBASE_SESSION_TIMEOUT` | Custom timeout (ms) | none |

---

## 5. MCP Client

### Location

`tools/mcp_tool.py` (MCP integration module)

### Purpose

Connects to external MCP (Model Context Protocol) servers via stdio or HTTP/StreamableHTTP transport, discovers their tools, and registers them into the hermes-agent tool registry so the agent can call them like any built-in tool.

### 5.1 Configuration

Read from `~/.hermes/config.yaml` under `mcp_servers` key:

```yaml
mcp_servers:
  filesystem:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    env: {}
    timeout: 120          # per-tool-call timeout
    connect_timeout: 60   # initial connection timeout
  github:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_PERSONAL_ACCESS_TOKEN: "ghp_..."
  remote_api:
    url: "https://my-mcp-server.example.com/mcp"
    headers:
      Authorization: "Bearer sk-..."
    timeout: 180
  analysis:
    command: "npx"
    args: ["-y", "analysis-server"]
    sampling:
      enabled: true
      model: "gemini-3-flash"      # override model
      max_tokens_cap: 4096
      timeout: 30
      max_rpm: 10
      allowed_models: []           # empty = all
      max_tool_rounds: 5
      log_level: "info"
```

### 5.2 Transport Modes

| Transport | Config | Use Case |
|-----------|--------|----------|
| **Stdio** | `command` + `args` | Local subprocess (npx, python scripts) |
| **HTTP/StreamableHTTP** | `url` + `headers` | Remote MCP servers |

### 5.3 Architecture

```
Background Event Loop (_mcp_loop) ← daemon thread
├── Server Task 1 (filesystem MCP)
├── Server Task 2 (github MCP)
└── Server Task 3 (remote_api MCP)
```

- Dedicated background event loop runs in a daemon thread
- Each MCP server runs as a long-lived asyncio Task
- Tool call coroutines scheduled via `run_coroutine_threadsafe()`
- On shutdown, server Tasks are signalled to exit their `async with` block, ensuring anyio cancel-scope cleanup happens in the same Task that opened the connection

### 5.4 Thread Safety

`_servers` and `_mcp_loop`/`_mcp_thread` accessed from both background thread and caller threads. All mutations protected by `_lock` — safe regardless of GIL presence (Python 3.13+ free-threading).

### 5.5 Reconnection

Automatic reconnection with exponential backoff (up to 5 retries) on connection failure.

### 5.6 Security

- **Environment variable filtering** for stdio subprocesses — secrets stripped from child environment
- **Credential stripping** in error messages returned to the LLM — prevents credential leakage via error text

### 5.7 MCP Sampling Support

MCP servers can request LLM completions via `sampling/createMessage`:

| Config | Purpose |
|--------|---------|
| `sampling.enabled` | Allow server-initiated LLM requests |
| `sampling.model` | Override model for sampling |
| `sampling.max_tokens_cap` | Max tokens per request |
| `sampling.timeout` | LLM call timeout (seconds) |
| `sampling.max_rpm` | Rate limit (requests per minute) |
| `sampling.allowed_models` | Model whitelist (empty = all) |
| `sampling.max_tool_rounds` | Tool loop limit (0 = disable tool use) |
| `sampling.log_level` | Audit verbosity |

Sampling types conditionally imported for backward compatibility:
- `CreateMessageResult`, `CreateMessageResultWithTools`
- `SamplingCapability`, `SamplingToolsCapability`
- `TextContent`, `ToolUseContent`

### 5.8 Dynamic Tool Discovery

Notification types for real-time tool updates:
- `ToolListChangedNotification` — server added/removed tools
- `PromptListChangedNotification` — server changed prompts
- `ResourceListChangedNotification` — server changed resources

Handler registered via `message_handler` kwarg on `ClientSession` (backward-compatible with older SDK versions that don't support notification handlers).

### 5.9 Optional Dependency

The `mcp` Python package is optional. If not installed, the module is a no-op and logs a debug message. All MCP-related imports use graceful try/except blocks.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Delegate blocked tools | 5 |
| Max delegation depth | 2 |
| Default max concurrent children | 3 |
| Default child max iterations | 50 |
| Delegation heartbeat interval | 30s |
| Browser snapshot summarize threshold | 8000 tokens |
| Browser default command timeout | 30s |
| MCP default tool call timeout | 120s |
| MCP default connect timeout | 60s |
| MCP max reconnection retries | 5 |
| Browser backends | 5 (Local, Browserbase, Browser Use, Camofox, Firecrawl) |

---

*Generated from source analysis of the Hermes Agent codebase.*
