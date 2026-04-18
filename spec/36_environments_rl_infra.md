# Hermes Agent — Environments & RL Training Infrastructure

This document covers the RL training environment system: base environments, the agent loop engine, tool context for reward functions, the agentic OPD environment, web research environment, benchmark environments, and tool call parsers.

---

## Table of Contents

1. [Base Environment Architecture](#1-base-environment-architecture)
2. [Agent Loop Engine](#2-agent-loop-engine)
3. [ToolContext](#3-toolcontext)
4. [Agentic OPD Environment](#4-agentic-opd-environment)
5. [Web Research Environment](#5-web-research-environment)
6. [Benchmark Environments](#6-benchmark-environments)
7. [Tool Call Parsers](#7-tool-call-parsers)
8. [Runtime Patches](#8-runtime-patches)

---

## 1. Base Environment Architecture

### Location

`environments/hermes_base_env.py` (~714 lines)

### Purpose

Abstract base class (`HermesAgentBaseEnv`) for all Hermes-Agent + Atropos RL environments. Provides the integration plumbing that all environments share.

### 1.1 Two-Mode Operation

| Mode | Server Type | Purpose |
|------|-------------|---------|
| Phase 1 | OpenAI-compatible (VLLM, SGLang, OpenRouter) | Standard tool-calling with ChatCompletion objects |
| Phase 2 | ManagedServer (VLLM with client-side parsing) | Token-level tracking with prompt_logprobs |

### 1.2 Subclass Interface

Subclasses implement only:

```python
class MyEnv(HermesAgentBaseEnv):
    def setup(self): ...           # Load dataset, initialize state
    def get_next_item(self): ...   # Return next item from dataset
    def format_prompt(self, item): ...  # Convert item to user message
    def compute_reward(self, item, result, ctx): ...  # Score rollout
    def evaluate(self): ...        # Periodic evaluation
```

### 1.3 Core Responsibilities

- **Group-level toolset resolution**: Each group gets its own tool distribution sampled from `toolset_distributions`
- **Agent loop orchestration**: Delegates to `HermesAgentLoop` for multi-turn execution
- **ToolContext creation**: Creates per-rollout `ToolContext` for reward functions
- **ScoredDataGroup construction**: Builds Atropos `ScoredDataGroup` from `ManagedServer` state

### 1.4 Configuration

```python
class HermesAgentEnvConfig(BaseEnvConfig):
    max_turns: int = 20          # Max tool-calling turns per rollout
    group_size: int = 8          # Parallel rollouts per batch
    data_path: Optional[str]     # Dataset path or HF dataset name
    tool_pool_size: int = 128    # Thread pool size for async tool calls
```

### 1.5 Path Resolution

```python
_repo_root = Path(__file__).resolve().parent.parent
if str(_repo_root) not in sys.path:
    sys.path.insert(0, str(_repo_root))
```

Ensures imports work regardless of invocation directory. Loads `.env` from repo root.

### 1.6 Async Patches

```python
from environments.patches import apply_patches
apply_patches()
```

Applies monkey patches for async-safe tool operation inside Atropos's event loop. Patches `SwerexModalEnvironment` to use a background thread instead of `asyncio.run()`, preventing deadlocks.

---

## 2. Agent Loop Engine

### Location

`environments/agent_loop.py` (~534 lines)

### Purpose

Reusable multi-turn agent engine. Runs the hermes-agent tool-calling loop using standard OpenAI-spec tool calling.

### 2.1 Compatibility

Works with any server returning `ChatCompletion` objects with `tool_calls`:
- Phase 1: OpenAI server type (VLLM, SGLang, OpenRouter, OpenAI API)
- Phase 2: ManagedServer with client-side tool call parser

### 2.2 Thread Pool

```python
_tool_executor = concurrent.futures.ThreadPoolExecutor(max_workers=128)
```

**Why so large**: Some benchmarks (TerminalBench2) run 89 concurrent tasks all making tool calls. Too small = thread pool starvation, tasks queue for minutes.

**Resize at runtime**:
```python
def resize_tool_pool(max_workers: int):
    """Replace global tool executor with new size."""
```

Called by `HermesAgentBaseEnv.__init__` based on `config.tool_pool_size`.

### 2.3 AgentResult

```python
@dataclass
class AgentResult:
    messages: List[Dict[str, Any]]       # Full conversation history (OpenAI format)
    managed_state: Optional[Dict] = None  # ManagedServer.get_state() if available
    turns_used: int = 0                   # LLM calls made
    finished_naturally: bool = False      # Model stopped vs hitting max_turns
    reasoning_per_turn: List[Optional[str]]  # Extracted reasoning (PR #297)
    tool_errors: List[ToolError]          # Tool execution errors
```

### 2.4 ToolError

```python
@dataclass
class ToolError:
    turn: int          # Which turn the error occurred on
    tool_name: str     # Which tool was called
    arguments: str     # Arguments passed (truncated)
    error: str         # Error message
    tool_result: str   # Raw result returned to model
```

### 2.5 Execution Flow

```
1. Create messages list with system + user prompt
2. Loop up to max_turns:
   a. Call LLM with tools= definitions
   b. Extract tool_calls from response
   c. If no tool_calls → finished_naturally = True, break
   d. Execute each tool call via handle_function_call()
      - Async tools: run in current event loop
      - Sync tools (modal/docker/daytona): run in thread pool
   e. Append tool results to messages
   f. Extract reasoning content from response
3. Return AgentResult(messages, managed_state, turns_used, ...)
```

### 2.6 ManagedServer State

When Phase 2 is active, captures `ManagedServer.get_state()` for token-level analysis:
- Token IDs and logprobs for each assistant turn
- Required for OPD (On-Policy Distillation) training

---

## 3. ToolContext

### Location

`environments/tool_context.py` (~474 lines)

### Purpose

Per-rollout handle giving reward/verification functions direct access to ALL hermes-agent tools, scoped to the rollout's `task_id`.

### 3.1 Design Philosophy

> "Open-ended access to all tools. The verifier author decides which tools to use. Nothing is hardcoded or gated."

### 3.2 Interface

```python
class ToolContext:
    def __init__(self, task_id: str):
        self.task_id = task_id

    # Terminal tools
    def terminal(self, command: str, ...) -> dict: ...

    # File tools
    def read_file(self, path: str, ...) -> dict: ...
    def write_file(self, path: str, content: str) -> dict: ...
    def search_files(self, pattern: str, ...) -> dict: ...

    # Web tools
    def web_search(self, query: str, ...) -> dict: ...
    def web_extract(self, urls: list) -> dict: ...

    # Browser tools
    def browser_snapshot(self, ...) -> dict: ...
    def browser_click(self, ref: str) -> dict: ...
    def browser_type(self, ref: str, text: str) -> dict: ...

    # Code execution
    def execute_code(self, code: str) -> dict: ...
```

### 3.3 Session Sharing

All tool calls use the rollout's `task_id`, meaning:
- Terminal: same sandbox session the model used
- Browser: same browser tab state
- Files: same filesystem state

### 3.4 Thread Pool for Sync Tools

```python
_tool_executor = concurrent.futures.ThreadPoolExecutor(max_workers=4)
```

For tools that internally use `asyncio.run()` (modal, docker, daytona backends):

```python
def _run_tool_in_thread(tool_name, arguments, task_id):
    try:
        loop = asyncio.get_running_loop()
        with ThreadPoolExecutor(max_workers=1) as pool:
            return pool.submit(handle_function_call, ...).result(timeout=300)
    except RuntimeError:
        # No running event loop — safe to call directly
        return handle_function_call(...)
```

### 3.5 Cleanup

```python
def cleanup(ctx_task_id):
    """Clean up terminal and browser resources for a rollout."""
    cleanup_vm(ctx_task_id)
    cleanup_browser(ctx_task_id)
```

---

## 4. Agentic OPD Environment

### Location

`environments/agentic_opd_env.py` (~1,214 lines)

### Purpose

First Atropos environment implementing On-Policy Distillation (OPD) for agentic tool-calling tasks. Populates `distill_token_ids` / `distill_logprobs` fields on `ScoredDataGroup`.

### 4.1 Key Idea

Based on OpenClaw-RL (Princeton, 2026, arXiv:2603.10165):

> Every time an agent receives a next-state signal (tool result, error trace, test verdict), that signal contains hindsight information about how the agent's PREVIOUS response could have been better.

### 4.2 OPD Pipeline

```
1. Run standard agentic rollouts (tool-calling agent loop)
2. Walk conversation to find (assistant_turn, next_state) pairs
3. Use LLM judge to extract "hints" from next-state signals
4. Build enhanced prompt (original context + hint)
5. Score student's response tokens under enhanced distribution
   using VLLM's prompt_logprobs (via Atropos get_logprobs API)
6. Package teacher's top-K predictions as distill_token_ids /
   distill_logprobs on ScoredDataGroup
```

### 4.3 Token-Level Advantage

```
A_t = teacher_logprob(token_t) - student_logprob(token_t)
Positive → teacher approves this token (upweight)
Negative → teacher disapproves (downweight)
```

**Benefit**: Dense, token-level training signal from every tool interaction, not just scalar reward at trajectory end.

### 4.4 Requirements

| Requirement | Why |
|-------------|-----|
| VLLM backend (`server_type: vllm`) | Needed for `prompt_logprobs` scoring |
| Phase 2 mode (`ManagedServer`) | Needed for token-level tracking |

### 4.5 Task

Coding tasks with test verification. Rich next-state signals from:
- Test results (pass/fail, error messages)
- Terminal output (compilation errors, runtime traces)
- Falls back to built-in coding problems if no HuggingFace dataset configured

### 4.6 Usage

```bash
# Process mode (offline data generation with OPD)
python environments/agentic_opd_env.py process \
    --env.total_steps 10 --env.group_size 2 \
    --env.data_path_to_save_groups output.jsonl \
    --openai.base_url http://localhost:8000/v1 \
    --openai.model_name Qwen/Qwen3-4B

# Serve mode (connected to Atropos trainer)
python environments/agentic_opd_env.py serve \
    --openai.base_url http://localhost:8000/v1 \
    --openai.model_name Qwen/Qwen3-4B

# Evaluate mode
python environments/agentic_opd_env.py evaluate \
    --env.eval_size 10 \
    --openai.base_url http://localhost:8000/v1 \
    --openai.model_name Qwen/Qwen3-4B
```

---

## 5. Web Research Environment

### Location

`environments/web_research_env.py` (~719 lines)

### Purpose

RL environment for training models to do accurate, efficient, multi-source web research.

### 5.1 Reward Signals

| Signal | Weight | Calculation |
|--------|--------|-------------|
| Answer correctness | Primary | LLM judge (0.0–1.0) |
| Source diversity | Bonus | Used ≥2 distinct domains |
| Efficiency | Penalty | Penalizes excessive tool calls |
| Tool usage | Bonus | Bonus for actually using web tools |

### 5.2 Dataset

**FRAMES benchmark** (Google, 2024): Multi-hop factual questions
- HuggingFace: `google/frames-benchmark`
- Fallback: built-in sample questions (no HF token needed)

### 5.3 Modes

| Mode | Purpose |
|------|---------|
| `serve` | Connected to Atropos trainer |
| `process` | Offline data generation |
| `evaluate` | Standalone evaluation |

### 5.4 Inspiration

> GroceryMind — production Hermes agent doing live web research across German grocery stores (firecrawl + hermes-agent)

### 5.5 Author

Built by: github.com/jackx707

---

## 6. Benchmark Environments

### 6.1 TerminalBench2

**Location**: `environments/benchmarks/terminalbench_2/terminalbench2_env.py` (~1,016 lines)

**Purpose**: RL environment for the TerminalBench2 benchmark. Tests agent ability to complete terminal-based tasks.

**Features**:
- 89+ tasks requiring terminal interaction
- Test verification via exit codes and output matching
- Requires large thread pool (89 concurrent tasks)

### 6.2 YC Bench

**Location**: `environments/benchmarks/yc_bench/yc_bench_env.py` (~848 lines)

**Purpose**: Benchmark for Y Combinator startup tasks.

### 6.3 tblite

**Location**: `environments/benchmarks/tblite/tblite_env.py` (~119 lines)

**Purpose**: Lightweight benchmark environment.

### 6.4 Hermes SWE Env

**Location**: `environments/hermes_swe_env/hermes_swe_env.py` (~229 lines)

**Purpose**: Software Engineering environment for SWE-bench style tasks.

### 6.5 Terminal Test Env

**Location**: `environments/terminal_test_env/terminal_test_env.py` (~292 lines)

**Purpose**: Simple terminal testing environment for development and validation.

---

## 7. Tool Call Parsers

### Location

`environments/tool_call_parsers/` (~800 lines total across 12 files)

### Purpose

Model-specific tool call parsing for Phase 2 (ManagedServer) mode. Each parser handles the unique tool call format of a specific model family.

### 7.1 Available Parsers

| Parser | File | Target Model |
|--------|------|--------------|
| Hermes | `hermes_parser.py` (~75 lines) | Nous Hermes |
| Qwen | `qwen_parser.py` (~19 lines) | Qwen series |
| Qwen3 Coder | `qwen3_coder_parser.py` (~163 lines) | Qwen3 Coder |
| LLaMA | `llama_parser.py` (~96 lines) | LLaMA series |
| Mistral | `mistral_parser.py` (~137 lines) | Mistral series |
| DeepSeek V3 | `deepseek_v3_parser.py` (~89 lines) | DeepSeek V3 |
| DeepSeek V3.1 | `deepseek_v3_1_parser.py` (~72 lines) | DeepSeek V3.1 |
| GLM-4.5 | `glm45_parser.py` (~109 lines) | GLM-4.5 |
| GLM-4.7 | `glm47_parser.py` (~35 lines) | GLM-4.7 |
| Kimi K2 | `kimi_k2_parser.py` (~93 lines) | Kimi K2 |
| LongCat | `longcat_parser.py` (~69 lines) | LongCat |

### 7.2 Parser Interface

Each parser extracts tool calls from model output text:

```python
def parse_tool_calls(text: str) -> List[ToolCall]:
    """Extract tool calls from model response text."""
```

Handles:
- Native tool calling (OpenAI format)
- Text-based tool calling (XML tags, JSON blocks)
- Mixed format responses

---

## 8. Runtime Patches

### Location

`environments/patches.py` (~35 lines)

### Purpose

Monkey patches for async-safe tool operation inside Atropos's event loop.

### 8.1 Key Patch

Patches `SwerexModalEnvironment` to use a background thread instead of `asyncio.run()`. Without this patch:

```
Atropos event loop → asyncio.run() → RuntimeError: cannot run in already-running loop
```

The patch is safe for normal CLI too (single-threaded context).

### 8.2 Application

```python
from environments.patches import apply_patches
apply_patches()
```

Called at module import time in `hermes_base_env.py`.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Base environment lines | 714 |
| Agent loop lines | 534 |
| ToolContext lines | 474 |
| Agentic OPD env lines | 1,214 |
| Web research env lines | 719 |
| TerminalBench2 env lines | 1,016 |
| Tool call parsers | 12 files |
| Default max turns | 20 |
| Default group size | 8 |
| Default thread pool | 128 workers |
| ToolContext thread pool | 4 workers |
| Tool call RPC timeout | 300 seconds |
| Exhausted credential cooldown | 1 hour |
| FRAMES benchmark source | google/frames-benchmark |
| OPD reference | arXiv:2603.10165 (OpenClaw-RL) |

---

*Generated from source analysis of the Hermes Agent codebase.*
