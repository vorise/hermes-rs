# Hermes Agent — Research & RL

This document covers the research pipeline: RL training environments, batch runner, trajectory compression, and Atropos integration.

---

## Table of Contents

1. [Research Overview](#1-research-overview)
2. [RL Environments](#2-rl-environments)
3. [Batch Runner](#3-batch-runner)
4. [Trajectory Compressor](#4-trajectory-compressor)
5. [RL CLI](#5-rl-cli)
6. [Mini SWE Runner](#6-mini-swe-runner)
7. [Atropos Integration](#7-atropos-integration)
8. [Tinker Integration](#8-tinker-integration)

---

## 1. Research Overview

### Purpose

Hermes includes a research pipeline for generating training data and running RL experiments with tool-calling agents.

### Key Components

| Component | Purpose |
|-----------|---------|
| `environments/` | RL training environments (Atropos) |
| `batch_runner.py` | Parallel batch trajectory generation |
| `trajectory_compressor.py` | Trajectory compression for training |
| `rl_cli.py` | RL training CLI |
| `mini_swe_runner.py` | Mini SWE agent runner |
| `toolset_distributions.py` | Toolset distribution utilities |

### Data Flow

```
Batch Runner → AIAgent instances → Tool execution → Trajectory files
     ↓
Trajectory Compressor → Compressed training data
     ↓
RL Training (Atropos/Tinker) → Trained model
```

---

## 2. RL Environments

### Location

`environments/` — 14 files

### Purpose

RL training environments built on the Atropos framework.

### Architecture

```
environments/
├── __init__.py
├── base.py              # Base environment class
├── terminal.py          # Terminal interaction environment
└── ...                  # Specific environments
```

### Base Environment

`environments/base.py` — Abstract base class for all RL environments.

#### Interface

| Method | Purpose |
|--------|---------|
| `reset()` | Reset environment to initial state |
| `step()` | Execute action, return new state |
| `reward()` | Calculate reward for action |
| `done()` | Check if episode is complete |
| `render()` | Render environment state |

### Terminal Environment

`environments/terminal.py` — Terminal interaction environment for tool-calling RL.

#### Features

- Shell command execution
- File system interaction
- Reward based on task completion
- Episode-based training

---

## 3. Batch Runner

### Location

`batch_runner.py` (~55K lines)

### Purpose

Parallel batch trajectory generation. Runs many agent conversations simultaneously to collect training data.

### Key Features

- Parallel execution via ThreadPoolExecutor
- Per-task isolation (separate session IDs, VMs)
- Trajectory saving to JSONL
- Progress tracking
- Error recovery
- Provider failover

### Batch Configuration

| Parameter | Purpose |
|-----------|---------|
| `num_parallel` | Number of parallel agents |
| `prompts` | List of prompts to run |
| `model` | Model to use |
| `save_trajectories` | Whether to save trajectories |
| `toolsets` | Toolsets to enable |
| `max_iterations` | Max iterations per agent |

### Trajectory Output

Each conversation is saved as JSONL with:
- System prompt
- All messages (user, assistant, tool)
- Tool definitions
- Metadata (model, cost, tokens, duration)

---

## 4. Trajectory Compressor

### Location

`trajectory_compressor.py` (~63K lines)

### Purpose

Compress trajectories for training data generation. Reduces token count while preserving conversation quality.

### Compression Strategies

| Strategy | Description |
|----------|-------------|
| Message summarization | Summarize long messages |
| Tool result pruning | Remove redundant tool results |
| Context window optimization | Optimize for training context size |
| Token reduction | Reduce token count while preserving meaning |

### Key Features

- Configurable compression levels
- Quality preservation checks
- Batch processing
- Token count reporting
- Output format validation

---

## 5. RL CLI

### Location

`rl_cli.py` (~16K lines)

### Purpose

CLI for RL training operations.

### Commands

| Command | Purpose |
|---------|---------|
| `rl train` | Start RL training |
| `rl evaluate` | Evaluate trained model |
| `rl generate` | Generate training data |
| `rl compress` | Compress trajectories |

---

## 6. Mini SWE Runner

### Location

`mini_swe_runner.py` (~27K lines)

### Purpose

Mini SWE (Software Engineering) agent runner for benchmarking agent capabilities on real-world tasks.

### Features

- SWE-bench compatible
- GitHub issue resolution
- Code modification tracking
- Success metric calculation

---

## 7. Atropos Integration

### Dependencies

```toml
[project.optional-dependencies]
rl = [
    "atroposlib @ git+https://github.com/NousResearch/atropos.git@...",
    "tinker @ git+https://github.com/thinking-machines-lab/tinker.git@...",
    "fastapi>=0.104.0,<1",
    "uvicorn[standard]>=0.24.0,<1",
    "wandb>=0.15.0,<1",
]
```

### Atropos

Atropos is an RL framework from Nous Research for training tool-calling models.

### Integration Points

| Point | Description |
|-------|-------------|
| Environment | Hermes environments wrap Atropos envs |
| Reward | Hermes calculates rewards for tool actions |
| Trajectory | Hermes generates trajectories for Atropos |
| Training | Atropos trains on Hermes trajectories |

---

## 8. Tinker Integration

### Tinker

Tinker is a reinforcement learning framework from Thinking Machines Lab.

### Integration

Hermes environments can be used with Tinker for:
- Tool-calling RL
- Multi-turn dialogue RL
- Code generation RL

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Environment files | 14 |
| Batch runner size | ~55K lines |
| Trajectory compressor size | ~63K lines |
| RL CLI size | ~16K lines |
| Mini SWE runner size | ~27K lines |
| Parallel agents | Configurable |
| RL extras | atroposlib, tinker, wandb |

---

*Generated from source analysis of the Hermes Agent codebase.*
