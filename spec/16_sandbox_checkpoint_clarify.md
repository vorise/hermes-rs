# Hermes Agent — Sandbox, Checkpoint & Clarify Systems

This document covers the code execution sandbox (Programmatic Tool Calling), checkpoint manager (shadow git repos), and clarify tool (interactive multi-choice).

---

## Table of Contents

1. [Code Execution Sandbox](#1-code-execution-sandbox)
2. [Checkpoint Manager](#2-checkpoint-manager)
3. [Clarify Tool](#3-clarify-tool)

---

## 1. Code Execution Sandbox

### Location

`tools/code_execution_tool.py` (~53K lines)

### Purpose

Programmatic Tool Calling (PTC) — lets the LLM write a Python script that calls Hermes tools via RPC, collapsing multi-step tool chains into a single inference turn. Intermediate tool results never enter the context window; only the script's stdout is returned to the LLM.

### Architecture Overview

Two transports depending on the configured terminal backend:

```
Local (UDS):                        Remote (File-based RPC):
┌──────────────┐                    ┌──────────────┐
│ Parent Proc   │                    │ Parent Proc   │
│  UDS Server   │                    │ Poll Thread   │
└──────┬───────┘                    └──────┬───────┘
       │ Unix Domain Socket                │ req/res files
       ▼                                   ▼
┌──────────────┐                    ┌──────────────┐
│ Child Proc    │                    │ Remote Env    │
│ script.py     │                    │ script.py     │
│ hermes_tools  │                    │ hermes_tools  │
└──────────────┘                    └──────────────┘
```

### 1.1 Tool Whitelist

Only 7 tools are allowed inside the sandbox (intersection with session's enabled tools determines actual availability):

| Tool | Signature | Purpose |
|------|-----------|---------|
| `web_search` | `query, limit=5` | Web search |
| `web_extract` | `urls` | Extract content from URLs |
| `read_file` | `path, offset=1, limit=500` | Read file (1-indexed lines) |
| `write_file` | `path, content` | Write file (overwrite) |
| `search_files` | `pattern, target="content", path=".", ...` | Search file contents or find files |
| `patch` | `path, old_string, new_string, ...` | Targeted find-and-replace or V4A multi-file patches |
| `terminal` | `command, timeout=None, workdir=None` | Run shell command (foreground only) |

Blocked terminal parameters inside sandbox: `background`, `pty`, `notify_on_complete`, `watch_patterns` — these are stripped from any terminal call made from within the sandbox.

### 1.2 Resource Limits

| Parameter | Default | Config Key |
|-----------|---------|------------|
| Script timeout | 300s (5 min) | `code_execution.timeout` |
| Max tool calls | 50 | `code_execution.max_tool_calls` |
| Max stdout | 50 KB | `MAX_STDOUT_BYTES` (hardcoded) |
| Max stderr | 10 KB | `MAX_STDERR_BYTES` (hardcoded) |

### 1.3 hermes_tools.py Generation

The parent generates a `hermes_tools.py` module containing:

1. **Transport header** — UDS or file-based RPC implementation
2. **Convenience helpers**:
   - `json_parse(text)` — `json.loads` with `strict=False` for tolerant parsing of raw tabs/newlines
   - `shell_quote(s)` — `shlex.quote` for safe shell interpolation
   - `retry(fn, max_attempts=3, delay=2)` — exponential backoff retry wrapper
3. **Per-tool stub functions** — each calls the internal `_call(tool_name, args)` RPC dispatcher

Only tools in both `SANDBOX_ALLOWED_TOOLS` and `enabled_tools` get stubs. If the intersection is empty, falls back to all 7 tools.

### 1.4 UDS Transport (Local Backend)

**File**: `tools/code_execution_tool.py`, lines ~207-244

Socket path on macOS uses `/tmp` to avoid the 104-byte `AF_UNIX` limit on long `/var/folders/...` paths. On Linux, uses `tempfile.gettempdir()`.

Protocol:
1. Client connects to Unix domain socket at `HERMES_RPC_SOCKET` env var path
2. Each tool call: `json.dumps({"tool": name, "args": args}) + "\n"` sent via `sendall()`
3. Server responds with newline-delimited JSON on the same socket
4. Socket timeout: 300 seconds
5. Response parsing: double-parse tolerant (if result is a string, tries `json.loads` again)

### 1.5 File-based RPC Transport (Remote Backends)

**File**: `tools/code_execution_tool.py`, lines ~248-296

Used when terminal backend is Docker, SSH, Modal, or Daytona.

Protocol:
1. RPC directory created at `$HERMES_RPC_DIR` (or `/tmp/hermes_rpc`)
2. Request files: `req_NNNNNN` (6-digit zero-padded sequence number)
3. Response files: `res_NNNNNN`
4. Atomic writes: requests written to `.tmp` then `os.rename`
5. Polling: starts at 50ms, backs off with 1.2x multiplier to max 250ms
6. Per-call timeout: 300 seconds
7. Response files cleaned up after reading

### 1.6 RPC Server Loop (Local)

**File**: `tools/code_execution_tool.py`, lines ~307-424

Runs in a daemon thread inside the parent process:

1. Accepts one UDS client connection (5s accept timeout)
2. Reads newline-delimited JSON requests from client
3. For each request:
   - Validates tool is in `allowed_tools` set
   - Checks `tool_call_counter[0] < max_tool_calls`
   - Strips blocked terminal parameters
   - Dispatches via `handle_function_call(tool_name, tool_args, task_id)`
   - Suppresses stdout/stderr during tool dispatch (redirects to `/dev/null`)
   - Logs tool call with 80-char args preview and duration
   - Sends JSON response back over socket
4. Handles socket timeout and OS errors gracefully

### 1.7 RPC Poll Loop (Remote)

**File**: `tools/code_execution_tool.py`, lines ~564-703

Runs in a daemon thread inside the parent process:

1. Polls remote filesystem via `env.execute("ls -1 req_*")` at 100ms intervals
2. For each request file found:
   - Reads request via `env.execute("cat req_NNNNNN")`
   - Removes malformed request files to avoid infinite retry
   - Validates tool allow-list and call limit
   - Dispatches via `handle_function_call` (stdout/stderr suppressed)
   - Writes response atomically via base64-encoded `echo | base64 -d > res.tmp && mv res.tmp res`
   - Removes request file
3. Uses `stop_event` for clean shutdown

### 1.8 Remote Execution Flow

**File**: `tools/code_execution_tool.py`, lines ~706-883

1. Creates or reuses terminal environment via `_get_or_create_env(task_id)`
2. Verifies Python 3 availability on remote (`command -v python3`)
3. Creates sandbox directory: `{temp_dir}/hermes_exec_{uuid12}/`
4. Ships `hermes_tools.py` and `script.py` to remote via base64 encoding:
   ```
   echo '<base64_content>' | base64 -d > {remote_path}
   ```
   Base64 used instead of stdin piping because Modal doesn't reliably deliver stdin to chained commands.
5. Starts RPC polling thread
6. Executes script: `cd {sandbox_dir} && HERMES_RPC_DIR=... python3 script.py`
7. Post-processes stdout: truncation, ANSI strip, secret redaction
8. Cleans up remote sandbox directory

### 1.9 Local Execution Flow

**File**: `tools/code_execution_tool.py`, lines ~890-1188

1. Creates temp directory with `hermes_tools.py` and `script.py`
2. Starts UDS server on socket path
3. Builds minimal child environment:
   - **Excludes** env vars containing secret-like substrings: KEY, TOKEN, SECRET, PASSWORD, CREDENTIAL, PASSWD, AUTH
   - **Passes through** safe prefixes: PATH, HOME, USER, LANG, LC_, TERM, TMPDIR, etc.
   - **Always passes** skill-declared env vars via `env_passthrough` registry
   - Sets `HERMES_RPC_SOCKET`, `PYTHONDONTWRITEBYTECODE=1`, `PYTHONPATH` (hermes-agent root)
   - Per-profile HOME isolation via `get_subprocess_home()` when configured
4. Spawns child process with `stdin=DEVNULL`, `setsid` (process group isolation)
5. Poll loop monitors for exit, timeout, and interrupt:
   - Uses cooperative interrupt check via `tools.interrupt.is_interrupted()`
   - Escalating kill on timeout: `_kill_process_group(proc, escalate=True)`
6. Background thread readers prevent pipe buffer deadlocks:
   - **stdout**: head+tail strategy (40% head, 60% tail rolling window)
   - **stderr**: head-only (errors appear early)
7. Post-processing: ANSI strip, secret redaction, status encoding

### 1.10 Output Truncation Strategy

Two-phase truncation:

**During streaming (live pipe)**:
- stdout: 40% head buffer + 60% rolling tail buffer
- stderr: head-only up to MAX_STDERR_BYTES

**Post-execution (remote path)**:
- If total stdout exceeds MAX_STDOUT_BYTES: keep first 40% and last 60% with omission notice

### 1.11 Response Format

```json
{
  "status": "success" | "error" | "timeout" | "interrupted",
  "output": "...stdout with ANSI stripped and secrets redacted...",
  "tool_calls_made": 12,
  "duration_seconds": 45.3,
  "error": "optional error message"
}
```

### 1.12 Platform Gate

`SANDBOX_AVAILABLE = sys.platform != "win32"` — disabled on Windows. Local path requires Unix domain sockets (POSIX only). Remote path requires Python 3 in the terminal backend.

---

## 2. Checkpoint Manager

### Location

`tools/checkpoint_manager.py` (~23K lines)

### Purpose

Transparent filesystem snapshots via shadow git repos. Creates automatic checkpoints before file-mutating operations (write_file, patch), triggered once per conversation turn. The LLM never sees this — it's infrastructure controlled by the `checkpoints` config flag or `--checkpoints` CLI flag.

### 2.1 Shadow Repo Architecture

```
~/.hermes/checkpoints/{sha256(abs_dir)[:16]}/
├── HEAD, refs/, objects/      # Standard git internals
├── HERMES_WORKDIR              # Original directory path
└── info/exclude                # Default exclude patterns
```

Isolation via `GIT_DIR` + `GIT_WORK_TREE` environment variables — no git state leaks into the user's project directory. The shadow repo is completely independent of any `.git/` in the working directory.

### 2.2 Shadow Repo Path

Deterministic: `sha256(absolute_path)[:16]` under `~/.hermes/checkpoints/`.

```python
def _shadow_repo_path(working_dir: str) -> Path:
    abs_path = str(_normalize_path(working_dir))
    dir_hash = hashlib.sha256(abs_path.encode()).hexdigest()[:16]
    return CHECKPOINT_BASE / dir_hash
```

### 2.3 Git Environment

```python
env["GIT_DIR"] = str(shadow_repo)
env["GIT_WORK_TREE"] = str(normalized_working_dir)
# Remove interfering git env vars
env.pop("GIT_INDEX_FILE", None)
env.pop("GIT_NAMESPACE", None)
env.pop("GIT_ALTERNATE_OBJECT_DIRECTORIES", None)
```

### 2.4 Default Excludes

20 patterns written to `info/exclude` in each shadow repo:

| Category | Patterns |
|----------|----------|
| Dependencies | `node_modules/`, `.venv/`, `venv/` |
| Build output | `dist/`, `build/`, `.next/`, `.nuxt/` |
| Python cache | `__pycache__/`, `*.pyc`, `*.pyo` |
| Environment | `.env`, `.env.*`, `.env.local`, `.env.*.local` |
| System | `.DS_Store`, `.cache/` |
| Logs/coverage | `*.log`, `coverage/`, `.pytest_cache/` |
| Git | `.git/` |

### 2.5 Git Subprocess Handling

All git commands go through `_run_git()`:

- **Timeout**: configurable via `HERMES_CHECKPOINT_TIMEOUT` (default 30s, clamped 10-60s)
- **Error handling**: distinguishes expected non-zero exits (via `allowed_returncodes`) from failures
- **Safety checks**: validates working directory exists and is a directory before execution
- **Return**: `(ok: bool, stdout: str, stderr: str)` tuple

### 2.6 Input Validation

**Commit hash validation** (`_validate_commit_hash`):
- Must be 4-64 hex characters (short or full SHA-1/SHA-256)
- Must not start with `-` (prevents git flag injection, e.g., `--patch`)
- Regex: `^[0-9a-fA-F]{4,64}$`

**File path validation** (`_validate_file_path`):
- Must be relative (no absolute paths — restore targets must be relative to workdir)
- Must not escape working directory via path traversal (verified with `relative_to()`)

### 2.7 Checkpoint Lifecycle

**Per-turn deduplication**:
```python
def new_turn(self):
    self._checkpointed_dirs.clear()

def ensure_checkpoint(self, working_dir, reason="auto"):
    if abs_dir in self._checkpointed_dirs:
        return False  # Already checkpointed this turn
    self._checkpointed_dirs.add(abs_dir)
    return self._take(abs_dir, reason)
```

**Take a snapshot** (`_take`):
1. Initialize shadow repo if needed
2. File count guard: skip if >50,000 files
3. `git add -A` — stage everything
4. `git diff --cached --quiet` — skip if no changes
5. `git commit -m {reason}` — create checkpoint
6. `_prune()` — enforce max snapshot limit

### 2.8 Safety Guards

| Guard | Behavior |
|-------|----------|
| Root/home skip | Refuses to checkpoint `/`, `$HOME`, or other overly broad directories |
| File count limit | Skips directories with >50,000 files |
| Git availability | Lazy probe via `shutil.which("git")` — silently disabled if git not found |
| Max snapshots | Configurable limit (default 50) — pruning limits log view only |
| Pre-rollback snapshot | Before any restore, takes a checkpoint of current state so you can undo the undo |
| Never raises | All errors logged silently; `ensure_checkpoint` returns `False` on failure |

### 2.9 Rollback Flow

**Full directory restore**:
```
git checkout {commit_hash} -- .
```

**Single file restore**:
```
git checkout {commit_hash} -- {relative_file_path}
```

Key properties:
- Uses `git checkout` (not `git reset`) — restores tracked files without moving HEAD
- Safe and reversible
- Pre-rollback snapshot taken automatically before restore

### 2.10 Public API

| Method | Purpose |
|--------|---------|
| `new_turn()` | Reset per-turn deduplication set |
| `ensure_checkpoint(dir, reason)` | Take snapshot if not already done this turn |
| `list_checkpoints(dir)` | List checkpoints with hash, timestamp, reason, shortstat |
| `diff(dir, commit_hash)` | Show diff between checkpoint and current working tree |
| `restore(dir, commit_hash, file_path?)` | Restore to checkpoint state |
| `get_working_dir_for_path(file_path)` | Walk up from file to find project root (`.git`, `pyproject.toml`, etc.) |

### 2.11 Shadow Repo Initialization

```python
def _init_shadow_repo(shadow_repo, working_dir):
    if (shadow_repo / "HEAD").exists():
        return None  # Already initialized

    shadow_repo.mkdir(parents=True, exist_ok=True)
    git init
    git config user.email "hermes@local"
    git config user.name "Hermes Checkpoint"
    # Write DEFAULT_EXCLUDES to info/exclude
    # Write HERMES_WORKDIR with normalized path
```

Git user identity: `Hermes Checkpoint <hermes@local>` — local-only, never pushes.

### 2.12 Checkpoint Listing

```
git log --format=%H|%h|%aI|%s -n {max_snapshots}
```

Each entry includes:
- `hash`: full SHA
- `short_hash`: abbreviated SHA
- `timestamp`: ISO 8601 date
- `reason`: commit message
- `files_changed`, `insertions`, `deletions`: from `git diff --shortstat`

First commit has no parent — `allowed_returncodes={128, 129}` suppresses the expected error.

---

## 3. Clarify Tool

### Location

`tools/clarify_tool.py` (142 lines)

### Purpose

Interactive multi-choice question tool. Allows the agent to present structured clarifying questions to the user, with platform-specific UI rendering (arrow-key navigation in CLI, numbered lists on messaging platforms).

### 3.1 Core Constraint

`MAX_CHOICES = 4` — the agent can offer at most 4 predefined choices. The UI always appends a 5th "Other (type your answer)" option.

### 3.2 Two Modes

**Multiple choice** (with `choices` parameter):
```
Question text?
1. Choice A
2. Choice B
3. Choice C
4. Choice D
5. Other (type your answer)
```

**Open-ended** (no `choices` parameter):
```
Question text?
(free-form text input)
```

### 3.3 Platform Callback Mechanism

The actual UI interaction is delegated to a platform-provided callback:

```python
def clarify_tool(question, choices=None, callback=None):
    # ... validation ...
    user_response = callback(question, choices)
    return json.dumps({
        "question": question,
        "choices_offered": choices,
        "user_response": str(user_response).strip(),
    })
```

- **CLI mode** (`cli.py`): Uses `prompt_toolkit` with arrow-key navigation
- **Messaging platforms** (`gateway/run.py`): Renders as numbered list with inline reply
- **Callback signature**: `callback(question: str, choices: Optional[List[str]]) -> str`
- **Injection**: The callback is injected by the agent runner via `kw.get("callback")`

### 3.4 OpenAI Function-Calling Schema

```json
{
  "name": "clarify",
  "parameters": {
    "question": { "type": "string" },
    "choices": {
      "type": "array",
      "items": { "type": "string" },
      "maxItems": 4
    },
    "required": ["question"]
  }
}
```

### 3.5 LLM Usage Guidelines

From the schema description:

**Use when**:
- Task is ambiguous and user needs to choose an approach
- Want post-task feedback ("How did that work out?")
- Want to offer to save a skill or update memory
- A decision has meaningful trade-offs the user should weigh

**Do NOT use for**:
- Simple yes/no confirmation of dangerous commands (terminal tool handles that)
- Low-stakes decisions — prefer making a reasonable default choice

### 3.6 Validation Rules

| Rule | Behavior |
|------|----------|
| Empty question | Returns error: "Question text is required." |
| Non-list choices | Returns error: "choices must be a list of strings." |
| >4 choices | Truncated to first 4 |
| Empty choices list | Converted to `None` (open-ended mode) |
| No callback available | Returns error: "Clarify tool is not available in this execution context." |

### 3.7 Response Format

```json
{
  "question": "How should we proceed?",
  "choices_offered": ["Option A", "Option B"],
  "user_response": "Option A"
}
```

For open-ended questions, `choices_offered` is `null`.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Sandbox allowed tools | 7 |
| Default script timeout | 300s |
| Default max tool calls in sandbox | 50 |
| Max stdout | 50 KB |
| Max stderr | 10 KB |
| Checkpoint max files | 50,000 |
| Checkpoint git timeout | 30s (configurable) |
| Max checkpoints per directory | 50 |
| Default exclude patterns | 20 |
| Max clarify choices | 4 (+ 1 "Other") |
| Clarify tool lines | 142 |

---

*Generated from source analysis of the Hermes Agent codebase.*
