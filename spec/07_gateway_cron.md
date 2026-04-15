# Hermes Agent — Gateway, Cron & Session Management

This document covers the gateway main loop, cron scheduler, session store, and stream consumer.

---

## Table of Contents

1. [GatewayRunner — Main Loop](#1-gatewayrunner--main-loop)
2. [Gateway Configuration](#2-gateway-configuration)
3. [Session Store](#3-session-store)
4. [Stream Consumer](#4-stream-consumer)
5. [Cron Scheduler](#5-cron-scheduler)
6. [Cron Jobs](#6-cron-jobs)
7. [Message Delivery](#7-message-delivery)
8. [DM Pairing](#8-dm-pairing)
9. [Gateway Hooks](#9-gateway-hooks)
10. [Gateway Status](#10-gateway-status)
11. [Session Context](#11-session-context)
12. [Channel Directory](#12-channel-directory)
13. [Display Config](#13-display-config)

---

## 1. GatewayRunner — Main Loop

### Location

`gateway/run.py` (~443K lines)

### Purpose

Main gateway process. Manages the lifecycle of all platform adapters, dispatches messages to the AIAgent, and routes responses back to platforms.

### Key Responsibilities

1. **Platform initialization** — Load and connect all configured platforms
2. **Message dispatch** — Route incoming messages to the AIAgent
3. **Slash command routing** — Handle `/command` in messaging platforms
4. **Session management** — Create/manage sessions per platform-user pair
5. **Response streaming** — Stream agent responses back to platforms
6. **Lifecycle management** — Graceful startup/shutdown

### Message Dispatch Flow

```
Platform receives message
     ↓
Platform adapter parses message
     ↓
Is it a slash command? → Yes → handle_command()
     ↓ No
Create/get session for platform-user pair
     ↓
Load conversation history from SQLite
     ↓
Create AIAgent with platform context
     ↓
Call AIAgent.run_conversation()
     ↓
Stream response back to platform
     ↓
Save session to SQLite
```

### Slash Command Handling in Gateway

| Command | Gateway Behavior |
|---------|-----------------|
| `/new` | Clear session, create new one |
| `/reset` | Same as /new |
| `/model` | Switch model, update config |
| `/compress` | Trigger context compression |
| `/usage` | Show session token/cost usage |
| `/insights` | Show usage analytics |
| `/stop` | Interrupt current tool execution |
| `/undo` | Remove last turn from history |
| `/retry` | Re-send last user message |
| `/tools` | List/enable/disable tools |
| `/skills` | Browse/search skills |
| `/memory` | View/manage memory |
| `/personality` | Set personality |
| `/status` | Show gateway status |
| `/sethome` | Set current channel as home |
| `/help` | Show help |
| `/platforms` | Show platform status |

### Connection Management

- SSL certificate auto-detection (NixOS, non-standard systems)
- Dead connection cleanup
- Graceful shutdown on SIGTERM/SIGINT
- Platform reconnect on failure

---

## 2. Gateway Configuration

### Location

`gateway/config.py` (~53K lines)

### Purpose

Gateway-specific configuration separate from the main `~/.hermes/config.yaml`.

### Key Settings

| Setting | Purpose |
|---------|---------|
| Platform enable/disable | Which platforms to connect |
| Platform credentials | Bot tokens, API keys |
| Webhook settings | Webhook URLs and ports |
| Session settings | Session limits, compression |
| Delivery settings | Message formatting preferences |
| Cron settings | Scheduler configuration |

---

## 3. Session Store

### Location

`gateway/session.py` (~42K lines)

### Purpose

Manages per-platform conversation sessions in the gateway context.

### Key Features

- Session creation per platform-user pair
- Message history loading from SQLite
- Context compression awareness
- Session persistence
- Session expiration/cleanup

### Key Methods

| Method | Purpose |
|--------|---------|
| `get_or_create_session()` | Get existing or create new session |
| `load_history()` | Load conversation history |
| `save_message()` | Save message to session |
| `clear_session()` | Clear session history |
| `get_session_id()` | Get session identifier |

---

## 4. Stream Consumer

### Location

`gateway/stream_consumer.py` (~34K lines)

### Purpose

Handles streaming responses from the AIAgent and delivers them to platforms in real-time.

### Key Features

- Text delta buffering
- Tool execution notifications
- Typing indicator management
- Message editing for streaming updates
- Rate limit handling
- Platform-specific delivery formatting

### Streaming Flow

```
AIAgent generates text delta
     ↓
StreamConsumer receives delta
     ↓
Buffer and batch deltas
     ↓
Send typing indicator to platform
     ↓
Send/edit message with accumulated text
     ↓
Tool use detected → show tool notification
     ↓
Tool result → continue streaming
```

---

## 5. Cron Scheduler

### Location

`cron/scheduler.py`

### Purpose

Scheduled automation engine. Runs jobs on cron schedules and delivers results to any platform.

### Key Features

- Standard cron expression support
- Natural language scheduling
- Delivery to any connected platform
- Job execution history
- Error handling and retry
- Job enable/disable

### Schedule Examples

| Schedule | Expression | Purpose |
|----------|------------|---------|
| Daily report | `0 9 * * *` | Daily summary at 9am |
| Hourly check | `0 * * * *` | Every hour |
| Weekly audit | `0 0 * * 1` | Monday midnight |
| Every 5 min | `*/5 * * * *` | Every 5 minutes |

---

## 6. Cron Jobs

### Location

`cron/jobs.py`

### Purpose

Cron job definitions and execution.

### Key Features

- Job creation from natural language
- Job scheduling
- Job execution with AIAgent
- Result delivery to configured platform
- Job history tracking

---

## 7. Message Delivery

### Location

`gateway/delivery.py` (~9K lines)

### Purpose

Abstracts message delivery across platforms.

### Key Features

- Platform-specific formatting
- File delivery
- Multi-part messages
- Delivery confirmation
- Retry on failure

---

## 8. DM Pairing

### Location

`gateway/pairing.py` (~11K lines)

### Purpose

DM pairing system for platforms that support group/channel access but need DM-only sessions.

### Key Features

- Pair code generation
- DM-only mode enforcement
- User verification
- Cross-platform pairing

---

## 9. Gateway Hooks

### Location

`gateway/hooks.py` (~6K lines)

### Purpose

Gateway hook system for custom event handling.

### Key Features

- Pre-message hooks
- Post-message hooks
- Tool execution hooks
- Custom plugin integration

---

## 10. Gateway Status

### Location

`gateway/status.py` (~15K lines)

### Purpose

Gateway status reporting and health checks.

### Key Features

- Platform connection status
- Session count tracking
- Uptime monitoring
- Resource usage
- Error rate tracking

---

## 11. Session Context

### Location

`gateway/session_context.py` (~4K lines)

### Purpose

Tracks session metadata and context within the gateway.

### Key Features

- Current turn tracking
- Platform user ID
- Channel tracking
- Last activity timestamp

---

## 12. Channel Directory

### Location

`gateway/channel_directory.py` (~9K lines)

### Purpose

Tracks channels across platforms for routing and management.

### Key Features

- Channel registration
- Platform tracking
- Channel metadata

---

## 13. Display Config

### Location

`gateway/display_config.py` (~7K lines)

### Purpose

Platform-specific display configuration.

### Key Features

- Markdown/HTML formatting support per platform
- File size limits
- Message length limits
- Emoji/sticker support flags

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Gateway main file | ~443K lines |
| Platform adapters | 18+ |
| Cron scheduler | Yes |
| Session store | SQLite-based |
| Stream consumer | Real-time |
| DM pairing | Supported |

---

*Generated from source analysis of the Hermes Agent codebase.*
