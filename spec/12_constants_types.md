# Hermes Agent — Constants, Types & Configuration

This document covers all constants, system prompts, model catalog, tool limits, and configuration schema.

---

## Table of Contents

1. [Hermes Constants](#1-hermes-constants)
2. [System Prompts](#2-system-prompts)
3. [Model Catalog](#3-model-catalog)
4. [Tool Limits](#4-tool-limits)
5. [Configuration Schema](#5-configuration-schema)
6. [Environment Variables](#6-environment-variables)
7. [Hermes Home](#7-hermes-home)
8. [Hermes Time](#8-hermes-time)
9. [Hermes Logging](#9-hermes-logging)

---

## 1. Hermes Constants

### Location

`hermes_constants.py` (~10K lines)

### Key Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `get_hermes_home()` | `~/.hermes` | Hermes home directory |
| `display_hermes_home()` | String representation | Display hermes home path |
| `OPENROUTER_BASE_URL` | `https://openrouter.ai/api/v1` | OpenRouter API URL |
| `DEFAULT_AGENT_IDENTITY` | String | Default agent identity |
| `PLATFORM_HINTS` | Dict | Platform-specific hints |
| `MEMORY_GUIDANCE` | String | Memory system instructions |
| `SESSION_SEARCH_GUIDANCE` | String | Session search hints |
| `SKILLS_GUIDANCE` | String | Skills system hints |

### Hermes Home

The `~/.hermes` directory contains:
- `config.yaml` — User configuration
- `.env` — API keys and secrets
- `state.db` — SQLite session store
- `SOUL.md` — User personality file
- `AGENTS.md` — Project instructions
- `memory/` — Persistent memories
- `skills/` — Installed skills
- `logs/` — Session logs

---

## 2. System Prompts

### Location

`agent/prompt_builder.py` (~46K lines)

### System Prompt Constants

| Constant | Purpose |
|----------|---------|
| `DEFAULT_AGENT_IDENTITY` | Core agent identity |
| `PLATFORM_HINTS` | Platform-specific formatting hints |
| `MEMORY_GUIDANCE` | Memory system instructions |
| `SESSION_SEARCH_GUIDANCE` | Session search hints |
| `SKILLS_GUIDANCE` | Skills system hints |
| `TOOL_USE_ENFORCEMENT_GUIDANCE` | How to use tools properly |
| `TOOL_USE_ENFORCEMENT_MODELS` | Models that need tool enforcement |
| `DEVELOPER_ROLE_MODELS` | Developer role models |
| `GOOGLE_MODEL_OPERATIONAL_GUIDANCE` | Google model operation hints |
| `OPENAI_MODEL_EXECUTION_GUIDANCE` | OpenAI model execution hints |

### System Prompt Assembly

The system prompt is assembled in this order:
1. Agent identity
2. Personality (from config or soul)
3. Platform hints
4. Tool usage guidance
5. Memory guidance
6. Session search guidance
7. Skills guidance
8. Context files (AGENTS.md, .cursorrules)
9. Environment hints
10. Nous subscription prompt
11. Skills system prompt

---

## 3. Model Catalog

### Location

`hermes_cli/models.py` (~72K lines), `agent/model_metadata.py` (~44K lines)

### Model Metadata

Each model has metadata including:
- Context window size
- Max output tokens
- Token estimation parameters
- Capability flags (tools, vision, reasoning)
- Pricing information

---

## 4. Tool Limits

### Parallel Execution Limits

| Limit | Value | Purpose |
|-------|-------|---------|
| `_MAX_TOOL_WORKERS` | 8 | Max concurrent tool threads |
| `_NEVER_PARALLEL_TOOLS` | `{"clarify"}` | Tools that must run sequentially |
| `_PARALLEL_SAFE_TOOLS` | ~10 tools | Tools safe for parallel execution |
| `_PATH_SCOPED_TOOLS` | `{read_file, write_file, patch}` | Tools with path overlap detection |

### Result Size Limits

| Tool | Limit | Purpose |
|------|-------|---------|
| Default | Configurable per tool | Prevent oversized results |
| Terminal | Configurable | Terminal output size |
| Web | Configurable | Web content size |

### Iteration Limits

| Limit | Default | Purpose |
|-------|---------|---------|
| `max_iterations` | 90 | Max tool-call iterations per turn |
| `delegation.max_iterations` | 50 | Max iterations per subagent |

---

## 5. Configuration Schema

### Location

`hermes_cli/config.py` (~136K lines)

### DEFAULT_CONFIG

The default configuration defines 200+ config keys covering:

| Category | Keys |
|----------|------|
| Provider/Model | provider, model, base_url, api_key, max_tokens |
| Tools | enabled_toolsets, disabled_toolsets |
| Skills | enabled_skills, disabled_skills |
| Terminal | backend, working_directory |
| Delegation | max_iterations, model override |
| Memory | enabled, honcho integration |
| Personality | personality, soul_file |
| Platform | per-platform settings |
| Cron | job definitions |
| Web | host, port, enabled |
| Logging | verbose, quiet |

### Config Migration

`hermes_cli/config.py` includes migration logic for config schema changes:
- Schema version tracking
- Backward-compatible migrations
- Value transformation

### OPTIONAL_ENV_VARS

List of optional environment variables that can override config values.

---

## 6. Environment Variables

### Core Environment Variables

| Variable | Purpose |
|----------|---------|
| `HERMES_HOME` | Override ~/.hermes directory |
| `HERMES_CONFIG` | Override config file path |
| `HERMES_QUIET` | Suppress startup messages |
| `HERMES_VERBOSE` | Enable verbose logging |
| `HERMES_DEBUG` | Enable debug logging |

### Provider Environment Variables

(See spec 09_auth_providers.md for full list)

---

## 7. Hermes Home

### Location

`hermes_constants.py` — `get_hermes_home()`, `display_hermes_home()`

### Directory Structure

```
~/.hermes/
├── config.yaml          # User configuration
├── .env                 # API keys and secrets
├── state.db             # SQLite session store
├── SOUL.md              # User personality (optional)
├── AGENTS.md            # Project instructions (optional)
├── memory/              # Persistent memories
│   ├── MEMORY.md        # Memory index
│   └── *.md             # Individual memory files
├── skills/              # Installed skills
│   └── *.md             # Skill definition files
├── logs/                # Session logs
│   └── *.jsonl          # Per-session log files
└── backups/             # Backup files
```

### HERMES_HOME Override

The `HERMES_HOME` environment variable can override the default `~/.hermes` path.

---

## 8. Hermes Time

### Location

`hermes_time.py` (~3K lines)

### Purpose

Time utilities for the Hermes agent.

### Functions

| Function | Purpose |
|----------|---------|
| `get_timestamp()` | Current timestamp |
| `format_duration()` | Format duration compactly |
| `parse_time()` | Parse time strings |

---

## 9. Hermes Logging

### Location

`hermes_logging.py` (~14K lines)

### Purpose

Logging configuration for the Hermes agent.

### Features

- Session context tracking (filter logs by session ID)
- Log file rotation
- Console and file handlers
- Structured logging support
- Debug mode

### Session Context

```python
from hermes_logging import set_session_context
set_session_context(session_id)
```

Sets the session context for all log records on the current thread, enabling `hermes logs --session <id>` filtering.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Config keys | 200+ |
| Environment variables | 20+ |
| System prompt constants | 10+ |
| Hermes home files | 7 directories |
| Tool parallel limits | 3 categories |
| Max tool workers | 8 |
| Default max iterations | 90 |

---

*Generated from source analysis of the Hermes Agent codebase.*
