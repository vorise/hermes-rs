# Hermes Agent — Gateway Internals

This document covers the gateway lifecycle, message processing pipeline, session management, event hooks, delivery routing, session context, PID/status management, and configuration loading.

---

## Table of Contents

1. [Gateway Runner Architecture](#1-gateway-runner-architecture)
2. [Startup Lifecycle](#2-startup-lifecycle)
3. [Message Processing Pipeline](#3-message-processing-pipeline)
4. [Session Management](#4-session-management)
5. [Event Hook System](#5-event-hook-system)
6. [Delivery Router](#6-delivery-router)
7. [Session Context (ContextVars)](#7-session-context-contextvars)
8. [PID File & Scoped Locks](#8-pid-file--scoped-locks)
9. [Shutdown & Restart](#9-shutdown--restart)
10. [Background Tasks](#10-background-tasks)
11. [Configuration Loading Pipeline](#11-configuration-loading-pipeline)
12. [Session Mirroring](#12-session-mirroring)
13. [BOOT.md Hook](#13-bootmd-hook)

---

## 1. Gateway Runner Architecture

### Location

`gateway/run.py` (~4500+ lines)

### Purpose

`GatewayRunner` is the central controller managing the lifecycle of all platform adapters, message routing, session state, agent execution, and graceful shutdown.

### 1.1 Core State

```python
class GatewayRunner:
    config: GatewayConfig                       # Loaded gateway configuration
    adapters: Dict[Platform, BasePlatformAdapter]  # Connected platform adapters
    session_store: SessionStore                 # Session tracking + transcript storage
    delivery_router: DeliveryRouter             # Output delivery management
    hooks: HookRegistry                         # Event hook system
    pairing_store: PairingStore                 # DM pairing approval

    _running_agents: Dict[str, Any]             # session_key → AIAgent (or _AGENT_PENDING_SENTINEL)
    _running_agents_ts: Dict[str, float]        # session_key → start timestamp
    _agent_cache: Dict[str, tuple]              # session_key → (AIAgent, config_signature) for prompt caching
    _agent_cache_lock: threading.Lock           # Protects _agent_cache

    _session_model_overrides: Dict[str, Dict]   # session_key → {model, provider, api_key, ...}
    _pending_approvals: Dict[str, Dict]         # session_key → pending exec approval info
    _failed_platforms: Dict[Platform, Dict]     # platform → {config, attempts, next_retry}
    _background_tasks: set[asyncio.Task]        # Fire-and-forget async tasks
    _voice_mode: Dict[str, str]                 # chat_id → "off" | "voice_only" | "all"
    _update_prompt_pending: Dict[str, bool]     # session_key → True when awaiting /update response

    _running: bool                              # Gateway is running
    _draining: bool                             # Gateway is shutting down
    _restart_requested: bool                    # Restart was requested
    _restart_detached: bool                     # Detached restart (setsid)
    _restart_via_service: bool                  # Service-managed restart (systemd)
    _exit_code: Optional[int]                   # Exit code for service restart
    _shutdown_event: asyncio.Event              # Signal for wait_for_shutdown()
```

### 1.2 Agent Pending Sentinel

```python
_AGENT_PENDING_SENTINEL = object()
```

Placed into `_running_agents` **before** any `await` point in `_handle_message()`. Prevents duplicate agent creation during async gaps (vision enrichment, STT, session compression). Cleaned up in `finally` block.

### 1.3 AIAgent Caching

Cache key: `session_key` → `(AIAgent, config_signature_str)`.

**Why**: Without caching, every message creates a new `AIAgent`, rebuilding the system prompt (including memory) from scratch — breaking Anthropic's prefix prompt caching and costing ~10x more.

Cache invalidated on: `/reset`, `/new`, session expiry, model override changes.

---

## 2. Startup Lifecycle

### 2.1 Import Order

```
1. _ensure_ssl_certs()          # SSL cert auto-detection (NixOS, macOS Homebrew)
2. sys.path.insert(0, parent)   # Enable imports from project root
3. get_hermes_home()            # Resolve HERMES_HOME
4. load_hermes_dotenv()         # Load .env from HERMES_HOME + project root
5. config.yaml → env bridging   # terminal.*, auxiliary.*, agent.*, display.*, timezone
6. apply_ipv4_preference()      # Force IPv4 if configured
7. print_config_warnings()      # Validate config structure
8. HERMES_QUIET=1               # Gateway quiet mode
9. HERMES_EXEC_ASK=1            # Interactive exec approval enabled
10. TERMINAL_CWD resolution     # Respect configured cwd or default to home
```

### 2.2 SSL Certificate Auto-Detection

Checks in order:
1. `SSL_CERT_FILE` env var (user-set → skip)
2. `ssl.get_default_verify_paths()` (Python compiled-in defaults)
3. `certifi.where()` (Mozilla bundle via certifi package)
4. Common distro paths: Debian, RHEL, SUSE, Alpine, macOS Homebrew

### 2.3 Config.yaml to Environment Bridging

Bridges nested config.yaml keys to env vars so `os.getenv()` works uniformly:

| Config Path | Env Var |
|------------|---------|
| `terminal.backend` | `TERMINAL_ENV` |
| `terminal.cwd` | `TERMINAL_CWD` |
| `terminal.docker_image` | `TERMINAL_DOCKER_IMAGE` |
| `auxiliary.vision.provider` | `AUXILIARY_VISION_PROVIDER` |
| `auxiliary.web_extract.model` | `AUXILIARY_WEB_EXTRACT_MODEL` |
| `auxiliary.approval.api_key` | `AUXILIARY_APPROVAL_API_KEY` |
| `agent.max_turns` | `HERMES_MAX_ITERATIONS` |
| `agent.gateway_timeout` | `HERMES_AGENT_TIMEOUT` |
| `display.busy_input_mode` | `HERMES_GATEWAY_BUSY_INPUT_MODE` |
| `timezone` | `HERMES_TIMEZONE` |
| `security.redact_secrets` | `HERMES_REDACT_SECRETS` |

**Precedence**: env var already set → don't override. Config.yaml values take precedence over `.env` for terminal settings.

### 2.4 Adapter Startup Sequence

```
start():
  1. Discover and load event hooks (self.hooks.discover_and_load())
  2. Recover background processes from checkpoint (crash recovery)
  3. Suspend recently active sessions (unless .clean_shutdown marker exists)
  4. Auto-suspend stuck-loop sessions (3+ consecutive restarts while active)
  5. For each enabled platform:
     a. _create_adapter(platform, config)
     b. Set handlers: message, fatal error, session store, busy session
     c. await adapter.connect()
     d. On success: add to self.adapters, sync voice modes
     e. On failure: queue for background reconnection
  6. Update delivery router with connected adapters
  7. Set _running = True
  8. Emit gateway:startup hook
  9. Build channel directory for name resolution
  10. Send /update notification if pending
  11. Send /restart notification if applicable
  12. Resume recovered process watchers
  13. Start _session_expiry_watcher() background task
  14. Start _platform_reconnect_watcher() background task
```

### 2.5 Session Suspension on Startup

Prevents stuck sessions from blindly resuming after crash:

```python
if not .clean_shutdown marker:
    suspended = session_store.suspend_recently_active()
```

Skipped after clean shutdown (`hermes update`, `hermes gateway restart`, `/restart`).

### 2.6 Stuck-Loop Detection

Tracks sessions active across consecutive restarts in `.restart_failure_counts.json`:

```python
_STUCK_LOOP_THRESHOLD = 3  # restarts while active before auto-suspend
```

On startup: increments counters for sessions active at shutdown, removes counters for sessions that completed successfully. Sessions hitting threshold are auto-suspended.

---

## 3. Message Processing Pipeline

### 3.1 Entry Point: `_handle_message(event)`

```
1. Check authorization (_is_user_authorized)
   - HOMEASSISTANT, WEBHOOK: always authorized (system-generated/HMAC-validated)
   - Per-platform allow-all flag (TELEGRAM_ALLOW_ALL_USERS=true)
   - DM pairing store approval
   - Platform-specific allowlist (TELEGRAM_ALLOWED_USERS=123,456)
   - Global allowlist (GATEWAY_ALLOWED_USERS=...)
   - WhatsApp: phone↔LID alias expansion from bridge session files
   - Default: deny (pairing flow in DMs, silent ignore in groups)

2. Check for /update prompt response
   - If session awaiting update input: write response to .update_response file
   - /approve, /yes → "y"; /deny, /no → "n"; otherwise raw text

3. Staleness eviction
   - If _running_agents entry older than HERMES_AGENT_TIMEOUT (default 1800s)
     AND agent is idle beyond timeout → evict and unlock session
   - Wall-clock extreme guard: 10x timeout or 2h, whichever is larger

4. Running agent intercept (_quick_key in _running_agents)
   - /status → show session status
   - /restart → initiate gateway restart
   - /stop → hard-kill agent, clear pending queue, release lock
   - /new → soft-reset, clear pending queue, dispatch to _handle_reset_command
   - /queue <text> → queue message without interrupting
   - /model → reject (agent running)
   - /approve, /deny → direct to approval handler (bypasses interrupt — agent thread blocked on threading.Event)
   - /background → start parallel task (must not interrupt)
   - Photo follow-up → queue without interrupt (adapter-level batching)
   - Draining → queue or reject based on busy_input_mode
   - Default → interrupt running agent, queue pending message, send ack

5. Command dispatch (no running agent)
   - Emit command:{command} hook
   - Resolve aliases to canonical names
   - Handle: /new, /help, /commands, /profile, /status, /restart, /stop,
     /reasoning, /fast, /verbose, /yolo, /model, /provider, /personality,
     /plan, /retry, /undo, /sethome, /compress, /usage, /insights,
     /reload-mcp, /approve, /deny, /update, /debug, /title, /resume,
     /branch, /rollback, /background, /btw, /voice

6. Quick commands (user-defined in config.yaml)
   - type: "exec" → run shell command, return stdout (30s timeout)
   - type: "alias" → redirect to another command with args appended

7. Plugin commands → get_plugin_command_handler()

8. Skill commands → /skill-name loads skill, builds invocation message

9. Claim session (write _AGENT_PENDING_SENTINEL to _running_agents)

10. _handle_message_with_agent(event, source, _quick_key)
```

### 3.2 `_handle_message_with_agent()`

```
1. Get or create session (session_store.get_or_create_session)
2. Emit session:start hook (for new or auto-reset sessions)
3. Build session context (build_session_context)
4. Set session ContextVars (task-local state)
5. Build context prompt (build_session_context_prompt, with optional PII redaction)
6. Auto-reset notice injection (if session was auto-reset due to idle/daily)
   - Sends notification to user explaining reset reason
7. Auto-load skill(s) for topic/channel bindings (new sessions only)
8. Load conversation history from transcript
9. Session hygiene: auto-compress pathologically large transcripts
   - Threshold: 85% of model context length OR 400+ messages
   - Uses AIAgent with max_iterations=4, quiet_mode=True
   - Writes to NEW session (old transcript preserved for search)
10. First-message onboarding (very first interaction ever)
11. Home channel setup prompt (if no home channel configured)
12. Discord voice channel context injection
13. Prepare inbound message text:
    a. Shared thread: prepend [username]
    b. Image paths: run vision enrichment
    c. Audio paths: run STT transcription
    d. Documents: inject context note for text files
    e. Reply-to: inject quoted reply snippet (if not in history)
    f. @ references: expand context references
14. Emit agent:start hook
15. Run agent (_run_agent)
16. Stop typing indicator
17. Process agent result:
    a. Clear stuck-loop counter (successful turn)
    b. Handle context-overflow errors → user-friendly message
    c. Prepend reasoning/thinking if display.show_reasoning enabled
    d. Emit agent:end hook
    e. Drain process watchers
    f. Save transcript (JSONL + SQLite)
    g. Update session token counts
    h. Auto voice reply if configured
    i. Deliver MEDIA: files from response (if streaming already sent text)
18. Exception handling:
    a. Stop typing indicator
    b. Map error codes to user-friendly hints:
       - 401: Check API key
       - 402: Balance exceeded
       - 429: Rate limited (check plan usage limit reset time)
       - 529: API overloaded
       - 400/500 + long history: Context overflow
19. Finally: clear session ContextVars
```

### 3.3 Session Hygiene Compression

Pre-agent safety net for sessions that grew too large between turns:

| Metric | Value |
|--------|-------|
| Hygiene threshold | 85% of context length |
| Hard message limit | 400 messages |
| Agent compressor | 50% of context length |
| Retry limit | 3 attempts, then mark as flushed |

Token estimation priority:
1. API-reported `prompt_tokens` from last turn (stored in session entry)
2. Rough char-based estimate (`str(msg) // 4`) — overestimates by 30-50% but safe

Previous 1.4x multiplier removed — `85% * 1.4 = 119%` exceeded model limit, preventing hygiene from ever firing for some models.

---

## 4. Session Management

### Location

`gateway/session.py` (~2000+ lines)

### 4.1 SessionSource

Describes message origin for routing, context injection, and cron delivery:

```python
@dataclass
class SessionSource:
    platform: Platform
    chat_id: str
    chat_name: Optional[str]
    chat_type: str              # "dm", "group", "channel", "thread"
    user_id: Optional[str]
    user_name: Optional[str]
    thread_id: Optional[str]
    chat_topic: Optional[str]
    user_id_alt: Optional[str]  # Signal UUID
    chat_id_alt: Optional[str]  # Signal group internal ID
```

### 4.2 SessionContext

Full context for dynamic system prompt injection:

```python
@dataclass
class SessionContext:
    source: SessionSource
    connected_platforms: List[Platform]
    home_channels: Dict[Platform, HomeChannel]
    session_key: str
    session_id: str
    created_at: datetime
    updated_at: datetime
```

### 4.3 PII Redaction

```python
_PII_SAFE_PLATFORMS = frozenset({
    Platform.WHATSAPP, Platform.SIGNAL,
    Platform.TELEGRAM, Platform.BLUEBUBBLES,
})
```

For these platforms, IDs are hashed when `redact_pii=True`:
- `_hash_sender_id("+1234567890")` → `"user_<12hex>"`
- `_hash_chat_id("telegram:12345")` → `"telegram:<12hex>"`

Discord excluded — needs raw IDs for `<@user_id>` mentions.

### 4.4 Session Key Generation

Format: `agent:main:{platform}:{chat_type}:{chat_id}[:{extra}...]`

Configuration options:
- `group_sessions_per_user`: Group DMs share session per user (default: true)
- `thread_sessions_per_user`: Separate sessions per thread (default: false)

---

## 5. Event Hook System

### Location

`gateway/hooks.py` (~171 lines)

### 5.1 Architecture

```python
class HookRegistry:
    _handlers: Dict[str, List[Callable]]   # event_type → [handler_fn, ...]
    loaded_hooks: List[str]                # discovered hook names
```

### 5.2 Discovery

Scans `~/.hermes/hooks/*/HOOK.yaml` + `handler.py`:

```yaml
# HOOK.yaml
name: my-hook
description: Does something on events
events:
  - gateway:startup
  - session:start
  - agent:end
```

### 5.3 Events

| Event | Context | When |
|-------|---------|------|
| `gateway:startup` | `{platforms: [...]}` | After all adapters connected |
| `session:start` | `{platform, user_id, session_id, session_key}` | New or auto-reset session |
| `session:end` | `{platform, user_id, session_key}` | Session ending (/reset) |
| `session:reset` | `{platform, user_id, session_key}` | After session reset |
| `agent:start` | `{platform, user_id, session_id, message}` | Before agent turn |
| `agent:step` | `{platform, user_id, session_id, tool, args}` | Each tool call |
| `agent:end` | `{platform, user_id, session_id, response}` | After agent turn |
| `command:*` | `{platform, user_id, command, args}` | Any recognized slash command |

### 5.4 Wildcard Matching

`command:*` matches `command:reset`, `command:help`, etc. Implemented via prefix matching.

### 5.5 Built-in Hook: boot-md

Runs `~/.hermes/BOOT.md` on gateway startup. Spawns a one-shot `AIAgent` with:
- `quiet_mode=True`, `skip_context_files=True`, `skip_memory=True`
- `max_iterations=20`
- `[SILENT]` marker suppresses delivery if nothing to report

---

## 6. Delivery Router

### Location

`gateway/delivery.py` (~257 lines)

### 6.1 DeliveryTarget Parsing

```python
DeliveryTarget(platform="telegram", chat_id="123456", thread_id=None, is_origin=False, is_explicit=False)
```

Parse formats:
| Input | Result |
|-------|--------|
| `"origin"` | Target = message source (cron delivery) |
| `"local"` | Save to local files only |
| `"telegram"` | Telegram home channel |
| `"telegram:123456"` | Specific Telegram chat |
| `"telegram:123456:thread"` | Telegram chat with thread |

### 6.2 Delivery

```python
deliver(targets, content, job_id=None):
  For each target:
    if "local":
      Save to ~/.hermes/cron/output/{job_id}/{timestamp}.md
    if platform:
      adapter.send(chat_id, truncated_content)
      If content > 4000 chars: truncate, save full version to disk
      Include thread_id in metadata if present
```

### 6.3 Origin Fallback

When `deliver=origin` but no origin metadata (job created via API/script):
Tries home channels in order: matrix → telegram → discord → slack → bluebubbles.

---

## 7. Session Context (ContextVars)

### Location

`gateway/session_context.py` (~129 lines)

### 7.1 Problem

`os.environ` is process-global — concurrent gateway sessions race on env var reads/writes. Replaced with `ContextVar` for task-local state.

### 7.2 ContextVars

| Variable | Purpose |
|----------|---------|
| `current_platform` | Platform enum value |
| `current_chat_id` | Chat ID |
| `current_chat_name` | Chat display name |
| `current_thread_id` | Thread/forum ID |
| `current_user_id` | User ID |
| `current_user_name` | User display name |
| `current_session_key` | Session key |

### 7.3 API

```python
tokens = set_session_vars(platform, chat_id, chat_name, thread_id, user_id, user_name, session_key)
# ... do work ...
clear_session_vars(tokens)  # Restores previous values
```

### 7.4 Resolution Order

`get_session_env(name)`:
1. ContextVar (task-local, set by gateway)
2. os.environ (CLI fallback, legacy compatibility)
3. Default value

---

## 8. PID File & Scoped Locks

### Location

`gateway/status.py` (~456 lines)

### 8.1 PID File

`gateway.pid` — JSON record:
```json
{
  "pid": 12345,
  "kind": "gateway",
  "argv": ["python", "-m", "gateway.run"],
  "start_time": 1234567890.0
}
```

### 8.2 PID Verification

`get_running_pid()`:
1. Read PID file
2. Verify process alive via `os.kill(pid, 0)`
3. Check start_time matches
4. Validate cmdline contains gateway process

Stale cleanup: removes file if process dead or not gateway.

### 8.3 Scoped Locks

`acquire_scoped_lock(scope, identity)`:
- Creates lock file: `XDG_STATE_HOME/hermes/gateway-locks/{scope}-{hash(identity)}.lock`
- `O_CREAT | O_EXCL` — atomic creation, fails if exists
- Writes PID + start_time + identity to lock file

Stale lock detection:
1. PID liveness check
2. Start_time comparison (is lock holder newer?)
3. `/proc/PID/status` stopped state check (frozen/hung process)

`release_all_scoped_locks()`: Cleans up stale locks during `--replace`.

Lock directory: `XDG_STATE_HOME/hermes/gateway-locks/` or `HERMES_GATEWAY_LOCK_DIR`.

### 8.4 Process Termination

`terminate_pid()`:
- POSIX: `os.killpg(os.getpgid(pid), SIGTERM)` → wait → `SIGKILL`
- Windows: `taskkill /T /F /PID {pid}`

### 8.5 Runtime Status

`gateway_state.json`:
```json
{
  "gateway_state": "running",
  "exit_reason": null,
  "restart_requested": false,
  "active_agents": 2,
  "platforms": {
    "telegram": {"platform_state": "connected"},
    "discord": {"platform_state": "fatal", "error_code": "auth_failed"}
  }
}
```

---

## 9. Shutdown & Restart

### 9.1 Drain Sequence

```
stop(restart=False, detached_restart=False, service_restart=False):
  1. Set _running = False, _draining = True
  2. Notify active sessions (adapters still connected)
  3. Drain active agents (timeout: HERMES_RESTART_DRAIN_TIMEOUT, default 30s)
  4. If timed out: interrupt remaining agents, wait 5s
  5. If detached restart: launch setsid + bash helper
  6. Finalize agents (on_session_finalize hook, shutdown_memory, close tool resources)
  7. Disconnect all adapters (cancel background tasks first)
  8. Cancel background tasks
  9. Clear all state (adapters, running_agents, pending_messages, approvals)
  10. Global cleanup: kill_all(), cleanup_all_environments(), cleanup_all_browsers()
  11. Remove PID file
  12. Write .clean_shutdown marker (unless timed out)
  13. Increment restart failure counters for stuck-loop detection
  14. If service restart: set exit code 75 (systemd restart)
  15. Set _draining = False, update runtime status
```

### 9.2 Drain Timeout

Configurable via `agent.restart_drain_timeout` in config.yaml or `HERMES_RESTART_DRAIN_TIMEOUT` env var.

### 9.3 Detached Restart

```python
_launch_detached_restart_command():
  # Wait for current PID to die
  "while kill -0 {current_pid} 2>/dev/null; do sleep 0.2; done; "
  # Then start new gateway
  "{hermes} gateway restart"
  # Run via setsid (new session, detached from parent)
```

### 9.4 Service Restart

When `INVOCATION_ID` env var is set (systemd): exits with code 75 (`GATEWAY_SERVICE_RESTART_EXIT_CODE`). Service manager handles restart.

### 9.5 Clean Shutdown Marker

`.clean_shutdown` marker file — next startup skips session suspension if present.

**Skipped when**: drain timed out (agents were force-interrupted, sessions may be in incomplete state).

---

## 10. Background Tasks

### 10.1 Session Expiry Watcher

Runs every 300 seconds (5 min), initial delay 60s.

For each expired session (per reset policy):
1. Flush memories in thread pool
2. Shutdown memory provider on cached agent
3. Close tool resources (terminal, browser)
4. Mark `memory_flushed = True`, persist to disk

Retry: 3 attempts max, then mark as flushed to prevent infinite loop.

### 10.2 Platform Reconnection Watcher

Runs every 10 seconds, initial delay 10s.

For each failed platform:
1. Check if retry time reached
2. If attempts >= 20: give up, remove from queue
3. Create adapter, connect
4. On success: add to adapters, sync voice modes, rebuild channel directory
5. On failure: exponential backoff (30s → 60s → 120s → 240s → 300s cap)

Non-retryable errors (bad auth token): removed from queue immediately.

### 10.3 Background Task Tracking

```python
self._background_tasks: set[asyncio.Task]
task.add_done_callback(self._background_tasks.discard)
```

Prevents garbage collection of fire-and-forget tasks. Cleared on shutdown.

---

## 11. Configuration Loading Pipeline

### Location

`gateway/config.py` (~1161 lines)

### 11.1 Platform Enum (20 platforms)

LOCAL, TELEGRAM, DISCORD, WHATSAPP, SLACK, SIGNAL, MATTERMOST, MATRIX, HOMEASSISTANT, EMAIL, SMS, DINGTALK, API_SERVER, WEBHOOK, FEISHU, WECOM, WECOM_CALLBACK, WEIXIN, BLUEBUBBLES, QQBOT

### 11.2 Config Loading Priority

```
1. Environment variables (highest priority)
2. config.yaml
3. gateway.json (legacy)
4. Built-in defaults (lowest priority)
```

### 11.3 SessionResetPolicy

```python
@dataclass
class SessionResetPolicy:
    mode: str          # "daily", "idle", "both", "none"
    at_hour: int       # Hour for daily reset (default: 4)
    idle_minutes: int  # Idle timeout (default: 1440 = 24h)
    notify: bool       # Send reset notification (default: true)
    notify_exclude_platforms: List[str]  # Platforms to exclude
```

### 11.4 StreamingConfig

```python
@dataclass
class StreamingConfig:
    enabled: bool
    transport: str          # "edit" (message editing)
    edit_interval: float    # 1.0 seconds
    buffer_threshold: int   # 40 chars
    cursor: str             # Cursor marker for streaming
```

### 11.5 Validation

- `at_hour`: must be 0-23
- `idle_minutes`: must be > 0
- Warns on empty tokens
- Rejects placeholder tokens via `has_usable_secret()`

---

## 12. Session Mirroring

### Location

`gateway/mirror.py` (~133 lines)

### Purpose

When a message is sent to a platform (via `send_message` or cron delivery), appends a "delivery-mirror" record to the target session's transcript so the receiving-side agent has context about what was sent.

### 12.1 Flow

```python
mirror_to_session(platform, chat_id, message_text, source_label, thread_id):
  1. _find_session_id(platform, chat_id, thread_id)
     - Scans sessions.json for matching origin
     - Returns most recently updated session_id
  2. Build mirror message:
     {"role": "assistant", "content": text, "mirror": True, "mirror_source": label}
  3. Append to JSONL transcript
  4. Append to SQLite session DB
```

All errors caught — never fatal. Returns `False` if no matching session found.

---

## 13. BOOT.md Hook

### Location

`gateway/builtin_hooks/boot_md.py` (~86 lines)

### Purpose

Runs `~/.hermes/BOOT.md` instructions on every gateway startup.

### 13.1 Activation

Create `~/.hermes/BOOT.md` with instructions:

```markdown
# Startup Checklist

1. Check if any cron jobs failed overnight
2. Send a status update to Discord #general
3. If there are errors in /opt/app/deploy.log, summarize them
```

### 13.2 Execution

```python
handle("gateway:startup", context):
  1. Check if BOOT.md exists and is non-empty
  2. Build prompt: "Follow BOOT.md instructions exactly. Reply [SILENT] if nothing needs attention."
  3. Spawn daemon thread:
     - AIAgent(quiet_mode=True, skip_context_files=True, skip_memory=True, max_iterations=20)
     - Runs in background — doesn't block gateway startup
  4. If response contains [SILENT]: suppress delivery
```

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Platform adapters | 20 |
| Slash commands | 30+ |
| Hook events | 7 + wildcard |
| Session expiry check interval | 300 seconds |
| Platform reconnect check interval | 10 seconds |
| Max reconnect attempts | 20 |
| Reconnect backoff cap | 300 seconds |
| Stuck-loop threshold | 3 consecutive restarts |
| Drain timeout default | 30 seconds |
| Session hygiene threshold | 85% of context length |
| Hard message limit | 400 messages |
| Memory flush retries | 3 |
| Dedup cache size | 2000 entries |
| Dedup TTL | 300 seconds |
| Voice modes | 3 (off, voice_only, all) |
| Service restart exit code | 75 |
| Agent pending sentinel | Prevents duplicate agents during async gap |
| AIAgent cache key | session_key → (AIAgent, config_signature) |
| ContextVars | 7 task-local variables |
| Bootstrap file | BOOT.md |

---

*Generated from source analysis of the Hermes Agent codebase.*
