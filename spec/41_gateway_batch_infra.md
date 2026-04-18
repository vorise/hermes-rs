# Hermes Agent — Gateway & Batch Infrastructure

This document covers the gateway service management, batch runner, trajectory compressor, MCP serve, toolset distributions, toolsets, logging, and RL CLI.

---

## Table of Contents

1. [Gateway Service Management](#1-gateway-service-management)
2. [Batch Runner](#2-batch-runner)
3. [Trajectory Compressor](#3-trajectory-compressor)
4. [MCP Serve](#4-mcp-serve)
5. [Toolset Distributions](#5-toolset-distributions)
6. [Supporting Modules](#6-supporting-modules)

---

## 1. Gateway Service Management

### Location

`hermes_cli/gateway.py` (~3,161 lines)

### Purpose

Gateway subcommand handler. Manages the persistent gateway process that connects to messaging platforms and dispatches agent sessions.

### 1.1 Commands

| Command | Purpose |
|---------|---------|
| `hermes gateway run` | Start gateway in foreground |
| `hermes gateway start` | Start as background service |
| `hermes gateway stop` | Stop running service |
| `hermes gateway restart` | Restart service (with drain) |
| `hermes gateway status` | Show service status |
| `hermes gateway install` | Install as systemd/launchd service |
| `hermes gateway uninstall` | Remove service |
| `hermes gateway setup` | Interactive setup wizard |

### 1.2 Service Detection

```python
def _get_service_pids() -> set:
    """Return PIDs managed by systemd or launchd."""
    # systemd: systemctl list-units hermes-gateway*
    # launchd: launchctl list <label>
```

### 1.3 Platform Support

| Platform | Service Manager | Label |
|----------|----------------|-------|
| Linux (systemd) | `systemctl --user` | `hermes-gateway.service` |
| macOS (launchd) | `launchctl` | `com.nousresearch.hermes-gateway` |
| Windows | Manual PID tracking | — |

### 1.4 Restart with Drain

```python
DEFAULT_GATEWAY_RESTART_DRAIN_TIMEOUT = 30  # seconds
GATEWAY_SERVICE_RESTART_EXIT_CODE = 42
```

Graceful restart sequence:
1. Signal existing gateway to stop accepting new messages
2. Wait for in-progress sessions to complete (up to drain timeout)
3. Kill remaining processes
4. Start new gateway

### 1.5 Process Sweeping

After service restart, sweeps for stale manual gateway processes:

```python
# Kill any hermes CLI processes that aren't part of the service
# but exclude PIDs returned by _get_service_pids()
```

---

## 2. Batch Runner

### Location

`batch_runner.py` (~1,287 lines)

### Purpose

Parallel batch processing of agent across multiple prompts from a dataset. Used for data generation and model training pipeline.

### 2.1 Architecture

```
batch_runner.py → multiprocessing.Pool → AIAgent × N workers
                    ↓
            Rich progress bar
                    ↓
        Trajectory JSONL output
```

### 2.2 Key Parameters

| Parameter | Purpose |
|-----------|---------|
| `--dataset_file` | Input JSONL file with prompts |
| `--batch_size` | Number of parallel workers |
| `--run_name` | Output directory name |
| `--resume` | Resume interrupted run |
| `--distribution` | Toolset distribution name |

### 2.3 Tool Stats Normalization

```python
ALL_POSSIBLE_TOOLS = set(TOOL_TO_TOOLSET_MAP.keys())
DEFAULT_TOOL_STATS = {'count': 0, 'success': 0, 'failure': 0}

def _normalize_tool_stats(tool_stats):
    """Ensure HuggingFace datasets can load JSONL without schema mismatch."""
    normalized = {}
    for tool in ALL_POSSIBLE_TOOLS:
        normalized[tool] = tool_stats.get(tool, DEFAULT_TOOL_STATS.copy())
    return normalized
```

### 2.4 Checkpointing

```python
# Global lock for worker processes
_WORKER_CONFIG = {}

# Worker processes share:
# - Dataset file path
# - Run configuration
# - Tool definitions
# - Model parameters
```

### 2.5 Output Format

JSONL with trajectory data:
```json
{
  "prompt": "user query",
  "response": "model output",
  "messages": [...],
  "tool_stats": {"tool_name": {"count": N, "success": N, "failure": N}},
  "turns_used": N,
  "finished_naturally": true
}
```

---

## 3. Trajectory Compressor

### Location

`trajectory_compressor.py` (~1,462 lines)

### Purpose

Post-processes completed agent trajectories to compress them within a target token budget while preserving training signal quality.

### 3.1 Compression Strategy

1. **Protect first turns**: system, human, first assistant, first tool response
2. **Protect last N turns**: final actions and conclusions
3. **Compress MIDDLE turns only**: starting from 2nd tool response
4. **Compress only as needed**: to fit under target token budget
5. **Replace with summary**: Single human summary message replaces compressed region
6. **Keep tool calls intact**: model continues working after summary

### 3.2 Configuration

```python
@dataclass
class CompressionConfig:
    # Tokenizer
    tokenizer_name: str = "moonshotai/Kimi-K2-Thinking"
    trust_remote_code: bool = True

    # Token targets
    target_max_tokens: int = 15250
    summary_target_tokens: int = 750

    # Protection
    protect_first_system: bool = True
    protect_first_human: bool = True
    protect_first_gpt: bool = True
    protect_first_tool: bool = True
    protect_last_n_turns: int = 4

    # Summarization (via OpenRouter)
    summarization_model: str = "google/gemini-3-flash-preview"
    base_url: str = OPENROUTER_BASE_URL
    temperature: float = 0.3
    max_retries: int = 3
```

### 3.3 Usage

```bash
# Compress a directory of JSONL files
python trajectory_compressor.py --input=data/my_run

# Compress a single file
python trajectory_compressor.py --input=data/trajectories.jsonl

# Compress 15% sample
python trajectory_compressor.py --input=data/trajectories.jsonl --sample_percent=15

# Custom token target
python trajectory_compressor.py --input=data/trajectories.jsonl --target_max_tokens=16000
```

### 3.4 Summarization

Uses OpenRouter with `google/gemini-3-flash-preview` (cheap, fast) to generate summaries of compressed regions.

### 3.5 Progress Display

Uses Rich progress bars with spinner, bar, time remaining, and task completion indicators.

---

## 4. MCP Serve

### Location

`mcp_serve.py` (~867 lines)

### Purpose

MCP (Model Context Protocol) server that exposes Hermes agent capabilities as MCP tools. Allows external MCP clients to interact with Hermes tools.

### 4.1 Architecture

```
MCP Client → mcp_serve.py (stdio transport) → Hermes tools
```

### 4.2 Tool Exposure

Exposes Hermes tools as MCP-compatible tools via stdio transport. Supports:
- File tools (read, write, search, patch)
- Terminal commands
- Web search/extract
- Memory operations

### 4.3 Transport

Standard MCP stdio protocol:
- JSON-RPC 2.0 messages over stdin/stdout
- Tool definitions via `tools/list`
- Tool execution via `tools/call`

---

## 5. Toolset Distributions

### Location

`toolset_distributions.py` (~364 lines)

### Purpose

Probabilistic toolset sampling for RL training. Defines named distributions over toolsets, used by batch runner and RL environments.

### 5.1 Distribution Format

```python
DISTRIBUTIONS = {
    "default": {"core": 1.0, "web": 0.5, "terminal": 0.3},
    "image_gen": {"core": 1.0, "image": 1.0},
    "web_research": {"core": 1.0, "web": 1.0, "browser": 0.5},
    ...
}
```

### 5.2 Sampling

```python
def sample_toolsets_from_distribution(distribution_name: str) -> List[str]:
    """Sample toolsets based on probability weights."""

def list_distributions() -> List[str]:
    """List all available distribution names."""

def validate_distribution(name: str) -> bool:
    """Check if a distribution name is valid."""
```

### 5.3 Integration

Used by:
- `batch_runner.py`: `--distribution` flag
- RL environments: Per-group toolset resolution
- `run_agent.py`: `enabled_toolsets` parameter

---

## 6. Supporting Modules

### 6.1 Toolsets

**Location**: `toolsets.py` (~702 lines)

Master toolset definitions. Each toolset is a named collection of tools with:
- Tool list
- Dependencies (Python packages, system tools)
- Description
- Required environment variables

### 6.2 Hermes Logging

**Location**: `hermes_logging.py` (~390 lines)

Centralized logging setup:
- `agent.log` (INFO+) — full conversation trace
- `errors.log` (WARNING+) — errors only
- Session context filtering: `hermes logs --session <id>`
- Thread-safe with session ID tagging via `set_session_context()`

### 6.3 RL CLI

**Location**: `rl_cli.py` (~446 lines)

Command-line interface for RL training operations:
- Launch training runs
- Monitor training status
- Manage environments
- View training results

### 6.4 Mini SWE Runner

**Location**: `mini_swe_runner.py` (~709 lines)

Lightweight SWE-bench runner for evaluating agent coding capabilities on software engineering tasks.

### 6.5 Hermes State

**Location**: `hermes_state.py` (~1,238 lines)

SQLite-based state store. See spec 06 for full documentation.

### 6.6 Hermes Time

**Location**: `hermes_time.py` (~104 lines)

Timezone-aware time utilities. Respects `HERMES_TIMEZONE` environment variable.

### 6.7 Utils

**Location**: `utils.py` (~164 lines)

Shared utilities:
- `atomic_json_write()` — write JSON atomically (temp file + rename)
- `env_var_enabled()` — check boolean env vars

---

## Key Numbers

| Metric | Value |
|--------|-------|
| run_agent.py lines | 11,024 |
| cli.py lines | 10,024 |
| gateway.py lines | 3,161 |
| trajectory_compressor.py lines | 1,462 |
| hermes_state.py lines | 1,238 |
| batch_runner.py lines | 1,287 |
| mcp_serve.py lines | 867 |
| toolsets.py lines | 702 |
| mini_swe_runner.py lines | 709 |
| rl_cli.py lines | 446 |
| hermes_logging.py lines | 390 |
| toolset_distributions.py lines | 364 |
| Target max tokens (compression) | 15,250 |
| Summary target tokens | 750 |
| Protected last N turns | 4 |
| Summarization model | gemini-3-flash-preview |
| Gateway restart drain | 30 seconds |
| Gateway restart exit code | 42 |
| Context pressure cooldown | 300 seconds |
| Log files | 2 (agent.log, errors.log) |

---

*Generated from source analysis of the Hermes Agent codebase.*
