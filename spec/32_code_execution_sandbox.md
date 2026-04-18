# Hermes Agent — Code Execution Sandbox

This document covers the `execute_code` tool: a Python sandbox with RPC access to a subset of Hermes tools, supporting both local (UDS) and remote (file-based RPC) execution.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Tool Allow-List and Schema](#2-tool-allow-list-and-schema)
3. [Local Backend (UDS Transport)](#3-local-backend-uds-transport)
4. [Remote Backend (File-Based RPC)](#4-remote-backend-file-based-rpc)
5. [Child Process Security](#5-child-process-security)
6. [Output Handling](#6-output-handling)
7. [hermes_tools.py Module Generator](#7-hermes_toolspy-module-generator)

---

## 1. Architecture Overview

### Location

`tools/code_execution_tool.py` (~1,378 lines)

### Purpose

Lets the LLM write a Python script that calls Hermes tools via RPC, collapsing multi-step tool chains into a single inference turn. Only the script's stdout is returned to the LLM; intermediate tool results never enter the context window.

### 1.1 Two Transport Modes

**Local backend (UDS)**:
```
1. Parent generates hermes_tools.py with UDS RPC stubs
2. Parent opens Unix domain socket, starts RPC listener thread
3. Parent spawns child process running the LLM's script
4. Tool calls travel over UDS back to parent for dispatch
5. Parent collects stdout/stderr, returns to LLM
```

**Remote backend (file-based RPC)**:
```
1. Parent generates hermes_tools.py with file-based RPC stubs
2. Parent ships both hermes_tools.py and script.py to remote environment
3. Script runs inside terminal backend (Docker/SSH/Modal/Daytona/etc.)
4. Tool calls written as request files in shared RPC dir
5. Parent's polling thread reads requests via env.execute(), dispatches, writes responses
6. Script polls for response files and continues
```

### 1.2 Platform Availability

```python
SANDBOX_AVAILABLE = sys.platform != "win32"
```

Disabled on Windows (UDS requires POSIX). Remote backends additionally require Python 3 in the terminal environment.

### 1.3 Resource Limits

| Limit | Default | Config Key |
|-------|---------|------------|
| Script timeout | 300s (5 min) | `code_execution.timeout` |
| Max tool calls | 50 | `code_execution.max_tool_calls` |
| Stdout cap | 50 KB | hardcoded `MAX_STDOUT_BYTES` |
| Stderr cap | 10 KB | hardcoded `MAX_STDERR_BYTES` |

---

## 2. Tool Allow-List and Schema

### 2.1 Allowed Tools (7)

```python
SANDBOX_ALLOWED_TOOLS = frozenset([
    "web_search",
    "web_extract",
    "read_file",
    "write_file",
    "search_files",
    "patch",
    "terminal",
])
```

The sandbox gets the intersection of `SANDBOX_ALLOWED_TOOLS` and the session's `enabled_tools`. If the intersection is empty, all 7 tools are provided as fallback.

### 2.2 Blocked Terminal Parameters

Sandbox scripts cannot use these terminal parameters:

```python
_TERMINAL_BLOCKED_PARAMS = {"background", "pty", "notify_on_complete", "watch_patterns"}
```

Stripped from `terminal()` calls in both local and remote paths.

### 2.3 Function Schema

```python
EXECUTE_CODE_SCHEMA = {
    "name": "execute_code",
    "description": "Run a Python script that can call Hermes tools programmatically...",
    "parameters": {
        "type": "object",
        "properties": {"code": {"type": "string", "description": "Python code to execute..."}},
        "required": ["code"],
    },
}
```

The schema description dynamically lists only enabled tools. When web tools are disabled via `hermes tools`, the description omits `web_search`/`web_extract` to prevent the model from trying to use them.

### 2.4 Built-in Helpers (no import needed)

| Helper | Purpose |
|--------|---------|
| `json_parse(text)` | `json.loads` with `strict=False` — tolerant of control chars in terminal output |
| `shell_quote(s)` | `shlex.quote()` — safe shell escaping for dynamic command interpolation |
| `retry(fn, max_attempts=3, delay=2)` | Exponential backoff retry for transient failures |

---

## 3. Local Backend (UDS Transport)

### 3.1 Socket Setup

```python
_sock_tmpdir = "/tmp" if sys.platform == "darwin" else tempfile.gettempdir()
sock_path = os.path.join(_sock_tmpdir, f"hermes_rpc_{uuid.uuid4().hex}.sock")
```

**macOS workaround**: Uses `/tmp` instead of `tempfile.gettempdir()` (which returns `/var/folders/...` on macOS) to avoid exceeding the 104-byte `AF_UNIX` socket path limit.

### 3.2 UDS RPC Protocol

**Client** (inside sandbox script):
```python
def _call(tool_name, args):
    conn = _connect()  # socket to HERMES_RPC_SOCKET
    request = json.dumps({"tool": tool_name, "args": args}) + "\n"
    conn.sendall(request.encode())
    # Read newline-delimited response
    buf = b""
    while True:
        chunk = conn.recv(65536)
        if not chunk: raise RuntimeError("Agent process disconnected")
        buf += chunk
        if buf.endswith(b"\n"): break
    return json.loads(buf.decode().strip())
```

**Server** (parent thread):
```python
def _rpc_server_loop(server_sock, task_id, tool_call_log, tool_call_counter, ...):
    conn, _ = server_sock.accept()
    conn.settimeout(300)
    while True:
        # Read newline-delimited requests
        # Enforce allow-list
        # Enforce tool call limit
        # Strip forbidden terminal params
        # Dispatch via handle_function_call()
        # Send newline-delimited response
```

### 3.3 Child Process Spawn

```python
proc = subprocess.Popen(
    [sys.executable, "script.py"],
    cwd=tmpdir,
    env=child_env,          # filtered environment
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    stdin=subprocess.DEVNULL,
    preexec_fn=os.setsid,   # process group (Unix)
)
```

### 3.4 Poll Loop

```python
while proc.poll() is None:
    if _is_interrupted():
        _kill_process_group(proc)
        status = "interrupted"
        break
    if time.monotonic() > deadline:
        _kill_process_group(proc, escalate=True)
        status = "timeout"
        break
    time.sleep(0.2)
```

Uses per-thread `_is_interrupted()` for cooperative cancellation.

### 3.5 Process Group Killing

```python
def _kill_process_group(proc, escalate=False):
    os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
    if escalate:
        proc.wait(timeout=5)  # wait 5s
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)  # then SIGKILL
```

Escalation path: SIGTERM → 5s grace → SIGKILL.

### 3.6 stdout/stderr Drain

**Two background reader threads** prevent pipe buffer deadlocks:

- **stdout**: Head+tail strategy — keeps first 40% and last 60% of output (configurable via `MAX_STDOUT_BYTES`). Ensures both initial setup output and final `print()` results are preserved.
- **stderr**: Head-only — keeps first `MAX_STDERR_BYTES` (errors appear early).

```python
def _drain_head_tail(pipe, head_chunks, tail_chunks, head_bytes, tail_bytes, total_ref):
    # Fill head buffer first (40%)
    # Everything past head goes into rolling deque tail (60%)
    # Evict oldest tail data to stay within budget
```

---

## 4. Remote Backend (File-Based RPC)

### 4.1 Environment Creation

Reuses the same terminal environment (container/sandbox/SSH session) as the terminal and file tools:

```python
env, env_type = _get_or_create_env(effective_task_id)
```

Supports: Docker, SSH, Modal, Daytona, Singularity. Creates environment if not already active.

### 4.2 File Shipping

```python
def _ship_file_to_remote(env, remote_path, content):
    encoded = base64.b64encode(content.encode()).decode()
    env.execute(f"echo '{encoded}' | base64 -d > {quoted_remote_path}", cwd="/", timeout=30)
```

Uses base64 encoding rather than stdin piping because some backends (Modal) don't reliably deliver `stdin_data` to chained commands.

### 4.3 Sandbox Directory Structure

```
/tmp/hermes_exec_<uuid>/
├── hermes_tools.py    # file-based RPC stubs
├── script.py          # LLM's code
└── rpc/               # request/response files
    ├── req_000001
    ├── res_000001
    ├── req_000002
    └── res_000002
```

### 4.4 File-Based RPC Protocol

**Client** (inside sandbox script):
```python
def _call(tool_name, args):
    _seq += 1
    req_file = f"{_RPC_DIR}/req_{_seq:06d}"
    res_file = f"{_RPC_DIR}/res_{_seq:06d}"

    # Write request atomically
    tmp = req_file + ".tmp"
    json.dump({"tool": tool_name, "args": args, "seq": _seq}, open(tmp, "w"))
    os.rename(tmp, req_file)

    # Poll for response
    deadline = time.monotonic() + 300
    poll_interval = 0.05
    while not os.path.exists(res_file):
        if time.monotonic() > deadline: raise RuntimeError("RPC timeout")
        time.sleep(poll_interval)
        poll_interval = min(poll_interval * 1.2, 0.25)  # back off to 250ms

    result = json.loads(open(res_file).read())
    os.unlink(res_file)  # cleanup
    return result
```

**Server** (parent polling thread):
```python
def _rpc_poll_loop(env, rpc_dir, task_id, ...):
    while not stop_event.is_set():
        # ls -1 rpc_dir/req_* 2>/dev/null
        for req_file in pending_requests:
            # cat req_file → parse JSON
            # handle_function_call(tool_name, tool_args)
            # Write response atomically: base64 encode → echo | base64 -d > res.tmp → mv
            # rm req_file
        sleep(0.1)
```

Each `env.execute()` spawns an independent process, so polling and script execution run concurrently.

### 4.5 Python Availability Check

```python
py_check = env.execute("command -v python3 >/dev/null 2>&1 && echo OK", cwd="/", timeout=15)
if "OK" not in py_check.get("output", ""):
    return error("Python 3 is not available in the {env_type} terminal environment")
```

### 4.6 Cleanup

After script execution:
1. Stop RPC polling thread (`stop_event.set()`, join with 5s timeout)
2. Delete remote sandbox directory: `rm -rf /tmp/hermes_exec_<uuid>`
3. Socket/temp cleanup on local path

---

## 5. Child Process Security

### 5.1 Environment Variable Filtering

The child process receives a **filtered** environment to prevent credential exfiltration from LLM-generated scripts:

```python
_SECRET_SUBSTRINGS = ("KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "PASSWD", "AUTH")
_SAFE_ENV_PREFIXES = ("PATH", "HOME", "USER", "LANG", "LC_", "TERM", "TMPDIR", "TMP",
                       "TEMP", "SHELL", "LOGNAME", "XDG_", "PYTHONPATH", "VIRTUAL_ENV", "CONDA")
```

**Filtering logic** (evaluated in order):
1. Passthrough vars (skill-declared via `env_passthrough` or user-configured in `config.yaml → terminal.env_passthrough`) → always passed
2. Secret-like names (containing KEY, TOKEN, SECRET, etc.) → **blocked**
3. Safe prefixes (PATH, HOME, LANG, etc.) → passed
4. Everything else → **blocked**

**Exception**: `HERMES_RPC_SOCKET` and `PYTHONDONTWRITEBYTECODE` are always injected.

### 5.2 PYTHONPATH Injection

```python
_hermes_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
child_env["PYTHONPATH"] = _hermes_root + (os.pathsep + _existing_pp if _existing_pp else "")
```

Ensures the hermes-agent root is importable so `hermes_tools.py` can import from the repo.

### 5.3 HOME Isolation

```python
_profile_home = get_subprocess_home()
if _profile_home:
    child_env["HOME"] = _profile_home
```

Per-profile HOME isolation redirects system tool configs into `{HERMES_HOME}/home/` when configured.

### 5.4 TZ Injection

```python
_tz_name = os.getenv("HERMES_TIMEZONE", "").strip()
if _tz_name:
    child_env["TZ"] = _tz_name
```

Ensures `datetime.now()` in sandboxed code reflects the user's configured timezone.

### 5.5 Stdin Blocked

```python
stdin=subprocess.DEVNULL
```

Child process cannot read from stdin — prevents interactive prompts or credential reading.

### 5.6 Output Redaction

Both stdout and stderr pass through `redact_sensitive_text()` before being returned to the LLM, catching any secrets the script might have read from disk (e.g., `open('~/.hermes/.env')`).

---

## 6. Output Handling

### 6.1 stdout Head+Tail Truncation

```python
if total_stdout > MAX_STDOUT_BYTES and stdout_tail:
    omitted = total_stdout - len(head) - len(tail)
    stdout_text = head + f"\n\n... [OUTPUT TRUNCATED - {omitted:,} chars omitted] ...\n\n" + tail
```

Preserves both the beginning (setup, imports) and the end (final results) of output.

### 6.2 ANSI Escape Stripping

```python
from tools.ansi_strip import strip_ansi
stdout_text = strip_ansi(stdout_text)
```

Prevents terminal formatting codes from leaking into the LLM context (which would cause the model to copy escapes into file writes).

### 6.3 Status Codes

| Status | Trigger |
|--------|---------|
| `success` | Exit code 0 |
| `timeout` | Exit code 124 or deadline exceeded |
| `interrupted` | Exit code 130 or `_is_interrupted()` fired |
| `error` | Non-zero exit code or exception |

### 6.4 Error Response

```json
{
  "status": "error",
  "output": "stdout content...\n--- stderr ---\ntraceback...",
  "error": "Script exited with code 1",
  "tool_calls_made": 5,
  "duration_seconds": 12.34
}
```

Stderr is included in `output` so the LLM can see the traceback for debugging.

---

## 7. hermes_tools.py Module Generator

### 7.1 Stub Templates

Each of the 7 allowed tools has a pre-defined stub:

```python
_TOOL_STUBS = {
    "web_search": ("web_search", "query: str, limit: int = 5",
                   '"""Search the web..."""', '{"query": query, "limit": limit}'),
    "web_extract": ("web_extract", "urls: list",
                    '"""Extract content from URLs..."""', '{"urls": urls}'),
    "read_file": ("read_file", "path: str, offset: int = 1, limit: int = 500",
                  '"""Read a file..."""', '{"path": path, "offset": offset, "limit": limit}'),
    "write_file": ("write_file", "path: str, content: str",
                   '"""Write content to a file..."""', '{"path": path, "content": content}'),
    "search_files": ("search_files", "pattern: str, target: str = \"content\", ...",
                     '"""Search file contents..."""', '{...}'),
    "patch": ("patch", "path: str = None, old_string: str = None, ...",
              '"""Targeted find-and-replace..."""', '{...}'),
    "terminal": ("terminal", "command: str, timeout: int = None, workdir: str = None",
                 '"""Run a shell command..."""', '{"command": command, "timeout": timeout, "workdir": workdir}'),
}
```

### 7.2 Transport Headers

**UDS header** (~30 lines): Imports `json, os, socket, shlex, time`. Creates `socket.AF_UNIX` connection to `$HERMES_RPC_SOCKET`. `_call()` sends JSON request, reads newline-delimited response.

**File header** (~35 lines): Imports `json, os, shlex, tempfile, time`. `_call()` writes request file atomically, polls for response file with adaptive backoff (50ms → 250ms), reads and cleans up.

### 7.3 Generation Logic

```python
def generate_hermes_tools_module(enabled_tools, transport="uds"):
    tools_to_generate = sorted(SANDBOX_ALLOWED_TOOLS & set(enabled_tools))
    # For each tool: generate def func_name(sig): return _call(name, args_dict)
    # Prepend transport header + common helpers
    return header + "\n".join(stub_functions)
```

Only tools present in both `SANDBOX_ALLOWED_TOOLS` and `enabled_tools` get stubs generated.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Allowed sandbox tools | 7 |
| Blocked terminal params | 4 (background, pty, notify_on_complete, watch_patterns) |
| Default timeout | 300 seconds |
| Max tool calls per script | 50 |
| Stdout cap | 50,000 bytes |
| Stderr cap | 10,000 bytes |
| stdout head ratio | 40% |
| stdout tail ratio | 60% |
| UDS socket timeout | 300 seconds |
| File RPC poll start | 50ms |
| File RPC poll max | 250ms |
| File RPC per-call timeout | 300 seconds |
| File RPC poll interval | 100ms |
| macOS socket path limit | 104 bytes |
| Secret substring patterns | 7 (KEY, TOKEN, SECRET, PASSWORD, CREDENTIAL, PASSWD, AUTH) |
| Safe env prefixes | 16 |
| SIGTERM→SIGKILL escalation | 5 seconds |
| Poll loop sleep | 0.2 seconds |
| Max result size (registry) | 100,000 chars |
| RPC poll loop stop timeout | 5 seconds |
| Process reader drain timeout | 3 seconds |
| File shipping timeout | 30 seconds |
| Sandbox cleanup timeout | 15 seconds |

---

*Generated from source analysis of the Hermes Agent codebase.*
