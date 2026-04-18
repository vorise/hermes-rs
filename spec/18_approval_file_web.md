# Hermes Agent — Approval, File Tools & Web Systems

This document covers the dangerous command approval system, file tools module, and web tool architecture.

---

## Table of Contents

1. [Dangerous Command Approval System](#1-dangerous-command-approval-system)
2. [File Tools](#2-file-tools)
3. [Web Tools](#3-web-tools)

---

## 1. Dangerous Command Approval System

### Location

`tools/approval.py` (~927 lines)

### Purpose

Single source of truth for pre-execution security checks on terminal commands. Combines pattern-based dangerous command detection, tirith security scanning, smart LLM-based approval, and interactive user approval prompting into one orchestration flow.

### 1.1 Detection Patterns

`DANGEROUS_PATTERNS` — ~35 regex patterns covering:

| Category | Examples |
|----------|----------|
| Destructive file ops | `rm -r /`, `rm --recursive`, `find -delete`, `xargs rm` |
| Permission changes | `chmod 777`, `chmod --recursive ... 777` |
| Ownership changes | `chown -R root` |
| Disk operations | `mkfs`, `dd if=`, `> /dev/sd` |
| SQL destruction | `DROP TABLE`, `DELETE FROM` (no WHERE), `TRUNCATE` |
| System config | `> /etc/`, `sed -i ... /etc/`, `tee ... /etc/` |
| Service management | `systemctl stop/restart/disable/mask` |
| Process killing | `kill -9 -1`, `pkill -9`, `kill $(pgrep hermes)` |
| Fork bombs | `:(){ :|:& };:` |
| Remote execution | `curl ... | sh`, `bash < <(curl ...)`, heredoc script execution |
| Shell invocation | `bash -c`, `python -e`, `perl -c` |
| Gateway self-kill | `hermes gateway stop/restart`, `hermes update`, `pkill gateway` |
| Git destruction | `git reset --hard`, `git push --force`, `git clean -f`, `git branch -D` |
| Execution after chmod | `chmod +x ... ; ./` |
| Gateway outside systemd | `nohup gateway run`, `gateway run & disown` |

### 1.2 Command Normalization

Before pattern matching, commands are normalized to prevent obfuscation bypass:

1. **ANSI escape stripping** — via `tools.ansi_strip`
2. **Null byte removal** — `command.replace('\x00', '')`
3. **Unicode normalization** — `unicodedata.normalize('NFKC', command)` — defeats fullwidth character obfuscation

### 1.3 Approval Keys & Legacy Aliases

Each pattern has a canonical key (human-readable description) and a legacy key (regex-derived). The `_approval_key_aliases` map ensures both work for backward compatibility with existing `command_allowlist` entries.

### 1.4 Sensitive Path Protection

Additional patterns for sensitive system files:

```python
_SENSITIVE_PATH_PREFIXES = (
    "/etc/", "/boot/", "/usr/lib/systemd/",
    "/private/etc/", "/private/var/",
)
_SENSITIVE_EXACT_PATHS = {"/var/run/docker.sock", "/run/docker.sock"}
```

SSH and Hermes env files detected via shell variable expansion:
- `$HOME/.ssh`, `~/.ssh`
- `$HERMES_HOME/.env`, `$HOME/.hermes/.env`

### 1.5 Approval State

Three tiers of approval:

| Tier | Storage | Scope |
|------|---------|-------|
| **Once** | None | Single execution only |
| **Session** | `_session_approved[session_key]` set | Duration of session |
| **Permanent** | `_permanent_approved` set + config.yaml | Across sessions |

Approval state is keyed by session for thread safety in gateway mode:

```python
_pending: dict[str, dict]           # session_key → approval data
_session_approved: dict[str, set]   # session_key → {pattern_keys}
_session_yolo: set[str]             # session_keys with YOLO bypass
_permanent_approved: set            # global permanent allowlist
```

### 1.6 YOLO Mode

Bypasses all approval prompts. Two entry points:

1. **CLI**: `--yolo` flag → sets `HERMES_YOLO_MODE` env var (process-scoped)
2. **Gateway**: `/yolo` slash command → session-scoped via `enable_session_yolo(session_key)`

### 1.7 Approval Modes

Configured via `approvals.mode` in config.yaml:

| Mode | Behavior |
|------|----------|
| `manual` (default) | Prompt user for every dangerous command |
| `smart` | Use auxiliary LLM for risk assessment first |
| `off` | No approval checks at all |

### 1.8 Smart Approval (Auxiliary LLM)

When `approvals.mode=smart`, an auxiliary LLM assesses risk before prompting the user:

```python
def _smart_approve(command, description) -> str:
    # Returns: "approve", "deny", or "escalate"
```

Prompt instructs the LLM to:
- **APPROVE** clearly safe commands (benign script execution, dev tools, package installs)
- **DENY** genuinely dangerous commands (recursive delete, disk wipe, database drop)
- **ESCALATE** if uncertain (falls through to manual user prompt)

Configured timeout: 16 max tokens, temperature 0.

### 1.9 CLI Interactive Approval

```
  ⚠️  DANGEROUS COMMAND: recursive delete
      rm -rf /path/to/project

      [o]nce  |  [s]ession  |  [a]lways  |  [d]eny

      Choice [o/s/a/D]:
```

Timeout: default 60s (configurable via `approvals.timeout`). On timeout, defaults to "deny".

When tirith warnings are present, the `[a]lways` option is hidden — broad permanent allowlisting is inappropriate for content-level security findings.

### 1.10 Gateway Blocking Approval

Queue-based system for concurrent gateway sessions:

```python
class _ApprovalEntry:
    event = threading.Event()
    data = {"command": ..., "description": ..., "pattern_keys": ...}
    result = None  # "once"|"session"|"always"|"deny"

_gateway_queues: dict[str, list]  # session_key → [_ApprovalEntry, ...]
_gateway_notify_cbs: dict[str, callable]  # session_key → async callback
```

**Flow**:
1. Agent thread creates `_ApprovalEntry` and appends to session queue
2. Notifies user via callback (bridges sync → async)
3. Blocks on `entry.event.wait(timeout=gateway_timeout)` — default 300s
4. User responds with `/approve` or `/deny` → `resolve_gateway_approval()` sets the event
5. Agent thread unblocks and continues

**Parallel support**: Multiple threads (parallel subagents, execute_code RPC handlers) can block concurrently — each gets its own `_ApprovalEntry`. `/approve all` resolves every pending approval in the session.

### 1.11 Session Key Resolution

Thread-safe session identity for gateway mode:

```python
_approval_session_key: ContextVar[str]  # context-local (per-gateway-thread)

def get_current_session_key():
    # 1. approval-specific contextvars (set by gateway before agent.run)
    # 2. session_context contextvars (set by _set_session_env)
    # 3. os.environ fallback (CLI, cron, tests)
```

### 1.12 Container Bypass

Container environments skip approval entirely — commands in Docker, Singularity, Modal, and Daytona sandboxes don't trigger dangerous command checks:

```python
if env_type in ("docker", "singularity", "modal", "daytona"):
    return {"approved": True, "message": None}
```

### 1.13 Combined Guard (Tirith + Dangerous Command)

`check_all_command_guards()` orchestrates both checks in three phases:

**Phase 1: Gather findings**
- Tirith security scan → `{"action": "block"|"warn"|"allow", "findings": [...]}`
- Dangerous command detection → `(is_dangerous, pattern_key, description)`

**Phase 2: Decide**
- Collect unapproved warnings into a list
- If `approvals.mode=smart`: run auxiliary LLM assessment
  - `approve` → auto-approve, skip user prompt
  - `deny` → block with definitive message
  - `escalate` → fall through to manual prompt

**Phase 3: Approval**
- Combine all warnings into a single prompt
- Gateway: queue-based blocking approval
- CLI: single combined interactive prompt
- Tirith warnings: session-only approval (no permanent allowlisting)

### 1.14 Permanent Allowlist Persistence

```yaml
# ~/.hermes/config.yaml
command_allowlist:
  - "script execution via -c/-lc flag"
  - "package install via pip"
```

Loaded on module import, saved when user selects "always" in CLI mode.

### 1.15 Approval Timeout Configuration

| Config Key | Default | Purpose |
|------------|---------|---------|
| `approvals.timeout` | 60s | CLI interactive prompt timeout |
| `approvals.gateway_timeout` | 300s | Gateway blocking approval timeout |
| `approvals.mode` | `"manual"` | Approval mode (manual/smart/off) |

---

## 2. File Tools

### Location

`tools/file_tools.py` (large file), `tools/file_operations.py` (ShellFileOperations backend)

### Purpose

LLM agent file manipulation tools: read, write, search, and patch operations backed by terminal environments (local, Docker, SSH, Modal, Daytona, Singularity).

### 2.1 Environment Integration

File operations go through `ShellFileOperations` which wraps the terminal environment for the task. Each task gets its own environment with caching:

```python
def _get_file_ops(task_id):
    # Fast path: check cache + verify env is still alive
    # Slow path: create environment using configured backend
    # Uses per-task creation locks to prevent duplicate sandbox creation
```

### 2.2 Read File

```python
read_file_tool(path, offset=1, limit=500, task_id="default")
```

**Security guards** (in order):

1. **Device path blocklist** — pure path check, no I/O:
   - `/dev/zero`, `/dev/random`, `/dev/urandom`, `/dev/full` (infinite output)
   - `/dev/stdin`, `/dev/tty`, `/dev/console` (blocks on input)
   - `/proc/*/fd/0-2` (Linux stdio aliases)

2. **Binary file guard** — extension-based check via `has_binary_extension()`:
   - Redirects to `vision_analyze` for images, `terminal` for binary inspection

3. **Hermes internal path guard** — blocks access to skill cache/hub files to prevent prompt injection

4. **Dedup check** — skips re-reads of unchanged files (same path, offset, limit, unchanged mtime):
   ```json
   {"content": "File unchanged since last read...", "dedup": true}
   ```

5. **Character-count guard** — model-agnostic proxy for token limits:
   - Default: 100,000 characters (~25-35K tokens)
   - Configurable via `file_read_max_chars` in config.yaml
   - Checked on formatted content (with line-number prefixes), not raw file size

**Post-read processing**:
- Secret redaction via `redact_sensitive_text()`
- Large-file hint when file > 512KB and no narrow range requested

### 2.3 Read Tracker

Per-task tracking for loop detection and deduplication:

```python
_read_tracker[task_id] = {
    "last_key": None,           # most recent read/search call key
    "consecutive": 0,           # repeat count of exact same call
    "read_history": set(),      # (path, offset, limit) tuples
    "dedup": {},               # (resolved_path, offset, limit) → mtime
    "read_timestamps": {},      # resolved_path → mtime (for write-time staleness check)
}
```

Dedup entries are reset on context compression (original content is summarized away, so model needs full content again).

### 2.4 Write File

Sensitive path protection blocks writes to system directories without going through terminal tool's approval system:

```python
_SENSITIVE_PATH_PREFIXES = ("/etc/", "/boot/", "/usr/lib/systemd/", ...)
_SENSITIVE_EXACT_PATHS = {"/var/run/docker.sock", "/run/docker.sock"}
```

Staleness detection: if file was modified externally between the agent's read and write, warns the agent.

### 2.5 Expected Write Exceptions

These errors don't hit error logs (expected OS-level denials):

```python
_EXPECTED_WRITE_ERRNOS = {errno.EACCES, errno.EPERM, errno.EROFS}
```

### 2.6 File Operation Caching

```python
_file_ops_cache: dict  # task_id → ShellFileOperations
_file_ops_lock: threading.Lock()
```

Cache invalidated when underlying environment is killed by cleanup thread.

---

## 3. Web Tools

### Location

`tools/web_tools.py`, `tools/web_search.py`, `tools/web_extract.py`

### Purpose

Web search and content extraction tools for the agent.

### 3.1 Web Search

Uses configurable search backends. Default is Tavily API with fallback to alternative providers.

**Search result format**:
```json
{
  "web": [
    {"url": "...", "title": "...", "description": "..."}
  ]
}
```

### 3.2 Web Extract

Extracts content from URLs using browser-based or HTTP-based extraction.

**Result format**:
```json
{
  "results": [
    {"url": "...", "title": "...", "content": "...", "error": "..."}
  ]
}
```

### 3.3 URL Safety

Optional `tools.url_safety.is_safe_url()` module consulted before browser/extract operations. Fail-closed: if module unavailable, blocks all URLs.

### 3.4 Website Policy

Optional `tools.website_policy.check_website_access(url)` module for access control. Fail-open: if module unavailable, allows all.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Dangerous command patterns | ~35 |
| Sensitive path prefixes | 5 |
| Sensitive exact paths | 2 |
| Approval modes | 3 (manual, smart, off) |
| Approval tiers | 3 (once, session, permanent) |
| Default CLI approval timeout | 60s |
| Default gateway approval timeout | 300s |
| Default max read characters | 100,000 |
| Large file hint threshold | 512 KB |
| Blocked device paths | 10 |
| Expected write errnos | 3 |

---

*Generated from source analysis of the Hermes Agent codebase.*
