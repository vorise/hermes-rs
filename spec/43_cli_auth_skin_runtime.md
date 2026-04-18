# Hermes Agent — CLI Entry Point, Authentication, Skin Engine & Runtime Provider

This document covers `hermes_cli/main.py` (CLI entry point and command routing), `hermes_cli/auth.py` (OAuth and API key authentication for 25+ providers), `hermes_cli/skin_engine.py` (TUI theming), and `hermes_cli/runtime_provider.py` (runtime credential resolution).

---

## Table of Contents

1. [CLI Entry Point](#1-cli-entry-point)
2. [Authentication System](#2-authentication-system)
3. [Skin Engine](#3-skin-engine)
4. [Runtime Provider Resolution](#4-runtime-provider-resolution)

---

## 1. CLI Entry Point

### Location

`hermes_cli/main.py` (~6,121 lines)

### Purpose

Main CLI entry point for all `hermes` subcommands. Handles profile overrides, environment loading, and command routing.

### 1.1 Profile Override System

```python
def _apply_profile_override() -> None:
    """Pre-parse --profile/-p from sys.argv BEFORE any hermes module import."""
```

**Why before imports**: Setting `HERMES_HOME` after modules have been imported means those modules already cached the default path. The override must happen at module load time.

**Resolution order**:
1. `--profile <name>` or `-p <name>` flag on command line
2. `~/.hermes/active_profile` file (persisted from previous session)
3. Default profile

**Implementation**: Strips the flag from `sys.argv` after setting `HERMES_HOME`, so downstream argparse never sees it.

### 1.2 TTY Guard

```python
def _require_tty() -> None:
    """Guard interactive commands against pipe invocation."""
```

Prevents 100% CPU spin when interactive commands (curses, input()) are invoked via pipe. Checks `sys.stdin.isatty()`.

### 1.3 Early Config Loading

```python
# IPv4 preference applied before any HTTP clients are created
if config["network"]["force_ipv4"]:
    apply_ipv4_preference(force=True)
```

### 1.4 Provider Configuration Check

```python
def _has_any_provider_configured() -> bool:
    """Check if at least one inference provider is usable."""
```

Checks in order:
1. Environment variables (all provider env vars)
2. `.env` file for keys
3. Provider-specific auth fallbacks (e.g., Copilot via `gh auth`)
4. Nous Portal OAuth credentials (`auth.json`)
5. `config.yaml` model dict with explicit provider/base_url/api_key
6. Claude Code OAuth (`~/.claude/.credentials.json`) — only if Hermes has explicit config

### 1.5 Session Browser

```python
def _session_browse_picker(sessions: list) -> Optional[str]:
    """Interactive curses-based session browser with live search filtering."""
```

Uses `curses` (not `simple_term_menu`) to avoid ghost-duplication rendering bug in tmux/iTerm when arrow keys are used.

### 1.6 Command Routing

| Command | Handler | Purpose |
|---------|---------|---------|
| `hermes chat` | `hermes_cli.cli` | Interactive TUI chat |
| `hermes gateway` | `hermes_cli.gateway` | Gateway service management |
| `hermes setup` | `hermes_cli.config` | Setup wizard |
| `hermes doctor` | `hermes_cli.doctor` | Diagnostics |
| `hermes model` | `hermes_cli.auth` | Model/provider switching |
| `hermes auth` | `hermes_cli.auth` | Authentication management |
| `hermes sessions` | `hermes_cli.sessions` | Session management |
| `hermes honcho` | `hermes_cli.honcho` | Honcho memory CLI |
| `hermes claw` | `hermes_cli.claw` | Claw migration |
| `hermes acp` | `hermes_cli.acp` | ACP adapter |
| `hermes skin` | `hermes_cli.skin_engine` | Skin switching |
| `hermes update` | `hermes_cli.update` | Self-update |
| `hermes plugins` | `hermes_cli.plugins` | Plugin management |
| `hermes logs` | `hermes_cli.logs` | Log viewing |
| `hermes skills` | `hermes_cli.skills` | Skills management |
| `hermes tools` | — | List available tools |
| `hermes version` | — | Show version |

---

## 2. Authentication System

### Location

`hermes_cli/auth.py` (~3,275 lines)

### Purpose

Multi-provider authentication supporting OAuth device code flow, external OAuth, API keys, and external process credentials for 25+ inference providers.

### 2.1 Provider Registry

```python
@dataclass
class ProviderConfig:
    id: str
    name: str
    auth_type: str  # "oauth_device", "oauth_external", "api_key", "external_process"
    portal_base_url: str = ""
    inference_base_url: str = ""
    client_id: str = ""
    scope: str = ""
    api_key_env_vars: tuple = ()
    base_url_env_var: str = ""
```

### 2.2 Provider Registry (25+ providers)

| Provider ID | Name | Auth Type | Key Env Vars |
|-------------|------|-----------|--------------|
| `openrouter` | OpenRouter | api_key | `OPENROUTER_API_KEY` |
| `nous` | Nous Portal | oauth_device | (Portal OAuth) |
| `openai-codex` | OpenAI Codex | oauth_external | (ChatGPT OAuth) |
| `qwen-oauth` | Qwen OAuth | oauth_external | (Qwen Portal OAuth) |
| `anthropic` | Anthropic | api_key | `ANTHROPIC_API_KEY`, `ANTHROPIC_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN` |
| `copilot` | GitHub Copilot | api_key | `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, `GITHUB_TOKEN` |
| `copilot-acp` | GitHub Copilot ACP | external_process | — |
| `gemini` | Google AI Studio | api_key | `GOOGLE_API_KEY`, `GEMINI_API_KEY` |
| `zai` | Z.AI / GLM | api_key | `GLM_API_KEY`, `ZAI_API_KEY`, `Z_AI_API_KEY` |
| `kimi-coding` | Kimi / Moonshot | api_key | `KIMI_API_KEY` |
| `kimi-coding-cn` | Kimi / Moonshot (China) | api_key | `KIMI_CN_API_KEY` |
| `arcee` | Arcee AI | api_key | `ARCEEAI_API_KEY` |
| `minimax` | MiniMax | api_key | `MINIMAX_API_KEY` |
| `minimax-cn` | MiniMax (China) | api_key | `MINIMAX_CN_API_KEY` |
| `alibaba` | Alibaba Cloud (DashScope) | api_key | `DASHSCOPE_API_KEY` |
| `deepseek` | DeepSeek | api_key | `DEEPSEEK_API_KEY` |
| `xai` | xAI | api_key | `XAI_API_KEY` |
| `ai-gateway` | Vercel AI Gateway | api_key | `AI_GATEWAY_API_KEY` |
| `opencode-zen` | OpenCode Zen | api_key | `OPENCODE_ZEN_API_KEY` |
| `opencode-go` | OpenCode Go | api_key | `OPENCODE_GO_API_KEY` |
| `kilocode` | Kilo Code | api_key | `KILOCODE_API_KEY` |
| `huggingface` | Hugging Face | api_key | `HF_TOKEN` |
| `xiaomi` | Xiaomi MiMo | api_key | `XIAOMI_API_KEY` |

### 2.3 Auth Types

| Type | Description | Example |
|------|-------------|---------|
| `oauth_device` | Device code flow (user visits URL, enters code) | Nous Portal |
| `oauth_external` | External OAuth via browser/CLI | OpenAI Codex, Qwen OAuth |
| `api_key` | Environment variable or .env file | OpenRouter, Anthropic |
| `external_process` | Credentials from subprocess (e.g., `gh auth`) | Copilot ACP |

### 2.4 OAuth Device Code Flow (Nous Portal)

```python
def resolve_nous_runtime_credentials(min_key_ttl_seconds=1800, timeout_seconds=15):
    """Resolve Nous Portal credentials, minting a new agent key if needed."""
```

**Flow**:
1. Load OAuth tokens from `auth.json`
2. Refresh access token if expired
3. Mint short-lived agent key (~30 min TTL) for inference
4. Return `{api_key, base_url, expires_at, source}`

### 2.5 Auth Store

**Location**: `~/.hermes/auth.json`

```json
{
  "version": 1,
  "active_provider": "nous",
  "providers": {
    "nous": {
      "access_token": "...",
      "refresh_token": "...",
      "agent_key": "...",
      "agent_key_expires_at": "...",
      "inference_base_url": "..."
    }
  },
  "credential_pool": {
    "openrouter": [...]
  },
  "suppressed_sources": {}
}
```

### 2.6 Cross-Process Locking

```python
@contextmanager
def _auth_store_lock(timeout_seconds=15.0):
    """Cross-process advisory lock for auth.json reads+writes. Reentrant."""
```

**Implementation**:
- Unix: `fcntl.flock(LOCK_EX | LOCK_NB)` with retry loop
- Windows: `msvcrt.locking(LK_NBLCK)`
- Lock file: `~/.hermes/auth.json.lock`
- Reentrant: thread-local depth counter prevents deadlock
- Timeout: 15 seconds default

### 2.7 Auth Store Persistence

```python
def _save_auth_store(auth_store: Dict[str, Any]) -> Path:
    """Write auth.json atomically with fsync and chmod 600."""
```

**Safety**:
1. Write to temp file with PID + UUID suffix
2. `fsync()` on file and parent directory
3. `os.replace()` for atomic rename
4. `chmod(S_IRUSR | S_IWUSR)` — owner-only permissions

### 2.8 Credential Pool

```python
def read_credential_pool(provider_id=None) -> Dict[str, Any]:
    """Return persisted credential pool, or one provider slice."""

def write_credential_pool(provider_id, entries) -> Path:
    """Persist one provider's credential pool."""
```

Multi-credential support per provider for rotation/failover.

### 2.9 Endpoint Probing

**Z.AI/GLM**: 4 endpoint candidates (global, China, coding-global, coding-CN) probed at setup time. Result cached in auth state keyed by API key SHA-256 hash.

**Kimi**: `sk-kimi-` prefixed keys route to `api.kimi.com/coding/v1`; legacy keys to `api.moonshot.ai/v1`.

### 2.10 Auth Error System

```python
class AuthError(RuntimeError):
    """Structured auth error with UX mapping hints."""
    def __init__(self, message, *, provider="", code=None, relogin_required=False):
```

**Error codes**: `subscription_required`, `insufficient_credits`, `temporarily_unavailable`

**format_auth_error()**: Maps failures to user-facing guidance ("Run `hermes model` to re-authenticate").

### 2.11 Secret Validation

```python
_PLACEHOLDER_SECRET_VALUES = {"*", "**", "***", "changeme", "your_api_key", ...}

def has_usable_secret(value, *, min_length=4) -> bool:
    """Return True when a configured secret looks usable, not empty/placeholder."""
```

### 2.12 OAuth Trace Logging

```python
def _oauth_trace(event, *, sequence_id=None, **fields) -> None:
    """Structured OAuth event logging, gated by HERMES_OAUTH_TRACE env var."""
```

For debugging OAuth flows. Outputs JSON-structured log events.

### 2.13 Explicit Configuration Gate

```python
def is_provider_explicitly_configured(provider_id) -> bool:
    """Return True only if user has explicitly configured this provider."""
```

Checks:
1. `auth.json` active_provider matches
2. `config.yaml` model.provider matches
3. Provider-specific env vars set (excluding `CLAUDE_CODE_OAUTH_TOKEN`)

Prevents auto-discovery of external credentials without user's explicit choice.

---

## 3. Skin Engine

### Location

`hermes_cli/skin_engine.py` (~817 lines)

### Purpose

Data-driven TUI theming system. Skins are YAML files — no code changes needed for new themes.

### 3.1 Skin Schema

```yaml
name: mytheme
description: Short description
colors:
  banner_border: "#CD7F32"
  banner_title: "#FFD700"
  # ... 20+ color keys
spinner:
  waiting_faces: ["(⚔)", "(⛨)"]
  thinking_faces: ["(⌁)", "(<>)"]
  thinking_verbs: ["forging", "plotting"]
  wings: [["⟪⚔", "⚔⟫"]]  # spinner decorations
branding:
  agent_name: "Hermes Agent"
  welcome: "Welcome message"
  goodbye: "Goodbye!"
  response_label: " ⚕ Hermes "
  prompt_symbol: "❯ "
  help_header: "Available Commands"
tool_prefix: "┊"
tool_emojis:
  terminal: "⚔"
banner_logo: "..."  # ASCII art
banner_hero: "..."   # ASCII art
```

### 3.2 Built-in Skins (10)

| Skin | Theme |
|------|-------|
| `default` | Classic Hermes gold/kawaii |
| `ares` | Crimson/bronze war-god theme |
| `mono` | Clean grayscale monochrome |
| `slate` | Cool blue developer-focused |
| `daylight` | Light background theme |
| `warm-lightmode` | Warm brown/gold for light terminals |
| `poseidon` | Ocean-god deep blue and seafoam |
| `sisyphus` | Austere grayscale with persistence theme |
| `charizard` | Volcanic burnt orange and ember |

### 3.3 Skin Loading

```python
def load_skin(name: str) -> SkinConfig:
    """Load a skin by name. Checks user skins first, then built-in."""
```

**Priority**:
1. `~/.hermes/skins/<name>.yaml` (user skins)
2. Built-in skin definitions
3. Fallback to `default`

### 3.4 Inheritance

Missing color/spinner/branding values inherit from the `default` skin via dict merge.

### 3.5 prompt_toolkit Integration

```python
def get_prompt_toolkit_style_overrides() -> Dict[str, str]:
    """Return prompt_toolkit style overrides derived from the active skin."""
```

Maps skin colors to 30+ prompt_toolkit style classes: `input-area`, `status-bar`, `completion-menu`, `clarify-border`, `sudo-prompt`, `voice-status`, etc.

### 3.6 Activation

- CLI: `/skin <name>` command
- Config: `display.skin: <name>` in `config.yaml`
- API: `set_active_skin("ares")`

---

## 4. Runtime Provider Resolution

### Location

`hermes_cli/runtime_provider.py` (~893 lines)

### Purpose

Resolves runtime provider credentials at agent execution time. Bridges config, auth store, credential pools, and environment variables into a single runtime dict.

### 4.1 Resolution Pipeline

```python
def resolve_runtime_provider(requested=None, explicit_api_key=None, explicit_base_url=None):
    """Resolve runtime provider credentials for agent execution."""
```

**Order**:
1. Named custom provider (`custom:<name>` from `providers:` dict in config)
2. Custom endpoint from `custom_providers:` list
3. Explicit provider resolution via `resolve_provider()`
4. Explicit runtime (user-provided api_key/base_url)
5. Credential pool lookup
6. Direct runtime credential resolution (OAuth minting, etc.)
7. API-key provider resolution
8. OpenRouter fallback

### 4.2 API Mode Detection

```python
def _detect_api_mode_for_url(base_url: str) -> Optional[str]:
    """Auto-detect api_mode from the resolved base URL."""
```

- `api.openai.com` → `codex_responses`
- URL ends with `/anthropic` → `anthropic_messages`
- Otherwise → `chat_completions`

### 4.3 Provider API Mode Staleness Guard

```python
def _provider_supports_explicit_api_mode(provider, configured_provider=None) -> bool:
    """Check whether a persisted api_mode should be honored."""
```

Prevents stale `api_mode` from a previous provider leaking after a provider switch.

### 4.4 Copilot API Mode

```python
def _copilot_runtime_api_mode(model_cfg, api_key) -> str:
```

Uses `copilot_model_api_mode()` to determine whether Copilot needs `chat_completions` or `codex_responses` based on the model name.

### 4.5 OpenCode /v1 Stripping

```python
# Anthropic SDK prepends /v1/messages to base_url.
# OpenCode base URLs end with /v1, so strip it to avoid /v1/v1/messages.
if api_mode == "anthropic_messages" and provider in ("opencode-zen", "opencode-go"):
    base_url = re.sub(r"/v1/?$", "", base_url)
```

### 4.6 Custom Provider Resolution

```python
def _get_named_custom_provider(requested_provider: str) -> Optional[Dict[str, Any]]:
```

**New-style** (`providers:` dict in config):
```yaml
providers:
  my-llm:
    name: "My LLM"
    api: "https://my-llm.example.com/v1"
    key_env: MY_LLM_API_KEY
    default_model: "my-model-7b"
```

**Legacy** (`custom_providers:` list):
```yaml
custom_providers:
  - name: "My LLM"
    base_url: "https://my-llm.example.com/v1"
    api_key: "sk-..."
    api_mode: "chat_completions"
```

### 4.7 Credential Pool Integration

```python
def _resolve_runtime_from_pool_entry(provider, entry, requested_provider, model_cfg, pool):
    """Resolve runtime from a pooled credential entry."""
```

Handles provider-specific logic for pool entries:
- `openai-codex`: sets `codex_responses` mode, DEFAULT_CODEX_BASE_URL
- `qwen-oauth`: DEFAULT_QWEN_BASE_URL
- `anthropic`: `anthropic_messages` mode, config base_url guard
- `nous`: chat_completions mode
- `copilot`: model-based API mode detection
- `opencode-zen/go`: per-model API mode via `opencode_model_api_mode()`

### 4.8 Nous Credential Resolution

```python
if provider == "nous":
    creds = resolve_nous_runtime_credentials(
        min_key_ttl_seconds=max(60, int(os.getenv("HERMES_NOUS_MIN_KEY_TTL_SECONDS", "1800"))),
        timeout_seconds=float(os.getenv("HERMES_NOUS_TIMEOUT_SECONDS", "15")),
    )
```

Configurable via `HERMES_NOUS_MIN_KEY_TTL_SECONDS` (default 1800 = 30 min) and `HERMES_NOUS_TIMEOUT_SECONDS` (default 15).

### 4.9 Auto-Detect Fallback

When `requested_provider` is `"auto"` and a specific provider's credentials fail, the resolution falls through to the next provider instead of raising an error. Explicit provider requests always raise on failure.

### 4.10 Local Model Auto-Detection

```python
def _auto_detect_local_model(base_url: str) -> str:
    """Query a local server for its model name when only one model is loaded."""
```

Queries `/v1/models` endpoint. If exactly one model is returned, uses it as the default.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| main.py lines | ~6,121 |
| auth.py lines | ~3,275 |
| skin_engine.py lines | ~817 |
| runtime_provider.py lines | ~893 |
| Providers in registry | 25+ |
| Auth types | 4 (oauth_device, oauth_external, api_key, external_process) |
| Built-in skins | 10 |
| Skin color keys | 20+ |
| prompt_toolkit style classes | 30+ |
| Auth lock timeout | 15 seconds |
| Auth store version | 1 |
| Auth file permissions | 600 (owner-only) |
| Z.AI endpoint candidates | 4 |
| Default agent key TTL | 1800 seconds (30 min) |
| Provider aliases | 20+ (glm→zai, claude→anthropic, etc.) |

---

*Generated from source analysis of the Hermes Agent codebase.*
