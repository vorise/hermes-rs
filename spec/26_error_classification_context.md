# Hermes Agent — Error Classification, Smart Routing, Anthropic Adapter & Context References

This document covers the API error classification pipeline, smart model routing, Anthropic Messages API adapter, context references (@file/@folder/@git/@url), subdirectory hint discovery, and the retry/jitter system.

---

## Table of Contents

1. [API Error Classification](#1-api-error-classification)
2. [Smart Model Routing](#2-smart-model-routing)
3. [Anthropic Adapter](#3-anthropic-adapter)
4. [Context References](#4-context-references)
5. [Subdirectory Hints](#5-subdirectory-hints)
6. [Retry & Jitter](#6-retry--jitter)
7. [Insights Engine](#7-insights-engine)
8. [Session Mirror](#8-session-mirror)
9. [Hook System](#9-hook-system)

---

## 1. API Error Classification

### Location

`agent/error_classifier.py` (~450 lines)

### Purpose

Centralized taxonomy of API errors with priority-ordered classification pipeline that determines the correct recovery action (retry, rotate credential, fallback to another provider, compress context, or abort).

### 1.1 FailoverReason Enum

```python
class FailoverReason(enum.Enum):
    # Authentication
    auth = "auth"                        # Transient (401/403) — refresh/rotate
    auth_permanent = "auth_permanent"    # Auth failed after refresh — abort

    # Billing / quota
    billing = "billing"                  # 402 or credit exhaustion — rotate
    rate_limit = "rate_limit"            # 429 or throttling — backoff then rotate

    # Server-side
    overloaded = "overloaded"            # 503/529 — backoff
    server_error = "server_error"        # 500/502 — retry

    # Transport
    timeout = "timeout"                  # Connection/read timeout — rebuild + retry

    # Context / payload
    context_overflow = "context_overflow"  # Too large — compress, not failover
    payload_too_large = "payload_too_large"  # 413 — compress payload

    # Model
    model_not_found = "model_not_found"  # 404 — fallback to different model

    # Request format
    format_error = "format_error"        # 400 — abort or strip + retry

    # Provider-specific
    thinking_signature = "thinking_signature"  # Anthropic thinking sig invalid
    long_context_tier = "long_context_tier"    # Anthropic "extra usage" tier gate

    # Catch-all
    unknown = "unknown"                  # Retry with backoff
```

### 1.2 ClassifiedError Dataclass

```python
@dataclass
class ClassifiedError:
    reason: FailoverReason
    status_code: Optional[int] = None
    provider: Optional[str] = None
    model: Optional[str] = None
    message: str = ""
    error_context: Dict[str, Any] = field(default_factory=dict)

    # Recovery action hints
    retryable: bool = True
    should_compress: bool = False
    should_rotate_credential: bool = False
    should_fallback: bool = False
```

### 1.3 Pattern Sets

**Billing patterns** (10 patterns):
- `insufficient credits`, `insufficient_quota`, `credit balance`, `credits have been exhausted`, `top up your credits`, `payment required`, `billing hard limit`, `exceeded your current quota`, `account is deactivated`, `plan does not include`

**Rate limit patterns** (10 patterns):
- `rate limit`, `too many requests`, `throttled`, `requests per minute`, `tokens per minute`, `try again in`, `please retry after`, `resource_exhausted`, `rate increased too quickly`

**Usage limit patterns** (ambiguous — need disambiguation):
- `usage limit`, `quota`, `limit exceeded`, `key limit exceeded`

Disambiguation: if transient signals present (`try again`, `retry`, `resets at`, `reset in`, `wait`, `requests remaining`, `periodic`, `window`) → rate_limit, else → billing.

**Context overflow patterns** (20+ patterns):
- English: `context length`, `token limit`, `too many tokens`, `reduce the length`, `exceeds the limit`, `context window`, `prompt is too long`
- vLLM: `exceeds the max_model_len`, `max_model_len`
- Ollama: `context length exceeded`, `truncating input`
- llama.cpp: `slot context`, `n_ctx_slot`
- Chinese: `超过最大长度`, `上下文长度`

**Auth patterns** (9 patterns):
- `invalid api key`, `authentication`, `unauthorized`, `forbidden`, `invalid token`, `token expired`, `token revoked`, `access denied`

**Server disconnect patterns** (7 patterns):
- `server disconnected`, `peer closed connection`, `connection reset by peer`, `connection was closed`, `network connection lost`, `unexpected eof`, `incomplete chunked read`

**Transport errors** (13 types):
- `ReadTimeout`, `ConnectTimeout`, `PoolTimeout`, `ConnectError`, `RemoteProtocolError`, `ConnectionError`, `APIConnectionError`, `APITimeoutError`, etc.

### 1.4 Classification Pipeline (Priority-Ordered)

```
classify_api_error(error, provider, model, approx_tokens, context_length)

1. Provider-specific patterns (highest priority)
   - Thinking signature invalid: status=400 + "signature" + "thinking"
   - Long-context tier gate: status=429 + "extra usage" + "long context"

2. HTTP status code classification
   - 400: Check if format_error or model_not_found
   - 401: auth (retryable)
   - 402: billing (rotate credential) or rate_limit (if disambiguation signals)
   - 403: auth or auth_permanent
   - 404: model_not_found (fallback)
   - 413: payload_too_large
   - 429: rate_limit or billing (disambiguation) or long_context_tier
   - 500: server_error (retryable)
   - 502: server_error (retryable)
   - 503: overloaded (backoff)
   - 529: overloaded (backoff)

3. Error code classification (from body)
   - "invalid_api_key" → auth
   - "rate_limit_exceeded" → rate_limit
   - "context_length_exceeded" → context_overflow

4. Message pattern matching (no status code)
   - Match against all pattern sets above

5. Server disconnect + large session → context overflow
   - is_disconnect AND (tokens > 60% context OR tokens > 120K OR messages > 200)
   - MUST come before generic transport catch

6. Transport / timeout heuristics
   - Error type in _TRANSPORT_ERROR_TYPES → timeout (retryable)

7. Fallback: unknown (retryable with backoff)
```

### 1.5 Error Message Assembly

Multiple sources combined for pattern matching:
1. `str(error)` — SDK exception message
2. Error body `.message` — structured response body
3. Metadata `.raw` — OpenRouter wraps provider errors inside `{"error": {"message": "Provider returned error", "metadata": {"raw": "<actual error>"}}}`

### 1.6 Context Overflow Heuristic

When no explicit status code but disconnect occurs:
```python
is_large = (approx_tokens > context_length * 0.6
            or approx_tokens > 120000
            or num_messages > 200)
```

---

## 2. Smart Model Routing

### Location

`agent/smart_model_routing.py` (~196 lines)

### Purpose

Per-turn model routing based on message complexity. Simple queries routed to cheaper models, complex work stays on the primary model.

### 2.1 Complexity Detection

**Complexity keywords** (40+ keywords that trigger primary model):
```python
_COMPLEX_KEYWORDS = {
    "debug", "implement", "implementation", "refactor", "patch",
    "traceback", "stacktrace", "exception", "error",
    "analyze", "analysis", "investigate", "architecture", "design",
    "compare", "benchmark", "optimize", "review",
    "terminal", "shell", "tool", "tools", "pytest", "test",
    "plan", "planning", "delegate", "subagent",
    "cron", "docker", "kubernetes",
}
```

**Simple message criteria** (ALL must be true for cheap routing):
- Length ≤ `max_simple_chars` (default 160)
- Word count ≤ `max_simple_words` (default 28)
- No newlines (≤ 1 newline)
- No code blocks (no ``` or `)
- No URLs
- No complex keywords

### 2.2 Routing Resolution

```python
resolve_turn_route(user_message, routing_config, primary) → {
    "model": "...",
    "runtime": {api_key, base_url, provider, api_mode, command, args, credential_pool},
    "label": "smart route → model (provider)",  # or None if using primary
    "signature": (model, provider, base_url, api_mode, command, (args,)),
}
```

The signature tuple is used for agent caching — different routes create different cached agents.

### 2.3 Fallback Safety

If cheap model resolution fails (provider not configured, auth error during resolution), falls back to primary model silently.

---

## 3. Anthropic Adapter

### Location

`agent/anthropic_adapter.py` (~600+ lines)

### Purpose

Translates between Hermes's internal OpenAI-style message format and Anthropic's Messages API. Isolates all provider-specific logic.

### 3.1 Authentication Modes

| Token Type | Detection | Auth Method |
|-----------|-----------|-------------|
| API key (`sk-ant-api*`) | `key.startswith("sk-ant-api")` | `x-api-key` header |
| OAuth/setup token (`sk-ant-oat*`) | `key.startswith("sk-ant-")` | Bearer auth + beta headers |
| JWT (`eyJ*`) | `key.startswith("eyJ")` | Bearer auth + beta headers |
| Claude Code credentials | Read from `~/.claude/.credentials.json` | Bearer auth + Claude Code identity |

### 3.2 Third-Party Endpoint Detection

```python
def _is_third_party_anthropic_endpoint(base_url):
    # No base_url → direct Anthropic API (OAuth applies)
    # Contains "anthropic.com" → direct (OAuth applies)
    # Any other → third-party proxy (Azure, Bedrock, etc.) — skip OAuth, use x-api-key
```

**Bearer-auth override**: MiniMax endpoints (`api.minimax.io/anthropic`, `api.minimaxi.com/anthropic`) require Bearer auth even for regular API keys.

### 3.3 Beta Headers

| Beta | Applied When |
|------|-------------|
| `interleaved-thinking-2025-05-14` | All requests |
| `fine-grained-tool-streaming-2025-05-14` | All except Bearer-auth endpoints (MiniMax breaks on this) |
| `fast-mode-2026-02-01` | When speed="fast" parameter used (Opus 4.6 ~2.5x throughput) |
| `claude-code-20250219` | OAuth requests only |
| `oauth-2025-04-20` | OAuth requests only |

### 3.4 Claude Code Identity Spoofing

Required for OAuth requests to avoid intermittent 500s from Anthropic's infrastructure:
```python
"user-agent": f"claude-cli/{version} (external, cli)"
"x-app": "cli"
```

Version detection: runs `claude --version` or `claude-code --version`, parses output. Falls back to `2.1.74`.

### 3.5 Max Output Limits

```python
_ANTHROPIC_OUTPUT_LIMITS = {
    "claude-opus-4-6":      128_000,
    "claude-sonnet-4-6":     64_000,
    "claude-opus-4-5":       64_000,
    "claude-sonnet-4-5":     64_000,
    "claude-haiku-4-5":      64_000,
    "claude-3-7-sonnet":    128_000,
    "claude-3-5-sonnet":      8_192,
    "claude-3-opus":          4_096,
    ...
}
```

Longest-prefix substring match for date-stamped model IDs (`claude-sonnet-4-5-20250929`) and variant suffixes (`:1m`, `:fast`). Dots normalized to hyphens.

Default for unknown models: 128,000.

### 3.6 Thinking Support

```python
THINKING_BUDGET = {"xhigh": 32000, "high": 16000, "medium": 8000, "low": 4000}
ADAPTIVE_EFFORT_MAP = {"xhigh": "max", "high": "high", "medium": "medium", "low": "low", "minimal": "low"}
```

Adaptive thinking: Claude 4.6 models (`"4-6"` or `"4.6"` in model name).

### 3.7 OAuth Token Refresh

```python
refresh_anthropic_oauth_pure(refresh_token, use_json=False)
```

Two endpoints tried in order:
1. `https://platform.claude.com/v1/oauth/token`
2. `https://console.anthropic.com/v1/oauth/token`

Both form-encoded and JSON content types supported.

### 3.8 Claude Code Credentials

Reads from `~/.claude/.credentials.json`:
```json
{
  "claudeAiOauth": {
    "accessToken": "...",
    "refreshToken": "...",
    "expiresAt": 1234567890000
  }
}
```

Validity check: `now_ms < (expiresAt - 60_000)` — 60-second buffer.

Does NOT read `~/.claude.json` `primaryApiKey` — that's Claude's managed key, not refreshable.

---

## 4. Context References

### Location

`agent/context_references.py` (~200+ lines)

### Purpose

Parses `@file`, `@folder`, `@git`, `@url`, `@diff`, `@staged` references from user messages and expands them into inline context before the agent processes the message.

### 4.1 Reference Syntax

```
@file:path/to/file.py          # File content (with optional line range)
@file:`path with spaces.py`    # Quoted path
@file:path/to/file.py:10-20    # Line range
@folder:src/components         # Directory listing
@git                           # Git status
@url:https://example.com       # URL content fetch
@diff                          # Git diff
@staged                        # Git staged changes
```

### 4.2 Parsing Regex

```python
REFERENCE_PATTERN = re.compile(
    r'(?<![\w/])@(?:(?P<simple>diff|staged)\b|'
    r'(?P<kind>file|folder|git|url):'
    r'(?P<value>`[^`\n]+`|"[^"\n]+"|\'[^\']+\')|\S+))'
)
```

### 4.3 Security Guards

**Sensitive directory blocks**: `.ssh`, `.aws`, `.gnupg`, `.kube`, `.docker`, `.azure`, `.config/gh`

**Sensitive file blocks**: `~/.ssh/authorized_keys`, `~/.ssh/id_rsa`, `~/.bashrc`, `~/.zshrc`, `~/.netrc`, `~/.pgpass`, `~/.npmrc`, `~/.pypirc`

**Hermes internal blocks**: `skills/.hub`

**Allowed root**: Defaults to CWD — references cannot escape the active workspace unless caller explicitly widens the root.

### 4.4 Token Budget Limits

| Limit | Threshold | Behavior |
|-------|-----------|----------|
| Soft limit | 25% of context_length | Warning injected |
| Hard limit | 50% of context_length | Entire expansion blocked |

### 4.5 Async/Safe Execution

```python
preprocess_context_references(message, cwd, context_length, url_fetcher, allowed_root)
```

Uses `asyncio.get_running_loop()` detection — safe for both CLI (no loop) and gateway (loop already running). When loop is running, uses `ThreadPoolExecutor` to run `asyncio.run(coro)` without blocking the event loop.

### 4.6 Result Format

```python
ContextReferenceResult(
    message="user message with @ref replaced --- Attached Context --- [expanded content]",
    original_message="original user message",
    references=[ContextReference(...)],
    warnings=["injection warning: 150K tokens exceeds soft limit"],
    injected_tokens=150000,
    expanded=True,
    blocked=False,
)
```

---

## 5. Subdirectory Hints

### Location

`agent/subdirectory_hints.py` (~200 lines)

### Purpose

As the agent navigates into subdirectories via tool calls, discovers and loads project context files (AGENTS.md, CLAUDE.md, .cursorrules) from those directories. Complements startup context loading which only loads from CWD.

### 5.1 Hint Files

```python
_HINT_FILENAMES = [
    "AGENTS.md", "agents.md",
    "CLAUDE.md", "claude.md",
    ".cursorrules",
]
```

### 5.2 Discovery Algorithm

1. After each tool call, extract paths from tool args
2. Resolve to absolute path, take parent directory
3. Walk up to `_MAX_ANCESTOR_WALK` (5) ancestor directories
4. For each new directory, check for hint files
5. Load all found hints (not first-wins — multiple hints from different directories)
6. Mark directories as `_loaded_dirs` to avoid re-scanning

### 5.3 Path Extraction

**Direct path args**: `path`, `file_path`, `workdir`

**Shell command parsing**: For `terminal` tool, splits command with `shlex.split()`, extracts path-like tokens (contains `/` or `.`, not a URL or flag).

### 5.4 Injection Strategy

Hints appended to tool result strings — NOT system prompt modifications. This preserves prompt caching.

Max chars per hint file: 8,000. Truncated with `[...truncated {filename}: {len:,} chars total]`.

### 5.5 Security Scanning

Hint files pass through `_scan_context_content()` (same security scan as startup context loading in `prompt_builder.py`).

---

## 6. Retry & Jitter

### Location

`agent/retry_utils.py` (~58 lines)

### Purpose

Jittered exponential backoff to prevent thundering-herd retry spikes when multiple sessions hit the same rate-limited provider concurrently.

### 6.1 Algorithm

```python
jittered_backoff(attempt, base_delay=5.0, max_delay=120.0, jitter_ratio=0.5)
```

- Exponential: `min(base_delay * 2^(attempt-1), max_delay)`
- Jitter: uniform random in `[0, jitter_ratio * delay]`
- Seed: `time.time_ns() ^ (counter * 0x9E3779B9)` — decorrelates even with coarse clocks
- Counter: monotonic thread-safe counter (`_jitter_counter`)
- Cap: attempt 63+ always returns max_delay

### 6.2 Why Not Fixed Backoff

With N concurrent sessions hitting the same provider, fixed exponential backoff causes all sessions to retry at the same instant, perpetuating the rate limit. Jitter spreads retries across the delay window.

---

## 7. Insights Engine

### Location

`agent/insights.py` (~400+ lines)

### Purpose

Analyzes historical session data from SQLite to produce usage insights — token consumption, cost estimates, tool usage patterns, activity trends, model/platform breakdowns.

### 7.1 Report Generation

```python
engine.generate(days=30, source=None) → {
    "overview": {total_sessions, total_tokens, total_cost, avg_session_duration, ...},
    "models": [{model, sessions, tokens, cost, percentage}, ...],
    "platforms": [{platform, sessions, tokens, cost}, ...],
    "tools": [{tool, count, percentage}, ...],
    "activity": {hourly_distribution, daily_distribution},
    "top_sessions": [{session_id, model, tokens, duration, cost}, ...],
}
```

### 7.2 SQL Queries

Pre-computed query strings (evaluated at class definition, not runtime):
```python
_SESSION_COLS = ("id, source, model, started_at, ended_at, "
                 "message_count, tool_call_count, input_tokens, output_tokens, "
                 "cache_read_tokens, cache_write_tokens, billing_provider, "
                 "billing_base_url, billing_mode, estimated_cost_usd, "
                 "actual_cost_usd, cost_status, cost_source")
```

### 7.3 Tool Usage Detection

Two sources combined:
1. `tool_name` column on `tool` role messages (set by gateway)
2. `tool_calls` JSON on `assistant` role messages (covers CLI where tool_name column isn't set)

### 7.4 Cost Estimation

Uses `usage_pricing.estimate_usage_cost()` with model/provider/base_url for accurate per-model pricing.

### 7.5 Terminal Display

```python
engine.format_terminal(report) → str
```

ASCII bar charts, formatted tables, compact duration strings.

---

## 8. Session Mirror

### Location

`gateway/mirror.py` (~133 lines)

### Purpose

Cross-platform message delivery mirroring. When a message is sent to a platform (via send_message or cron delivery), appends a "delivery-mirror" record to the target session's transcript so the receiving-side agent has context.

### 8.1 Mirror Message

```python
{
    "role": "assistant",
    "content": message_text,
    "timestamp": "...",
    "mirror": True,
    "mirror_source": "cli" | "cron" | "gateway",
}
```

### 8.2 Session Lookup

Scans `sessions.json` entries matching platform + chat_id + optional thread_id. Selects most recently updated match.

### 8.3 Dual Write

Writes to both:
1. JSONL transcript file: `~/.hermes/sessions/{session_id}.jsonl`
2. SQLite session database: `SessionDB.append_message()`

All errors caught — mirror is never fatal to the main flow.

---

## 9. Hook System

### Location

`gateway/hooks.py` (~171 lines)

### Purpose

Event-driven extension system. Hooks discovered from `~/.hermes/hooks/` directories, each containing a `HOOK.yaml` manifest and `handler.py` Python module.

### 9.1 Hook Structure

```
~/.hermes/hooks/
├── my-hook/
│   ├── HOOK.yaml
│   └── handler.py
└── another-hook/
    ├── HOOK.yaml
    └── handler.py
```

**HOOK.yaml**:
```yaml
name: my-hook
description: Does something on agent start
events:
  - agent:start
  - session:start
```

**handler.py**:
```python
async def handle(event_type, context):
    # context is a dict with event-specific data
    pass
```

### 9.2 Built-in Hooks

- **boot-md**: Runs `~/.hermes/BOOT.md` on gateway startup. Registered automatically.

### 9.3 Event Types

| Event | When Fired |
|-------|-----------|
| `gateway:startup` | Gateway process starts |
| `session:start` | New session created |
| `session:end` | Session ends (/new or /reset) |
| `session:reset` | Session reset completed |
| `agent:start` | Agent begins processing |
| `agent:step` | Each turn in tool-calling loop |
| `agent:end` | Agent finishes processing |
| `command:*` | Any slash command (wildcard) |

### 9.4 Wildcard Matching

`command:*` matches any `command:reset`, `command:model`, etc. Only exact prefix + `:*` matching — `agent` does NOT match `agent:start`.

### 9.5 Error Isolation

Hook errors caught and logged but never block the main pipeline.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Failover reasons | 14 |
| Billing patterns | 10 |
| Rate limit patterns | 10 |
| Context overflow patterns | 20+ |
| Auth patterns | 9 |
| Transport error types | 13 |
| Server disconnect patterns | 7 |
| Complex routing keywords | 40+ |
| Anthropic output limits tracked | 12+ models |
| Thinking budget levels | 4 (xhigh/high/medium/low) |
| Beta headers | 5 |
| Context ref types | 6 (file, folder, git, url, diff, staged) |
| Context ref soft limit | 25% of context |
| Context ref hard limit | 50% of context |
| Hint files tracked | 6 filenames |
| Max ancestor walk | 5 levels |
| Max hint chars | 8,000 |
| Retry base delay | 5s |
| Retry max delay | 120s |
| Jitter ratio | 0.5 |
| Event types | 7 (+ wildcard) |
| Sensitive directories blocked | 7 |
| Sensitive files blocked | 12 |

---

*Generated from source analysis of the Hermes Agent codebase.*
