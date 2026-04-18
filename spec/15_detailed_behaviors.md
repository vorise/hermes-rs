# Hermes Agent — Detailed Behavior Spec

This document captures implementation-level behaviors from the Python source (`run_agent.py`, `model_tools.py`, and core agent modules) that drive the hermes-rs implementation.

---

## 1. Conversation Loop Architecture

### 1.1 Main Loop Structure

`run_conversation()` runs a two-level loop:

**Outer loop** (API call iterations):
- Condition: `(api_call_count < max_iterations && budget.remaining > 0) || budget_grace_call`
- Each iteration: check interrupt, consume budget, build API messages, call API with retry, process response
- Tool calls: execute, append results, continue outer loop
- `finish_reason == "stop"`: return final response

**Inner loop** (API retries, max 3):
- Build API kwargs, call streaming or non-streaming API
- On success: break inner loop, process response
- On failure: classify error, retry/backoff/fallback/compress

### 1.2 Per-Turn Initialization

At start of each `run_conversation()`:
- Restore primary runtime if previous turn activated fallback
- Sanitize surrogates from user input
- Reset all retry counters (invalid_tool, invalid_json, empty_content, scratchpad, codex_incomplete, thinking_prefill, post_tool_empty, mute_post_response, unicode_sanitization_passes)
- Dead connection cleanup for non-anthropic modes
- Replay compression warning through status_callback
- Create fresh IterationBudget instance
- Hydrate todo store from conversation history if empty
- Increment `_user_turn_count`, check memory nudge trigger

### 1.3 System Prompt Caching

1. First turn: check SQLite for stored prompt, reuse if found; otherwise build from scratch
2. Subsequent turns: reuse cached prompt to preserve Anthropic cache prefix
3. Only rebuilt after context compression
4. Plugin hook `on_session_start` fires once on brand-new session creation

### 1.4 Preflight Context Compression

Before entering main loop, if loaded history exceeds threshold:
- Estimate token count including tool schema tokens (20-30K+ with many tools)
- Run up to 3 compression passes
- Reset retry counters after compression
- Re-estimate after each pass, break when under threshold

### 1.5 Plugin Context Injection

`pre_llm_call` hook fires once per turn:
- Plugins return context dict with `context` key or plain string
- All injected context appended to user message (NOT system prompt)
- Preserves prompt cache prefix
- All injected context is ephemeral (not persisted to session DB)

### 1.6 External Memory Provider Prefetch

If `_memory_manager` active:
- Call `prefetch_all(user_message)` once before tool loop
- Cache result in `_ext_prefetch_cache` - reused on every iteration
- Uses `original_user_message` (clean input)

### 1.7 Message Preparation for API

- Current-turn user message: inject memory prefetch context (fenced) + plugin context
- Assistant messages: copy `reasoning` to `reasoning_content`, then remove `reasoning` field
- Remove `finish_reason`, `_thinking_prefill` internal marker
- For strict providers: sanitize tool call fields (remove Codex-specific fields)
- Keep `reasoning_details` for OpenRouter multi-turn reasoning context

### 1.8 System Message Assembly

`effective_system = active_system_prompt + ephemeral_system_prompt`
- Ephemeral additions are API-call-time only
- External recall context goes into user message, not system prompt

### 1.9 Prefill Messages

- Inserted right after system prompt but before conversation history
- Never stored in messages list
- Automatically re-applied on every API call

### 1.10 Prompt Caching Application

If `_use_prompt_caching`:
- Apply Anthropic cache control: `cache_control` breakpoints on system + last 3 messages
- For OpenRouter Claude: `x-anthropic-beta: fine-grained-tool-streaming-2025-05-14` header

### 1.11 Message Sanitization

Before sending to API:
- Strip orphaned tool results / add stubs for missing results
- Normalize whitespace on all message content
- Normalize tool-call JSON: `json.dumps(args, separators=(",",":"), sort_keys=True)`

### 1.12 Loop Exit Conditions

- `interrupt_requested` is true
- Budget exhausted (after grace call consumed)
- `api_call_count >= max_iterations`
- Response `finish_reason == "stop"`

Grace call: when budget exhausted, set `_budget_grace_call = True` for one more iteration, then exit.

