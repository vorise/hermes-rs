# Hermes Agent — Prompt Caching & Session Title Generation

This document covers the Anthropic prompt caching system and automatic session title generation.

---

## Table of Contents

1. [Anthropic Prompt Caching](#1-anthropic-prompt-caching)
2. [Session Title Generation](#2-session-title-generation)

---

## 1. Anthropic Prompt Caching

### Location

`agent/prompt_caching.py` (73 lines)

### Purpose

Reduces input token costs by ~75% on multi-turn conversations by caching the conversation prefix. Uses Anthropic's `cache_control` breakpoints.

### 1.1 Strategy: `system_and_3`

Places up to 4 `cache_control` breakpoints (Anthropic's maximum):

| Breakpoint | Target | Rationale |
|------------|--------|-----------|
| 1 | System prompt | Stable across all turns |
| 2-4 | Last 3 non-system messages | Rolling window of recent context |

### 1.2 Cache Marker Application

```python
marker = {"type": "ephemeral"}
# Optional: marker["ttl"] = "1h" for 1-hour cache TTL
```

Applied differently based on message format:

| Message Type | How Marker Applied |
|-------------|-------------------|
| `tool` role (native Anthropic) | `msg["cache_control"] = marker` |
| Empty content | `msg["cache_control"] = marker` |
| String content | Converted to `[{"type": "text", "text": content, "cache_control": marker}]` |
| List content | Marker added to last element: `content[-1]["cache_control"] = marker` |

### 1.3 Pure Functions

No class state, no AIAgent dependency — pure functions that take a message list and return a deep copy with cache breakpoints injected.

### 1.4 Cache TTL

Configurable via `cache_ttl` parameter:
- `"5m"` (default) — 5-minute cache window
- `"1h"` — 1-hour cache window (adds `ttl: "1h"` to marker)

---

## 2. Session Title Generation

### Location

`agent/title_generator.py` (126 lines)

### Purpose

Auto-generates short session titles from the first user/assistant exchange. Runs asynchronously after the first response so it never adds latency.

### 2.1 Title Prompt

```
Generate a short, descriptive title (3-7 words) for a conversation that starts with
the following exchange. The title should capture the main topic or intent.
Return ONLY the title text, nothing else. No quotes, no punctuation at the end, no prefixes.
```

### 2.2 Generation Parameters

| Parameter | Value |
|-----------|-------|
| Model | Auxiliary LLM (cheapest/fastest available) |
| Max tokens | 30 |
| Temperature | 0.3 |
| Timeout | 30s |
| Message truncation | 500 chars per message |

### 2.3 Cleanup

Post-processing removes common LLM formatting artifacts:
- Strips surrounding quotes
- Removes `Title:` prefix
- Enforces 80-character max (truncates with `...`)

### 2.4 Trigger Conditions

Title generation fires when:
1. First user → assistant exchange completes
2. Session doesn't already have a title (user may have set one via `/title`)
3. User message count in history is <= 2

### 2.5 Async Execution

```python
thread = threading.Thread(
    target=auto_title_session,
    args=(session_db, session_id, user_message, assistant_response),
    daemon=True,
    name="auto-title",
)
thread.start()
```

Runs on a background daemon thread named `auto-title`. Silently skips if:
- `session_db` is None
- Session already has a title
- Title generation fails

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Max cache breakpoints | 4 |
| Default cache TTL | 5 minutes |
| Title word range | 3-7 words |
| Title max length | 80 characters |
| Title generation timeout | 30s |
| Title temperature | 0.3 |
| Title max tokens | 30 |
| Message snippet length | 500 chars |

---

*Generated from source analysis of the Hermes Agent codebase.*
