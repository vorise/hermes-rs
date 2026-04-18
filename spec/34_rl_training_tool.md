# Hermes Agent — RL Training Tool

This document covers the RL (Reinforcement Learning) training tool module, which provides direct subprocess management of Tinker-Atropos training runs with environment discovery, configuration management, and WandB metrics monitoring.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Environment Discovery](#2-environment-discovery)
3. [Configuration Management](#3-configuration-management)
4. [Training Run Lifecycle](#4-training-run-lifecycle)
5. [Monitoring and Results](#5-monitoring-and-results)

---

## 1. Architecture Overview

### Location

`tools/rl_training_tool.py` (~1,396 lines)

### Purpose

Direct management of RL training processes without requiring a separate API server. Integrates Tinker-Atropos training infrastructure into the Hermes agent tool system, allowing the LLM to discover, configure, launch, and monitor RL training runs.

### 1.1 Directory Layout

```
hermes-agent/
├── tinker-atropos/                    # Git submodule
│   ├── tinker_atropos/
│   │   └── environments/              # Environment Python files (*.py)
│   └── configs/                       # Run configuration YAML files
└── ~/.hermes/
    └── logs/
        └── rl_training/               # Per-run log files
            ├── api_{run_id}.log
            ├── trainer_{run_id}.log
            └── env_{run_id}.log
```

### 1.2 Required Environment Variables

| Variable | Purpose |
|----------|---------|
| `TINKER_API_KEY` | API key for Tinker service |
| `WANDB_API_KEY` | API key for Weights & Biases metrics |

### 1.3 Tool Functions (9)

| Tool | Purpose |
|------|---------|
| `rl_list_environments` | Discover available RL environments via AST scanning |
| `rl_select_environment` | Select an environment and load its config fields |
| `rl_get_current_config` | Show all configurable and locked fields |
| `rl_edit_config` | Modify a specific configurable field |
| `rl_start_training` | Launch a training run (3 subprocesses) |
| `rl_check_status` | Get run status and WandB metrics (30-min rate limit) |
| `rl_stop_training` | Terminate a running training job |
| `rl_get_results` | Get final results and WandB history |
| `rl_list_runs` | List all training runs and their status |

---

## 2. Environment Discovery

### 2.1 AST-Based Scanning

```python
def _scan_environments() -> List[EnvironmentInfo]:
    for py_file in ENVIRONMENTS_DIR.glob("*.py"):
        if py_file.name.startswith("_"): continue
        tree = ast.parse(f.read())
        for node in ast.walk(tree):
            if isinstance(node, ast.ClassDef):
                for base in node.bases:
                    if base_name == "BaseEnv":
                        # Extract: name, description, config_class
```

**Why AST** (not import): Scans without executing environment code, avoiding side effects and dependency issues.

### 2.2 Discovery Details

- **Name**: From class attribute `name = "env-name"` or falls back to filename stem
- **Description**: First line of class docstring
- **Config class**: From `env_config_cls` attribute, defaults to `BaseEnvConfig`

### 2.3 Config Field Introspection

```python
def _get_env_config_fields(env_file_path) -> Dict[str, Dict[str, Any]]:
    # Dynamic import of environment module
    spec = importlib.util.spec_from_file_location(...)
    # Find BaseEnv subclass with config_init()
    env_config, server_configs = env_class.config_init()
    # Extract Pydantic model fields
    for field_name, field_info in config_class.model_fields.items():
        fields[field_name] = {
            "type": ..., "default": ..., "description": ...,
            "locked": field_name in LOCKED_FIELD_NAMES,
            "current_value": ...,
        }
```

**Fallback**: If `config_init()` fails, tries importing `BaseEnvConfig` from `atroposlib.envs.base` directly.

**Serialization**: Handles enum values (`.value` or `.name`), None, and primitives.

---

## 3. Configuration Management

### 3.1 Locked Fields

Infrastructure settings that cannot be changed by the model:

```python
LOCKED_FIELDS = {
    "env": {
        "tokenizer_name": "Qwen/Qwen3-8B",
        "rollout_server_url": "http://localhost:8000",
        "use_wandb": True,
        "max_token_length": 8192,
        "max_num_workers": 2048,
        "worker_timeout": 3600,
        "total_steps": 2500,
        "steps_per_eval": 25,
        "max_batches_offpolicy": 3,
        "inference_weight": 1.0,
        "eval_limit_ratio": 0.1,
    },
    "openai": [{
        "model_name": "Qwen/Qwen3-8B",
        "base_url": "http://localhost:8001/v1",
        "api_key": "x",
        "weight": 1.0,
        "num_requests_for_eval": 256,
        "timeout": 3600,
        "server_type": "sglang",
    }],
    "tinker": {
        "lora_rank": 32,
        "learning_rate": 0.00004,
        "max_token_trainer_length": 9000,
        "checkpoint_dir": "./temp/",
        "save_checkpoint_interval": 25,
    },
    "slurm": False,
    "testing": False,
}
```

### 3.2 Config Merging

When creating a run config:
1. Start with deep copy of `LOCKED_FIELDS` as base
2. Overlay configurable fields from `_current_config`
3. Auto-set `wandb_project` (default: `"atropos-tinker"`) and `wandb_run_name`
4. Auto-set `env.wandb_name` to `{env_name}-{timestamp}` to avoid overlaps
5. Write as YAML to `tinker-atropos/configs/run_{run_id}.yaml`

### 3.3 Config Tools Flow

```
rl_select_environment(name)
  → Scan environments (AST)
  → Find matching env
  → Introspect config fields (dynamic import)
  → Initialize _current_config with non-locked defaults
  → Auto-set wandb_name = "{name}-{timestamp}"

rl_get_current_config()
  → Return configurable_fields + locked_fields

rl_edit_config(field, value)
  → Check field exists
  → Check not locked
  → Update _current_config[field] = value
```

---

## 4. Training Run Lifecycle

### 4.1 Three-Process Architecture

A training run requires three cooperating subprocesses:

```
1. run-api          → Atropos API server (trajectory management)
2. launch_training.py → Tinker trainer + inference server (port 8001)
3. environment.py serve → RL environment (worker processes)
```

### 4.2 Spawn Sequence

```python
async def _spawn_training_run(run_state, config_path):
    # Step 1: Start API server
    api_process = Popen(["run-api"], stdout=api_log, stderr=STDOUT,
                        cwd=TINKER_ATROPOS_ROOT)
    await asyncio.sleep(5)  # Wait for API to start
    if api_process.poll() is not None:
        fail("API server exited immediately")

    # Step 2: Start trainer (FastAPI inference server on port 8001)
    trainer_process = Popen([python, "launch_training.py", "--config", config_path],
                            env={**os.environ, "TINKER_API_KEY": ...})
    await asyncio.sleep(30)  # Wait for trainer to initialize
    if trainer_process.poll() is not None:
        fail("Trainer exited immediately")

    # Step 3: Start environment (after trainer is ready)
    await asyncio.sleep(90)  # Additional 90s for full trainer setup
    env_process = Popen([python, env_file, "serve", "--config", config_path])
    await asyncio.sleep(10)  # Wait for environment to connect
    if env_process.poll() is not None:
        fail("Environment exited immediately")

    run_state.status = "running"
    run_state.start_time = time.time()
    asyncio.create_task(_monitor_training_run(run_state))
```

**Total startup delay**: ~135 seconds (5s API + 30s trainer + 90s trainer setup + 10s env connect).

### 4.3 Process Startup Delays

| Phase | Duration | Purpose |
|-------|----------|---------|
| API server start | 5s | Atropos API server initialization |
| Trainer start | 30s | FastAPI inference server on port 8001 |
| Trainer setup | 90s | Model loading, weight initialization |
| Environment connect | 10s | Worker processes connecting to API |

### 4.4 Background Monitor

```python
async def _monitor_training_run(run_state):
    while run_state.status == "running":
        await asyncio.sleep(30)  # Check every 30 seconds

        # Check if any process has died
        if env_process.poll() is not None:
            if exit_code == 0: status = "completed"
            else: status = "failed"
            _stop_training_run(run_state); break

        # Same for trainer and API processes
```

Monitors all three processes. If any dies, stops the entire run.

### 4.5 Shutdown Sequence (reverse order)

```python
def _stop_training_run(run_state):
    # 1. Stop environment
    env_process.terminate()
    env_process.wait(timeout=10) → kill() if still alive

    # 2. Stop trainer
    trainer_process.terminate()
    trainer_process.wait(timeout=10) → kill() if still alive

    # 3. Stop API server
    api_process.terminate()
    api_process.wait(timeout=10) → kill() if still alive

    # 4. Close log file handles
    # 5. Update status to "stopped"
```

**Graceful shutdown**: SIGTERM → 10s grace → SIGKILL.

---

## 5. Monitoring and Results

### 5.1 Status Check (Rate Limited)

```python
MIN_STATUS_CHECK_INTERVAL = 30 * 60  # 30 minutes

async def rl_check_status(run_id):
    now = time.time()
    if run_id in _last_status_check:
        elapsed = now - _last_status_check[run_id]
        if elapsed < MIN_STATUS_CHECK_INTERVAL:
            remaining = MIN_STATUS_CHECK_INTERVAL - elapsed
            return {"rate_limited": True,
                    "next_check_in_seconds": remaining}

    _last_status_check[run_id] = now
```

### 5.2 WandB Metrics Integration

```python
api = wandb.Api()
runs = api.runs(
    f"{os.getenv('WANDB_ENTITY', 'nousresearch')}/{wandb_project}",
    filters={"display_name": wandb_run_name}
)
if runs:
    wandb_run = runs[0]
    metrics = {
        "step": wandb_run.summary.get("_step", 0),
        "reward_mean": wandb_run.summary.get("train/reward_mean"),
        "percent_correct": wandb_run.summary.get("train/percent_correct"),
        "eval_percent_correct": wandb_run.summary.get("eval/percent_correct"),
    }
```

### 5.3 Run States

| State | Transition |
|-------|------------|
| `pending` | Initial state after creation |
| `starting` | Spawn training subprocesses |
| `running` | All three processes alive |
| `stopped` | User called rl_stop_training |
| `completed` | Environment exited with code 0 |
| `failed` | Any process exited with non-zero code |

### 5.4 Results Retrieval

```python
async def rl_get_results(run_id):
    # Basic: run_id, status, environment, wandb info
    # Extended: WandB final_metrics (full summary dict)
    # History: Last 10 rows of training history
```

Returns WandB URL, full metrics summary, and last 10 history rows.

### 5.5 Global State

```python
_environments: List[EnvironmentInfo]       # Discovered envs (lazy init)
_current_env: Optional[str]                # Selected environment name
_current_config: Dict[str, Any]            # Current configurable values
_env_config_cache: Dict[str, Dict]         # Cached config fields per env
_active_runs: Dict[str, RunState]          # All training runs
_last_status_check: Dict[str, float]       # Rate limiting timestamps
```

---

## Key Numbers

| Metric | Value |
|--------|-------|
| RL training tools | 9 |
| Training subprocesses | 3 (API, trainer, environment) |
| Startup delay (total) | ~135 seconds |
| API server wait | 5 seconds |
| Trainer initialization | 30 seconds |
| Trainer setup | 90 seconds |
| Environment connection | 10 seconds |
| Monitor interval | 30 seconds |
| Status check rate limit | 30 minutes |
| Shutdown grace period | 10 seconds per process |
| Locked env fields | 10 |
| Locked tinker fields | 5 |
| Default lora_rank | 32 |
| Default learning_rate | 0.00004 |
| Default max_token_length | 8192 |
| Default max_num_workers | 2048 |
| Default total_steps | 2500 |
| Default steps_per_eval | 25 |
| WandB entity default | nousresearch |
| Run ID format | First 8 chars of uuid4 |

---

*Generated from source analysis of the Hermes Agent codebase.*