---

## 2. Error Classification and Recovery

### 2.1 Error Classification

All API errors pass through `classify_api_error()`:
- `reason`: FailoverReason enum (rate_limit, billing, overloaded, context_overflow, payload_too_large, long_context_tier, thinking_signature, auth, etc.)
- `status_code`, `retryable`, `should_compress`, `should_rotate_credential`, `should_fallback`

### 2.2 Recovery Decision Tree

1. UnicodeEncodeError (surrogates/ASCII codec) -> sanitize -> retry (max 2 passes)
2. Credential pool rotation -> if recovered, continue retry loop
3. Codex/Nous/Anthropic auth refresh (401) -> refresh -> retry (once each)
4. Thinking signature invalid -> strip reasoning_details -> retry (once)
5. Rate limit (429/billing) -> check credential pool -> if pool can recover, retry; else eagerly fallback
6. Payload too large (413) -> compress context (max 3 attempts)
7. Context length error -> parse actual limit -> step down context_length OR reduce max_tokens -> compress
8. Anthropic long-context tier (429) -> reduce to 200K -> compress
9. Non-retryable client error -> try fallback -> if no fallback, abort with error dump
10. Max retries exhausted -> try transport recovery -> try fallback -> abort

### 2.3 Credential Pool Recovery

- Called on rate-limit and auth errors
- Pool rotates to next available credential
- Eager fallback suppressed when pool may still recover

### 2.4 Context Compression on Errors

Triggered on: 413, context overflow, Anthropic long-context tier gate
- Increment `compression_attempts`, max 3
- If messages reduced -> restart with compressed messages
- Outer loop decrements `api_call_count`, refunds budget, retries
- `retry_count` still incremented to prevent infinite loops

### 2.5 Context Length Probing

- Try to parse actual limit from error message
- If found: use as new `context_length`
- Otherwise: step down to next probe tier (`get_next_probe_tier`)
- Only persist limits parsed from provider error (not guessed tiers)

### 2.6 Output Cap Adjustment

When `max_tokens` too large:
- Parse `available_output_tokens` from error
- Set `_ephemeral_max_output_tokens = available - 64`
- Retry without touching `context_length`

### 2.7 Thinking Budget Exhaustion Detection

When `finish_reason == "length"`:
- Check if response has think tags but no visible content after them
- Only flag when model produced reasoning blocks but no text
- Models without think tags treated as normal truncations

### 2.8 Truncated Tool Call Recovery

When `finish_reason == "length"` with tool calls:
- Retry API call once (don't append broken response)
- If still truncated -> refuse to execute incomplete tool arguments

### 2.9 Length Continuation

When `finish_reason == "length"` without tool calls:
- Append partial assistant message, send continuation prompt
- Retry up to 3 times, then return partial response

### 2.10 Transport Recovery

Before falling back on max retries:
- `_try_recover_primary_transport()` rebuilds HTTP client
- Cleans up dead connections in connection pool
- One-shot attempt per API call block

### 2.11 API Error Context Extraction

Parses: response body (error.code, error.message, resets_at), response headers (Retry-After, x-ratelimit-reset), error message regex patterns

### 2.12 Error Summary for Display

- Cloudflare HTML: extract `<title>` tag + Ray ID
- JSON body errors: extract `error.message`
- Fallback: truncate `str(error)` to 500 chars

### 2.13 Debug Dump on Failure

On non-retryable errors or max retries:
- Writes JSON file to logs directory with request/response details
- Masked API key (first 8 + last 4 chars)

---

## 3. Fallback Chain and Credential Pool

### 3.1 Fallback Chain Structure

List of (model, provider, api_key, base_url) tuples in `config.yaml` under `fallback_models`.

### 3.2 Fallback Activation

`_try_activate_fallback()`:
- Iterate through fallback chain, resolve credentials, build client
- On success: update `_fallback_activated = True`, return True

### 3.3 Primary Runtime Restoration

At start of each new turn:
- If `_fallback_activated` is true, restore `_primary_runtime` snapshot
- Gives preferred model a fresh attempt each turn

### 3.4 Fallback vs Credential Pool Interaction

When rate-limited:
- Check if credential pool has available credentials
- If yes: don't eagerly fallback (pool's retry-then-rotate cycle needs to fire)
- If no: eagerly switch to fallback provider

