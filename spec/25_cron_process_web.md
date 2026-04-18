# Hermes Agent — Cron, Process Registry & Web Server

This document covers the cron job scheduler, background process registry, and the FastAPI web UI server.

---

## Table of Contents

1. [Cron Job System](#1-cron-job-system)
2. [Process Registry](#2-process-registry)
3. [Web Server](#3-web-server)

---

## 1. Cron Job System

### Location

`cron/scheduler.py` (~200 lines), `cron/jobs.py` (~400 lines)

### Purpose

Scheduled job execution with flexible scheduling (cron expressions, intervals, one-shot timestamps), output delivery to messaging platforms, and agent-based execution.

### 1.1 Storage

| Path | Purpose |
|------|---------|
| `~/.hermes/cron/jobs.json` | Job definitions |
| `~/.hermes/cron/output/{job_id}/{timestamp}.md` | Job outputs |
| `~/.hermes/cron/.tick.lock` | Tick execution lock |

Directory permissions: `0700` (owner-only). File permissions: `0600`.

### 1.2 Schedule Parsing

Three schedule types:

| Input | Kind | Behavior |
|-------|------|----------|
| `"30m"`, `"2h"`, `"1d"` | `once` | One-shot, runs after duration from now |
| `"every 30m"`, `"every 2h"` | `interval` | Recurring at specified interval |
| `"0 9 * * *"` | `cron` | Standard 5-field cron expression |
| `"2026-02-03T14:00"` | `once` | One-shot at specific timestamp |

Duration parsing:
```python
parse_duration("30m")  → 30 minutes
parse_duration("2h")   → 120 minutes
parse_duration("1d")   → 1440 minutes
```

Cron validation requires `croniter` package.

### 1.3 Tick Scheduler

`tick()` — called every 60 seconds from gateway background thread:

```
1. Acquire file-based lock (~/.hermes/cron/.tick.lock)
2. Load due jobs from jobs.json
3. For each due job:
   a. Create cron_ prefixed session_id
   b. Execute job via agent or direct command
   c. Save output to local files
   d. Deliver to configured target (origin, local, platform:chat_id)
   e. Advance next_run timestamp (for recurring jobs)
   f. Mark job run in storage
```

File-based lock prevents concurrent ticks from gateway + daemon + systemd timer.

### 1.4 Delivery Resolution

`_resolve_delivery_target(job)` — determines where cron output goes:

| Deliver Value | Target |
|--------------|--------|
| `"local"` | Save to local files only |
| `"origin"` | Back to message source that created the job |
| `"telegram"` | Telegram home channel |
| `"telegram:123456"` | Specific Telegram chat |
| `"discord:789:thread"` | Discord channel with thread |

**Origin fallback**: When `deliver=origin` but no origin metadata (job created via API/script), tries home channels in order: matrix → telegram → discord → slack → bluebubbles.

**Channel name resolution**: Human-friendly labels like `"Alice (dm)"` resolved via `resolve_channel_name()` to real chat IDs.

### 1.5 Media Detection

Cron outputs containing file references are detected by extension:

| Extension Set | Adapter Method |
|--------------|----------------|
| `.ogg`, `.opus`, `.mp3`, `.wav`, `.m4a` | `send_voice()` |
| `.mp4`, `.mov`, `.avi`, `.mkv`, `.webm`, `.3gp` | `send_video()` |
| `.jpg`, `.jpeg`, `.png`, `.webp`, `.gif` | `send_image_file()` |
| Other | `send_document()` |

### 1.6 Silent Marker

When a cron agent has nothing new to report, it can prefix output with `[SILENT]` to suppress delivery. Output is still saved locally for audit.

### 1.7 Multi-Skill Support

Jobs can declare multiple skills:
```json
{
  "skills": ["skill-one", "skill-two"],
  "skill": "skill-one"  // legacy field, kept in sync
}
```

Normalized via `_normalize_skill_list()` — deduplicates, preserves order.

### 1.8 Security

- **Prompt injection scanning**: Cron job prompts scanned for injection patterns
- **Delivery platform validation**: `_KNOWN_DELIVERY_PLATFORMS` frozenset prevents env var enumeration via crafted platform names
- **Directory permissions**: `0700` for dirs, `0600` for files

### 1.9 One-Shot Grace

`ONESHOT_GRACE_SECONDS = 120` — one-shot jobs get a 2-minute grace window after their scheduled time before being considered missed.

---

## 2. Process Registry

### Location

`tools/process_registry.py` (~800+ lines)

### Purpose

In-memory registry for managed background processes spawned via `terminal(background=true)`. Provides output buffering, status polling, blocking wait, process killing, and crash recovery.

### 2.1 Limits

| Metric | Value |
|--------|-------|
| Max output buffer | 200KB (rolling window) |
| Finished TTL | 30 minutes |
| Max concurrent processes | 64 (LRU pruning) |
| Watch max per window | 8 notifications |
| Watch window | 10 seconds |
| Watch overload kill | 45 seconds sustained |

### 2.2 ProcessSession

```python
@dataclass
class ProcessSession:
    id: str                          # "proc_xxxxxxxxxxxx"
    command: str                     # Original command
    task_id: str                     # Task/sandbox isolation key
    session_key: str                 # Gateway session key
    pid: int                         # OS process ID
    process: subprocess.Popen        # Handle (local only)
    env_ref: Any                     # Reference to environment object
    cwd: str                         # Working directory
    started_at: float                # time.time()
    exited: bool                     # Whether finished
    exit_code: int | None            # Exit code
    output_buffer: str               # Rolling output (last 200KB)
    detached: bool                   # Recovered from crash (no pipe)
    pid_scope: str                   # "host" or "sandbox"
    # Watcher metadata (persisted for crash recovery)
    watcher_platform: str
    watcher_chat_id: str
    watcher_user_id: str
    watcher_interval: int            # 0 = no watcher
    notify_on_complete: bool         # Queue notification on exit
    watch_patterns: List[str]        # Trigger patterns
```

### 2.3 Spawn Modes

**`spawn_local()`** — For `TERMINAL_ENV=local`:
- Uses user's login shell (`bash -lic command`)
- `PYTHONUNBUFFERED=1` forced for visible output
- Process group creation via `os.setsid` (Unix)
- Output reader thread drains stdout into rolling buffer

**PTY mode** (`use_pty=True`):
- Uses `ptyprocess` (Unix) or `winpty` (Windows)
- For interactive CLI tools (Codex, Claude Code, Python REPL)
- Terminal dimensions: 30 rows × 120 columns
- Falls back to standard Popen if ptyprocess unavailable

**`spawn_via_env()`** — For sandboxed backends (Docker, SSH, Modal, etc.):
- Command runs inside the sandbox environment
- Output collected via environment's execute interface
- No direct PID access (PID scope = "sandbox")

### 2.4 Output Reader Loop

```python
def _reader_loop(self, session):
    while not session.exited:
        line = session.process.stdout.readline()
        if not line:
            break
        # Append to rolling buffer (trim to MAX_OUTPUT_CHARS)
        # Strip shell startup noise from beginning
        # Scan watch patterns
        # Queue notifications
```

Shell noise stripped from beginning:
- `bash: cannot set terminal process group`
- `bash: no job control in this shell`
- `tcsetattr: Inappropriate ioctl for device`

### 2.5 Watch Pattern System

Scans new output line-by-line for configured patterns:

```python
watch_patterns: ["ERROR", "FAILED", "completed"]
```

**Rate limiting**: Max 8 notifications per 10-second rolling window.

**Overload kill switch**: If sustained overload (matches suppressed for 45+ seconds), watching is permanently disabled for that process.

Notifications queued to `completion_queue`:
```python
{
    "session_id": "proc_xxx",
    "command": "...",
    "type": "watch_match",
    "pattern": "ERROR",
    "output": "matched line...",
    "suppressed": 5,
}
```

### 2.6 Completion Queue

Unified queue for all background process events:
- `"complete"` — process finished (notify_on_complete)
- `"watch_match"` — watch pattern matched
- `"watch_disabled"` — overload disabled watching

Consumed by:
- CLI `process_loop` — auto-triggers new agent turns
- Gateway drain loop — processes after each agent turn

`_completion_consumed` set tracks sessions whose completion was already consumed via `wait`/`poll`/`log` — drain loops skip these.

### 2.7 Crash Recovery

Checkpoint file: `~/.hermes/processes.json`

```python
def _write_checkpoint(self):
    # Serialize running sessions to JSON
    # Atomic write (tempfile + os.replace)

def load_checkpoint(self):
    # Load from JSON
    # Mark sessions as detached (no pipe available)
    # Verify PIDs still alive (os.kill(pid, 0))
    # Move dead sessions to finished
```

Detached sessions (recovered from crash):
- No stdout pipe — cannot read more output
- PID liveness checked via `os.kill(pid, 0)`
- Exit code unavailable once original process object is gone

### 2.8 Operations

| Method | Purpose |
|--------|---------|
| `spawn_local()` | Spawn background process locally |
| `spawn_via_env()` | Spawn inside sandbox environment |
| `poll(session_id)` | Get status + output snapshot |
| `wait(session_id, timeout)` | Block until finished or timeout |
| `kill(session_id)` | Terminate process |
| `log(session_id, lines)` | Get last N lines of output |
| `list(task_id)` | List processes for a task |
| `reset_session_processes(session_key)` | Kill all processes for a gateway session |

### 2.9 LRU Pruning

When `_running` + `_finished` exceeds `MAX_PROCESSES` (64):
- Prune oldest finished processes first
- Never prune running processes

### 2.10 Process Termination

```python
def _terminate_host_pid(pid):
    # Windows: os.kill(pid, SIGTERM)
    # Unix: os.killpg(os.getpgid(pid), SIGTERM) → SIGTERM
```

---

## 3. Web Server

### Location

`hermes_cli/web_server.py` (~600+ lines)

### Purpose

FastAPI backend serving the Vite/React frontend and REST API for managing configuration, environment variables, and sessions.

### 3.1 Security

**Session token**: Fresh `secrets.token_urlsafe(32)` generated on every server start. Injected into SPA HTML. Dies when process exits.

**Auth middleware**: All `/api/` routes require `Authorization: Bearer {token}` except public paths:
- `/api/status`
- `/api/config/defaults`
- `/api/config/schema`
- `/api/model/info`

Uses `hmac.compare_digest` to prevent timing side-channels.

**CORS**: Restricted to localhost origins only:
```python
allow_origin_regex=r"^https?://(localhost|127\.0\.0\.1)(:\d+)?"
```

Binding to `0.0.0.0` with `allow_origins=["*"]` would let any website read/modify config and secrets.

**Rate limiter**: `/api/config/reveal` endpoint limited to 5 requests per 30 seconds.

### 3.2 Config Schema

Auto-generated from `DEFAULT_CONFIG` with manual overrides for select fields:

| Field | Type | Options |
|-------|------|---------|
| `terminal.backend` | select | local, docker, ssh, modal, daytona, singularity |
| `terminal.modal_mode` | select | sandbox, function |
| `tts.provider` | select | edge, elevenlabs, openai, neutts |
| `stt.provider` | select | local, openai, mistral |
| `display.skin` | select | default, ares, mono, slate |
| `memory.provider` | select | builtin, honcho |
| `approvals.mode` | select | ask, yolo, deny |
| `context.engine` | select | default, custom |

### 3.3 Endpoints

| Path | Method | Purpose |
|------|--------|---------|
| `/api/status` | GET | Gateway runtime status |
| `/api/config` | GET/PUT | Read/write config.yaml |
| `/api/config/defaults` | GET | Default config values |
| `/api/config/schema` | GET | JSON schema for config |
| `/api/env` | GET/POST/DELETE | Manage .env variables |
| `/api/config/reveal` | GET | Show redacted secrets (rate-limited) |
| `/api/model/info` | GET | Model metadata |
| `/` | GET | Serve SPA (index.html) |

### 3.4 Static Files

Web assets served from `hermes_cli/web_dist/` — built Vite/React output.

### 3.5 Default Port

`9119` (configurable via `--port` flag).

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Cron schedule types | 3 (once, interval, cron) |
| Cron tick interval | 60 seconds |
| One-shot grace period | 120 seconds |
| Known delivery platforms | 17 |
| Process output buffer | 200KB |
| Process finished TTL | 30 minutes |
| Max concurrent processes | 64 |
| Watch rate limit | 8 per 10s window |
| Watch overload kill | 45 seconds |
| Web server default port | 9119 |
| Web session token length | 32 bytes (urlsafe) |
| Reveal rate limit | 5 per 30s |
| CORS allowed origins | localhost + 127.0.0.1 only |
| Cron directory permissions | 0700 |
| Cron file permissions | 0600 |

---

*Generated from source analysis of the Hermes Agent codebase.*
