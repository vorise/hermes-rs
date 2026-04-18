# Hermes Agent — Gateway Architecture

This document covers the gateway runner, session management, delivery routing, and platform adapter lifecycle.

---

## Table of Contents

1. [Gateway Runner](#1-gateway-runner)
2. [Session Management](#2-session-management)
3. [Delivery Router](#3-delivery-router)
4. [Platform Adapter Lifecycle](#4-platform-adapter-lifecycle)

---

## 1. Gateway Runner

### Location

`gateway/run.py` (~3000+ lines)

### Purpose

Main controller for messaging platform integrations. Manages the lifecycle of all platform adapters (Telegram, Discord, Slack, WhatsApp, Signal, etc.) and routes messages to/from the agent.

### 1.1 Startup Sequence

```
start_gateway()
└── GatewayRunner(config)
    ├── Load config.yaml → env var bridge
    ├── Initialize SessionStore (SQLite)
    ├── Initialize DeliveryRouter
    ├── Initialize HookRegistry
    ├── Initialize PairingStore (DM code auth)
    ├── Ensure tirith security scanner installed
    ├── Resolve runtime provider credentials
    └── Start platform adapters
```

### 1.2 Configuration Bridge

Config.yaml values bridged to environment variables at startup:

| Config Path | Env Var |
|-------------|---------|
| `terminal.backend` | `TERMINAL_ENV` |
| `terminal.cwd` | `TERMINAL_CWD` |
| `terminal.timeout` | `TERMINAL_TIMEOUT` |
| `terminal.docker_image` | `TERMINAL_DOCKER_IMAGE` |
| `terminal.ssh_host` | `TERMINAL_SSH_HOST` |
| `terminal.container_cpu` | `TERMINAL_CONTAINER_CPU` |
| `agent.max_turns` | `HERMES_MAX_ITERATIONS` |
| `agent.gateway_timeout` | `HERMES_AGENT_TIMEOUT` |
| `agent.restart_drain_timeout` | `HERMES_RESTART_DRAIN_TIMEOUT` |
| `auxiliary.vision.provider` | `AUXILIARY_VISION_PROVIDER` |
| `auxiliary.vision.model` | `AUXILIARY_VISION_MODEL` |
| `auxiliary.approval.provider` | `AUXILIARY_APPROVAL_PROVIDER` |
| `timezone` | `HERMES_TIMEZONE` |
| `network.force_ipv4` | (applied via `apply_ipv4_preference`) |
| `security.redact_secrets` | `HERMES_REDACT_SECRETS` |

### 1.3 SSL Certificate Auto-Detection

Runs before any HTTP library import (discord, aiohttp, etc.):

1. Python's compiled-in defaults
2. certifi Mozilla bundle
3. Common distro paths (Debian, RHEL, Alpine, macOS Homebrew)

### 1.4 Agent Caching

Without caching, a new AIAgent is created per message, rebuilding the system prompt (including memory) every turn — breaking Anthropic prompt cache prefix and costing ~10x more.

```python
self._agent_cache: Dict[str, tuple]  # session_key → (AIAgent, config_signature_str)
self._agent_cache_lock = threading.Lock()
```

### 1.5 Running Agent Tracking

```python
self._running_agents: Dict[str, AIAgent]  # session_key → agent instance
self._running_agents_ts: Dict[str, float]  # start timestamp per session
self._pending_messages: Dict[str, str]     # queued messages during interrupt
self._busy_ack_ts: Dict[str, float]        # last busy-ack timestamp (debounce)
```

### 1.6 Interrupt Handling

When a user sends a new message while the agent is still processing:

- **`_AGENT_PENDING_SENTINEL`** — placed into `_running_agents` immediately, before any await, to prevent a second message bypassing the guard during the async gap
- **Pending message queue** — second message stored in `_pending_messages` and processed after current turn completes
- **Busy ack debounce** — `_busy_ack_ts` prevents spamming busy responses

### 1.7 Memory Flush on Session Reset

Before a session is automatically reset (due to inactivity or scheduled daily reset):

1. Load transcript from session store
2. Create temporary AIAgent with `skip_memory=True` (flush agent — no memory provider)
3. Enable `["memory", "skills"]` toolsets
4. Inject live memory state (MEMORY.md, USER.md) so the flush agent sees current state
5. Run with prompt: "Review the conversation and save important facts/preferences/decisions to memory"
6. Silently discard output (no-op `_print_fn`)

```python
def _flush_memories_for_session(self, old_session_id, session_key):
    # Skip cron sessions — headless with no meaningful conversation
    if old_session_id.startswith("cron_"):
        return
    # Max 8 iterations for flush agent
    # Reads live memory files to avoid overwriting newer entries
```

### 1.8 Session Model Overrides

Per-session `/model` command overrides stored in `_session_model_overrides`:

```python
# Key: session_key, Value: {model, provider, api_key, base_url, api_mode}
```

Resolution order:
1. Session override with complete provider bundle → use directly
2. Session override with model only → resolve global runtime, apply model on top
3. No override → resolve from config.yaml + env vars

### 1.9 Smart Model Routing

Per-turn model routing based on message complexity:

```python
def _resolve_turn_agent_config(self, user_message, model, runtime_kwargs):
    route = resolve_turn_route(user_message, self._smart_model_routing, primary)
    # Returns: {model, api_key, base_url, provider, api_mode, command, args, credential_pool}
```

### 1.10 Platform Failure Reconnection

When a platform adapter fails after startup:

```python
if adapter.fatal_error_retryable:
    self._failed_platforms[adapter.platform] = {
        "config": platform_config,
        "attempts": 0,
        "next_retry": time.monotonic() + 30,
    }
```

Background reconnection loop retries failed platforms.

### 1.11 WhatsApp Identifier Resolution

WhatsApp uses JID/LID syntax (`+1234567890@s.whatsapp.net`). Normalization:

```python
def _normalize_whatsapp_identifier(value):
    return value.strip().replace("+", "", 1).split(":")[0].split("@")[0]
```

**Bridge session mapping**: LID aliases resolved via `~/.hermes/whatsapp/session/lid-mapping-{current}{_reverse}.json` files — walks the mapping graph transitively.

### 1.12 Voice Mode Persistence

Per-chat voice reply modes persisted to `~/.hermes/gateway_voice_mode.json`:

| Mode | Behavior |
|------|----------|
| `off` | No auto-TTS |
| `voice_only` | Voice-only responses |
| `all` | Voice + text responses |

### 1.13 Unavailable Skill Detection

When a slash command matches a known-but-inactive skill:

1. Check disabled skills → "The skill is installed but disabled. Enable it with: `hermes skills config`"
2. Check optional skills (shipped but not installed) → "The skill is available but not installed. Install with: `hermes skills install official/<category>/<name>`"

### 1.14 Gateway Flags

```python
os.environ["HERMES_QUIET"] = "1"        # Quiet mode
os.environ["HERMES_EXEC_ASK"] = "1"      # Interactive exec approval
```

### 1.15 Restart System

```python
GATEWAY_SERVICE_RESTART_EXIT_CODE  # Exit code that signals systemd to restart
DEFAULT_GATEWAY_RESTART_DRAIN_TIMEOUT  # Seconds to wait for agents to finish
```

Restart modes:
- **Service restart** — via systemd (preferred)
- **Detached restart** — `nohup` / `disown` (fallback)

---

## 2. Session Management

### Location

`gateway/session.py`

### Purpose

Session context tracking, storage, reset policy evaluation, and dynamic system prompt injection.

### 2.1 SessionSource

Describes where a message originated:

```python
@dataclass
class SessionSource:
    platform: Platform
    chat_id: str
    chat_name: Optional[str]
    chat_type: str           # "dm", "group", "channel", "thread"
    user_id: Optional[str]
    user_name: Optional[str]
    thread_id: Optional[str]  # Forum topics, Discord threads
    chat_topic: Optional[str] # Channel topic/description
    user_id_alt: Optional[str]  # Signal UUID
    chat_id_alt: Optional[str]  # Signal group internal ID
```

### 2.2 SessionContext

Full context for dynamic system prompt injection:

```python
@dataclass
class SessionContext:
    source: SessionSource
    connected_platforms: List[Platform]
    home_channels: Dict[Platform, HomeChannel]
    session_key: str
    session_id: str
```

### 2.3 PII Redaction

For safe platforms (WhatsApp, Signal, Telegram, BlueBubbles), user IDs are hashed:

```python
def _hash_sender_id(value: str) -> str:
    return f"user_{sha256(value)[:12]}"

def _hash_chat_id(value: str) -> str:
    # "telegram:12345" → "telegram:<hash>"
    # "12345" → "<hash>"
```

Discord is excluded — mentions use `<@user_id>` and the LLM needs the real ID.

### 2.4 Session Key Generation

Session keys uniquely identify a conversation session based on platform, chat, user, and threading configuration.

### 2.5 Session Store

SQLite-backed persistent storage for:
- Session metadata (created_at, updated_at, source)
- Transcript (message history)
- Session titles (auto-generated or user-set)

---

## 3. Delivery Router

### Location

`gateway/delivery.py`

### Purpose

Routes cron job outputs and agent responses to appropriate destinations.

### 3.1 Delivery Target Format

```
origin              → back to message source
local               → save to local files only
telegram            → Telegram home channel
telegram:123456     → specific Telegram chat
discord:789:thread  → Discord channel with thread
```

### 3.2 Local Delivery

Outputs saved to `~/.hermes/cron/output/{job_id}/{timestamp}.md`:

```markdown
# Job Name

**Timestamp:** 2026-04-17 10:30:00
**Job ID:** abc123

---

Output content...
```

### 3.3 Platform Delivery

Messages truncated to `MAX_PLATFORM_OUTPUT = 4000` characters (with `TRUNCATED_VISIBLE = 3800` visible before truncation notice).

---

## 4. Platform Adapter Lifecycle

### 4.1 Adapter States

| State | Description |
|-------|-------------|
| `connecting` | Initial connection in progress |
| `connected` | Ready to receive messages |
| `retrying` | Temporary failure, will reconnect |
| `fatal` | Permanent failure, disconnected |

### 4.2 Runtime Status

Platform runtime status tracked via `_update_platform_runtime_status()`:

```python
self._update_platform_runtime_status(
    platform_state="retrying" | "fatal" | "connected",
    error_code="...",
    error_message="...",
)
```

### 4.3 Fatal Error Handling

```python
async def _handle_adapter_fatal_error(self, adapter):
    # 1. Log error
    # 2. Update status
    # 3. Disconnect adapter
    # 4. If retryable → queue for background reconnection
    # 5. If no platforms remain → shutdown gateway
```

### 4.4 Pairing Store

DM pairing for code-based user authorization. Stores code-to-chat mappings for initial user onboarding on platforms that require explicit bot-user pairing.

### 4.5 Hook Registry

Event hook system for gateway-level extensibility. Registered hooks fire at key points in the message lifecycle.

### 4.6 Background Task Tracking

`_background_tasks: set` — prevents garbage collection mid-execution for async tasks spawned by the gateway.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Max platform output characters | 4,000 |
| Truncated visible characters | 3,800 |
| Flush agent max iterations | 8 |
| Platform retry initial delay | 30s |
| Voice modes | 3 (off, voice_only, all) |
| PII-safe platforms | 4 (WhatsApp, Signal, Telegram, BlueBubbles) |

---

*Generated from source analysis of the Hermes Agent codebase.*