---

## 4. Budget System

### 4.1 IterationBudget

Thread-safe budget with atomic operations:
- `consume()`: atomically increment `used`, return True if within budget
- `refund()`: atomically decrement `used` (for compression restarts)

### 4.2 Grace Call Pattern

When budget exhausted:
1. Set `_budget_grace_call = True`
2. Next iteration: consume grace flag (don't call `consume()`)
3. Loop exits after this iteration regardless of outcome

### 4.3 Budget Refund on Compression

When context compression triggers restart:
- `api_call_count -= 1`, `iteration_budget.refund()`
- `retry_count += 1` (prevent infinite loops)

---

## 5. Context Compression

### 5.1 Context Engine Selection

Config-driven: `context.engine` in config.yaml (default: "compressor")
- Try plugins/context_engine/<name>/, then general plugin system
- Fall back to built-in ContextCompressor

### 5.2 Context Length Resolution

Priority: config.yaml model.context_length > custom_providers per-model > auto-detection > default 128K

### 5.3 Minimum Context Length

Reject models with context window below 64K tokens.

### 5.4 Compression Configuration

```yaml
compression:
  enabled: true
  threshold: 0.50
  target_ratio: 0.20
  protect_last_n: 20
```

### 5.5 Compression Feasibility Check

At init time, verify auxiliary compression model can handle content:
- If aux context < threshold: warn with fix options
- Warning stored and replayed through status_callback on first run_conversation

### 5.6 Context Pressure Tiered Warnings

- At 85% of compaction threshold: first warning
- At 95% of compaction threshold: second warning

---

## 6. Message Sanitization

### 6.1 Surrogate Character Sanitization

Two-level recovery for UnicodeEncodeError:
- Pass 1: Strip lone surrogates (U+D800..U+DFFF) from clipboard paste
- Pass 2: If ASCII codec, strip all non-ASCII from messages, prefill, tools, system prompt, headers, API key
- Maximum 2 passes

### 6.2 User Input Sanitization

At `run_conversation()` entry: sanitize surrogates from user_message and persist_user_message.

### 6.3 Tool Call Sanitization for Strict APIs

For providers that reject Codex-specific fields: remove `call_id`, `response_item_id` from tool calls.

### 6.4 API Message Sanitization

`_sanitize_api_messages()`: strips orphaned tool results, adds stubs for missing results.

---

## 7. API Call Execution

### 7.1 Streaming vs Non-Streaming

Always prefers streaming:
- 90s stale-stream detection, 60s read timeout
- Falls back to non-streaming if provider doesn't support it

### 7.2 Stream Callbacks

`_interruptible_streaming_api_call()`:
- `on_first_delta` callback fires on first token
- Stale stream detection: 90s without data -> error

### 7.3 Ollama Context Injection

Detect via `is_local_endpoint(base_url)`:
- Query `/api/show` for model's max context
- Pass `num_ctx` on every chat request
- User override: `model.ollama_num_ctx` in config.yaml

### 7.4 API Mode Detection

1. Provider == "openai-codex" -> codex_responses
2. Provider == "anthropic" or base_url contains "api.anthropic.com" -> anthropic_messages
3. Base_url ends with "/anthropic" -> anthropic_messages
4. GPT-5.x models -> codex_responses
5. Default -> chat_completions

### 7.5 Max Tokens Parameter

- Direct OpenAI URL -> `max_completion_tokens`
- OpenRouter, local models -> `max_tokens`

### 7.6 Token Usage Tracking

On successful response:
- `normalize_usage()` canonicalizes usage across providers/API modes
- Update session cumulative counters
- `estimate_usage_cost()` for cost estimation
- Update session DB token counts

---

## 8. Session Persistence

### 8.1 Dual-Path Persistence

1. JSON log file: atomic write to session log
2. SQLite database: incremental message flush with dedup

### 8.2 JSON Session Log

Format includes: session_id, model, base_url, platform, timestamps, system_prompt, tools, messages
- Assistant content cleaned: REASONING_SCRATCHPAD converted to think tags
- Guard: never overwrite larger log with fewer messages

### 8.3 SQLite Message Flush

`_flush_messages_to_session_db()`:
- Tracks `_last_flushed_db_idx` to prevent duplicate writes
- Includes: role, content, tool_name, tool_calls, tool_call_id, finish_reason, reasoning, reasoning_details
- Ensures session row exists via `ensure_session()` (INSERT OR IGNORE)

### 8.4 Persisted System Prompt

- System prompt snapshot stored in SQLite via `update_system_prompt()`
- Continuing sessions load stored prompt from DB to match Anthropic cache prefix

### 8.5 Persistence Skip on Context Overflow

When status 400 + large session (approx_tokens > 50000 or messages > 80):
- Skip session persistence to prevent growth loop

---

## 9. Interrupt System

### 9.1 Thread-Scoped Interrupts

`interrupt(message)`:
- Sets `_interrupt_requested = True` and `_interrupt_message = message`
- Scopes interrupt to agent's execution thread
- Propagates to all active child agents (subagent delegation)

### 9.2 Interrupt Propagation to Subagents

When `interrupt()` called:
1. Set interrupt flag on parent
2. Signal tool-level interrupt for parent's thread
3. Copy `_active_children` list under lock
4. For each child: `child.interrupt(message)` -> recursive propagation

### 9.3 Interrupt Points

Checked at:
- Start of each outer loop iteration -> break immediately
- During retry backoff waits (every 200ms) -> abort retry
- During API call (via InterruptedError) -> catch, persist session, break
- During error handling -> abort retries

### 9.4 Interrupt Clearing

`clear_interrupt()`:
- Reset `_interrupt_requested` and `_interrupt_message`
- Clear tool-level interrupt signal for agent's thread
- Called at start of each `run_conversation()`

---

## 10. Plugin Hook System

### 10.1 Hook Types and Timing

| Hook | When Fired | Purpose |
|------|-----------|---------|
| on_session_start | New session created | Initialize session-scoped state |
| pre_llm_call | Before tool-calling loop | Inject context into user message |
| pre_api_request | Before each API call | Logging, debugging, metrics |
| post_api_request | After API response | Token usage reporting |

### 10.2 Hook Invocation Pattern

- All plugin results collected
- Exceptions caught per-plugin (one failing plugin doesn't break others)
- Results aggregated (strings joined, dict contexts merged)

### 10.3 Context Injection Target

Plugin context ALWAYS goes into user message, never system prompt:
- System prompt modifications break Anthropic cache prefix
- Plugin context is ephemeral (not persisted to session DB)

---

## 11. Memory Provider Integration

### 11.1 Built-in Memory Store

`MemoryStore`: MEMORY.md + USER.md files on disk, memory_enabled and user_profile_enabled flags from config.

### 11.2 External Memory Provider Plugin

Plugin-based: config `memory.provider` (honcho, mem0, supermemory, byterover, retaindb, holographic)
- Auto-migrate: if Honcho configured but no provider set, auto-activate honcho
- Provider tool schemas injected into agent's tool list

### 11.3 Memory Nudge Logic

Turn-based nudge: `_memory_nudge_interval` from config (default 10 turns)

### 11.4 Skill Nudge Logic

Iteration-based nudge: `_skill_nudge_interval` from config (default 10)

### 11.5 Background Review System

`_spawn_background_review()`:
- Spawns daemon thread with full AIAgent fork
- Same model, tools, context as main session
- stdout/stderr redirected to /dev/null
- Scans review agent's messages for successful tool actions
- Compact summary printed to user

---

## 12. Context Engine Plugin System

### 12.1 Engine Architecture

Default: built-in `ContextCompressor`. Config: `context.engine` in config.yaml.

### 12.2 Plugin Engine Initialization

If engine != "compressor": try plugins/context_engine/<name>/, then general plugin system, then fall back.

### 12.3 Context Engine Lifecycle

`update_model()`, `get_tool_schemas()`, `on_session_start()`, `on_session_reset()`, `on_session_end()`

---

## 13. Ollama Context Injection

Detect via `is_local_endpoint(base_url)`:
- Query `/api/show` for model's max context
- Pass `num_ctx` on every chat request
- User override: `model.ollama_num_ctx` in config.yaml

---

## 14. Prompt Caching

### 14.1 Auto-Detection

`_use_prompt_caching` enabled when:
- OpenRouter + Claude model
- Native Anthropic: api_mode == "anthropic_messages" AND provider == "anthropic"

### 14.2 Cache Control Application

`apply_anthropic_cache_control()`:
- System prompt: cache_control breakpoint
- Last 3 messages: cache_control breakpoint
- Strategy: system_and_3 - cache system prompt + last 3 messages (4 breakpoints total)

### 14.3 Fine-Grained Tool Streaming

For Claude via OpenRouter:
- Header: `x-anthropic-beta: fine-grained-tool-streaming-2025-05-14`
- Required for tool-use with Claude models on OpenRouter

---

## 15. Model Switching

### 15.1 In-Place Model Switch

`switch_model(new_model, new_provider, api_key, base_url, api_mode)`:
1. Determine api_mode if not provided
2. Swap core runtime fields
3. Build new client (Anthropic or OpenAI-compatible)
4. Re-evaluate prompt caching flags
5. Update context compressor
6. Invalidate cached system prompt
7. Update `_primary_runtime` snapshot
8. Reset fallback state

### 15.2 Anthropic Client Building

For `anthropic_messages` mode:
- Only fall back to ANTHROPIC_TOKEN when provider is actually Anthropic
- Other anthropic_messages providers must use their own API key
- OAuth token detection

### 15.3 Runtime Snapshot

`_primary_runtime` dict stores: model, provider, base_url, api_mode, api_key, client_kwargs, use_prompt_caching, compressor state, Anthropic-specific fields

---

## 16. Background Review System

### 16.1 Review Triggers

Two independent triggers:
- Memory review: every `_memory_nudge_interval` user turns
- Skill review: every `_skill_nudge_interval` tool-calling iterations per turn

### 16.2 Review Agent Configuration

Forked agent: same model/provider/tools, max_iterations=8, quiet_mode=True, stdout/stderr redirected to /dev/null

### 16.3 Review Result Processing

After review agent completes:
- Scan session messages for tool results with success: true
- Extract action descriptions: "created", "updated", "added", "removed", "replaced"
- Deduplicate, format summary for user

---

## 17. Trajectory Format

### 17.1 Conversion

`_convert_to_trajectory_format()`:
- System message with tool definitions in XML tools format
- User message from original query
- Assistant messages with: reasoning wrapped in think tags, tool calls in XML tags, content cleaned
- Tool responses: XML result tags with JSON content
- Every assistant turn must have think tags (empty if no reasoning)

### 17.2 Saving

`_save_trajectory()`: converts messages to trajectory format, writes to JSONL file
- Used for training data generation
- Atomic writes with temp file + rename

---

## 18. Rate Limit Tracking

### 18.1 Header Parsing

`_capture_rate_limits()`:
- Called after each streaming API call
- Parses x-ratelimit-* headers from HTTP response
- Uses `parse_rate_limit_headers()` from `agent/rate_limit_tracker.py`
- State cached in `_rate_limit_state`

### 18.2 Rate Limit State Access

`get_rate_limit_state()`: returns last captured RateLimitState, or None
- Used by gateway to report rate limit status to users

---

## 19. Activity Monitoring

### 19.1 Activity Tracking

`_touch_activity(desc)`:
- Updates `_last_activity_ts` and `_last_activity_desc` (thread-safe)
- Called at key points: API call start, API call complete, error recovery, backoff waits

### 19.2 Activity Summary

`get_activity_summary()`:
- Returns snapshot: last_activity_ts, last_activity_desc, seconds_since_activity, current_tool, api_call_count, budget used/max
- Called by gateway timeout handler and periodic "still working" notifications

---

## 20. Response Normalization

### 20.1 Codex Responses

`_normalize_codex_response()`:
- Converts Codex Responses API format to assistant message
- Extracts output items, output_text, reasoning items
- Returns (assistant_message, finish_reason)

### 20.2 Anthropic Messages

`normalize_anthropic_response()`:
- Converts Anthropic API response to assistant message
- Extracts content blocks (text, tool_use)
- `strip_tool_prefix` for OAuth clients
- Returns (assistant_message, finish_reason)

### 20.3 Chat Completions

Standard OpenAI format: `response.choices[0].message`

### 20.4 Content Normalization

After response normalization:
- If content is not a string (dict or list):
  - Dict: extract `text` or `content` key, or json.dumps
  - List: extract text parts from multimodal content
  - Fallback: str(content)

---

## 21. Thinking Block Handling

### 21.1 Think Block Stripping

`_strip_think_blocks()`:
- Removes all reasoning tag variants: think, thinking, THINKING, reasoning, REASONING_SCRATCHPAD, thought
- Case-insensitive for some tags
- Returns only visible text

### 21.2 Content After Think Block Check

`_has_content_after_think_block()`:
- Strips all think blocks
- Checks if any non-whitespace content remains
- Used to detect thinking-budget exhaustion

### 21.3 Reasoning Extraction

`_extract_reasoning()`:
- Extracts reasoning from multiple provider formats:
  - `message.reasoning` (DeepSeek, Qwen, etc.)
  - `message.reasoning_content` (Moonshot AI, Novita)
  - `message.reasoning_details` array (OpenRouter unified)
  - Inline think blocks in content (fallback)
- Combines all parts with double newlines

### 21.4 Session Content Cleaning

`_clean_session_content()`:
- Converts REASONING_SCRATCHPAD to think tags
- Cleans up whitespace around think blocks

---

## 22. Stream Consumer System

### 22.1 Stream Consumer Registration

Stream consumers registered for:
- CLI TUI display (token-by-token rendering)
- TTS pipeline (streaming audio generation)
- Gateway platform callbacks

### 22.2 Stream Consumer Detection

`_has_stream_consumers()`:
- Returns True if any display or TTS consumers registered
- Affects: thinking spinner display, vprint suppression, streaming path selection

### 22.3 Stream Delivery Tracking

`_reset_stream_delivery_tracking()`:
- Called before each API call
- Used for stale stream detection (90s without data)

---

## 23. Quiet Mode and Spinner

### 23.1 Safe Print

`_safe_print()`:
- Handles broken pipes / closed stdout
- In headless environments (systemd, Docker, nohup) stdout may become unavailable
- Routes through `_print_fn` for custom rendering (prompt_toolkit ANSI handling)

### 23.2 Verbose Print

`_vprint()`:
- Suppressed when actively streaming tokens
- `force=True` for error/warning messages always shown
- Allowed during tool execution (no tokens streaming)
- Suppressed in `suppress_status_output` mode (CLI automation)
- Suppressed after main response delivered (`_mute_post_response`)

### 23.3 Quiet Mode Spinner

`_should_start_quiet_spinner()`:
- Allow quiet-mode spinner only when:
  - Output rerouted via `_print_fn`, OR
  - stdout is a real TTY
- Prevents corrupting protocol streams (ACP JSON-RPC)

### 23.4 KawaiiSpinner

- Animated thinking spinner with kawaii faces
- Random face selection from KAWAII_THINKING frames
- Random verb selection from THINKING_VERBS
- Multiple spinner types: brain, sparkle, pulse, moon, star

### 23.5 Thinking Callback

`thinking_callback`:
- CLI TUI mode uses callback instead of raw spinner
- Works in both streaming and non-streaming modes
- Empty string callback stops the spinner

---

## 24. Status Emission

### 24.1 Status Callback

`_emit_status(message)`:
- Emits lifecycle status to both CLI and gateway channels
- CLI: `_vprint(force=True)` - always visible
- Gateway: `status_callback("lifecycle", message)`
- Never raises - exceptions swallowed

### 24.2 Status Callback Types

Multiple callback types registered:
- `status_callback("lifecycle", message)` - lifecycle events
- `step_callback(iteration, previous_tools)` - per-iteration status
- `tool_progress_callback(message)` - tool execution progress
- `background_review_callback(summary)` - background review results
- `thinking_callback(message)` - thinking spinner control

---

*Generated from deep source analysis of Hermes Agent Python codebase.*