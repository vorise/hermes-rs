# Hermes Agent — Tools: Terminal, Skills, Code Execution, RL Training

This document covers the `tools/` subsystem: `terminal_tool.py` (~1,749 lines), `skills_tool.py` (~1,419 lines), `code_execution_tool.py` (~1,377 lines), and `rl_training_tool.py` (~1,396 lines).

---

## Table of Contents

1. [Terminal Tool](#1-terminal-tool)
2. [Skills Tool](#2-skills-tool)
3. [Code Execution Tool](#3-code-execution-tool)
4. [RL Training Tool](#4-rl-training-tool)

---

## 1. Terminal Tool

### Location

`tools/terminal_tool.py` (~1,749 lines)

### Purpose

Executes shell commands across 6 backend environments: local, Docker, SSH, Modal (cloud), Singularity, and Daytona. Supports foreground and background execution with persistent shells.

### 1.1 Environment Selection

Via `TERMINAL_ENV` environment variable:
| Value | Backend | Notes |
|-------|---------|-------|
| `local` | Direct host execution | Default, fastest |
| `docker` | Docker container | Isolated, requires Docker |
| `ssh` | Remote SSH host | Requires TERMINAL_SSH_HOST/USER |
| `modal` | Modal cloud sandbox | Direct or managed gateway |
| `singularity` | Singularity container | HPC/sandbox environments |
| `daytona` | Daytona sandbox | Isolated cloud environment |

### 1.2 Environment Classes

All environment implementations live in `tools/environments/`:
- `LocalEnvironment` — direct `subprocess.Popen`
- `DockerEnvironment` — container lifecycle management
- `SSHEnvironment` — paramiko SSH connection with persistent shell
- `ModalEnvironment` — Modal cloud sandbox
- `ManagedModalEnvironment` — Nous-managed Modal gateway
- `SingularityEnvironment` — Singularity container with scratch dir

### 1.3 Configuration from Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `TERMINAL_ENV` | `local` | Backend type |
| `TERMINAL_TIMEOUT` | `180` | Command timeout (seconds) |
| `TERMINAL_LIFETIME_SECONDS` | `300` | Sandbox lifetime |
| `TERMINAL_DOCKER_IMAGE` | `nikolaik/python-nodejs:python3.11-nodejs20` | Default container image |
| `TERMINAL_MODAL_IMAGE` | (same) | Modal sandbox image |
| `TERMINAL_SSH_HOST` | `""` | SSH target |
| `TERMINAL_SSH_USER` | `""` | SSH username |
| `TERMINAL_SSH_PORT` | `22` | SSH port |
| `TERMINAL_SSH_KEY` | `""` | SSH private key |
| `TERMINAL_SSH_PERSISTENT` | `true` | Persistent shell for SSH |
| `TERMINAL_LOCAL_PERSISTENT` | `false` | Persistent shell for local |
| `TERMINAL_CONTAINER_CPU` | `1` | CPU allocation for containers |
| `TERMINAL_CONTAINER_MEMORY` | `5120` | MB (5 GB default) |
| `TERMINAL_CONTAINER_DISK` | `50200` | MB (50 GB default) |
| `TERMINAL_CONTAINER_PERSISTENT` | `true` | Persist container filesystem |
| `TERMINAL_MAX_FOREGROUND_TIMEOUT` | `600` | Hard cap (10 minutes) |
| `TERMINAL_DISK_WARNING_GB` | `500` | Disk usage warning threshold |
| `TERMINAL_DOCKER_MOUNT_CWD_TO_WORKSPACE` | `false` | Remap host cwd to `/workspace` |
| `TERMINAL_DOCKER_FORWARD_ENV` | `[]` | JSON list of env vars to forward |
| `TERMINAL_DOCKER_VOLUMES` | `[]` | JSON list of volume mounts |

### 1.4 CWD Handling

- **Local**: Uses host's current directory
- **SSH**: Starts in `~`
- **Containers/Modal**: Starts in `/root`
- Docker with `TERMINAL_DOCKER_MOUNT_CWD_TO_WORKSPACE=true`: Remaps host path → `/workspace`
- Host/relative paths are rejected for container backends (they won't exist inside the sandbox)

### 1.5 Sudo Password Handling

**Interactive prompt**: `_prompt_for_sudo_password()` reads from `/dev/tty` with echo disabled (Unix) or `msvcrt.getwch()` (Windows). 45-second timeout.

**Session caching**: `_cached_sudo_password` persists until CLI exits.

**Callback integration**: `_sudo_password_callback` registered by CLI to route through prompt_toolkit.

**Gateway hint**: On sudo failure in gateway sessions, adds tip about adding `SUDO_PASSWORD` to `~/.hermes/.env`.

### 1.6 Command Guard System

Delegates to `tools/approval.check_all_command_guards()`:
- Tirith security scanner
- Dangerous command pattern matching
- CLI approval callback for dangerous commands

### 1.7 Workdir Validation

`_validate_workdir()` uses allowlist regex `^[A-Za-z0-9/_\-.~ +@=,]+$` — rejects shell metacharacters.

### 1.8 Sudo Rewriting

`_rewrite_real_sudo_invocations()` transforms unquoted `sudo` command words into `sudo -S` with password piped, while preserving:
- Quoted mentions of "sudo" in strings
- Environment variable assignments before sudo
- sudo mentions in comments/text

### 1.9 Dangerous Command Approval

Uses `_approval_callback` (registered by CLI) to prompt user for:
- `once` — allow this execution
- `session` — allow for this session
- `always` — permanently allow this pattern
- `deny` — block execution

### 1.10 Background Execution

- Returns `session_id` for tracking
- Supports `notify_on_complete` for auto-notification
- `process(action="poll")` for progress checks
- `process(action="wait")` to block until done

### 1.11 PTY Mode

`pty=true` for interactive CLI tools (Codex, Claude Code, Python REPL). Required for tools that need a pseudo-terminal.

### 1.12 Task Environment Overrides

`register_task_env_overrides(task_id, overrides)` allows Atropos environments to configure per-task sandbox settings (custom Dockerfile, image, cwd) before the agent loop starts.

### 1.13 Cleanup Thread

Background thread periodically checks `_last_activity` dict, reaps sandboxes past `TERMINAL_LIFETIME_SECONDS`.

### 1.14 Disk Usage Warning

`_check_disk_usage_warning()` scans `hermes-*` directories in scratch dir, warns when total exceeds `TERMINAL_DISK_WARNING_GB` threshold.

### 1.15 Modal Backend Resolution

Uses `resolve_modal_backend_state()` from `tools.tool_backend_helpers`:
- Checks `TERMINAL_MODAL_MODE` (auto, direct, managed)
- Detects direct Modal credentials
- Checks managed gateway readiness

---

## 2. Skills Tool

### Location

`tools/skills_tool.py` (~1,419 lines)

### Purpose

Two agent-facing tools for listing and viewing skills: `skills_list` (tier 1 metadata) and `skill_view` (tier 2-3 full content). Implements progressive disclosure architecture.

### 2.1 Progressive Disclosure

| Tier | Tool | Content | Token Cost |
|------|------|---------|------------|
| 1 | `skills_list` | name + description only | Minimal |
| 2 | `skill_view(skill)` | Full SKILL.md instructions | Moderate |
| 3 | `skill_view(skill, "references/file.md")` | Linked reference files | On demand |

### 2.2 SKILL.md Format

```yaml
---
name: skill-name                    # Required, max 64 chars
description: Brief description       # Required, max 1024 chars
version: 1.0.0                       # Optional
license: MIT                         # Optional
platforms: [macos]                   # Optional — restrict to OS
prerequisites:                       # Optional — legacy runtime requirements
  env_vars: [API_KEY]
  commands: [curl, jq]
compatibility: Requires X            # Optional
metadata:                            # Optional, arbitrary key-value
  hermes:
    tags: [fine-tuning, llm]
    related_skills: [peft, lora]
setup:
  help: "Instructions for setup..."
  collect_secrets:
    - env_var: API_KEY
      prompt: "Enter your API key"
      provider_url: "https://..."
      secret: true
required_environment_variables:      # Modern format (supersedes prerequisites.env_vars)
  - name: API_KEY
    prompt: "Enter your API key"
    help: "https://..."
    optional: false
    required_for: "web search"
---

# Skill Title
Full instructions...
```

### 2.3 Platform Filtering

```python
_PLATFORM_MAP = {
    "macos": "darwin",
    "linux": "linux",
    "windows": "win32",
}
```

Skills with `platforms` frontmatter that doesn't match `sys.platform` are excluded from `skills_list`.

### 2.4 Skills Directory

Single source of truth: `~/.hermes/skills/`. Bypasses `.git`, `.github`, `.hub` directories.

### 2.5 External Skills Dirs

`get_external_skills_dirs()` from `agent.skill_utils` returns additional scan locations beyond the default.

### 2.6 Prompt Injection Detection

```python
_INJECTION_PATTERNS = [
    "ignore previous instructions",
    "ignore all previous",
    "you are now",
    "disregard your",
    "forget your instructions",
    "new instructions:",
    "system prompt:",
    "<system>",
    "]]>",
]
```

Shared between local-skill and plugin-skill paths.

### 2.7 Secret Capture

`_secret_capture_callback` (registered by CLI) prompts for missing environment variables required by skills. Flow:
1. `_find_all_skills()` identifies skills with `required_environment_variables`
2. `skills_list()` checks which requirements are unsatisfied
3. If gateway surface: returns `gateway_setup_hint` for user to configure externally
4. Otherwise: invokes callback for each missing secret

### 2.8 Setup Metadata

`_normalize_setup_metadata()` extracts from frontmatter:
- `setup.help` — human-readable setup instructions
- `setup.collect_secrets` — list of `{env_var, prompt, provider_url, secret}`

### 2.9 Skills List Response

Returns JSON with:
```json
{
  "success": true,
  "skills": [{"name": "...", "description": "...", "category": "..."}],
  "categories": ["category1", "category2"],
  "categories_with_descriptions": [{"name": "...", "description": "..."}]
}
```

Categories can have `DESCRIPTION.md` files for extended descriptions.

### 2.10 Skill View Response

`skill_view(skill_name, file_path)`:
- Loads SKILL.md + optional linked files from `references/`, `templates/`, `assets/`
- Returns content with path validation (no directory traversal)
- Detects and flags prompt injection patterns in loaded content
- Checks skill readiness status (available, setup_needed, unsupported)

### 2.11 Disabled Skills

Skills can be disabled via:
- `config.yaml` → `skills.disabled: ["skill-name"]`
- `config.yaml` → `skills.platform_disabled.linux: ["skill-name"]`

Disabled skills are excluded from `skills_list` but still visible in `hermes skills` config UI.

---

## 3. Code Execution Tool

### Location

`tools/code_execution_tool.py` (~1,377 lines)

### Purpose

Programmatic Tool Calling (PTC) — lets the LLM write a Python script that calls Hermes tools via RPC, collapsing multi-step tool chains into a single inference turn.

### 3.1 Architecture: Two Transports

**Local backend (UDS)**:
1. Parent generates `hermes_tools.py` stub module with UDS RPC functions
2. Parent opens Unix domain socket, starts RPC listener thread
3. Parent spawns child process running the LLM's script
4. Tool calls travel over UDS back to parent for dispatch

**Remote backends (file-based RPC)**:
1. Parent generates `hermes_tools.py` with file-based RPC stubs
2. Parent ships both files to remote environment via `env.execute()`
3. Script runs inside terminal backend (Docker/SSH/Modal/Daytona)
4. Tool calls written as request files; polling thread reads via `env.execute()`
5. Parent dispatches, writes response files; script polls for responses

### 3.2 Allowed Tools (7)

```python
SANDBOX_ALLOWED_TOOLS = frozenset([
    "web_search", "web_extract",
    "read_file", "write_file", "search_files",
    "patch", "terminal",
])
```

Only tools in both `SANDBOX_ALLOWED_TOOLS` and the session's enabled tools get stubs.

### 3.3 Resource Limits

| Limit | Default | Purpose |
|-------|---------|---------|
| `DEFAULT_TIMEOUT` | 300s (5 min) | Script execution timeout |
| `DEFAULT_MAX_TOOL_CALLS` | 50 | Max RPC calls per script |
| `MAX_STDOUT_BYTES` | 50,000 | Stdout cap (50 KB) |
| `MAX_STDERR_BYTES` | 10,000 | Stderr cap (10 KB) |

### 3.4 hermes_tools.py Module Generator

`generate_hermes_tools_module(enabled_tools, transport)`:
- Generates per-tool stub functions matching the tool's signature
- Includes transport header (UDS or file-based)
- Embeds common helpers: `json_parse()`, `shell_quote()`, `retry()`

### 3.5 UDS Transport

```python
def _call(tool_name, args):
    conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    conn.connect(os.environ["HERMES_RPC_SOCKET"])
    conn.sendall(json.dumps({"tool": tool_name, "args": args}) + "\n")
    # Read newline-delimited response
```

### 3.6 File-based RPC Transport

```python
def _call(tool_name, args):
    # Write request atomically: req_NNNNNN.tmp → req_NNNNNN
    # Poll for response: res_NNNNNN
    # 5-minute timeout per tool call, adaptive polling (50ms → 250ms)
```

### 3.7 RPC Server (UDS)

`_rpc_server_loop()`:
- Accepts one client connection on background thread
- Dispatches tool calls via `model_tools.handle_function_call()`
- Enforces allow-list and max call count
- Strips blocked terminal params: `background`, `pty`, `notify_on_complete`, `watch_patterns`
- Suppresses stdout/stderr from internal tool handlers

### 3.8 RPC Poll Loop (Remote)

`_rpc_poll_loop()`:
- Runs in background thread
- Polls remote filesystem via `env.execute("ls -1 req_*")`
- Reads request files, dispatches via `handle_function_call()`
- Writes responses atomically (base64 encoding for shell safety)
- Uses base64 encoding because Modal doesn't reliably deliver stdin to chained commands

### 3.9 Script Execution

**Local**: `subprocess.Popen([sys.executable, script_path], ...)` with resource limits.

**Remote**: `_ship_file_to_remote()` writes script and `hermes_tools.py` to remote environment using `echo | base64 -d` (reliable across all backends including Modal).

### 3.10 Output Handling

- Only script's stdout returned to LLM; intermediate tool results never enter context window
- stdout capped at 50 KB, stderr at 10 KB
- Tool call log recorded for diagnostics (tool name, args preview, duration)

### 3.11 Platform Support

Linux/macOS only (Unix domain sockets). Disabled on Windows.

---

## 4. RL Training Tool

### Location

`tools/rl_training_tool.py` (~1,396 lines)

### Purpose

Direct RL training management via Tinker-Atropos. Discovers environments, manages configurations, and orchestrates the 3-process training pipeline.

### 4.1 Directory Structure

```
tinker-atropos/                      # Git submodule
  tinker_atropos/
    environments/                    # BaseEnv subclasses
  configs/                           # Training configs
~/.hermes/logs/rl_training/          # Run logs
```

### 4.2 Environment Discovery

`_scan_environments()` — AST-based scanning:
- Parses each `.py` file in `environments/`
- Finds classes inheriting from `BaseEnv`
- Extracts `name` class attribute and docstring
- Does NOT import the module (avoids side effects)

### 4.3 Config Field Introspection

`_get_env_config_fields()`:
- Dynamically imports environment module
- Calls `env_class.config_init()` to get config class
- Falls back to `atroposlib.envs.BaseEnvConfig` if config_init fails
- Extracts all Pydantic `model_fields` with types, defaults, descriptions
- Marks locked fields as non-editable

### 4.4 Locked Configuration

Fields that cannot be changed by the model (tuned for infrastructure):

| Path | Field | Value |
|------|-------|-------|
| `env` | `tokenizer_name` | `Qwen/Qwen3-8B` |
| `env` | `rollout_server_url` | `http://localhost:8000` |
| `env` | `use_wandb` | `true` |
| `env` | `max_token_length` | 8192 |
| `env` | `max_num_workers` | 2048 |
| `env` | `worker_timeout` | 3600 |
| `env` | `total_steps` | 2500 |
| `env` | `steps_per_eval` | 25 |
| `env` | `max_batches_offpolicy` | 3 |
| `env` | `inference_weight` | 1.0 |
| `env` | `eval_limit_ratio` | 0.1 |
| `tinker` | `lora_rank` | 32 |
| `tinker` | `learning_rate` | 0.00004 |
| `tinker` | `max_token_trainer_length` | 9000 |
| `tinker` | `checkpoint_dir` | `./temp/` |
| `tinker` | `save_checkpoint_interval` | 25 |
| `openai[0]` | `model_name` | `Qwen/Qwen3-8B` |
| `openai[0]` | `base_url` | `http://localhost:8001/v1` |
| `openai[0]` | `server_type` | `sglang` |

### 4.5 Training Run State

```python
@dataclass
class RunState:
    run_id: str
    environment: str
    config: Dict[str, Any]
    status: str  # pending | starting | running | stopping | stopped | completed | failed
    api_process: Optional[Popen]
    trainer_process: Optional[Popen]
    env_process: Optional[Popen]
    wandb_project: str
    wandb_run_name: str
```

### 4.6 3-Process Training Pipeline

`_spawn_training_run()`:

1. **Atropos API server** (`run-api`): Starts first, waits 5 seconds
2. **Tinker trainer** (`launch_training.py --config`): Starts second, waits 30 seconds for inference server on port 8001
3. **Environment** (`environment.py serve`): Starts third, waits 90 more seconds

**Startup delays**: 5s + 30s + 90s = 125s total before all processes running.

### 4.7 Available Tools

| Tool | Purpose |
|------|---------|
| `rl_list_environments` | Discover BaseEnv subclasses via AST scanning |
| `rl_select_environment` | Select env, load config fields into memory |
| `rl_get_current_config` | Show configurable + locked fields |
| `rl_edit_config(field, value)` | Update one configurable field |
| `rl_start_training` | Spawn 3-process training pipeline |
| `rl_check_status` | Check training status (rate-limited to every 30 min) |
| `rl_stop_training` | Gracefully stop all 3 processes |
| `rl_get_results` | Fetch training results and metrics |

### 4.8 Status Check Rate Limiting

`MIN_STATUS_CHECK_INTERVAL = 30 * 60` (30 minutes). Prevents excessive WandB API calls.

### 4.9 WandB Integration

- Auto-generates `wandb_name` as `{env_name}-{DATETIME}` to avoid overlaps
- Requires `WANDB_API_KEY` environment variable
- Monitors training metrics via WandB API

### 4.10 Required Environment Variables

| Variable | Purpose |
|----------|---------|
| `TINKER_API_KEY` | API key for Tinker service |
| `WANDB_API_KEY` | API key for Weights & Biases |

### 4.11 Process Cleanup

`_stop_training_run()` closes log file handles and terminates all 3 subprocesses (API, trainer, environment) in sequence.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| terminal_tool.py lines | ~1,749 |
| skills_tool.py lines | ~1,419 |
| code_execution_tool.py lines | ~1,377 |
| rl_training_tool.py lines | ~1,396 |
| Terminal backends | 6 (local, docker, ssh, modal, singularity, daytona) |
| Terminal env vars | 20+ |
| Sudo prompt timeout | 45 seconds |
| Foreground max timeout | 600s (10 min) |
| Container CPU default | 1 |
| Container memory default | 5,120 MB (5 GB) |
| Container disk default | 50,200 MB (50 GB) |
| Sandbox lifetime default | 300s (5 min) |
| Skill name max length | 64 chars |
| Skill description max length | 1,024 chars |
| Prompt injection patterns | 9 |
| Platform identifiers | 3 (macos, linux, windows) |
| Excluded skill dirs | 3 (.git, .github, .hub) |
| Sandbox allowed tools | 7 |
| Max tool calls per script | 50 |
| Script timeout | 300s (5 min) |
| Max stdout | 50,000 bytes |
| Max stderr | 10,000 bytes |
| UDS socket timeout | 300s |
| File RPC timeout | 300s per call |
| File RPC poll interval | 50ms → 250ms (adaptive) |
| RL locked config fields | 18 |
| RL training processes | 3 (API, trainer, env) |
| RL startup sequence delay | 125s total (5 + 30 + 90) |
| RL status check interval | 30 minutes |
| RL default total steps | 2,500 |
| RL default max workers | 2,048 |
| RL tokenizer | Qwen/Qwen3-8B |
| RL trainer | sglang |
| RL LoRA rank | 32 |
| RL learning rate | 0.00004 |
| Terminal blocked sandbox params | 4 (background, pty, notify_on_complete, watch_patterns) |
| Config validation patterns | Env var name regex: ^[A-Za-z_][A-Za-z0-9_]*$ |

---

*Generated from source analysis of the Hermes Agent codebase.*
