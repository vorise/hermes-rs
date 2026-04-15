# Hermes Agent — Services, Context & State

This document covers the service layer: SQLite state store, memory management, prompt building, model metadata, auxiliary client, and credential management.

---

## Table of Contents

1. [SessionDB — SQLite State Store](#1-sessiondb--sqlite-state-store)
2. [Memory Manager](#2-memory-manager)
3. [Memory Provider](#3-memory-provider)
4. [Prompt Builder](#4-prompt-builder)
5. [Model Metadata](#5-model-metadata)
6. [Auxiliary Client](#6-auxiliary-client)
7. [Credential Pool](#7-credential-pool)
8. [Error Classifier](#8-error-classifier)
9. [Usage Pricing](#9-usage-pricing)
10. [Rate Limit Tracker](#10-rate-limit-tracker)
11. [Display & Spinner](#11-display--spinner)
12. [Insights & Analytics](#12-insights--analytics)
13. [Title Generator](#13-title-generator)
14. [Context Engine](#14-context-engine)
15. [Context References](#15-context-references)

---

## 1. SessionDB — SQLite State Store

### Location

`hermes_state.py` (~50K lines)

### Purpose

Persistent session storage with FTS5 full-text search. Replaces per-session JSONL files with a relational database.

### Schema Version

Current: `SCHEMA_VERSION = 6`

### Tables

#### sessions

| Column | Type | Purpose |
|--------|------|---------|
| `id` | TEXT PK | Session UUID |
| `source` | TEXT | Platform source (cli, telegram, discord, etc.) |
| `user_id` | TEXT | Gateway user ID |
| `model` | TEXT | Model name used |
| `model_config` | TEXT | Model configuration JSON |
| `system_prompt` | TEXT | System prompt snapshot |
| `parent_session_id` | TEXT | Parent session (compression chain) |
| `started_at` | REAL | Session start timestamp |
| `ended_at` | REAL | Session end timestamp |
| `end_reason` | TEXT | Why session ended |
| `message_count` | INTEGER | Total messages |
| `tool_call_count` | INTEGER | Total tool calls |
| `input_tokens` | INTEGER | Input tokens used |
| `output_tokens` | INTEGER | Output tokens used |
| `cache_read_tokens` | INTEGER | Cache read tokens |
| `cache_write_tokens` | INTEGER | Cache write tokens |
| `reasoning_tokens` | INTEGER | Reasoning tokens |
| `billing_provider` | TEXT | Billing provider |
| `billing_base_url` | TEXT | Billing endpoint |
| `billing_mode` | TEXT | Billing mode |
| `estimated_cost_usd` | REAL | Estimated cost |
| `actual_cost_usd` | REAL | Actual cost |
| `cost_status` | TEXT | Cost status |
| `cost_source` | TEXT | Cost source |
| `pricing_version` | TEXT | Pricing table version |
| `title` | TEXT | Session title |

#### messages

| Column | Type | Purpose |
|--------|------|---------|
| `id` | INTEGER PK AUTOINCREMENT | Message ID |
| `session_id` | TEXT FK | Session reference |
| `role` | TEXT | Message role (user, assistant, tool, system) |
| `content` | TEXT | Message content |
| `tool_call_id` | TEXT | Tool call reference |
| `tool_calls` | TEXT | Tool calls JSON array |
| `tool_name` | TEXT | Tool name (for tool results) |
| `timestamp` | REAL | Message timestamp |
| `token_count` | INTEGER | Estimated token count |
| `finish_reason` | TEXT | API finish reason |
| `reasoning` | TEXT | Reasoning text |
| `reasoning_details` | TEXT | Reasoning detail JSON |
| `codex_reasoning_items` | TEXT | Codex reasoning items |

#### messages_fts

FTS5 virtual table for full-text search on message content.

### Indexes

| Index | Columns | Purpose |
|-------|---------|---------|
| `idx_sessions_source` | `source` | Filter by platform |
| `idx_sessions_parent` | `parent_session_id` | Compression chain |
| `idx_sessions_started` | `started_at DESC` | Recent sessions |
| `idx_messages_session` | `session_id, timestamp` | Messages by session |

### Key Methods

| Method | Purpose |
|--------|---------|
| `create_session()` | Create new session record |
| `add_message()` | Add message to session |
| `get_session()` | Get session metadata |
| `get_messages()` | Get messages for session |
| `update_system_prompt()` | Store system prompt snapshot |
| `search_sessions()` | FTS5 search across sessions |
| `get_session_summaries()` | Get recent session summaries |
| `update_session_stats()` | Update token/cost stats |
| `count_sessions()` | Count sessions by source |

### FTS5 Search

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    content=messages,
    content_rowid=id
);
```

Triggers maintain FTS index on insert/update/delete.

### WAL Mode

SQLite WAL mode enables concurrent readers + one writer, supporting the gateway's multi-platform operation.

---

## 2. Memory Manager

### Location

`agent/memory_manager.py` (~14K lines)

### Purpose

Manages persistent memory with honcho integration. Handles memory creation, retrieval, and consolidation.

### Key Methods

| Method | Purpose |
|--------|---------|
| `prefetch_all()` | Fetch all relevant memories for a query |
| `save_memory()` | Save a new memory entry |
| `get_memories()` | Retrieve memories by query |
| `clear_memories()` | Clear all memories |
| `get_user_id()` | Get user identifier |

### Honcho Integration

The memory manager integrates with the Honcho dialectic user modeling system for deep user understanding across sessions.

---

## 3. Memory Provider

### Location

`agent/memory_provider.py` (~10K lines)

### Purpose

Abstract interface for memory backends. Allows plugging different memory systems.

### Interface

| Method | Purpose |
|--------|---------|
| `query()` | Query memories |
| `save()` | Save memory |
| `delete()` | Delete memory |
| `clear()` | Clear all memories |

---

## 4. Prompt Builder

### Location

`agent/prompt_builder.py` (~46K lines)

### Purpose

Assembles system prompts from multiple components.

### System Prompt Components

| Component | Function | Purpose |
|-----------|----------|---------|
| Identity | `DEFAULT_AGENT_IDENTITY` | Core agent identity |
| Personality | User config / soul | Agent personality |
| Tool guidance | `TOOL_USE_ENFORCEMENT_GUIDANCE` | How to use tools properly |
| Platform hints | `PLATFORM_HINTS` | Platform-specific formatting |
| Memory guidance | `MEMORY_GUIDANCE` | Memory system instructions |
| Session search guidance | `SESSION_SEARCH_GUIDANCE` | Search hints |
| Skills guidance | `SKILLS_GUIDANCE` | Skills system hints |
| Context files | `build_context_files_prompt()` | AGENTS.md, .cursorrules |
| Environment hints | `build_environment_hints()` | Backend capabilities |
| Soul | `load_soul_md()` | SOUL.md personality |
| Nous subscription | `build_nous_subscription_prompt()` | Nous Portal hints |
| Skills system | `build_skills_system_prompt()` | Skills instructions |

### Prompt Assembly Flow

```
1. Load identity
2. Load personality (from config or soul)
3. Add platform hints
4. Add tool usage guidance
5. Add memory guidance (if memory enabled)
6. Add skills guidance (if skills enabled)
7. Add context files (AGENTS.md, etc.)
8. Add environment hints
9. Add Nous subscription prompt (if applicable)
10. Add skills system prompt (if skills enabled)
11. Combine all components
```

### Caching

The assembled system prompt is cached per session to enable Anthropic prefix caching. Only rebuilt after:
- Context compression events
- Memory changes (with cache invalidation)
- Session restart with different configuration

### Special Models

Some models require specific operational guidance:

| Model Family | Guidance |
|-------------|----------|
| Google models | `GOOGLE_MODEL_OPERATIONAL_GUIDANCE` |
| OpenAI models | `OPENAI_MODEL_EXECUTION_GUIDANCE` |
| Developer role | `DEVELOPER_ROLE_MODELS` |

---

## 5. Model Metadata

### Location

`agent/model_metadata.py` (~44K lines)

### Purpose

Model context lengths, token estimation, and metadata management.

### Key Functions

| Function | Purpose |
|----------|---------|
| `fetch_model_metadata()` | Fetch metadata for a model |
| `estimate_tokens_rough()` | Rough token count for text |
| `estimate_messages_tokens_rough()` | Token count for message list |
| `estimate_request_tokens_rough()` | Full request token estimate |
| `get_next_probe_tier()` | Context probing tier selection |
| `parse_context_limit_from_error()` | Extract context limit from API error |
| `parse_available_output_tokens_from_error()` | Extract output limit from error |
| `save_context_length()` | Cache context length for model |
| `is_local_endpoint()` | Check if endpoint is local |
| `query_ollama_num_ctx()` | Query Ollama context length |

### Token Estimation

Uses character-count-based estimation (~4 chars per token for English text).

### Context Length Probing

Supports multiple tiers of context length probing:
1. Small probe → detect minimum context
2. Medium probe → detect reasonable context
3. Large probe → detect maximum context

---

## 6. Auxiliary Client

### Location

`agent/auxiliary_client.py` (~110K lines)

### Purpose

Secondary LLM client used for auxiliary tasks: context compression, vision, summarization, session search, and title generation.

### Key Features

- Independent API configuration (can use different provider than main agent)
- Streaming support
- Image/vision support
- Low-latency model selection
- Cost tracking separate from main agent

### Use Cases

| Use Case | Description |
|----------|-------------|
| Context compression | Summarize older conversation turns |
| Session summarization | Create session summaries for search |
| Vision analysis | Analyze images with vision-capable model |
| Title generation | Auto-generate session titles |
| Session search summarization | LLM-powered search result summaries |
| Memory consolidation | Consolidate memories during idle |

---

## 7. Credential Pool

### Location

`agent/credential_pool.py` (~58K lines)

### Purpose

Manages provider credentials with automatic failover.

### Key Concepts

- **Credentials** — Each credential = provider + API key + base URL
- **Active set** — Subset of credentials currently available
- **Failover** — Automatic switching to next credential on failure
- **Recovery** — Credentials periodically re-tested for availability
- **Thread-safe** — All operations are thread-safe

### Credential States

| State | Description |
|-------|-------------|
| Active | Available for use |
| Unavailable | Temporarily disabled (rate limit, error) |
| Recovering | Being re-tested |

### Failover Flow

1. API call fails with rate limit / error
2. Current credential marked unavailable
3. Pool selects next active credential
4. API call retried with new credential
5. Unavailable credentials periodically re-tested
6. If recovered, credential reactivated

---

## 8. Error Classifier

### Location

`agent/error_classifier.py` (~28K lines)

### Purpose

Classifies API errors to determine recovery strategy.

### Failover Reasons

| FailoverReason | Recovery Strategy |
|----------------|------------------|
| Rate limit | Retry with backoff, failover if persistent |
| Context length | Trigger compression, retry |
| Invalid request | Abort, report to user |
| Network error | Retry with connection cleanup |
| Provider unavailable | Failover to next credential |
| Authentication failure | Report to user |
| Timeout | Retry with timeout adjustment |
| Empty response | Retry |

### Classification Flow

```
API Error → classify_api_error() → FailoverReason → Recovery Strategy
```

---

## 9. Usage Pricing

### Location

`agent/usage_pricing.py` (~22K lines)

### Purpose

Calculate token costs across all supported providers and models.

### Key Functions

| Function | Purpose |
|----------|---------|
| `estimate_usage_cost()` | Calculate cost for token usage |
| `normalize_usage()` | Normalize usage data for display |
| `format_duration_compact()` | Format duration compactly |
| `format_token_count_compact()` | Format token count compactly |

### Pricing Data

Maintains pricing tables for all supported models:
- Input price per 1M tokens
- Output price per 1M tokens
- Cache read price per 1M tokens
- Cache write price per 1M tokens

---

## 10. Rate Limit Tracker

### Location

`agent/rate_limit_tracker.py` (~8K lines)

### Purpose

Track rate limit status across providers.

### Key Features

- Per-provider rate limit tracking
- Reset time calculation
- Warning thresholds
- Integration with credential pool failover

---

## 11. Display & Spinner

### Location

`agent/display.py` (~40K lines)

### Purpose

UI display components: spinner animations, tool preview formatting, cute tool messages, emoji detection.

### Key Classes/Functions

| Item | Purpose |
|------|---------|
| `KawaiiSpinner` | Animated spinner for tool execution |
| `build_tool_preview()` | Format tool call preview for display |
| `get_cute_tool_message()` | Cute/kawaii tool messages |
| `_detect_tool_failure()` | Detect failed tool execution |
| `get_tool_emoji()` | Get emoji for tool name |
| `format_tool_name()` | Format tool name for display |

### Spinner Frames

```python
_COMMAND_SPINNER_FRAMES = ("⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏")
```

---

## 12. Insights & Analytics

### Location

`agent/insights.py` (~34K lines)

### Purpose

Usage analytics and insights across sessions.

### Key Features

- Session count by period
- Token usage breakdown
- Cost analysis
- Model usage patterns
- Tool usage patterns
- Platform breakdown
- Peak usage detection
- Trend analysis

---

## 13. Title Generator

### Location

`agent/title_generator.py` (~4K lines)

### Purpose

Auto-generate session titles from first message.

### Key Features

- LLM-based title generation
- Fallback to first message snippet
- Title length limiting

---

## 14. Context Engine

### Location

`agent/context_engine.py` (~7K lines)

### Purpose

High-level context management. Coordinates compression, memory, and prompt building.

---

## 15. Context References

### Location

`agent/context_references.py` (~17K lines)

### Purpose

Track and manage context file references (AGENTS.md, .cursorrules, SOUL.md, etc.)

### Key Features

- Auto-discovery of context files
- Content loading and injection
- Change detection
- Cache invalidation

---

## Key Files by Importance

| Rank | File | Size | Role |
|------|------|------|------|
| 1 | `agent/auxiliary_client.py` | ~110K | Auxiliary LLM client |
| 2 | `agent/credential_pool.py` | ~58K | Provider credential failover |
| 3 | `agent/model_metadata.py` | ~44K | Model context/metadata |
| 4 | `agent/display.py` | ~40K | UI display components |
| 5 | `agent/prompt_builder.py` | ~46K | System prompt assembly |
| 6 | `agent/context_compressor.py` | ~49K | Auto context compression |
| 7 | `agent/insights.py` | ~34K | Usage analytics |
| 8 | `agent/error_classifier.py` | ~28K | API error classification |
| 9 | `agent/usage_pricing.py` | ~22K | Token cost calculation |
| 10 | `hermes_state.py` | ~50K | SQLite session store |

---

*Generated from source analysis of the Hermes Agent codebase.*
