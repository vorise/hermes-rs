# Hermes Agent — Gateway Runner & Core Infrastructure

This document covers the gateway runner (`gateway/run.py`), the core lifecycle manager for all messaging platform integrations, message routing, session management, and background tasks.

---

## Table of Contents

1. [GatewayRunner Architecture](#1-gatewayrunner-architecture)
2. [Startup Lifecycle](#2-startup-lifecycle)
3. [Agent Factory & Caching](#3-agent-factory--caching)
4. [Message Routing](#4-message-routing)
5. [Session Management](#5-session-management)
6. [Background Tasks](#6-background-tasks)
7. [Shutdown & Restart](#7-shutdown--restart)
8. [Security & Access Control](#8-security--access-control)
9. [Supporting Gateway Modules](#9-supporting-gateway-modules)

---

## 1. GatewayRunner Architecture

### Location

`gateway/run.py` (~9,646 lines)

### Purpose

Main gateway controller. Manages the lifecycle of all platform adapters and routes messages to/from the agent.

### 1.1 Core Components

```python
class GatewayRunner:
    config: GatewayConfig                    # Gateway configuration
    adapters: Dict[Platform, BasePlatformAdapter]  # Platform adapters
    session_store: SessionStore              # Session transcript storage
    delivery_router: DeliveryRouter          # Auto-delivery routing
    pairing_store: PairingStore             # DM pairing for code-based auth
    hooks: HookRegistry                      # Event hook system
    _session_db: SessionDB                   # SQLite session store
    _running_agents: Dict[str, AIAgent]     # Active agent instances per session
    _agent_cache: Dict[str, tuple]          # Cached AIAgent for prompt caching
    _pending_approvals: Dict[str, dict]     # Pending exec approvals
    _failed_platforms: Dict[Platform, dict] # Platforms needing reconnection
    _voice_mode: Dict[str, str]             # Per-chat voice reply mode
```

### 1.2 SSL Certificate Auto-Detection

Runs before any HTTP library imports:

```python
def _ensure_ssl_certs() -> None:
    """Set SSL_CERT_FILE if the system doesn't expose CA certs to Python."""
    # 1. Python's compiled-in defaults (ssl.get_default_verify_paths)
    # 2. certifi (ships Mozilla bundle)
    # 3. Common distro/macOS locations (10 candidates checked)
```

**Why**: NixOS and non-standard systems don't expose CA certs to Python by default, causing discord.py, aiohttp, etc. to fail SSL verification.

### 1.3 Config Bridging

Bridges `config.yaml` values into environment variables:

```python
# Terminal config → TERMINAL_* env vars
terminal.backend → TERMINAL_ENV
terminal.cwd → TERMINAL_CWD
terminal.timeout → TERMINAL_TIMEOUT
terminal.docker_image → TERMINAL_DOCKER_IMAGE
# ... and more
```

**Priority**: `config.yaml` > `.env` for terminal settings.

---

## 2. Startup Lifecycle

### 2.1 Startup Sequence

```
1. SSL certificate detection (before any HTTP imports)
2. Load .env from HERMES_HOME
3. Bridge config.yaml → env vars
4. Initialize GatewayRunner
5. Write runtime status: "starting"
6. Warn if no user allowlists configured
7. Discover and load event hooks
8. Recover background processes from checkpoint
9. Suspend recently-active sessions (unless clean shutdown)
10. Detect stuck-loop sessions (3+ consecutive restarts)
11. Start platform adapters (async)
12. Start stream consumer
13. Start cron scheduler
14. Start web UI (if configured)
15. Write runtime status: "running"
```

### 2.2 Clean Shutdown Detection

```python
_clean_marker = _hermes_home / ".clean_shutdown"
if _clean_marker.exists():
    # Skip session suspension — previous process drained agents cleanly
    _clean_marker.unlink()
else:
    # Suspend in-flight sessions — prevents stuck sessions on restart
    suspended = self.session_store.suspend_recently_active()
```

**Why**: Prevents unwanted auto-resets after `hermes update`, `hermes gateway restart`, or `/restart`.

### 2.3 Stuck-Loop Detection

```python
# If a session has been active across 3+ consecutive restarts,
# it's probably stuck in a loop. Auto-suspend for clean slate.
```

### 2.4 Process Recovery

```python
from tools.process_registry import process_registry
recovered = process_registry.recover_from_checkpoint()
if recovered:
    logger.info("Recovered %s background process(es) from previous run", recovered)
```

Recovers background terminal processes from crash checkpoints.

---

## 3. Agent Factory & Caching

### 3.1 AIAgent Cache

```python
# Cache AIAgent instances per session to preserve prompt caching.
# Without this, a new AIAgent is created per message, rebuilding the
# system prompt every turn — breaking Anthropic prefix cache.
self._agent_cache: Dict[str, tuple] = {}  # (AIAgent, config_signature_str)
self._agent_cache_lock = threading.Lock()
```

**Key insight**: The gateway creates a new AIAgent per message by default. Caching preserves the Anthropic prompt cache prefix across messages.

### 3.2 Runtime Resolution

```python
def _resolve_session_agent_runtime(self, session_key=None):
    """Resolve model, provider, and kwargs for the session."""
    # Checks session model overrides (/model command)
    # Applies fallback model configuration
    # Applies smart model routing
    # Returns (model, runtime_kwargs)
```

### 3.3 Session Model Overrides

```python
self._session_model_overrides: Dict[str, Dict[str, str]] = {}
# Key: session_key
# Value: {"model": ..., "provider": ..., "api_key": ..., "base_url": ..., "api_mode": ...}
```

Set via `/model` command for per-session model switching.

### 3.4 Pending Approvals

```python
self._pending_approvals: Dict[str, Dict[str, Any]] = {}
# Key: session_key
# Value: {"command": str, "pattern_key": str, ...}
```

Tracks pending `execute_code` approval responses per session.

---

## 4. Message Routing

### 4.1 Platform Adapter Integration

Each platform adapter (Telegram, Discord, etc.) receives incoming messages and routes them to the gateway:

```
Platform Adapter → GatewayRunner → Session Store → AIAgent → Response → Platform Adapter
```

### 4.2 Interrupt Support

```python
self._running_agents: Dict[str, AIAgent]      # Active agents per session
self._running_agents_ts: Dict[str, float]     # Start timestamp per session
self._pending_messages: Dict[str, str]         # Queued messages during interrupt
self._busy_ack_ts: Dict[str, float]            # Busy-ack debounce per session
```

When a user sends a new message while the agent is working:
1. Check if agent is running for this session
2. If running → interrupt (or queue based on `_busy_input_mode`)
3. If not running → start new agent

### 4.3 Busy Input Modes

| Mode | Behavior |
|------|----------|
| `interrupt` | New message interrupts current agent |
| `queue` | New message waits for current agent to finish |
| `reject` | New message is rejected with busy notice |

### 4.4 Delivery Router

```python
self.delivery_router = DeliveryRouter(self.config)
```

Routes agent responses back to the correct platform, chat, and thread. Handles auto-delivery for cron sessions.

---

## 5. Session Management

### 5.1 Session Store

```python
from gateway.session import SessionStore
self.session_store = SessionStore(
    self.config.sessions_dir, self.config,
    has_active_processes_fn=lambda key: process_registry.has_active_for_session(key),
)
```

Manages session transcripts with reset protection based on active processes.

### 5.2 Session Database

```python
from hermes_state import SessionDB
self._session_db = SessionDB()
```

SQLite FTS5 session search across all conversations.

### 5.3 Pre-Reset Memory Flush

Before resetting an inactive session:

```python
def _flush_memories_for_session(self, old_session_id, session_key=None):
    # 1. Skip cron sessions (headless, no meaningful conversation)
    # 2. Load transcript history
    # 3. Create temporary AIAgent with memory + skills toolsets only
    # 4. Send flush prompt with current live memory state
    # 5. Agent uses memory/skill_manage tools to save important info
    # 6. Suppress all output (quiet mode + no-op _print_fn)
```

**Flush prompt**: Asks agent to review conversation and save important facts to memory before context is lost.

### 5.4 Voice Mode Persistence

```python
_VOICE_MODE_PATH = _hermes_home / "gateway_voice_mode.json"

def _load_voice_modes(self) -> Dict[str, str]:
    # Returns {chat_id: "off"|"voice_only"|"all"}

def _save_voice_modes(self) -> None:
    # Persists to JSON file
```

Per-chat voice reply modes persisted across gateway restarts.

---

## 6. Background Tasks

### 6.1 Task Tracking

```python
self._background_tasks: set = set()
```

Tracks background asyncio tasks to prevent garbage collection mid-execution.

### 6.2 Platform Reconnection

```python
async def _platform_reconnect_watcher(self) -> None:
    """Monitor failed platforms and attempt background reconnection."""
    self._failed_platforms: Dict[Platform, Dict[str, Any]]
    # Key: Platform enum
    # Value: {"config": platform_config, "attempts": int, "next_retry": float}
```

### 6.3 Cron Scheduler

Integrated cron scheduler (covered in spec 07). Runs headless sessions on schedule with auto-delivery.

### 6.4 Stream Consumer

Processes streaming LLM responses and routes deltas to platform adapters in real-time (covered in spec 17).

---

## 7. Shutdown & Restart

### 7.1 Shutdown Sequence

```python
self._running = False
self._shutdown_event.set()
# 1. Stop accepting new messages
# 2. Drain active agents (wait for completion)
# 3. Stop platform adapters
# 4. Stop cron scheduler
# 5. Stop stream consumer
# 6. Write .clean_shutdown marker
# 7. Write runtime status: "stopped"
```

### 7.2 Restart with Drain

```python
_restart_drain_timeout: float = 30  # seconds
_restart_via_service: bool
_restart_detached: bool
```

Graceful restart:
1. Set `_draining = True` — stop accepting new messages
2. Wait up to drain timeout for active agents
3. Kill remaining agents
4. Write restart exit code (42)
5. Exit — service manager restarts

### 7.3 Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Clean exit |
| 42 | Restart requested (service manager should restart) |
| Other | Failure exit |

---

## 8. Security & Access Control

### 8.1 User Allowlists

All platforms support user allowlists:

```python
TELEGRAM_ALLOWED_USERS=your_id
DISCORD_ALLOWED_USERS=your_id
GATEWAY_ALLOWED_USERS=comma,separated,ids  # Global allowlist
GATEWAY_ALLOW_ALL_USERS=true  # Open access (not recommended)
```

### 8.2 Pairing Store

```python
from gateway.pairing import PairingStore
self.pairing_store = PairingStore()
```

Code-based user authorization for DM access. Users send a pairing code to verify identity.

### 8.3 Tirith Scanner

```python
from tools.tirith_security import ensure_installed
ensure_installed(log_failures=False)
```

Ensures Tirith security scanner is available at gateway startup (auto-downloads if needed). Non-fatal — fail-open at scan time.

---

## 9. Supporting Gateway Modules

### 9.1 Gateway Config

**Location**: `gateway/config.py` (~1,160 lines)

Gateway configuration loading and validation:
- Session directories
- Platform configurations
- Timeout settings
- Delivery rules

### 9.2 Gateway Session

**Location**: `gateway/session.py` (~1,086 lines)

Session transcript management:
- File-based storage (JSON per session)
- Session creation/loading
- Transcript append
- Reset protection (checks for active processes)
- Session suspension

### 9.3 Delivery Router

**Location**: `gateway/delivery.py` (~256 lines)

Routes agent responses to correct destinations:
- Platform detection
- Chat/thread routing
- Auto-delivery for cron sessions
- Format conversion per platform

### 9.4 Stream Consumer

**Location**: `gateway/stream_consumer.py` (~744 lines)

Processes streaming LLM responses:
- Token-by-token delta handling
- Tool call assembly from stream
- Platform adapter text forwarding
- Status updates during streaming

### 9.5 Session Context

**Location**: `gateway/session_context.py` (~128 lines)

Per-session context data shared across gateway components.

### 9.6 Mirror

**Location**: `gateway/mirror.py` (~132 lines)

Mirrors agent-sent messages back into session transcripts for conversation completeness.

### 9.7 Pairing Store

**Location**: `gateway/pairing.py` (~309 lines)

Manages code-based user pairing for DM access authorization.

### 9.8 Hooks

**Location**: `gateway/hooks.py` (~170 lines)

Event hook system for plugins:
- `on_session_start` — fired when new session begins
- `pre_llm_call` — fired before each API call
- Hook discovery and loading from `gateway/builtin_hooks/`

### 9.9 Builtin Hooks

**Location**: `gateway/builtin_hooks/boot_md.py` (~85 lines)

Built-in hook implementations. Example: `boot_md` injects `BOOT.md` content into session context.

### 9.10 Display Config

**Location**: `gateway/display_config.py` (~194 lines)

Display configuration for platform-specific message formatting.

### 9.11 Sticker Cache

**Location**: `gateway/sticker_cache.py` (~111 lines)

Caches sticker/GIF references for platform adapters.

### 9.12 Restart

**Location**: `gateway/restart.py` (~20 lines)

Restart constants:
```python
DEFAULT_GATEWAY_RESTART_DRAIN_TIMEOUT = 30
GATEWAY_SERVICE_RESTART_EXIT_CODE = 42
```

### 9.13 Status

**Location**: `gateway/status.py` (~455 lines)

Gateway status management:
- PID file writing
- Runtime status JSON
- Process termination
- Health checking

---

## Key Numbers

| Metric | Value |
|--------|-------|
| gateway/run.py lines | 9,646 |
| gateway/config.py lines | 1,160 |
| gateway/session.py lines | 1,086 |
| gateway/stream_consumer.py lines | 744 |
| gateway/status.py lines | 455 |
| gateway/pairing.py lines | 309 |
| gateway/delivery.py lines | 256 |
| gateway/display_config.py lines | 194 |
| gateway/hooks.py lines | 170 |
| gateway/session_context.py lines | 128 |
| gateway/sticker_cache.py lines | 111 |
| gateway/builtin_hooks lines | 85 |
| gateway/restart.py lines | 20 |
| Total gateway/ lines | ~45,107 |
| Platform adapter env vars | 15+ allowlist vars |
| Restart drain timeout | 30 seconds |
| Restart exit code | 42 |
| Stuck-loop threshold | 3+ consecutive restarts |
| Busy input modes | 3 (interrupt, queue, reject) |
| Voice modes | 3 (off, voice_only, all) |
| SSL cert fallback locations | 10 |

---

*Generated from source analysis of the Hermes Agent codebase.*
