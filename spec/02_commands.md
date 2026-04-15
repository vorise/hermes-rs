# Hermes Agent — Commands Reference

This document is an exhaustive reference for every slash command in Hermes Agent, derived from the source in `hermes_cli/commands.py`, `hermes_cli/main.py`, and shared command modules.

---

## Table of Contents

1. [Command System Architecture](#1-command-system-architecture)
2. [SlashCommandCompleter](#2-slashcommandcompleter)
3. [Command Dispatch](#3-command-dispatch)
4. [Individual Command Reference](#4-individual-command-reference)

---

## 1. Command System Architecture

### How Commands Work

Hermes uses a shared command system that works across both CLI and messaging platforms:

1. **CLI mode** (`hermes`): User types `/command` in the prompt_toolkit TUI
2. **Messaging mode** (Telegram, Discord, etc.): User sends `/command` as a message
3. Both routes converge on the same command handler logic

### Command Registration

Commands are defined in `hermes_cli/commands.py` with:
- Command name (without leading `/`)
- Description for autocomplete
- Optional parameters
- Handler function or method

### Shared vs Platform-Specific Commands

Most commands are shared between CLI and messaging platforms. Some commands are only available in specific contexts.

---

## 2. SlashCommandCompleter

### Purpose

Provides autocomplete suggestions as the user types `/` in the CLI TUI.

### Location

`hermes_cli/commands.py` — class `SlashCommandCompleter`

### Features

- Fuzzy matching against command names
- Context-aware (some commands only available when relevant)
- Description shown alongside suggestions
- Sorted by frequency/relevance

---

## 3. Command Dispatch

### Flow

```
User input: "/model anthropic/claude-sonnet-4-6"
     ↓
Command detection: starts with "/"
     ↓
Parse: command = "model", args = "anthropic/claude-sonnet-4-6"
     ↓
Lookup in command registry
     ↓
Execute handler with args
     ↓
Return result to user
```

### Error Handling

- Unknown command → show suggestions ("Did you mean...?")
- Invalid args → show usage help
- Permission denied → show error message

---

## 4. Individual Command Reference

### /new / /reset

**Aliases:** `/reset`, `/clear`
**Purpose:** Start a fresh conversation, clearing the current context.
**Args:** None
**Behavior:**
- Saves current session to SQLite
- Creates new session ID
- Clears conversation history
- Rebuilds system prompt

---

### /model

**Purpose:** Switch the current LLM provider and model.
**Args:** `[provider:model]` — optional, shows current model if no args
**Examples:**
- `/model anthropic/claude-sonnet-4-6`
- `/model openrouter/anthropic/claude-3-opus-20240229`
- `/model nous` — switch to Nous Portal default
**Behavior:**
- Validates provider/model combination
- Updates config.yaml
- Rebuilds API client with new endpoint
- Shows model context window info

**Implementation:** `hermes_cli/model_switch.py`

---

### /compress

**Purpose:** Manually trigger context compression.
**Args:** None
**Behavior:**
- Invokes ContextCompressor on current conversation
- Summarizes older turns using auxiliary LLM
- Splits session in SQLite (parent_session_id chain)
- Shows compression summary

---

### /usage

**Purpose:** Show current session token usage and cost.
**Args:** None
**Shows:**
- Input tokens, output tokens, cache read/write tokens
- Estimated cost in USD
- Total API calls made
- Current iteration budget remaining

---

### /insights

**Purpose:** Show usage analytics and insights.
**Args:** `[--days N]` — lookback period (default: 7)
**Shows:**
- Total sessions in period
- Total tokens used
- Total cost
- Most used models
- Most used tools
- Platform breakdown
- Peak usage times

---

### /undo

**Purpose:** Undo the last turn (remove last user message and assistant response).
**Args:** None
**Behavior:**
- Removes last user+assistant exchange from history
- Restores session to previous state
- Can be chained (multiple undos)

---

### /retry

**Purpose:** Retry the last turn with the same input.
**Args:** None
**Behavior:**
- Removes last assistant response
- Re-sends the user message
- Uses current model (may be different if /model was used)

---

### /stop

**Purpose:** Interrupt current tool execution.
**Args:** None
**Behavior:**
- Sets interrupt flag on AIAgent
- Stops current tool loop
- Returns control to user

---

### /tools

**Purpose:** List, enable, or disable tools.
**Args:** `[list|enable|disable] [tool_name]`
**Examples:**
- `/tools` — list all tools with status
- `/tools enable terminal` — enable terminal tool
- `/tools disable browser` — disable browser tool
**Behavior:**
- Updates config.yaml per-platform
- Rebuilds tool definitions on next turn

**Implementation:** `hermes_cli/tools_config.py`

---

### /skills

**Purpose:** Browse, search, or manage skills.
**Args:** `[search|install|view|list|enable|disable] [query]`
**Examples:**
- `/skills` — list installed skills
- `/skills search github` — search for GitHub skills
- `/skills install nous-research/hermes-agent-dev` — install a skill
- `/skills enable github-auth` — enable a skill
**Behavior:**
- Integrates with agentskills.io registry
- Per-platform skill enable/disable
- Skill version management

**Implementation:** `hermes_cli/skills_config.py`, `hermes_cli/skills_hub.py`

---

### /memory

**Purpose:** View or manage persistent memory.
**Args:** `[view|clear|export]`
**Behavior:**
- Shows current memory state
- Can clear all memories
- Export memories to JSON

---

### /personality

**Purpose:** Set the agent's personality.
**Args:** `[name]` — personality name
**Examples:**
- `/personality helpful` — helpful assistant
- `/personality kawaii` — cute/kawaii personality
- `/personality` — show current personality
**Behavior:**
- Updates personality in config
- Rebuilds system prompt on next turn

---

### /status

**Purpose:** Show current session and platform status.
**Args:** None
**Shows:**
- Current model and provider
- Session ID
- Platform (cli, telegram, discord, etc.)
- Iteration budget remaining
- Active tools
- Memory state

---

### /help

**Purpose:** Show help information.
**Args:** `[command]` — specific command help
**Behavior:**
- Shows all commands if no args
- Shows specific command help if command specified

---

### /title

**Purpose:** Set the session title.
**Args:** `[title text]`
**Behavior:**
- Updates session title in SQLite
- Auto-generated title if no args (from first message)

---

### /summarize

**Purpose:** Summarize the current conversation.
**Args:** None
**Behavior:**
- Uses auxiliary LLM to summarize conversation
- Shows summary to user

---

### /export

**Purpose:** Export the current conversation.
**Args:** `[format]` — export format (json, markdown, text)
**Behavior:**
- Exports conversation to file or stdout
- Includes tool calls and results

---

### /speak

**Purpose:** Text-to-speech mode.
**Args:** `[text]`
**Behavior:**
- Converts text to speech using TTS tool
- Plays audio output

---

### /voice

**Purpose:** Toggle voice input mode.
**Args:** None
**Behavior:**
- Starts/stops voice input
- Transcribes speech to text

---

### /platforms

**Purpose:** Show platform status (CLI-only).
**Args:** None
**Shows:**
- Connected messaging platforms
- Gateway status
- Platform-specific settings

---

### /sethome

**Purpose:** Set the home channel for a platform user.
**Args:** None
**Behavior:**
- Sets current channel as home
- Used for cross-platform conversation continuity

---

### /doctor

**Purpose:** Run diagnostics (aliased to `hermes doctor`).
**Args:** None
**Behavior:**
- Checks provider credentials
- Checks tool dependencies
- Checks platform connections
- Reports issues and fixes

---

### /config

**Purpose:** Get/set configuration values (aliased to `hermes config`).
**Args:** `[key] [value]`
**Examples:**
- `/config personality` — show personality
- `/config set personality helpful` — set personality

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Total slash commands | 60+ |
| Commands with autocomplete | All |
| Shared CLI/messaging commands | ~90% |
| Platform-specific commands | ~10% |

---

*Generated from source analysis of the Hermes Agent codebase.*
