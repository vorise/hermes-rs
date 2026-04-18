# Hermes Agent — Credential Pool & Rate Limit Systems

This document covers the persistent multi-credential pool for provider failover and the rate limit tracking system.

---

## Table of Contents

1. [Credential Pool](#1-credential-pool)
2. [Rate Limit Tracker](#2-rate-limit-tracker)

---

## 1. Credential Pool

### Location

`agent/credential_pool.py` (~1000+ lines)

### Purpose

Persistent multi-credential pool enabling same-provider failover with multiple authentication strategies. When one credential exhausts its quota (429 rate limit or 402 billing error), the pool automatically rotates to the next available credential.

### 1.1 Credential Model

```python
@dataclass
class PooledCredential:
    provider: str           # "openrouter", "nous", "anthropic", "openai-codex", "custom:name"
    id: str                 # 6-char hex UUID
    label: str              # Human-readable (derived from JWT token email)
    auth_type: str          # "oauth" or "api_key"
    priority: int           # Selection order (lower = higher priority)
    source: str             # "manual", "device_code", "claude_code", etc.
    access_token: str       # Runtime API key (or OAuth access token)
    refresh_token: str      # OAuth refresh token (single-use)
    base_url: str           # API endpoint
    request_count: int      # Usage counter for LEAST_USED strategy
    # ... additional fields for OAuth, agent keys, error state
```

### 1.2 Selection Strategies

Configured via `credential_pool_strategies` in config.yaml:

| Strategy | Behavior |
|----------|----------|
| `fill_first` (default) | Always use highest priority available credential |
| `round_robin` | Cycle through available credentials in order |
| `random` | Pick randomly from available credentials |
| `least_used` | Select credential with lowest `request_count` |

### 1.3 Exhaustion & Rotation

**Cooldown periods**:
- 429 (rate-limited): 1 hour cooldown
- 402 (billing/quota): 1 hour cooldown
- Provider-supplied `reset_at` timestamps override defaults

**Rotation flow**:
```python
def mark_exhausted_and_rotate(self, status_code, error_context):
    1. Mark current credential as STATUS_EXHAUSTED
    2. Persist state to auth.json
    3. Select next available credential
    4. Return next credential (or None if all exhausted)
```

**Exhaustion detection** — `_exhausted_until(entry)`:
1. Check provider-supplied `last_error_reset_at` timestamp
2. Fall back to `last_status_at + cooldown_ttl`
3. Parse reset timestamps from error messages (regex for `quotaResetDelay`, `retry after N seconds`)

### 1.4 OAuth Token Refresh

Supports automatic refresh for three OAuth providers:

| Provider | Refresh Method | Token Sync Target |
|----------|---------------|-------------------|
| **Anthropic** | `refresh_anthropic_oauth_pure()` | `~/.claude/.credentials.json` |
| **OpenAI Codex** | `refresh_codex_oauth_pure()` | `~/.codex/auth.json` |
| **Nous** | `refresh_nous_oauth_from_state()` | Auth store (providers.nous) |

**Single-use refresh token handling**:
OAuth refresh tokens are consumed on use. When something external refreshes the token (Claude Code CLI, another Hermes profile), the pool entry's refresh token becomes stale.

**Sync-on-demand pattern**:
```python
# Before refresh: check if external process already refreshed
synced = self._sync_anthropic_entry_from_credentials_file(entry)
if synced.refresh_token != entry.refresh_token:
    # External refresh happened — use synced tokens, retry
    return self._refresh_entry(synced, force=False)
```

### 1.5 Credential Lease System

Soft leases prevent concurrent requests from overwhelming a single credential:

```python
DEFAULT_MAX_CONCURRENT_PER_CREDENTIAL = 1

def acquire_lease(self, credential_id=None):
    # Prefer least-leased available credential
    # When all at soft cap, still return least-leased (don't block)
    # Returns credential_id string

def release_lease(self, credential_id):
    # Decrement lease count, remove when zero
```

Used by delegation system — each subagent acquires a lease on its credential so rotation doesn't affect running children.

### 1.6 Persistence

Credentials persisted to `~/.hermes/auth.json` via `write_credential_pool()`:

```python
def _persist(self):
    write_credential_pool(
        self.provider,
        [entry.to_dict() for entry in self._entries],
    )
```

Fields always emitted (even when null): `last_status`, `last_status_at`, `last_error_code`, `last_error_reason`, `last_error_message`, `last_error_reset_at`.

### 1.7 Custom Provider Pools

Custom OpenAI-compatible endpoints share provider=`custom` but are keyed by name:

```python
CUSTOM_POOL_PREFIX = "custom:"
# Pool key: "custom:together.ai", "custom:fireworks", etc.
```

Normalized from `custom_providers` config entries:
```yaml
custom_providers:
  - name: "Together AI"
    base_url: "https://api.together.xyz/v1"
    api_key_env: "TOGETHER_API_KEY"
```

### 1.8 Pool Operations

| Method | Purpose |
|--------|---------|
| `select()` | Select next credential using configured strategy |
| `peek()` | View current credential without changing state |
| `mark_exhausted_and_rotate()` | Mark current exhausted, rotate to next |
| `acquire_lease()` | Acquire soft lease for concurrent use |
| `release_lease()` | Release a lease |
| `try_refresh_current()` | Force OAuth refresh of current credential |
| `reset_statuses()` | Clear all error statuses (manual recovery) |
| `remove_index()` | Remove credential by index |
| `has_available()` | Check if any credential is not in cooldown |

### 1.9 Entry Resolution

Credentials resolved by index (1-based), ID (6-char hex), or label (case-insensitive):

```python
def resolve_target(self, target):
    # 1. Exact ID match
    # 2. Label match (error if ambiguous)
    # 3. Numeric index
```

### 1.10 Auth Type Detection

| Auth Type | Runtime Key Source |
|-----------|-------------------|
| `oauth` | `access_token` (or `agent_key` for Nous) |
| `api_key` | `access_token` |

### 1.11 PooledCredential.from_dict

Parses credential payloads from auth.json:
- Auto-generates 6-char ID if missing
- Defaults label from token source
- Separates known fields from extra keys
- Extra keys (token_type, scope, client_id, etc.) stored in `extra` dict and accessed via `__getattr__`

### 1.12 Label from JWT Token

```python
def label_from_token(token, fallback):
    claims = _decode_jwt_claims(token)
    # Prefers: email > preferred_username > upn
    return claims.get("email") or claims.get("preferred_username") or claims.get("upn") or fallback
```

---

## 2. Rate Limit Tracker

### Location

`agent/rate_limit_tracker.py` (247 lines)

### Purpose

Captures `x-ratelimit-*` headers from provider API responses and provides formatted display for the `/usage` slash command.

### 2.1 Header Schema

12 rate limit headers (provider response format used by Nous Portal, OpenRouter, and OpenAI-compatible APIs):

| Header | Purpose |
|--------|---------|
| `x-ratelimit-limit-requests` | RPM cap |
| `x-ratelimit-limit-requests-1h` | RPH cap |
| `x-ratelimit-limit-tokens` | TPM cap |
| `x-ratelimit-limit-tokens-1h` | TPH cap |
| `x-ratelimit-remaining-requests` | Requests left in minute window |
| `x-ratelimit-remaining-requests-1h` | Requests left in hour window |
| `x-ratelimit-remaining-tokens` | Tokens left in minute window |
| `x-ratelimit-remaining-tokens-1h` | Tokens left in hour window |
| `x-ratelimit-reset-requests` | Seconds until minute request window resets |
| `x-ratelimit-reset-requests-1h` | Seconds until hour request window resets |
| `x-ratelimit-reset-tokens` | Seconds until minute token window resets |
| `x-ratelimit-reset-tokens-1h` | Seconds until hour token window resets |

### 2.2 Data Model

```python
@dataclass
class RateLimitBucket:
    limit: int           # Cap
    remaining: int       # Remaining
    reset_seconds: float # Seconds until reset
    captured_at: float   # When captured

    @property
    def used(self) -> int           # limit - remaining
    @property
    def usage_pct(self) -> float    # (used / limit) * 100
    @property
    def remaining_seconds_now(self) -> float  # Adjusted for elapsed time

@dataclass
class RateLimitState:
    requests_min: RateLimitBucket
    requests_hour: RateLimitBucket
    tokens_min: RateLimitBucket
    tokens_hour: RateLimitBucket
    captured_at: float
    provider: str

    @property
    def has_data(self) -> bool
    @property
    def age_seconds(self) -> float
```

### 2.3 Parsing

Headers normalized to lowercase before lookup (HTTP headers are case-insensitive per RFC 7230). Returns `None` if no rate limit headers present.

### 2.4 Display Formatting

**Full display** (for `/usage` command):
```
Nous Rate Limits (captured just now):

  Requests/min   [████████░░░░░░░░░░░░]  40.0%  400/1K used  (600 left, resets in 45s)
  Requests/hr    [█░░░░░░░░░░░░░░░░░░░]   5.0%  5.0K/100K used  (95.0K left, resets in 58m)

  Tokens/min     [███████████████░░░░░]  75.0%  150K/200K used  (50.0K left, resets in 2m 14s)
  Tokens/hr      [░░░░░░░░░░░░░░░░░░░░]   1.2%  1.2M/100M used  (98.8M left, resets in 1h 2m)

  ⚠ tokens/min at 75% — resets in 2m 14s
```

**Compact display** (for status bars / gateway messages):
```
RPM: 600/1K | RPH: 95.0K/100K (resets 58m) | TPM: 50.0K/200K | TPH: 98.8M/100M (resets 1h 2m)
```

### 2.5 Warnings

Threshold: 80% usage on any bucket triggers a warning with reset time.

### 2.6 Number Formatting

Human-friendly numbers:
- `7999856` → `8.0M`
- `33599` → `33.6K`
- `799` → `799`

### 2.7 Time Formatting

Human-friendly durations:
- `58` → `58s`
- `134` → `2m 14s`
- `3537` → `58m 57s`
- `3720` → `1h 2m`

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Credential selection strategies | 4 |
| Default max concurrent per credential | 1 |
| Exhaustion cooldown (429/402) | 1 hour |
| OAuth providers with auto-refresh | 3 (Anthropic, Codex, Nous) |
| Rate limit headers tracked | 12 |
| Warning threshold | 80% usage |
| Custom provider pool key prefix | `custom:` |
| Credential ID length | 6 hex chars |

---

*Generated from source analysis of the Hermes Agent codebase.*
