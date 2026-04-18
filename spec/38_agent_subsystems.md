# Hermes Agent — Agent Subsystems

This document covers the agent/ modules: error classification, insights engine, credential pool, context engine, context compressor, context references, auxiliary client, copilot ACP client, display, models_dev, usage pricing, rate limit tracker, memory manager, skill utilities, skill commands, memory provider, prompt caching, retry utilities, title generator, subdirectory hints, smart model routing, trajectory, redact, and manual compression feedback.

---

## Table of Contents

1. [Error Classifier](#1-error-classifier)
2. [Insights Engine](#2-insights-engine)
3. [Credential Pool](#3-credential-pool)
4. [Context Engine & Compressor](#4-context-engine--compressor)
5. [Auxiliary Client](#5-auxiliary-client)
6. [Copilot ACP Client](#6-copilot-acp-client)
7. [Display System](#7-display-system)
8. [Usage Pricing](#8-usage-pricing)
9. [Supporting Modules](#9-supporting-modules)

---

## 1. Error Classifier

### Location

`agent/error_classifier.py` (~820 lines)

### Purpose

Centralized API error taxonomy and priority-ordered classification pipeline. Determines the correct recovery action for every API failure.

### 1.1 Error Taxonomy

```python
class FailoverReason(enum.Enum):
    # Authentication / authorization
    auth = "auth"                        # 401/403 — refresh/rotate
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
    context_overflow = "context_overflow"  # Context too large — compress
    payload_too_large = "payload_too_large"  # 413 — compress

    # Model
    model_not_found = "model_not_found"  # 404 — fallback to different model

    # Request format
    format_error = "format_error"        # 400 — abort or strip + retry

    # Provider-specific
    thinking_signature = "thinking_signature"  # Anthropic thinking block invalid
    long_context_tier = "long_context_tier"    # Anthropic "extra usage" tier gate

    # Catch-all
    unknown = "unknown"                  # Unclassifiable — retry with backoff
```

### 1.2 Classification Result

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

### 1.3 Recovery Properties

```python
@property
def is_transient(self) -> bool:
    """Error likely to resolve on its own."""

@property
def requires_user_action(self) -> bool:
    """Error requires manual intervention."""
```

### 1.4 Classification Pipeline

Priority-ordered matching:
1. **Status code matching** (401 → auth, 402 → billing, 429 → rate_limit, 500 → server_error, 503 → overloaded)
2. **Message pattern matching** (regex on error body for provider-specific strings)
3. **Context-based disambiguation** (provider, model, request type)

### 1.5 Provider-Specific Patterns

| Provider | Pattern | Classification |
|----------|---------|----------------|
| Anthropic | "overloaded" | overloaded |
| Anthropic | "rate_limit" | rate_limit |
| Anthropic | "credit balance" | billing |
| OpenRouter | "credit" | billing |
| OpenRouter | "rate limit" | rate_limit |
| OpenAI | "context_length" | context_overflow |
| All | "authentication" | auth |
| All | "invalid_api_key" | auth |

### 1.6 Integration

Used by `run_agent.py` retry loop:

```python
classified = classify_error(exception, provider=..., model=...)
if classified.should_compress:
    compress_context()
if classified.should_rotate_credential:
    rotate_credential()
if classified.should_fallback:
    switch_provider()
if classified.retryable:
    backoff_and_retry()
```

---

## 2. Insights Engine

### Location

`agent/insights.py` (~789 lines)

### Purpose

Analyzes historical session data from SQLite to produce usage insights — token consumption, cost estimates, tool usage patterns, activity trends, and model/platform breakdowns.

**Inspired by**: Claude Code's `/insights` command.

### 2.1 Report Generation

```python
def generate(days: int = 30) -> InsightsReport:
    """Analyze sessions from the last N days."""
```

### 2.2 Metrics Tracked

| Metric | Source |
|--------|--------|
| Total sessions | SQLite session table |
| Token consumption | `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens` |
| Cost estimates | `usage_pricing.py` pricing data |
| Tool usage | Tool call counts per session |
| Activity trends | Sessions per day/hour |
| Model breakdown | Sessions grouped by model |
| Platform breakdown | Sessions grouped by platform |

### 2.3 Cost Estimation

```python
def _estimate_cost(session_or_model, ...) -> tuple[float, str]:
    """Returns (cost_usd, pricing_status)."""
```

Uses `CanonicalUsage` data model with per-model pricing from `DEFAULT_PRICING`.

### 2.4 Terminal Formatting

```python
def format_terminal(report: InsightsReport) -> str:
    """Render report as colored terminal output."""
```

Color-coded sections for positive metrics, warnings, and concerning trends.

### 2.5 Duration Formatting

```python
def format_duration_compact(seconds: float) -> str:
    """Returns human-readable duration: "2h 34m" or "45s"."""
```

---

## 3. Credential Pool

### Location

`agent/credential_pool.py` (~1,416 lines)

### Purpose

Persistent multi-credential pool for same-provider failover. Allows multiple API keys for a single provider with configurable selection strategies.

### 3.1 Pool Strategies

| Strategy | Behavior |
|----------|----------|
| `fill_first` | Use first credential until exhausted, then next |
| `round_robin` | Cycle through credentials in order |
| `random` | Random selection from available credentials |
| `least_used` | Select the credential with fewest total uses |

### 3.2 Credential States

| Status | Meaning |
|--------|---------|
| `ok` | Active and usable |
| `exhausted` | Rate-limited or out of credits |

### 3.3 Exhaustion Cooldown

```python
EXHAUSTED_TTL_429_SECONDS = 60 * 60      # 1 hour for rate limits
EXHAUSTED_TTL_DEFAULT_SECONDS = 60 * 60  # 1 hour default
```

Provider-supplied `reset_at` timestamps override these defaults.

### 3.4 Credential Model

```python
@dataclass
class PooledCredential:
    provider: str
    auth_type: str        # "api_key" or "oauth"
    source: str           # "manual" or auto-imported
    status: str           # "ok" or "exhausted"
    api_key: Optional[str]
    created_at: datetime
    last_used_at: Optional[datetime]
    use_count: int
```

### 3.5 Custom Provider Support

```python
CUSTOM_POOL_PREFIX = "custom:"
```

Custom OpenAI-compatible endpoints share `provider='custom'` but are keyed by name: `custom:<normalized_name>`.

### 3.6 Pool Operations

| Function | Purpose |
|----------|---------|
| `load_pool(provider)` | Load credentials for a provider |
| `save_pool(provider, pool)` | Persist credentials |
| `read_credential_pool()` | Read all pools |
| `write_credential_pool()` | Write all pools |
| `get_pool_strategy(provider)` | Get selection strategy |
| `label_from_token(token)` | Infer provider from token format |

### 3.7 Integration with Auth

Imports from `hermes_cli.auth` for auth store loading/saving, provider state management, Codex CLI token import/export, OAuth token refresh, and JWT claim decoding.

---

## 4. Context Engine & Compressor

### Location

- `agent/context_engine.py` (~184 lines)
- `agent/context_compressor.py` (~1,091 lines)
- `agent/context_references.py` (~520 lines)

### 4.1 Context Engine ABC

```python
class ContextEngine(ABC):
    @property
    @abstractmethod
    def name(self) -> str: ...

    # Token state (read by run_agent.py for display)
    last_prompt_tokens: int = 0
    last_completion_tokens: int = 0
    last_total_tokens: int = 0
    threshold_tokens: int = 0
    context_length: int = 0
    compression_count: int = 0

    # Compaction parameters
    threshold_percent: float = 0.75
    protect_first_n: int = 3
    protect_last_n: int = 6

    @abstractmethod
    def update_from_response(self, usage: Dict[str, Any]) -> None: ...
    @abstractmethod
    def should_compress(self, prompt_tokens: int = None) -> bool: ...
    @abstractmethod
    def compress(self, messages: List[Dict], ...) -> List[Dict]: ...
```

### 4.2 Lifecycle

1. Engine instantiated and registered
2. `on_session_start()` at conversation begin
3. `update_from_response()` after each API response
4. `should_compress()` checked after each turn
5. `compress()` when should_compress() returns True
6. `on_session_end()` at real boundaries (CLI exit, /reset, gateway expiry)

### 4.3 Selection

Config-driven: `context.engine` in `config.yaml`. Default is `"compressor"` (built-in). Third-party engines can replace via plugin system or `plugins/context_engine/<name>/`.

### 4.4 Context Compressor

The built-in implementation:
- Summarizes older conversation turns
- Protects first N and last N messages
- Triggers at `threshold_percent` of model context window
- Uses auxiliary LLM for summarization

### 4.5 Context References

Tracks which messages are referenced by tool results. Enables the compressor to protect referenced messages from summarization.

---

## 5. Auxiliary Client

### Location

`agent/auxiliary_client.py` (~2,615 lines)

### Purpose

Shared client router for side tasks (context compression, session search, web extraction, vision analysis). All consumers pick up the best available backend without duplicating fallback logic.

### 5.1 Text Task Resolution (auto mode)

| Priority | Provider | Config |
|----------|----------|--------|
| 1 | OpenRouter | `OPENROUTER_API_KEY` |
| 2 | Nous Portal | `~/.hermes/auth.json` active provider |
| 3 | Custom endpoint | `config.yaml model.base_url` + `OPENAI_API_KEY` |
| 4 | Codex OAuth | Responses API via chatgpt.com (gpt-5.3-codex) |
| 5 | Native Anthropic | `ANTHROPIC_API_KEY` |
| 6 | Direct API-key providers | z.ai/GLM, Kimi/Moonshot, MiniMax |
| 7 | None | — |

### 5.2 Vision Task Resolution (auto mode)

| Priority | Provider | Purpose |
|----------|----------|---------|
| 1 | Selected main provider | If it supports vision |
| 2 | OpenRouter | Fallback |
| 3 | Nous Portal | Fallback |
| 4 | Codex OAuth | gpt-5.3-codex vision |
| 5 | Native Anthropic | Fallback |
| 6 | Custom endpoint | Local vision models (Qwen-VL, LLaVA, Pixtral) |
| 7 | None | — |

### 5.3 Provider Aliases

```python
_PROVIDER_ALIASES = {
    "google": "gemini", "google-gemini": "gemini",
    "glm": "zai", "z-ai": "zai", "z.ai": "zai", "zhipu": "zai",
    "kimi": "kimi-coding", "moonshot": "kimi-coding",
    "claude": "anthropic", "claude-code": "anthropic",
}
```

### 5.4 Direct API Provider Aux Models

```python
_API_KEY_PROVIDER_AUX_MODELS = {
    "gemini": "gemini-3-flash-preview",
    "zai": "glm-4.5-flash",
    "kimi-coding": "kimi-k2-turbo-preview",
    "minimax": "MiniMax-M2.7",
}
```

### 5.5 Credit Exhaustion Fallback

When a provider returns HTTP 402 or a credit-related error, `call_llm()` automatically retries with the next available provider in the chain.

### 5.6 Per-Task Overrides

Configured in `config.yaml -> auxiliary:` section:
```yaml
auxiliary:
  vision:
    provider: "openrouter"
    model: "gpt-4o"
  compression:
    model: "gpt-4o-mini"
```

### 5.7 Stale Base URL Warning

Detects when `OPENAI_BASE_URL` is set to an outdated endpoint and warns once per process.

---

## 6. Copilot ACP Client

### Location

`agent/copilot_acp_client.py` (~570 lines)

### Purpose

OpenAI-compatible shim that forwards Hermes requests to `copilot --acp`. Lets Hermes treat the GitHub Copilot ACP server as a chat-style backend.

### 6.1 Architecture

```
Hermes -> OpenAI-compatible shim -> copilot --acp --stdio -> ACP server
```

Each request starts a short-lived ACP session, sends the conversation as a single prompt, collects text chunks, and converts back to OpenAI response shape.

### 6.2 Configuration

| Env Var | Purpose |
|---------|---------|
| `HERMES_COPILOT_ACP_COMMAND` | Override `copilot` command path |
| `COPILOT_CLI_PATH` | Fallback command path |
| `HERMES_COPILOT_ACP_ARGS` | Override CLI args (default: `--acp --stdio`) |

### 6.3 Tool Call Extraction

Parses ACP tool call format from the response text. Uses regex to extract tool call blocks with JSON in OpenAI function-call shape.

### 6.4 Timeout

```python
_DEFAULT_TIMEOUT_SECONDS = 900.0  # 15 minutes
```

---

## 7. Display System

### Location

`agent/display.py` (~1,037 lines)

### Purpose

Terminal display and formatting for the CLI TUI. Handles:
- Agent output rendering with syntax highlighting
- Status bar updates
- Progress indicators
- Tool call display formatting

---

## 8. Usage Pricing

### Location

`agent/usage_pricing.py` (~613 lines)

### Purpose

Per-model USD cost estimation from token counts.

### 8.1 Data Model

```python
class CanonicalUsage:
    input_tokens: int
    output_tokens: int
    cache_read_tokens: int
    cache_write_tokens: int
```

### 8.2 Pricing Lookup

```python
def estimate_usage_cost(model: str, usage: CanonicalUsage, ...) -> CostResult:
    """Returns amount_usd and pricing_status."""
```

Uses `DEFAULT_PRICING` dict mapping model names to per-million-token rates.

---

## 9. Supporting Modules

### 9.1 Rate Limit Tracker

**Location**: `agent/rate_limit_tracker.py` (~246 lines)

Tracks API rate limit headers and enforces backoff. Parses `Retry-After`, `X-RateLimit-Reset` headers from API responses.

### 9.2 Memory Manager

**Location**: `agent/memory_manager.py` (~361 lines)

In-memory conversation memory. Stores recent messages, handles retrieval for conversation continuity across sessions.

### 9.3 Skill Utilities

**Location**: `agent/skill_utils.py` (~465 lines)

Helper functions for skill loading, validation, and execution. Bridges between the skill system and the agent loop.

### 9.4 Skill Commands

**Location**: `agent/skill_commands.py` (~370 lines)

Skill-related CLI commands. Handles skill installation, removal, and listing from the command line.

### 9.5 Memory Provider

**Location**: `agent/memory_provider.py` (~231 lines)

Interface to pluggable memory backends. Routes memory operations to the configured provider (built-in, OpenViking, Honcho, etc.).

### 9.6 Prompt Caching

**Location**: `agent/prompt_caching.py` (~72 lines)

Prompt cache management. Tracks cached prefixes for provider prompt caching (Anthropic, OpenAI).

### 9.7 Retry Utilities

**Location**: `agent/retry_utils.py` (~57 lines)

Exponential backoff helpers for API retries. Calculates delay based on attempt count and server Retry-After headers.

### 9.8 Title Generator

**Location**: `agent/title_generator.py` (~125 lines)

Generates conversation titles from the first few messages. Uses a lightweight LLM call or heuristic extraction.

### 9.9 Subdirectory Hints

**Location**: `agent/subdirectory_hints.py` (~224 lines)

Detects project structure and suggests working directories. Helps the agent understand where to operate.

### 9.10 Smart Model Routing

**Location**: `agent/smart_model_routing.py` (~195 lines)

Intelligent model selection based on task type. Routes coding tasks to code-optimized models, reasoning tasks to reasoning models.

### 9.11 Trajectory

**Location**: `agent/trajectory.py` (~56 lines)

Conversation trajectory tracking. Records the sequence of turns, tool calls, and outcomes for analysis.

### 9.12 Redact

**Location**: `agent/redact.py` (~181 lines)

Sensitive text redaction. Strips API keys, tokens, and other secrets from output before display or logging.

### 9.13 Manual Compression Feedback

**Location**: `agent/manual_compression_feedback.py` (~49 lines)

Handles user feedback during manual context compression. Lets users select which messages to keep or summarize.

### 9.14 Models Dev

**Location**: `agent/models_dev.py` (~585 lines)

Development model definitions and experimental model configurations. Used for testing new provider integrations.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Error taxonomy categories | 13 |
| Credential pool strategies | 4 |
| Exhausted credential TTL | 1 hour |
| Auxiliary text providers | 6-tier chain |
| Auxiliary vision providers | 7-tier chain |
| Provider aliases | 14 |
| Context engine protection | first 3 + last 6 messages |
| Compression threshold | 75% of context window |
| Copilot ACP timeout | 900 seconds |
| Aux model cache staleness warning | once per process |
| Direct API aux models configured | 4 providers |
| Total agent/ module lines | ~16,434 |
| Total agent/ Python files | 25 |

---

*Generated from source analysis of the Hermes Agent codebase.*
