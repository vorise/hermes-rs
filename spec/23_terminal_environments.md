# Hermes Agent — Terminal Environments & File Sync

This document covers the execution sandbox backends (Local, Docker, SSH, Modal, Daytona, Singularity, Managed Modal), the unified session snapshot model, and the file sync manager for remote environments.

---

## Table of Contents

1. [Unified Execution Model](#1-unified-execution-model)
2. [LocalEnvironment](#2-localenvironment)
3. [DockerEnvironment](#3-dockerenvironment)
4. [SSHEnvironment](#4-sshenvironment)
5. [ModalEnvironment](#5-modalenvironment)
6. [DaytonaEnvironment](#6-daytonaenvironment)
7. [SingularityEnvironment](#7-singularityenvironment)
8. [ManagedModalEnvironment](#8-managedmodalenvironment)
9. [FileSyncManager](#9-filesyncmanager)
10. [Credential File Mounts](#10-credential-file-mounts)

---

## 1. Unified Execution Model

### Location

`tools/environments/base.py` (~580 lines)

### Purpose

Abstract base class and shared infrastructure for all execution backends. Provides a unified `execute()` interface with session snapshot sourcing, CWD tracking, interrupt handling, and timeout enforcement.

### 1.1 ProcessHandle Protocol

```python
class ProcessHandle(Protocol):
    def poll(self) -> int | None: ...
    def kill(self) -> None: ...
    def wait(self, timeout: float | None = None) -> int: ...
    @property
    def stdout(self) -> IO[str] | None: ...
    @property
    def returncode(self) -> int | None: ...
```

`subprocess.Popen` satisfies this natively. SDK backends (Modal, Daytona) return `_ThreadedProcessHandle` which adapts blocking SDK calls to the protocol.

### 1.2 _ThreadedProcessHandle

Adapter for SDK backends without real subprocess:

```python
def __init__(self, exec_fn: Callable[[], tuple[str, int]],
             cancel_fn: Callable[[], None] | None = None):
    # Creates os.pipe() for stdout drainage
    # Spawns background thread to run exec_fn
    # kill() calls cancel_fn (sandbox.terminate, sandbox.stop)
```

Key design: writes output to pipe so the shared `_wait_for_process` drain thread reads it uniformly across all backends.

### 1.3 Session Snapshot Architecture

Spawn-per-call model: every `execute()` spawns a fresh `bash -c` process. A session snapshot (env vars, functions, aliases) is captured once at init and re-sourced before each command.

**Snapshot creation** (`init_session()`):
```bash
export -p > {snapshot_path}           # env vars
declare -f | grep -vE '^_[^_]' >> ... # functions (filtered)
alias -p >> {snapshot_path}           # aliases
echo 'shopt -s expand_aliases' >> ... # enable alias expansion
echo 'set +e' >> ...                  # don't exit on error
echo 'set +u' >> ...                  # don't exit on undefined vars
pwd -P > {cwd_file}                   # initial working directory
```

Snapshot timeout: 30s (overridable via `_snapshot_timeout`).

**Command wrapping** (`_wrap_command()`):
```bash
source {snapshot_path} 2>/dev/null || true
cd {cwd} || exit 126
eval '{escaped_command}'
__hermes_ec=$?
export -p > {snapshot_path} 2>/dev/null || true   # re-dump env
pwd -P > {cwd_file} 2>/dev/null || true
printf '\n{cwd_marker}%s{cwd_marker}\n' "$(pwd -P)"
exit $__hermes_ec
```

The wrapper:
1. Sources previous command's env state
2. Changes to working directory
3. Runs the command
4. Re-dumps env vars (last-writer-wins for concurrent calls)
5. Writes CWD to file (local reads this) and stdout marker (remote parses this)

### 1.4 CWD Persistence

Two mechanisms:
- **Local**: reads `{cwd_file}` temp file after each command
- **Remote**: parses `__HERMES_CWD_{session_id}__` markers from stdout

Marker format: `\n__HERMES_CWD_abc123__/actual/path__HERMES_CWD_abc123__\n`

Extraction searches for last pair of markers within 4096 chars, strips the marker line from output.

### 1.5 Stdin Handling

Two modes controlled by `_stdin_mode`:

| Mode | How stdin passed | Backends |
|------|-----------------|----------|
| `"pipe"` | Written to proc.stdin on daemon thread | Local, Docker, SSH, Singularity |
| `"heredoc"` | Embedded as shell heredoc in command string | Modal, Daytona |

Heredoc delimiter: `HERMES_STDIN_{uuid}`, unique per command.

### 1.6 Process Lifecycle

`_wait_for_process()` — shared across all backends:
- Polls at 0.2s intervals
- Checks `is_interrupted()` — kills process with returncode 130
- Enforces timeout — kills process with returncode 124
- Drains stdout on background thread (UnicodeDecodeError → binary hint)
- Fires `activity_callback` every 10s while running (gateway inactivity guard)

### 1.7 Activity Callback

```python
_activity_callback_local = threading.local()

def set_activity_callback(cb: Callable[[str], None] | None) -> None:
    _activity_callback_local.callback = cb
```

Thread-local so concurrent executions don't cross-talk. Callback fires: `"terminal command running ({elapsed}s elapsed)"`.

### 1.8 Sudo Transformation

`_prepare_command()` delegates to `terminal_tool._transform_sudo_command()` — when `SUDO_PASSWORD` env var is available, transforms `sudo ...` into `echo PASSWORD | sudo -S ...`.

### 1.9 Sandbox Directory

```python
def get_sandbox_dir() -> Path:
    # Configurable via TERMINAL_SANDBOX_DIR
    # Defaults to {HERMES_HOME}/sandboxes/
```

Used by Docker (persistent workspaces), Singularity (overlays/SIF cache).

---

## 2. LocalEnvironment

### Location

`tools/environments/local.py` (~315 lines)

### Purpose

Execute commands directly on the host machine. Spawn-per-call with session snapshot.

### 2.1 Bash Discovery

Resolution order:
1. `HERMES_GIT_BASH_PATH` (Windows custom path)
2. `shutil.which("bash")`
3. `/usr/bin/bash` → `/bin/bash`
4. `$SHELL`
5. `/bin/sh`

Windows: falls back to Git Bash install paths (`Program Files/Git/bin/bash.exe`, etc.). Raises RuntimeError if no bash found on Windows.

### 2.2 Environment Sanitization

`_sanitize_subprocess_env()` and `_make_run_env()` filter Hermes-managed secrets from subprocess environment:

**Blocklist** (~100+ env vars):
- Provider keys: `OPENAI_API_KEY`, `ANTHROPIC_BASE_URL`, `OPENROUTER_API_KEY`, etc.
- Platform config: `TELEGRAM_HOME_CHANNEL`, `DISCORD_HOME_CHANNEL`, `SIGNAL_HTTP_URL`, etc.
- Infrastructure: `MODAL_TOKEN_ID`, `DAYTONA_API_KEY`, `GH_TOKEN`

**Passthrough override**: keys declared via `tools.env_passthrough` bypass the blocklist.

**Force prefix**: `_HERMES_FORCE_` prefix stripped — allows forcing a blocked key.

**HOME isolation**: `get_subprocess_home()` redirects HOME to `{HERMES_HOME}/home/` for per-profile isolation when that directory exists.

### 2.3 Process Killing

Kills entire process group (not just parent):
```python
pgid = os.getpgid(proc.pid)
os.killpg(pgid, signal.SIGTERM)
# 1s grace → SIGKILL
```

### 2.4 Temp Directory

`get_temp_dir()` — handles Termux and other platforms where `/tmp` may not exist. Checks `TMPDIR`, `TMP`, `TEMP` env vars first.

---

## 3. DockerEnvironment

### Location

`tools/environments/docker.py` (~580 lines)

### Purpose

Hardened Docker container execution with resource limits and optional filesystem persistence.

### 3.1 Docker Executable Discovery

```
1. HERMES_DOCKER_BINARY env var (explicit override, e.g. /usr/bin/podman)
2. docker on PATH
3. podman on PATH (drop-in compatible)
4. macOS Docker Desktop paths: /usr/local/bin, /opt/homebrew/bin, /Applications
```

### 3.2 Security Arguments

Applied to every container:
```bash
--cap-drop ALL              # Drop all capabilities
--cap-add DAC_OVERRIDE      # Allow writing to bind-mounted dirs
--cap-add CHOWN             # Package managers need ownership control
--cap-add FOWNER
--security-opt no-new-privileges
--pids-limit 256
--tmpfs /tmp:rw,nosuid,size=512m
--tmpfs /var/tmp:rw,noexec,nosuid,size=256m
--tmpfs /run:rw,noexec,nosuid,size=64m
```

### 3.3 Resource Limits

| Resource | Flag | Notes |
|----------|------|-------|
| CPU | `--cpus N` | Fractional values supported |
| Memory | `--memory Nm` | Megabytes |
| Disk | `--storage-opt size=Nm` | Only overlay2 on XFS with pquota |

Disk limit probe: attempts `docker create --storage-opt size=1m hello-world` to verify support.

### 3.4 Writable Filesystems

**Persistent mode** (bind mounts):
```bash
-v {sandbox_dir}/home:/root
-v {sandbox_dir}/workspace:/workspace
```

**Non-persistent mode** (tmpfs):
```bash
--tmpfs /workspace:rw,exec,size=10g
--tmpfs /home:rw,exec,size=1g
--tmpfs /root:rw,exec,size=1g
```

### 3.5 Host CWD Auto-Mount

When `auto_mount_cwd=True` and `host_cwd` is a valid directory and no explicit `/workspace` mount:
```bash
-v {host_cwd_abs}:/workspace
```

### 3.6 Credential & Skill Mounts

Read-only mounts injected at container creation:
- Credential files (OAuth tokens, API keys)
- Skill directories (local + external skills)
- Cache directories (documents, images, audio, screenshots)

### 3.7 Container Lifecycle

```bash
docker run -d --init --name hermes-{uuid8} -w {cwd} ... image sleep infinity
```

`--init` uses tini/catatonit as PID 1 for zombie reaping.

Cleanup: `docker stop` (60s timeout) → `docker rm` (non-persistent only). Async to avoid blocking.

### 3.8 Environment Forwarding

Two sources merged during `init_session()`:
1. `docker_forward_env` — explicit config list
2. `env_passthrough` — skill-declared variables

`_HERMES_PROVIDER_ENV_BLOCKLIST` filters implicit passthrough keys but not explicit `docker_forward_env` entries.

---

## 4. SSHEnvironment

### Location

`tools/environments/ssh.py` (~260 lines)

### Purpose

Run commands on a remote machine over SSH with ConnectionMaster persistence.

### 4.1 SSH Connection

ControlMaster for connection reuse:
```bash
ssh -o ControlPath={socket} -o ControlMaster=auto -o ControlPersist=300
    -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10
```

Control socket: `{tempdir}/hermes-ssh/{user}@{host}:{port}.sock`

### 4.2 Remote Home Detection

```bash
ssh ... echo $HOME
# Falls back to /root or /home/{user}
```

### 4.3 File Sync

Uses `FileSyncManager` with transport callbacks:
- **Single file**: `scp` over ControlMaster (mkdir parent first)
- **Bulk upload**: `tar -chf - | ssh tar xf -` streaming pipeline (~580 files in one transfer)
- **Delete**: `ssh rm -f` batched

Bulk upload staging: symlinks local files into temp directory, `tar -chf - -C staging .` preserves paths.

### 4.4 Execution

Spawn-per-call: `ssh ... bash -c '{wrapped_command}'`. Each `execute()` creates a fresh SSH process.

### 4.5 Cleanup

```bash
ssh -O exit {user}@{host}   # Close ControlMaster
unlink {socket}             # Remove socket file
```

---

## 5. ModalEnvironment

### Location

`tools/environments/modal.py` (~435 lines)

### Purpose

Modal cloud execution via native Modal sandboxes with filesystem snapshotting.

### 5.1 AsyncWorker

Background thread with its own event loop for async Modal SDK calls:
```python
class _AsyncWorker:
    def start(self):           # Spawns thread, creates asyncio loop
    def run_coroutine(coro, timeout=600):  # asyncio.run_coroutine_threadsafe
    def stop(self):            # Stops loop, joins thread
```

Required because Modal SDK uses async methods (`sandbox.exec.aio()`, etc.).

### 5.2 Image Resolution

```python
if image_spec.startswith("im-"):
    return modal.Image.from_id(image_spec)  # Snapshot ID

if "ubuntu" or "debian" in image:
    # Auto-install python3 + ensurepip
```

### 5.3 Credential Mounts

Modal SDK Mount objects:
```python
modal.Mount.from_local_file(host_path, remote_path=container_path)
```

Includes credentials, skills files, and cache files.

### 5.4 File Sync

`FileSyncManager` with Modal transport:
- **Single file**: base64-encoded via stdin → `base64 -d > {path}` (1 MB chunks with drain)
- **Bulk upload**: gzipped tar via stdin → `base64 -d | tar xzf - -C /`
- **Delete**: `sandbox.exec("rm -f ...")`

Stdin chunk size: 1 MB — safe for both legacy server path (2 MB cap) and command-router path (16 MB cap).

### 5.5 Snapshot Persistence

On cleanup:
```python
snapshot_id = await sandbox.snapshot_filesystem.aio()
_store_direct_snapshot(task_id, snapshot_id)
```

Restored on next creation:
```python
restored_snapshot_id = snapshots.get(f"direct:{task_id}")
# Falls back to legacy key: snapshots.get(task_id)
```

Storage: `~/.hermes/modal_snapshots.json`

If snapshot restore fails, retries with base image and deletes stale snapshot.

### 5.6 Execution

```python
process = await sandbox.exec.aio("bash", "-c", cmd_string, timeout=timeout)
stdout = await process.stdout.read.aio()
exit_code = await process.wait.aio()
```

Cancel: `sandbox.terminate.aio()`.

---

## 6. DaytonaEnvironment

### Location

`tools/environments/daytona.py` (~230 lines)

### Purpose

Daytona cloud sandbox execution via Python SDK.

### 6.1 Sandbox Lifecycle

```python
self._daytona = Daytona()

# Persistent: try to resume by name
self._sandbox = self._daytona.get(f"hermes-{task_id}")
self._sandbox.start()

# Fallback: search by labels
page = self._daytona.list(labels={"hermes_task_id": task_id})

# Create new
self._daytona.create(CreateSandboxFromImageParams(
    image=image, name=f"hermes-{task_id}",
    resources=Resources(cpu=1, memory_gib, disk_gib),
    auto_stop_interval=0,
))
```

### 6.2 Resource Limits

| Resource | Config | Platform Cap |
|----------|--------|-------------|
| CPU | `cpu` (integer) | — |
| Memory | `memory` (MB) → GiB | — |
| Disk | `disk` (MB) → GiB | 10 GB max |

### 6.3 File Sync

`FileSyncManager` with Daytona SDK transport:
- **Single file**: `sandbox.fs.upload_file(host_path, remote_path)`
- **Bulk upload**: `sandbox.fs.upload_files([FileUpload(...)])` — batches all files into one multipart POST (~580 files: ~5 min → <2 s)
- **Delete**: `sandbox.process.exec("rm -f ...")`

### 6.4 Interrupt Recovery

Sandbox may be stopped by a previous interrupt:
```python
def _ensure_sandbox_ready(self):
    self._sandbox.refresh_data()
    if state in (STOPPED, ARCHIVED):
        self._sandbox.start()
```

### 6.5 Cleanup

- **Persistent**: `sandbox.stop()` — filesystem preserved
- **Non-persistent**: `daytona.delete(sandbox)` — sandbox destroyed

---

## 7. SingularityEnvironment

### Location

`tools/environments/singularity.py` (~260 lines)

### Purpose

Hardened Singularity/Apptainer container execution with writable overlay persistence.

### 7.1 Executable Discovery

```
1. apptainer (preferred)
2. singularity (legacy name)
```

### 7.2 SIF Image Building

One-time conversion from Docker images:
```bash
apptainer build {cache_dir}/{image_name}.sif docker://{image}
```

- Thread-safe with `_sif_build_lock`
- 600s timeout
- Falls back to `docker://` URL on failure
- Cache: `APPTAINER_CACHEDIR` or `{scratch_dir}/.apptainer/`

### 7.3 Scratch Directory

Resolution:
1. `TERMINAL_SCRATCH_DIR` env var
2. `/scratch/{USER}/hermes-agent` (if `/scratch` exists and writable)
3. `{HERMES_HOME}/sandboxes/singularity/`

### 7.4 Instance Start

```bash
apptainer instance start --containall --no-home
    [--overlay {overlay_dir} | --writable-tmpfs]
    [--bind credential:ro, skills:ro]
    [--memory NM] [--cpus N]
    {image} hermes_{uuid12}
```

**Persistent mode**: `--overlay {scratch}/hermes-overlays/{task_id}/` — directory survives across sessions.

**Non-persistent mode**: `--writable-tmpfs` — ephemeral writable layer.

### 7.5 Execution

Spawn-per-call via instance:
```bash
apptainer exec instance://{instance_id} bash -c '{command}'
```

### 7.6 Cleanup

```bash
apptainer instance stop {instance_id}
# Persistent: overlay dir saved to ~/.hermes/singularity_snapshots.json
```

---

## 8. ManagedModalEnvironment

### Location

`tools/environments/managed_modal.py` (~283 lines)

### Purpose

Gateway-owned Modal sandbox accessed via tool-gateway REST API. The gateway (not the agent) manages the Modal SDK.

### 8.1 Gateway Communication

```python
gateway = resolve_managed_tool_gateway("modal")
self._gateway_origin = gateway.gateway_origin.rstrip("/")
self._nous_user_token = gateway.nous_user_token
```

All requests authenticated with `Authorization: Bearer {nous_user_token}`.

### 8.2 Sandbox Creation

```
POST /v1/sandboxes
{
    "image": "...",
    "cwd": "/root",
    "cpu": 1,
    "memoryMiB": 5120,
    "timeoutMs": 3600000,
    "idleTimeoutMs": max(300000, timeout * 1000),
    "persistentFilesystem": true,
    "logicalKey": task_id
}
```

Idempotency: `x-idempotency-key: {uuid}` prevents duplicate sandboxes.

### 8.3 Execution Flow

Uses `BaseModalExecutionEnvironment` abstract class:

```
execute() → _prepare_modal_exec() → _start_modal_exec() → poll loop → result
```

Poll interval: 0.25s. Timeout: `prepared.timeout + _client_timeout_grace_seconds` (10s).

**Start exec**:
```
POST /v1/sandboxes/{sandbox_id}/execs
{
    "execId": "{uuid}",
    "command": "...",
    "cwd": "...",
    "timeoutMs": N,
    "stdinData": "..."
}
```

**Poll exec**:
```
GET /v1/sandboxes/{sandbox_id}/execs/{exec_id}
```

**Cancel exec**:
```
POST /v1/sandboxes/{sandbox_id}/execs/{exec_id}/cancel
```

**Terminate sandbox**:
```
POST /v1/sandboxes/{sandbox_id}/terminate
{"snapshotBeforeTerminate": true/false}
```

### 8.4 Stdin Modes

| Mode | Handling |
|------|----------|
| `"payload"` | Sent as `stdinData` in exec start request |
| `"heredoc"` | Embedded via `wrap_modal_stdin_heredoc()` |

Sudo pipe: `wrap_modal_sudo_pipe()` → `printf '%s\n' PASSWORD | sudo -S command`.

### 8.5 Credential Passthrough Guard

Managed Modal does NOT support host credential file mounting. Raises `ValueError` if skills declare credential mounts.

---

## 9. FileSyncManager

### Location

`tools/environments/file_sync.py` (~170 lines)

### Purpose

Tracks local file changes via mtime+size, detects deletions, and syncs to remote environments transactionally. Used by SSH, Modal, and Daytona. Not used by bind-mount backends (Docker, Singularity).

### 9.1 Sync Cycle

```python
def sync(self, *, force: bool = False) -> None:
    # 1. Rate limit: once per 5s unless force or HERMES_FORCE_FILE_SYNC=1
    # 2. Enumerate all files from get_files_fn()
    # 3. Detect uploads: new or changed (mtime+size differ)
    # 4. Detect deletes: synced paths no longer in current set
    # 5. Upload (bulk if available, else individual)
    # 6. Delete removed files
    # 7. Commit state or rollback on failure
```

### 9.2 Transactional State

```python
prev_files = dict(self._synced_files)  # Snapshot before work
try:
    # upload + delete
    self._synced_files = new_files   # Commit
except Exception:
    self._synced_files = prev_files  # Rollback
```

Failed syncs don't lose track of files — next cycle retries everything.

### 9.3 File Enumeration

`iter_sync_files(container_base="/root/.hermes")` combines:
- Credential file mounts (remapped from `/root/.hermes` to `container_base`)
- Skills files
- Cache files

### 9.4 Bulk Upload Optimization

When `bulk_upload_fn` is available:
- SSH: `tar -chf - | ssh tar xf -` (single TCP stream)
- Modal: gzipped tar via stdin
- Daytona: `sandbox.fs.upload_files()` (single multipart POST)

Without bulk: O(N) individual round-trips.

---

## 10. Credential File Mounts

### Location

`tools/credential_files.py`

### Purpose

Centralized credential file enumeration for mounting into sandboxes (Docker volumes, Modal mounts, SSH sync targets).

### 10.1 Mount Types

| Type | Source | Mount Mode |
|------|--------|-----------|
| Credential files | `get_credential_file_mounts()` | Read-only |
| Skill directories | `get_skills_directory_mount()` | Read-only |
| Cache directories | `get_cache_directory_mounts()` | Read-only |

### 10.2 Container Path Remapping

Remote backends remap `/root/.hermes` prefix to the actual container home directory:
- Modal/Daytona: `/root/.hermes` → `{remote_home}/.hermes`
- Docker: bind mounts at exact paths
- SSH: `scp` to `{remote_home}/.hermes/...`

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Execution backends | 7 (Local, Docker, SSH, Modal, Daytona, Singularity, Managed Modal) |
| Snapshot timeout | 30s |
| Activity callback interval | 10s |
| File sync interval | 5s |
| Docker security caps added | 3 (DAC_OVERRIDE, CHOWN, FOWNER) |
| Docker pids-limit | 256 |
| Modal stdin chunk size | 1 MB |
| Daytona disk cap | 10 GB |
| Singularity SIF build timeout | 600s |
| Managed Modal poll interval | 0.25s |
| Managed Modal client grace | 10s |
| Stdin modes | 2 (pipe, heredoc) |
| Process return codes | 130 (interrupt), 124 (timeout) |
| Blocked env vars | ~100+ |
| SIF build lock | Thread-safe singleton |

---

*Generated from source analysis of the Hermes Agent codebase.*
