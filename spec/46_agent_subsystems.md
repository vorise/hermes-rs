# Hermes Agent — Agent Subsystems

This document covers `agent/auxiliary_client.py` (~2,615 lines), `agent/anthropic_adapter.py` (~1,411 lines), and `agent/credential_pool.py` (~1,416 lines).

---

## Table of Contents

1. [Auxiliary Client](#1-auxiliary-client)
2. [Anthropic Adapter](#2-anthropic-adapter)
3. [Credential Pool](#3-credential-pool)

---

## 1. Auxiliary Client

### Location

`agent/auxiliary_client.py` (~2,615 lines)

### Purpose

Provides OpenAI-client-compatible API shim for side tasks (context compression, vision analysis, search) across multiple provider backends. Allows consumers to call `client.chat.completions.create(**kwargs)` regardless of the underlying provider.

### 1.1 Architecture

```
Consumer (compression, vision, search)
    ↓ calls client.chat.completions.create()
AuxiliaryClient (shim)
    ↓ routes to provider-specific adapter
Provider API (OpenAI, Anthropic, Codex, Nous, Custom)
```

### 1.2 Supported Backends

| Backend | Adapter Class | Protocol |
|---------|--------------|----------|
| OpenAI chat completions | Direct `OpenAI` client | OpenAI |
| Codex Responses | `_CodexCompletionsAdapter` | Responses API |
| Anthropic Messages | `_AnthropicCompletionsAdapter` | Messages API |
| Nous Portal | OpenAI-compatible | chat completions |
| Custom endpoint | OpenAI-compatible | chat completions |

### 1.3 Codex Responses Adapter

```python
class _CodexCompletionsAdapter:
    """Drop-in shim: chat.completions.create() → Codex Responses API."""
```

**Content conversion** (`_convert_content_for_responses()`):
- `{"type": "text", "text": "..."}` → `{"type": "input_text", "text": "..."}`
- `{"type": "image_url", "image_url": {"url": "..."}}` → `{"type": "input_image", "image_url": "..."}`

**Streaming**: Uses `client.responses.stream()` with event collection:
- `response.output_item.done` → collects output items
- `output_text.delta` → collects text deltas
- `function_call` events → tracks tool calls

**Backfill logic**: If `final.output` is empty, backfills from collected stream events:
- If output items collected → use those
- If only text deltas and no function calls → synthesize message from deltas

**Response shaping**: Builds `chat.completions`-style response with `choices[0].message`, `finish_reason`, and `usage`.

### 1.4 Anthropic Messages Adapter

```python
class _AnthropicCompletionsAdapter:
    """OpenAI-client-compatible adapter for Anthropic Messages API."""
```

**Conversion**:
- Calls `build_anthropic_kwargs()` to convert OpenAI-format kwargs to Anthropic-format
- Calls `normalize_anthropic_response()` to convert Anthropic response back to OpenAI-format
- Maps usage: `input_tokens` → `prompt_tokens`, `output_tokens` → `completion_tokens`

### 1.5 URL Rewriting

```python
def _to_openai_base_url(base_url: str) -> str:
    # Rewrites /anthropic → /v1 for providers with dual endpoints (e.g. MiniMax)
```

### 1.6 Vision Backend Resolution

```python
def get_available_vision_backends() -> list[str]:
```

Checks which vision providers have valid credentials. Returns list of available backend names (e.g., `["openrouter", "openai", "nous"]`).

### 1.7 Text Auxiliary Client Resolution

**Resolution chain** (single selection, first match wins):
1. **OpenRouter** → `OPENROUTER_API_KEY`
2. **Nous** → pool entry or auth.json
3. **Custom** → matching `custom_providers` entry by base URL
4. **Codex** → `openai-codex` pool entry or auth store
5. **Anthropic** → pool entry or credentials
6. **API-key providers** → direct env var resolution (Z.AI, Kimi, MiniMax, etc.)

Each provider maps to a default auxiliary model from `_API_KEY_PROVIDER_AUX_MODELS`.

### 1.8 Vision Auxiliary Client Resolution

**Resolution chain**:
1. OpenRouter (Gemini)
2. OpenAI-compatible endpoint (custom base URL + key + model)
3. Provider-specific vision overrides from config (`auxiliary.vision`)

**Custom vision provider support**: Allows custom endpoint as vision backend with base URL, API key, and model name.

### 1.9 Auxiliary Model Defaults

```python
_API_KEY_PROVIDER_AUX_MODELS = {
    "openrouter": "google/gemini-3-flash-preview",
    "nous": "nousresearch/hermes-3-llama-3.1-70b",
    "zai": "glm-5",
    "kimi-coding": "kimi-k2.5",
    "minimax": "minimax-m2.5",
    # ...
}

_PROVIDER_VISION_MODELS = {
    "openai": "gpt-4o-mini",
    "anthropic": "claude-sonnet-4-5",
    # ...
}
```

### 1.10 Client Wrappers

| Class | Purpose |
|-------|---------|
| `CodexAuxiliaryClient` | Sync Codex via Responses API |
| `AsyncCodexAuxiliaryClient` | Async Codex via `asyncio.to_thread()` |
| `AnthropicAuxiliaryClient` | Sync Anthropic via Messages API |
| `AsyncAnthropicAuxiliaryClient` | Async Anthropic via `asyncio.to_thread()` |

---

## 2. Anthropic Adapter

### Location

`agent/anthropic_adapter.py` (~1,411 lines)

### Purpose

Anthropic Messages API adapter with OAuth support, thinking budget management, per-model output limits, and Claude Code identity fingerprinting.

### 2.1 Thinking Budget

```python
THINKING_BUDGET = {
    "xhigh": 32000,
    "high": 16000,
    "medium": 8000,
    "low": 4000,
}
```

### 2.2 Per-Model Output Limits

```python
_ANTHROPIC_OUTPUT_LIMITS = {
    "claude-opus-4-6": 131072,   # 128K
    "claude-sonnet-4-6": 65536,  # 64K
    "claude-sonnet-4-5": 65536,
    "claude-sonnet-4": 65536,
    "claude-haiku-4-5": 65536,
    "claude-3-5-haiku": 8192,
    # ...
}
```

### 2.3 Beta Headers

```python
_COMMON_BETAS = [
    "interleaved-thinking-2025-06-03",
    "fine-grained-tool-streaming",
    "fast-mode-2025-08",
]
_OAUTH_ONLY_BETAS = [
    "claude-code",
    "oauth-2025-04-20",
]
```

**MiniMax Bearer-auth endpoints**: `fine-grained-tool-streaming` beta stripped (causes connection errors on every tool-use message).

### 2.4 OAuth Token Detection

```python
def _is_oauth_token(key: str) -> bool:
    # sk-ant-api* → False (regular API keys, use x-api-key)
    # sk-ant-*   → True (setup tokens, managed keys, use Bearer)
    # eyJ*       → True (JWTs from OAuth flow, use Bearer)
```

### 2.5 Third-Party Endpoint Detection

```python
def _is_third_party_anthropic_endpoint(base_url) -> bool:
    # "anthropic.com" in URL → False (direct Anthropic API)
    # Anything else → True (Azure AI Foundry, AWS Bedrock, etc.)
```

Third-party proxies use their own API keys via `x-api-key`, never OAuth.

### 2.6 Bearer Auth Providers

```python
def _requires_bearer_auth(base_url) -> bool:
    # MiniMax global and China endpoints require Bearer auth
    return base_url.startswith(("https://api.minimax.io/anthropic",
                                "https://api.minimaxi.com/anthropic"))
```

### 2.7 Client Building

```python
def build_anthropic_client(api_key, base_url=None):
```

**Auth routing** (checked in order):
1. **Bearer-auth providers** (MiniMax) → `auth_token=api_key`
2. **Third-party proxy** → `api_key=api_key` (x-api-key)
3. **OAuth token** → `auth_token=api_key` + Claude Code user-agent + all betas
4. **Regular API key** → `api_key=api_key` + common betas

**Claude Code fingerprint** (for OAuth):
```python
"user-agent": f"claude-cli/{version} (external, cli)",
"x-app": "cli",
```
Without this fingerprint, OAuth requests get intermittent 500 errors from Anthropic.

### 2.8 Claude Code Credentials

```python
def read_claude_code_credentials() -> dict:
    # Reads ~/.claude/.credentials.json → claudeAiOauth
    # Returns {accessToken, refreshToken, expiresAt}

def read_claude_managed_key() -> str:
    # Reads ~/.claude.json → primaryApiKey (diagnostics only)
```

### 2.9 OAuth Token Refresh

```python
def refresh_anthropic_oauth_pure(refresh_token, use_json=False):
    # POST to Anthropic OAuth token endpoint
    # Returns {access_token, refresh_token, expires_at_ms}
```

### 2.10 Anthropic kwargs Builder

```python
def build_anthropic_kwargs(model, messages, tools, max_tokens, reasoning_config, tool_choice, is_oauth):
```

Converts OpenAI-format messages to Anthropic-format:
- System messages → `system` parameter
- User/assistant messages → `messages` list with `role` + `content`
- Tool calls → `tools` with input schemas
- Tool results → `tool_use_id` references
- Image content → `{"type": "image", "source": {"type": "base64", ...}}`

### 2.11 Response Normalization

```python
def normalize_anthropic_response(response):
    # Extracts assistant message, tool_calls, finish_reason
    # Maps Anthropic usage to OpenAI-format
```

### 2.12 Claude Code Version Detection

```python
def _detect_claude_code_version() -> str:
    # Tries: Claude Code binary version → fallback constant
_CLAUDE_CODE_VERSION_FALLBACK = "1.0.0"
```

---

## 3. Credential Pool

### Location

`agent/credential_pool.py` (~1,416 lines)

### Purpose

Multi-credential pool per provider for failover and rotation. Supports OAuth token refresh, exhaustion cooldown, concurrent request tracking, and automatic sync with external credential files.

### 3.1 PooledCredential Dataclass

```python
@dataclass
class PooledCredential:
    id: str                    # Unique identifier (UUID)
    provider: str              # Provider slug
    auth_type: str             # "oauth", "api_key", "external_process"
    access_token: str
    refresh_token: Optional[str]
    expires_at_ms: Optional[int]
    agent_key: Optional[str]           # Nous-specific
    agent_key_expires_at: Optional[str]
    portal_base_url: Optional[str]
    inference_base_url: Optional[str]
    base_url: Optional[str]
    client_id: Optional[str]
    scope: Optional[str]
    token_type: str            # "Bearer"
    source: str                # "manual", "device_code", "claude_code", etc.
    priority: int              # Selection priority (lower = preferred)
    label: Optional[str]       # User-friendly label
    tls: Optional[str]         # TLS mode for Nous
    extra: dict                # Provider-specific extras
    # Runtime state (not persisted to config):
    last_status: Optional[str]       # None | "exhausted"
    last_status_at: Optional[float]
    last_error_code: Optional[int]
    last_error_reason: Optional[str]
    last_error_message: Optional[str]
    last_error_reset_at: Optional[float]
    last_refresh: Optional[float]
```

### 3.2 Constants

```python
EXHAUSTED_TTL_429_SECONDS = 3600       # 1 hour for rate limit
EXHAUSTED_TTL_DEFAULT_SECONDS = 300    # 5 minutes for other errors
STATUS_EXHAUSTED = "exhausted"
AUTH_TYPE_OAUTH = "oauth"
SOURCE_MANUAL = "manual"
CUSTOM_POOL_PREFIX = "custom:"

SUPPORTED_POOL_STRATEGIES = {"fill_first", "round_robin", "random", "least_used"}
STRATEGY_FILL_FIRST = "fill_first"  # Default
```

### 3.3 Extra Keys (JSON-only fields)

```python
_EXTRA_KEYS = frozenset({
    "obtained_at", "expires_in", "expires_at",
    "agent_key_id", "agent_key_expires_in", "agent_key_reused",
    "agent_key_obtained_at",
})
```

These fields are only valid in JSON storage, not in CLI `--extra` flags.

### 3.4 CredentialPool Class

```python
class CredentialPool:
    provider: str
    _entries: List[PooledCredential]  # Sorted by priority
    _current_id: Optional[str]
    _strategy: str
    _lock: threading.Lock
    _active_leases: Dict[str, int]    # entry_id → count
    _max_concurrent: int = 1
```

### 3.5 Selection Strategies

| Strategy | Behavior |
|----------|----------|
| `fill_first` | Always use first healthy entry (sticky) |
| `round_robin` | Rotate to next healthy entry after each selection |
| `random` | Pick random healthy entry |
| `least_used` | Pick entry with fewest active leases |

### 3.6 Selection Logic (`pool.select()`)

1. Filter out exhausted entries (cooldown not expired)
2. Sync OAuth entries from external files (Claude Code, Codex CLI)
3. Try to refresh expired OAuth entries
4. Apply selection strategy to remaining entries
5. If no healthy entry found → try refreshing any OAuth entry (force)
6. If still nothing → raise `CredentialPoolExhausted`

### 3.7 OAuth Token Sync

**Anthropic / Claude Code** (`_sync_anthropic_entry_from_credentials_file()`):
- Reads `~/.claude/.credentials.json`
- If refresh token differs → syncs new token pair
- Prevents stale single-use refresh tokens

**OpenAI Codex** (`_sync_codex_entry_from_cli()`):
- Reads `~/.codex/auth.json`
- If refresh token differs → syncs new token pair

**Device code to auth store** (`_sync_device_code_entry_to_auth_store()`):
- After pool-level OAuth refresh, writes fresh tokens back to `auth.json`
- Prevents `load_pool()` from re-seeding stale tokens
- Applies to `nous` and `openai-codex` providers

### 3.8 OAuth Token Refresh

```python
def _refresh_entry(entry, *, force=False) -> Optional[PooledCredential]:
```

**Per-provider refresh**:
- **Anthropic**: `refresh_anthropic_oauth_pure()` → writes back to `~/.claude/.credentials.json` if source is `claude_code`
- **OpenAI Codex**: Pre-syncs from CLI, then `refresh_codex_oauth_pure()`
- **Nous**: `refresh_nous_oauth_from_state()` → updates auth.json via `_sync_device_code_entry_to_auth_store()`
- **Others**: No refresh support

**Retry on consumed token**: For Anthropic `claude_code` entries, if refresh fails (token consumed by another process), syncs from credentials file and retries once.

### 3.9 Exhaustion Handling

```python
def _mark_exhausted(entry, status_code, error_context=None):
```

**Error context normalization** (`_normalize_error_context()`):
- Extracts `reason`, `message` from error
- Parses `reset_at` from: `reset_at`, `resets_at`, `retry_until` fields
- Falls back to extracting retry delay from error message text
- Parses timestamps: epoch seconds, epoch milliseconds, ISO-8601

**Exhaustion cooldown** (`_exhausted_until()`):
1. Uses explicit `reset_at` if available
2. Falls back to `last_status_at + TTL` (1 hour for 429, 5 min for others)

### 3.10 Concurrent Request Tracking

```python
def acquire(entry_id) -> bool:   # Increment active lease count
def release(entry_id) -> bool:   # Decrement active lease count
```

Default max concurrent per credential: 1.

### 3.11 Custom Provider Pools

```python
def get_custom_provider_pool_key(base_url) -> Optional[str]:
    # Matches base_url against custom_providers in config.yaml
    # Returns "custom:<normalized-name>" or None

def list_custom_pool_providers() -> List[str]:
    # Returns all "custom:*" pool keys with entries in auth.json
```

### 3.12 Pool Loading

```python
def load_pool(provider: str) -> CredentialPool:
```

**Seeding order**:
1. Load persisted entries from `auth.json` credential pool
2. Seed from singleton auth state (e.g., active provider tokens)
3. Deduplicate by (access_token, base_url) pairs

### 3.13 Pool Strategy Configuration

```python
def get_pool_strategy(provider) -> str:
    # Reads config.yaml → credential_pool_strategies[provider]
    # Default: "fill_first"

def set_pool_strategy(config, provider, strategy) -> None:
    # Writes to config.yaml credential_pool_strategies dict
```

### 3.14 Timestamp Parsing

```python
def _parse_absolute_timestamp(value) -> Optional[float]:
    # Accepts: epoch seconds, epoch milliseconds, ISO-8601 strings
    # Returns seconds since epoch
    # Heuristic: > 1,000,000,000,000 → milliseconds → divide by 1000
```

### 3.15 Retry Delay Extraction

```python
def _extract_retry_delay_seconds(message) -> Optional[float]:
    # Matches: "quotaResetDelay": 300ms, "retry after 30 seconds"
```

### 3.16 AuthError

```python
class AuthError(RuntimeError):
    def __init__(self, message, *, provider="", code=None, relogin_required=False):
```

Structured error with UX mapping hints. Error codes: `subscription_required`, `insufficient_credits`, `temporarily_unavailable`.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| auxiliary_client.py lines | ~2,615 |
| anthropic_adapter.py lines | ~1,411 |
| credential_pool.py lines | ~1,416 |
| Auxiliary backends | 5 (OpenAI, Codex, Anthropic, Nous, Custom) |
| Thinking budget levels | 4 (xhigh=32K, high=16K, medium=8K, low=4K) |
| Anthropic beta headers | 3 common + 2 OAuth-only |
| Anthropic output limits | Per-model (Opus=128K, Sonnet=64K, Haiku=64K, 3.5-Haiku=8K) |
| Pool strategies | 4 (fill_first, round_robin, random, least_used) |
| Exhausted TTL (429) | 3,600 seconds (1 hour) |
| Exhausted TTL (default) | 300 seconds (5 minutes) |
| Max concurrent per credential | 1 (default) |
| OAuth providers with sync | 3 (Anthropic, Codex, Nous) |
| External credential files synced | 2 (~/.claude/.credentials.json, ~/.codex/auth.json) |
| Custom pool prefix | "custom:" |
| Extra keys (JSON-only) | 8 |
| Claude Code timeout | 900s request, 10s connect |

---

*Generated from source analysis of the Hermes Agent codebase.*
