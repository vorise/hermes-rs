# Hermes Agent — Platform Adapters (Detailed)

This document covers the platform adapter system in depth: base interface, shared helpers, and key adapter implementations (Telegram, Discord, Slack).

---

## Table of Contents

1. [Base Adapter Interface](#1-base-adapter-interface)
2. [Shared Helpers](#2-shared-helpers)
3. [Telegram Adapter](#3-telegram-adapter)
4. [Discord Adapter](#4-discord-adapter)
5. [Slack Adapter](#5-slack-adapter)
6. [Media Cache System](#6-media-cache-system)
7. [Proxy Support](#7-proxy-support)

---

## 1. Base Adapter Interface

### Location

`gateway/platforms/base.py` (~2000+ lines)

### Purpose

Abstract base class `BasePlatformAdapter` defining the common interface for all 17+ messaging platform adapters. Provides message handling, retry logic, streaming support, media extraction, typing indicators, and lifecycle management.

### 1.1 Core Data Types

```python
class MessageType(Enum):
    TEXT = "text"
    LOCATION = "location"
    PHOTO = "photo"
    VIDEO = "video"
    AUDIO = "audio"
    VOICE = "voice"
    DOCUMENT = "document"
    STICKER = "sticker"
    COMMAND = "command"

class ProcessingOutcome(Enum):
    SUCCESS = "success"
    FAILURE = "failure"
    CANCELLED = "cancelled"

@dataclass
class MessageEvent:
    text: str
    message_type: MessageType = MessageType.TEXT
    source: SessionSource = None
    raw_message: Any = None
    message_id: Optional[str] = None
    media_urls: List[str] = field(default_factory=list)   # local file paths
    media_types: List[str] = field(default_factory=list)
    reply_to_message_id: Optional[str] = None
    reply_to_text: Optional[str] = None
    auto_skill: Optional[str | list[str]] = None          # topic/channel bindings
    internal: bool = False                                # bypasses auth checks
    timestamp: datetime = field(default_factory=datetime.now)

@dataclass
class SendResult:
    success: bool
    message_id: Optional[str] = None
    error: Optional[str] = None
    raw_response: Any = None
    retryable: bool = False   # transient connection failure
```

### 1.2 Abstract Interface

```python
class BasePlatformAdapter(ABC):
    @abstractmethod
    async def connect(self) -> bool: ...
    @abstractmethod
    async def disconnect(self) -> None: ...
    @abstractmethod
    async def send(self, chat_id, content, reply_to=None, metadata=None) -> SendResult: ...
    @abstractmethod
    async def get_chat_info(self, chat_id) -> Dict[str, Any]: ...

    # Optional overrides
    async def edit_message(self, chat_id, message_id, content) -> SendResult: ...
    async def send_typing(self, chat_id, metadata=None) -> None: ...
    async def stop_typing(self, chat_id) -> None: ...
    async def send_image(self, chat_id, image_url, caption=None, ...) -> SendResult: ...
    async def send_animation(self, chat_id, animation_url, ...) -> SendResult: ...
    async def send_voice(self, chat_id, audio_path, ...) -> SendResult: ...
    async def send_video(self, chat_id, video_path, ...) -> SendResult: ...
    async def send_document(self, chat_id, file_path, ...) -> SendResult: ...
    async def send_image_file(self, chat_id, image_path, ...) -> SendResult: ...
    async def play_tts(self, chat_id, audio_path, ...) -> SendResult: ...
```

### 1.3 Message Handling Pipeline

```
handle_message(event):
  1. Build session_key from event.source
  2. Check _active_sessions for existing handler
     a. If active: dispatch commands inline (/approve, /deny, /stop, /new, /reset, /background, /restart)
     b. If active + photo burst: queue photo, don't interrupt
     c. If active: set interrupt event, queue pending message
  3. Mark session active (BEFORE spawning task — closes race window)
  4. Create background task: _process_message_background(event, session_key)
  5. Add task to _background_tasks set with done callbacks
```

### 1.4 Background Processing

`_process_message_background()` lifecycle:

```
1. Start _keep_typing loop (refreshes every 2s, handles typing expiry)
2. Call on_processing_start hook
3. Call _message_handler(event) → response string
4. If interrupted + pending message exists: suppress stale response
5. Extract MEDIA:<path> tags and [[audio_as_voice]] directives
6. Extract image URLs from markdown/HTML
7. Auto-TTS for voice messages (before text, if enabled)
8. Play TTS audio (if generated)
9. Send text portion with _send_with_retry
10. Human-like pacing delay (configurable via HERMES_HUMAN_DELAY_MODE)
11. Send extracted images as native attachments
12. Send extracted media files (route by extension)
13. Send auto-detected local file paths
14. Call on_processing_complete hook
15. Check for pending interrupt messages → recurse
16. Finally: cancel typing task, clean up session tracking
```

### 1.5 Retry Logic

`_send_with_retry()` — 2 retries with exponential backoff:

```python
delay = base_delay * 2^(attempt-1) + random.uniform(0, 1)
```

**Retryable error patterns**: `connecterror`, `connectionerror`, `connectionreset`, `connectionrefused`, `connecttimeout`, `network`, `broken pipe`, `remotedisconnected`, `eoferror`.

**NOT retryable**: `timed out`, `readtimeout`, `writetimeout` — message may have already been delivered.

After all retries: sends delivery-failure notice to user, then tries plain-text fallback for formatting errors.

### 1.6 Message Truncation

`truncate_message()` splits long messages preserving code block boundaries:

1. Walks content for fenced code blocks (``` ... ```)
2. When split falls inside a code block: closes fence at chunk end, reopens with same language tag at next chunk start
3. Avoids splitting inside inline code spans (odd backtick count check)
4. Adds `(1/3)` indicators for multi-chunk messages
5. Supports custom `len_fn` (e.g., `utf16_len` for Telegram's UTF-16 code unit limit)

**UTF-16 length calculation**: `len(s.encode("utf-16-le")) // 2` — emoji and CJK Extension B characters consume 2 code units each.

### 1.7 Media Extraction

**`extract_images()`** — finds markdown `![alt](url)` and HTML `<img src="url">` tags, returns (url, alt_text) pairs and cleaned content.

**`extract_media()`** — finds `MEDIA:<path>` tags and `[[audio_as_voice]]` directives from TTS tool responses.

**`extract_local_files()`** — detects bare local file paths (`/...` or `~/...`) ending in image/video extensions. Skips paths inside code blocks. Validates with `os.path.isfile()`.

### 1.8 Fatal Error Handling

```python
def _set_fatal_error(self, code, message, retryable):
    self._running = False
    self._fatal_error_code = code
    self._fatal_error_message = message
    self._fatal_error_retryable = retryable
    write_runtime_status(platform=self.platform.value, platform_state="fatal", ...)
```

Platform lock acquisition prevents two gateways from using the same token:
- Scope: e.g., `telegram-bot-token`
- Identity: the actual token string
- Metadata: `{'platform': 'telegram'}`

### 1.9 Session Management

```python
self._active_sessions: Dict[str, asyncio.Event]   # session_key → interrupt event
self._pending_messages: Dict[str, MessageEvent]    # session_key → queued message
self._background_tasks: set[asyncio.Task]          # in-flight processing tasks
self._expected_cancelled_tasks: set[asyncio.Task]  # tasks cancelled during shutdown
self._typing_paused: set                           # chats where typing is paused
self._auto_tts_disabled_chats: set                 # chats with /voice off
```

---

## 2. Shared Helpers

### Location

`gateway/platforms/helpers.py` (~260 lines)

### Purpose

Extracted common patterns previously duplicated across 5-7 adapters.

### 2.1 MessageDeduplicator

TTL-based deduplication cache (max 2000 entries, 300s TTL):

```python
def is_duplicate(self, msg_id: str) -> bool:
    if msg_id in self._seen:
        return True
    self._seen[msg_id] = time.time()
    if len(self._seen) > self._max_size:
        # Evict expired entries
        cutoff = now - self._ttl
        self._seen = {k: v for k, v in self._seen.items() if v > cutoff}
    return False
```

Used by: Discord, Slack, DingTalk, WeCom, Weixin, Mattermost, Feishu.

### 2.2 TextBatchAggregator

Aggregates rapid-fire text events into single messages:

```python
TextBatchAggregator(
    handler=self._message_handler,
    batch_delay=0.6,       # normal flush delay
    split_delay=2.0,       # longer delay when last chunk looks like a split
    split_threshold=4000,  # chars that indicate a split message
)
```

Used by: Telegram, Discord, Matrix, WeCom, Feishu. Prevents the agent from processing each chunk of a multi-part message as a separate turn.

### 2.3 strip_markdown()

Pre-compiled regex pipeline stripping markdown for plain-text platforms (SMS, iMessage, Feishu):

- `**bold**` → `bold`
- `_italic_` / `*italic*` → `italic`
- `` ```code``` `` → ``
- `[link](url)` → `link`
- `## Heading` → `Heading`
- Triple+ newlines → double newline

### 2.4 ThreadParticipationTracker

Persistent tracking of threads the bot has participated in:

```python
ThreadParticipationTracker("discord")  # persists to ~/.hermes/discord_threads.json
tracker.mark(thread_id)                # add and save
thread_id in tracker                   # check membership
```

Max 500 tracked threads. Used by: Discord, Matrix.

### 2.5 redact_phone()

Phone number redaction for logging: preserves country code and last 4 digits.

```python
redact_phone("+1234567890") → "+123****7890"
```

---

## 3. Telegram Adapter

### Location

`gateway/platforms/telegram.py` (~1700+ lines)

### 3.1 Connection

Uses `aiogram` (v3) or `pyrogram` / `telethon` (MTProto) based on config:

| Mode | Library | Protocol |
|------|---------|----------|
| Bot API (default) | aiogram 3.x | HTTP Bot API |
| Userbot | pyrogram or telethon | MTProto |

**Proxy support**: Reads `TELEGRAM_PROXY` env var, supports HTTP and SOCKS5. SOCKS uses `aiohttp_socks` with `rdns=True` (remote DNS resolution for GFW bypass).

### 3.2 Message Handling

**Incoming message flow**:
1. Dedup check (event_id)
2. Bot message filtering (`allow_bots` config)
3. Determine chat type (DM vs group)
4. Text batch aggregation (multi-chapter messages)
5. Media download (photo → vision tool, voice → STT, document → file tool)
6. Sticker handling (via sticker cache — vision analysis cached by `file_unique_id`)
7. Skill auto-loading (via DM topics / channel bindings)
8. Build `MessageEvent` → `handle_message()`

**Telegram-specific features**:
- MarkdownV2 formatting with escaping (`_escape_markdown_v2()`)
- Message length limit: 4096 UTF-16 code units
- Reply quoting with entity-based quote markers
- Message editing for streaming/token output
- Persistent typing indicator loop
- Reaction-based progress indicators (👀 → ✅/❌)

### 3.3 Media Handling

| Type | Method | Notes |
|------|--------|-------|
| Photos | `send_photo()` | Downloads to cache for vision tool |
| Voice | `send_voice()` | OGG Opus format required |
| Video | `send_video()` | Native video playback |
| Animation | `send_animation()` | GIF auto-play inline |
| Documents | `send_document()` | File attachment |
| Stickers | Vision + cache | Described via vision tool, cached by `file_unique_id` |

### 3.4 Sticker Cache

When users send stickers, the adapter:
1. Checks sticker cache (`~/.hermes/sticker_cache.json`) for `file_unique_id`
2. If cache miss: downloads sticker, runs vision analysis
3. Caches description with emoji and set_name
4. Injects warm-style text: `[The user sent a sticker 😀 from "MyPack"~ It shows: "A cat waving" (=^.w.^=)]`
5. Animated stickers: `[The user sent an animated sticker {emoji}~ I can't see animated ones yet]`

### 3.5 Text Batch Aggregation

Telegram messages can arrive in multiple chunks (especially long responses split by the platform). The `TextBatchAggregator` waits 0.6s (normal) or 2.0s (split messages ≥4000 chars) before dispatching the batched text to the agent.

---

## 4. Discord Adapter

### Location

`gateway/platforms/discord.py` (~1200+ lines)

### 4.1 Connection

Uses `discord.py` SDK with `commands.Bot()`:

```python
bot = commands.Bot(command_prefix="!", intents=intents, **proxy_kwargs)
```

**Proxy support**: Same pattern as Telegram — `DISCORD_PROXY` env var, SOCKS with `rdns=True`.

### 4.2 Message Handling

**Event handlers**:
- `on_message()` — all messages
- `on_raw_message_edit()` — edited messages
- `on_raw_reaction_add()` — reaction-based approvals
- `on_ready()` — startup, guild enumeration

**Thread handling**:
- Bot participates in threads it creates
- Thread participation tracked via `ThreadParticipationTracker`
- Forum posts treated as threads
- Native GIF animation support (`send_animation` for `.gif` URLs)

### 4.3 Features

- Native GIF animation via `discord.File` with `spoiler=False`
- Message editing for streaming
- Reaction-based approval buttons
- File attachment support for all media types
- Embed-based rich formatting
- Typing indicator (`channel.typing()`)

### 4.4 PII Handling

Discord is **NOT** PII-hashed — raw user/channel IDs needed for `<@user_id>` mentions and thread management.

---

## 5. Slack Adapter

### Location

`gateway/platforms/slack.py` (~1600+ lines)

### 5.1 Connection

Uses `slack-bolt` with Socket Mode:
- Requires two tokens: `SLACK_BOT_TOKEN` (xoxb-...) and `SLACK_APP_TOKEN` (xapp-...)
- Multi-workspace support: comma-separated bot tokens
- Persistent token storage: `~/.hermes/slack_tokens.json`

### 5.2 Message Handling

**Event types**:
- `message` — all channel/DM messages
- `app_mention` — no-op (handled by message event)
- `assistant_thread_started` / `assistant_thread_context_changed` — lifecycle events
- `/hermes` — slash command
- Block Kit actions — approval buttons

**Thread context fetching**: When bot is mentioned mid-thread for the first time (no active session), fetches prior thread messages via `conversations_replies` API. Results cached for 60s.

**Mention gating**:
- `require_mention` config (default: true) — channel messages require @mention
- `free_response_channels` — channels where mention not required
- `allow_bots` — controls bot message processing (`none`/`mentions`/`all`)

### 5.3 Markdown → mrkdwn Conversion

Multi-pass conversion with placeholder protection:
1. Protect fenced code blocks and inline code
2. Convert markdown links → Slack `<url|text>`
3. Protect existing Slack entities
4. Escape control characters (`&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`)
5. Convert headers → bold, bold → `*text*`, italic → `_text_`
6. Restore placeholders in reverse order

### 5.4 Approval Buttons

Block Kit interactive buttons:
- Allow Once / Allow Session / Always Allow / Deny
- User authorization check (`SLACK_ALLOWED_USERS`)
- Atomic pop prevents double-clicks
- Message updated inline to show decision

### 5.5 Multi-Workspace Support

```python
self._team_clients: Dict[str, AsyncWebClient]    # team_id → client
self._team_bot_user_ids: Dict[str, str]           # team_id → bot_user_id
self._channel_team: Dict[str, str]                # channel_id → team_id
```

Each workspace gets its own `AsyncWebClient`. Channel-to-team mapping resolved dynamically.

---

## 6. Media Cache System

### Location

`gateway/platforms/base.py` (cache utilities, ~300 lines)

### 6.1 Three Cache Types

| Cache | Directory | Functions |
|-------|-----------|-----------|
| Images | `~/.hermes/cache/images/` | `cache_image_from_bytes()`, `cache_image_from_url()` |
| Audio | `~/.hermes/cache/audio/` | `cache_audio_from_bytes()`, `cache_audio_from_url()` |
| Documents | `~/.hermes/cache/documents/` | `cache_document_from_bytes()` |

### 6.2 Safety Features

**Image validation**: Magic byte checking before caching (PNG, JPEG, GIF, BMP, WebP signatures). Rejects HTML error pages.

**SSRF protection**: `is_safe_url()` check before downloading. Redirect guard (`_ssrf_redirect_guard`) re-validates each redirect target to prevent redirect-based SSRF attacks.

**Document path traversal**: Filename sanitized via `Path(filename).name`, null bytes stripped, final check: `filepath.resolve().is_relative_to(cache_dir.resolve())`.

**Retry logic**: 2 retries with exponential backoff (1.5s, 3.0s) for transient failures.

---

## 7. Proxy Support

### Location

`gateway/platforms/base.py` (proxy functions)

### 7.1 Resolution Order

```
1. Platform-specific env var (e.g., DISCORD_PROXY, TELEGRAM_PROXY)
2. HTTPS_PROXY / HTTP_PROXY / ALL_PROXY (case-insensitive)
3. macOS system proxy via `scutil --proxy` (auto-detect)
```

### 7.2 SOCKS Support

SOCKS proxies use `aiohttp_socks.ProxyConnector` with `rdns=True` — forces remote DNS resolution through the proxy. Essential for bypassing DNS pollution behind the GFW.

```python
proxy_kwargs_for_bot(proxy_url)    → {"connector": ProxyConnector(...)}
proxy_kwargs_for_aiohttp(proxy_url) → (session_kwargs, request_kwargs)
```

### 7.3 Network Accessibility Check

`is_network_accessible(host)` — determines if a binding address exposes the server beyond loopback. Resolves hostnames via DNS, checks for loopback addresses including IPv4-mapped IPv6 (`::ffff:127.0.0.1`).

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Platform adapters | 17+ |
| Message types | 9 |
| Dedup cache size | 2000 entries |
| Dedup TTL | 300 seconds |
| Text batch delay | 0.6s (normal) / 2.0s (split) |
| Split threshold | 4000 chars |
| Thread tracker max | 500 threads |
| Max message length | 4096 UTF-16 code units (Telegram) |
| Send retry attempts | 2 |
| Media cache cleanup | 24 hours |
| Retryable error patterns | 9 |
| Built-in skins | 8 |

---

*Generated from source analysis of the Hermes Agent codebase.*
