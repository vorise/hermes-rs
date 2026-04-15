# Hermes Agent — Messaging Platforms

This document covers all messaging platform adapters in the Hermes Agent gateway system.

---

## Table of Contents

1. [Gateway Architecture](#1-gateway-architecture)
2. [Base Platform Adapter](#2-base-platform-adapter)
3. [Platform Adapters](#3-platform-adapters)
4. [Session Routing](#4-session-routing)
5. [Message Formatting](#5-message-formatting)

---

## 1. Gateway Architecture

### Overview

The messaging gateway (`gateway/`) is a persistent process that connects to multiple messaging platforms simultaneously. A single `hermes gateway start` process can serve Telegram, Discord, Slack, WhatsApp, Signal, and 12+ other platforms at once.

### Location

`gateway/` — main module

### Key Files

| File | Purpose |
|------|---------|
| `gateway/run.py` | Main loop, platform dispatch, slash commands |
| `gateway/session.py` | SessionStore — conversation persistence |
| `gateway/config.py` | Gateway configuration |
| `gateway/stream_consumer.py` | Stream consumer for SSE-like platforms |
| `gateway/status.py` | Gateway status reporting |
| `gateway/delivery.py` | Message delivery abstraction |
| `gateway/pairing.py` | DM pairing system |
| `gateway/hooks.py` | Gateway hooks |
| `gateway/session_context.py` | Session context tracking |
| `gateway/channel_directory.py` | Channel tracking |
| `gateway/display_config.py` | Platform-specific display config |
| `gateway/sticker_cache.py` | Sticker/emoji caching |
| `gateway/restart.py` | Gateway restart management |
| `gateway/mirror.py` | Channel mirroring |

### Data Flow

```
┌──────────────────────────────────────────────────────────────┐
│                    Messaging Platforms                         │
│  Telegram │ Discord │ Slack │ WhatsApp │ Signal │ Matrix │... │
└────────────────────┬─────────────────────────────────────────┘
                     │
┌────────────────────▼──────────────────────────────────────┐
│                   Gateway Runner (run.py)                    │
│  Platform adapters → message dispatch → AIAgent            │
│  Slash command routing → session management → delivery     │
└────────────────────┬───────────────────────────────────────┘
                     │
┌────────────────────▼──────────────────────────────────────┐
│                   AIAgent (run_agent.py)                    │
│  Conversation loop → tool execution → response streaming   │
└────────────────────┬───────────────────────────────────────┘
                     │
┌────────────────────▼──────────────────────────────────────┐
│               Session Store (session.py)                   │
│  Per-platform session management → SQLite persistence      │
└───────────────────────────────────────────────────────────┘
```

### Platform Lifecycle

1. **Startup** — Load config, initialize adapters, connect to platforms
2. **Message received** — Platform adapter → GatewayRunner → AIAgent
3. **Response streaming** — AIAgent → GatewayRunner → Platform adapter → User
4. **Session management** — Each platform-user pair gets a session
5. **Shutdown** — Graceful disconnect from all platforms

---

## 2. Base Platform Adapter

### Location

`gateway/platforms/base.py`

### Purpose

Abstract base class that all platform adapters inherit from. Defines the interface for message sending, receiving, and platform-specific features.

### Interface

| Method | Purpose |
|--------|---------|
| `connect()` | Connect to platform |
| `disconnect()` | Disconnect from platform |
| `send_message()` | Send text message |
| `send_file()` | Send file/image/audio |
| `send_animation()` | Send GIF/animation |
| `send_voice()` | Send voice message |
| `send_sticker()` | Send sticker/emoji |
| `edit_message()` | Edit existing message |
| `delete_message()` | Delete message |
| `is_typing()` | Show typing indicator |
| `handle_message()` | Process incoming message |
| `handle_command()` | Process slash command |
| `get_user_id()` | Get user identifier |
| `get_channel_id()` | Get channel identifier |
| `is_dm()` | Check if direct message |

---

## 3. Platform Adapters

### Telegram

**File:** `gateway/platforms/telegram.py`
**Network:** `gateway/platforms/telegram_network.py`

#### Purpose

Telegram bot integration. Primary messaging platform for Hermes.

#### Features

- Bot API integration
- Long polling or webhook mode
- Markdown/HTML message formatting
- Inline keyboard support
- File/photo/document/voice/animation sending
- Reply-to-message support
- DM pairing
- Group/channel support
- Sticker/emoji caching
- Typing indicators
- Message editing for streaming updates
- TgNet (Telegram Network) support for distributed deployments

#### Configuration

```yaml
platforms:
  telegram:
    enabled: true
    bot_token: "..."
    # Optional
    webhook_url: "https://..."  # for webhook mode
    webhook_port: 8443
```

---

### Discord

**File:** `gateway/platforms/discord.py`

#### Purpose

Discord bot integration.

#### Features

- discord.py integration
- Embed messages for rich formatting
- File/attachment sending
- Native animation (GIF) playback
- Voice channel support
- Sticker/emoji sending
- Slash commands
- Typing indicators
- DM support
- Channel support (text channels)
- Reaction support

#### Configuration

```yaml
platforms:
  discord:
    enabled: true
    bot_token: "..."
```

---

### Slack

**File:** `gateway/platforms/slack.py`

#### Purpose

Slack bot integration.

#### Features

- slack-bolt integration
- Block Kit message formatting
- File sharing
- Thread support
- DM support
- Channel support
- Emoji reactions

#### Configuration

```yaml
platforms:
  slack:
    enabled: true
    bot_token: "..."
    app_token: "..."
```

---

### WhatsApp

**File:** `gateway/platforms/whatsapp.py`

#### Purpose

WhatsApp messaging via CalyxOS-compatible bridge.

#### Features

- Message sending/receiving
- Media support (images, documents, voice)
- DM support only
- Cross-platform conversation continuity

#### Configuration

```yaml
platforms:
  whatsapp:
    enabled: true
    # Bridge configuration
```

---

### Signal

**File:** `gateway/platforms/signal.py`

#### Purpose

Signal messaging via signal-cli.

#### Features

- End-to-end encrypted messaging
- Media support
- DM support
- Group support

#### Configuration

```yaml
platforms:
  signal:
    enabled: true
    phone_number: "+1234567890"
```

---

### Matrix

**File:** `gateway/platforms/matrix.py`

#### Purpose

Matrix messaging via mautrix.

#### Features

- End-to-end encryption support
- Media support
- DM and room support
- Typing indicators

#### Configuration

```yaml
platforms:
  matrix:
    enabled: true
    homeserver: "https://..."
    access_token: "..."
```

---

### Home Assistant

**File:** `gateway/platforms/homeassistant.py`

#### Purpose

Home Assistant notification/assistant integration.

#### Features

- Notification delivery
- Entity state queries
- Service calls
- Event handling

#### Configuration

```yaml
platforms:
  homeassistant:
    enabled: true
    url: "http://..."
    token: "..."
```

---

### Webhook

**File:** `gateway/platforms/webhook.py`

#### Purpose

Generic webhook adapter for custom integrations.

#### Features

- HTTP POST webhook
- Custom payload parsing
- Response delivery

---

### BlueBubbles

**File:** `gateway/platforms/bluebubbles.py`

#### Purpose

iMessage integration via BlueBubbles server.

#### Features

- iMessage sending/receiving
- Media support
- Tapback reactions
- Thread support

---

### DingTalk

**File:** `gateway/platforms/dingtalk.py`

#### Purpose

DingTalk (钉钉) integration.

#### Features

- Message sending/receiving
- File support
- Group support

---

### Feishu

**File:** `gateway/platforms/feishu.py`

#### Purpose

Feishu (飞书/Lark) integration.

#### Features

- Rich text messages
- File support
- Group support

---

### QQ Bot

**File:** `gateway/platforms/qqbot.py`

#### Purpose

QQ Bot integration.

#### Features

- Message sending/receiving
- Media support
- Group support

---

### WeCom

**File:** `gateway/platforms/wecom_*.py` (3 files)

#### Purpose

WeCom (企业微信/WeChat Work) integration.

#### Features

- Enterprise messaging
- Callback handling
- Message encryption/decryption

---

### WeChat

**File:** `gateway/platforms/weixin.py`

#### Purpose

WeChat integration.

---

### Mattermost

**File:** `gateway/platforms/mattermost.py`

#### Purpose

Mattermost integration.

#### Features

- Message sending/receiving
- Channel support
- Thread support

---

### SMS

**File:** `gateway/platforms/sms.py`

#### Purpose

SMS messaging.

#### Features

- SMS sending/receiving
- MMS support

---

### Email

**File:** `gateway/platforms/email.py`

#### Purpose

Email integration.

#### Features

- Email sending/receiving
- HTML/plain text
- Attachment support

---

### REST API Server

**File:** `gateway/platforms/api_server.py`

#### Purpose

REST API endpoint for programmatic access.

#### Features

- HTTP API for message sending
- Webhook for message receiving
- Authentication

---

## 4. Session Routing

### Session Store

`gateway/session.py` — manages per-platform sessions.

#### Features

- Session creation per platform-user pair
- Message history loading from SQLite
- Context compression awareness
- Session persistence

### Session Context

`gateway/session_context.py` — tracks session metadata.

#### Features

- Current turn tracking
- Platform user ID
- Channel tracking
- Last activity timestamp

---

## 5. Message Formatting

### Platform-Specific Formatting

`gateway/display_config.py` — platform-specific display settings.

| Platform | Markdown | HTML | Embeds | Files |
|----------|----------|------|--------|-------|
| Telegram | ✓ (MarkdownV2) | ✓ | ✓ | ✓ |
| Discord | ✓ (Markdown) | | ✓ | ✓ |
| Slack | ✓ (Mrkdwn) | | ✓ | ✓ |
| WhatsApp | ✓ (limited) | | | ✓ |
| Signal | ✓ (limited) | | | ✓ |
| Matrix | ✓ (HTML) | ✓ | | ✓ |
| Email | | ✓ | | ✓ |

### Stream Consumer

`gateway/stream_consumer.py` — handles streaming responses from the AIAgent.

#### Features

- Text delta buffering
- Tool execution notifications
- Typing indicator management
- Message editing for streaming updates
- Rate limit handling

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Total platform adapters | 18+ |
| Supported platforms | Telegram, Discord, Slack, WhatsApp, Signal, Matrix, Home Assistant, BlueBubbles, DingTalk, Feishu, QQ Bot, WeCom, WeChat, Mattermost, SMS, Email, Webhook, REST API |
| Max concurrent platforms | Unlimited (single gateway) |
| Session routing | Per platform-user pair |

---

*Generated from source analysis of the Hermes Agent codebase.*
