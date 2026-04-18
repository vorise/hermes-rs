# Hermes Agent — Core Agent Runtime

This document covers the core runtime files: `run_agent.py` (AIAgent class, conversation loop), `cli.py` (interactive TUI), `model_tools.py` (tool dispatch), and `hermes_constants.py`.

---

## Table of Contents

1. [AIAgent Class](#1-aiagent-class)
2. [Conversation Loop](#2-conversation-loop)
3. [Parallel Tool Execution](#3-parallel-tool-execution)
4. [API Mode Detection](#4-api-mode-detection)
5. [CLI TUI (cli.py)](#5-cli-tui-cli-py)
6. [Model Tools Dispatch](#6-model-tools-dispatch)
7. [Hermes Constants](#7-hermes-constants)

---

## 1. AIAgent Class

### Location

`run_agent.py` (~11,024 lines)

### Purpose

The central agent runtime. Manages the conversation flow, tool execution, error recovery, context compression, and provider failover for all LLM interactions.

### 1.1 Initialization Parameters (50+)

Key parameters:

| Parameter | Default | Purpose |
|-----------|---------|---------|
| `base_url` | None | API endpoint URL |
| `model` | "" | Model name |
| `max_iterations` | 90 | Max tool-calling turns per conversation |
| `tool_delay` | 1.0 | Seconds between tool calls |
| `enabled_toolsets` | None | Toolset allow-list |
| `disabled_toolsets` | None | Toolset deny-list |
| `save_trajectories` | False | Save conversation to JSONL |
| `session_id` | Auto-generated | Unique session identifier |
| `platform` | None | "cli", "telegram", "discord", etc. |
| `iteration_budget` | Shared | Thread-safe iteration counter |
| `fallback_model` | None | Legacy single fallback |
| `credential_pool` | None | Multi-credential failover pool |
| `checkpoints_enabled` | False | Filesystem checkpointing |
| `prefill_messages` | [] | Ephemeral few-shot priming |

### 1.2 API Mode Detection

```python
# Priority order:
if api_mode explicitly set → use it
elif provider == "openai-codex" → "codex_responses"
elif base_url contains "chatgpt.com/backend-api/codex" → "codex_responses"
elif provider == "anthropic" or base_url contains "api.anthropic.com" → "anthropic_messages"
elif base_url ends with "/anthropic" → "anthropic_messages"
else → "chat_completions"

# Post-detection upgrade:
if api_mode == "chat_completions" and (
    model starts with "gpt-5" or is_direct_openai_url()
):
    api_mode = "codex_responses"
```

### 1.3 Client Construction

| Provider | Client Type | Special Headers |
|----------|-------------|-----------------|
| OpenRouter | `openai.OpenAI` | `HTTP-Referer`, `X-OpenRouter-Title`, `X-OpenRouter-Categories` |
| GitHub Copilot | `openai.OpenAI` | `copilot_default_headers()` |
| Kimi | `openai.OpenAI` | `User-Agent: KimiCLI/1.30.0` |
| Qwen Portal | `openai.OpenAI` | `_qwen_portal_headers()` (mimics QwenCode CLI) |
| Anthropic native | `anthropic.Anthropic` | OAuth token resolution |
| Copilot ACP | Subprocess shim | `command`, `args` for `copilot --acp --stdio` |

### 1.4 Fallback Chain

```python
# Supports both legacy single fallback and new multi-provider chain:
if isinstance(fallback_model, list):
    self._fallback_chain = [f for f in fallback_model if valid]
elif isinstance(fallback_model, dict):
    self._fallback_chain = [fallback_model]

# Activated on: rate_limit, overloaded, billing, auth errors
# Each fallback gets one attempt before moving to next
```

### 1.5 Prompt Caching

Auto-enabled for Claude models:
```python
self._use_prompt_caching = (is_openrouter and is_claude) or is_native_anthropic
self._cache_ttl = "5m"  # 5-minute TTL (1.25x write cost)
```

Uses `apply_anthropic_cache_control()` with system_and_3 strategy (4 breakpoints).

### 1.6 Safe Stdio Wrapper

```python
class _SafeWriter:
    """Wraps stdout/stderr to catch OSError/ValueError from broken pipes."""
    def write(self, data):
        try: return self._inner.write(data)
        except (OSError, ValueError): return len(data)
```

Prevents crashes when running as systemd service, Docker container, or headless daemon where stdout pipe can become unavailable.

---

## 2. Conversation Loop

### 2.1 Main Loop Structure

```python
def run_conversation(self, user_message, system_message=None,
                     conversation_history=None, task_id=None,
                     stream_callback=None, persist_user_message=None):
    while (api_call_count < self.max_iterations and
           self.iteration_budget.remaining > 0) or self._budget_grace_call:

        # 1. Check interrupt
        # 2. Consume iteration budget
        # 3. Fire step_callback (gateway hooks)
        # 4. Build system prompt (cached per session)
        # 5. Pre-flight context compression if needed
        # 6. Fire pre_llm_call plugin hooks
        # 7. Memory provider prefetch (once per turn)
        # 8. Call LLM API (streaming or non-streaming)
        # 9. Process response (text, tool calls, thinking)
        # 10. Execute tools (parallel or sequential)
        # 11. Append tool results to messages
        # 12. Memory flush (periodic)
        # 13. Flush messages to session DB
        # 14. Save trajectory if enabled
```

### 2.2 System Prompt Caching

```python
# Built once, reused across turns for Anthropic prefix cache compatibility
if self._cached_system_prompt is None:
    # Try loading from session DB (continuing session)
    if conversation_history and self._session_db:
        stored_prompt = self._session_db.get_session(self.session_id)

    if stored_prompt:
        self._cached_system_prompt = stored_prompt  # Reuse for cache hit
    else:
        self._cached_system_prompt = self._build_system_prompt(...)
        # Fire on_session_start plugin hook
        # Store in session DB
```

**Key insight**: Rebuilding the system prompt would break the Anthropic prefix cache because the cached prefix wouldn't match.

### 2.3 Pre-flight Context Compression

Before entering the main loop, checks if loaded conversation history exceeds model's context threshold:

```python
if len(messages) > protect_first_n + protect_last_n + 1:
    _preflight_tokens = estimate_request_tokens_rough(messages, tools=...)
    if _preflight_tokens >= threshold_tokens:
        for _pass in range(3):  # May need multiple passes
            messages, active_system_prompt = self._compress_context(...)
            if len(messages) >= _orig_len: break  # Cannot compress further
```

Handles cases where user switches to a model with smaller context window.

### 2.4 Plugin Hooks

| Hook | When | Purpose |
|------|------|---------|
| `on_session_start` | First turn of new session | Initialize session-scoped plugin state |
| `pre_llm_call` | Before each API call | Append context to user message (ephemeral) |

**Plugin context injection**: Always appended to user message, never system prompt. This preserves the prompt cache prefix.

### 2.5 Memory Integration

- **Memory nudge**: Every `_memory_nudge_interval` turns, prompts model to review memory
- **Memory flush**: After `_memory_flush_min_turns`, writes memory to disk
- **Skill nudge**: Every `_skill_nudge_interval` tool iterations, suggests skill usage
- **External memory provider**: Prefetches once before tool loop, caches result

### 2.6 Budget Exhaustion Grace

When iteration budget is exhausted:

```python
elif not self.iteration_budget.consume():
    # Budget exhausted — break after this iteration
    break

# After loop: if model didn't produce text response, force summarization
if not final_text and not interrupted:
    # Inject one final message asking model to summarize
    self._budget_grace_call = True  # Allow one more API call
```

### 2.7 Error Recovery Pipeline

```python
try:
    response = self._interruptible_api_call(...)
except Exception as e:
    classified = classify_api_error(e, provider=self.provider)

    if classified.should_compress:
        messages, system_prompt = self._compress_context(...)
        retry
    if classified.should_rotate_credential:
        self._rotate_credential()
        self._rebuild_client()
        retry
    if classified.should_fallback:
        self._activate_fallback()
        retry
    if classified.retryable:
        backoff_and_retry()
```

### 2.8 Surrogate Sanitization

Clipboard paste from rich-text editors can inject lone surrogate code points (invalid UTF-8) that crash `json.dumps()` in the OpenAI SDK:

```python
_SURROGATE_RE = re.compile(r'[\ud800-\udfff]')

def _sanitize_surrogates(text: str) -> str:
    if _SURROGATE_RE.search(text):
        return _SURROGATE_RE.sub('\ufffd', text)  # U+FFFD replacement
    return text

def _sanitize_messages_surrogates(messages: list) -> bool:
    """Walks message dicts in-place, sanitizes content, name, tool_calls."""
```

### 2.9 Session Logging

```python
# Session logs: ~/.hermes/sessions/session_<id>.json
self.session_log_file = self.logs_dir / f"session_{self.session_id}.json"

# SQLite session store (optional)
if self._session_db:
    self._session_db.create_session(
        session_id=self.session_id,
        source=self.platform or "cli",
        model=self.model,
        ...
    )

# Conversation messages tracked for DB flush
self._session_messages: List[Dict[str, Any]] = []
self._last_flushed_db_idx = 0  # Prevents duplicate writes
```

### 2.10 Checkpoint Manager

```python
from tools.checkpoint_manager import CheckpointManager
self._checkpoint_mgr = CheckpointManager(
    enabled=checkpoints_enabled,
    max_snapshots=checkpoint_max_snapshots,
)
```

Filesystem checkpointing for transparent state snapshots. Resets per-turn via `self._checkpoint_mgr.new_turn()`.

### 2.11 Todo Store

```python
from tools.todo_tool import TodoStore
self._todo_store = TodoStore()

# Hydrate from conversation history (gateway creates fresh AIAgent per message)
if conversation_history and not self._todo_store.has_items():
    self._hydrate_todo_store(conversation_history)
```

---

## 3. Parallel Tool Execution

### 3.1 Tool Classification

```python
_NEVER_PARALLEL_TOOLS = frozenset({"clarify"})  # Interactive/user-facing

_PARALLEL_SAFE_TOOLS = frozenset({
    "ha_get_state", "ha_list_entities", "ha_list_services",
    "read_file", "search_files", "session_search", "skill_view",
    "skills_list", "vision_analyze", "web_search", "web_extract",
})

_PATH_SCOPED_TOOLS = frozenset({"read_file", "write_file", "patch"})
_MAX_TOOL_WORKERS = 8
```

### 3.2 Parallelization Decision

```python
def _should_parallelize_tool_batch(tool_calls) -> bool:
    if len(tool_calls) <= 1: return False
    if any(name in _NEVER_PARALLEL_TOOLS for name in tool_names): return False

    # Path-scoped tools: check for overlapping file targets
    for tool_call in tool_calls:
        if tool_name in _PATH_SCOPED_TOOLS:
            scoped_path = _extract_parallel_scope_path(tool_name, function_args)
            if any(_paths_overlap(scoped_path, existing) for existing in reserved_paths):
                return False  # Overlapping paths → sequential

        if tool_name not in _PARALLEL_SAFE_TOOLS:
            return False  # Unknown tool → sequential

    return True
```

### 3.3 Destructive Command Detection

```python
_DESTRUCTIVE_PATTERNS = re.compile(r"""(?:^|\s|&&|\|\||;|`)(?:
    rm\s|rmdir\s|mv\s|sed\s+-i|truncate\s|dd\s|shred\s|
    git\s+(?:reset|clean|checkout)\s
)""", re.VERBOSE)
_REDIRECT_OVERWRITE = re.compile(r'[^>]>[^>]|^>[^>]')
```

Used to detect potentially destructive terminal commands before execution.

---

## 4. CLI TUI (cli.py)

### Location

`cli.py` (~10,024 lines)

### Purpose

Interactive terminal interface inspired by Claude Code. Built on `prompt_toolkit` with fixed input area, live status bar, and rich formatting.

### 4.1 Key Features

- Fixed input area at bottom of terminal
- Live status bar showing model, tokens, cost
- Tool call preview with emoji
- Streaming token display
- Multi-line editing support
- Command completion menu
- Session persistence

### 4.2 TUI Layout

```
+----------------------------------+
| Banner + Model Info              |
|----------------------------------|
| Conversation History (scrollable)|
| ...                              |
| ...                              |
|----------------------------------|
| Status Bar (model, tokens, cost) |
+----------------------------------+
| Input Area (fixed, multi-line)   |
| > _                              |
+----------------------------------+
```

### 4.3 prompt_toolkit Components

| Component | Purpose |
|-----------|---------|
| `TextArea` | Input field with multi-line support |
| `FormattedTextControl` | Display areas (history, status) |
| `KeyBindings` | Keyboard shortcuts (Ctrl+C, Enter, etc.) |
| `CompletionsMenu` | Tab completion for commands |
| `patch_stdout` | Thread-safe output rendering |
| `CursorShape.BLOCK` | Non-blinking block cursor |

### 4.4 Spinner Frames

```python
_COMMAND_SPINNER_FRAMES = ("⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏")
```

### 4.5 Usage Modes

| Mode | Command |
|------|---------|
| Interactive (all tools) | `python cli.py` |
| With specific toolsets | `python cli.py --toolsets web,terminal` |
| With skills | `python cli.py --skills hermes-agent-dev,github-auth` |
| Single query | `python cli.py -q "your question"` |
| List tools | `python cli.py --list-tools` |

---

## 5. Model Tools Dispatch

### Location

`model_tools.py` (~562 lines)

### Purpose

Central tool dispatch system. Maps tool names to toolsets, generates tool definitions, and handles function call routing.

### 5.1 Key Functions

| Function | Purpose |
|----------|---------|
| `get_tool_definitions()` | Generate OpenAI-format tool schemas |
| `handle_function_call()` | Route tool call to implementation |
| `check_toolset_requirements()` | Validate dependencies |
| `get_toolset_for_tool()` | Map tool name to toolset |

### 5.2 Tool-to-Toolset Mapping

`TOOL_TO_TOOLSET_MAP` — master mapping used by:
- Tool definition generation
- Toolset filtering
- Batch runner schema generation
- `ALL_POSSIBLE_TOOLS` derivation

---

## 6. Hermes Constants

### Location

`hermes_constants.py` (~294 lines)

### Purpose

Central constants module shared across all modules.

### 6.1 Key Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `OPENROUTER_BASE_URL` | `https://openrouter.ai/api/v1` | OpenRouter API endpoint |
| `display_hermes_home()` | `Path` | HERMES_HOME display path |
| `get_hermes_home()` | `Path` | HERMES_HOME resolved path |
| `is_wsl()` | `bool` | WSL detection |
| `get_optional_skills_dir()` | `Path` | Optional skills directory |

---

## Key Numbers

| Metric | Value |
|--------|-------|
| run_agent.py lines | 11,024 |
| cli.py lines | 10,024 |
| model_tools.py lines | 562 |
| hermes_constants.py lines | 294 |
| Default max iterations | 90 |
| Default subagent iterations | 50 |
| Max tool workers | 8 |
| Parallel-safe tools | 12 |
| Never-parallel tools | 1 (clarify) |
| Path-scoped tools | 3 |
| API modes | 3 (chat_completions, codex_responses, anthropic_messages) |
| Fallback chain entries | Unlimited (list-based) |
| Prompt cache TTL | 5 minutes |
| Context pressure cooldown | 300 seconds |
| Compression passes max | 3 |
| Surrogate code point range | U+D800–U+DFFF |
| Tool definition source | TOOL_TO_TOOLSET_MAP |

---

*Generated from source analysis of the Hermes Agent codebase.*
