# Hermes Agent — Gateway Platform Adapters

This document covers the `gateway/platforms/` subsystem: the base adapter interface (`base.py`, ~2,087 lines), Telegram adapter (`telegram.py`, ~2,825 lines), Discord adapter (`discord.py`, ~3,082 lines), and shared helpers.

---

## Table of Contents

1. [Base Platform Adapter](#1-base-platform-adapter)
2. [Telegram Adapter](#2-telegram-adapter)
3. [Discord Adapter](#3-discord-adapter)
4. [Shared Helpers](#4-shared-helpers)
5. [Other Adapters](#5-other-adapters)

---

## 1. Base Platform Adapter

### Location

`gateway/platforms/base.py` (~2,087 lines)

### Purpose

Abstract base class and shared utilities for all platform adapters. Defines the contract for message handling, sending, media delivery, and typing indicators.

### 1.1 MessageEvent

```python
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
    auto_skill: Optional[str | list[str]] = None          # topic/channel skill bindings
    internal: bool = False                                # synthetic events bypass auth
    timestamp: datetime = field(default_factory=datetime.now)
```

### 1.2 MessageType Enum

| Type | Description |
|------|-------------|
| `TEXT` | Plain text message |
| `LOCATION` | GPS/location share |
| `PHOTO` | Image attachment |
| `VIDEO` | Video file |
| `AUDIO` | Audio file |
| `VOICE` | Voice message (bubble) |
| `DOCUMENT` | File attachment |
| `STICKER` | Sticker/GIF |
| `COMMAND` | `/command` style |

### 1.3 SendResult

```python
@dataclass
class SendResult:
    success: bool
    message_id: Optional[str] = None
    error: Optional[str] = None
    raw_response: Any = None
    retryable: bool = False  # transient connection failure → auto-retry
```

### 1.4 BasePlatformAdapter ABC

**Abstract methods**:
- `connect()` → bool: Connect and start receiving messages
- `disconnect()`: Disconnect from platform
- `send(chat_id, content, reply_to, metadata)` → SendResult: Send message
- `get_chat_info(chat_id)` → Dict: Get chat/channel info

**Optional overridable methods**:
- `edit_message(chat_id, message_id, content)`: Edit existing message
- `send_typing(chat_id)`: Show typing indicator
- `stop_typing(chat_id)`: Stop persistent typing indicator
- `send_image(chat_id, image_url, caption)`: Native image attachment
- `send_animation(chat_id, animation_url, caption)`: Native GIF
- `send_voice(chat_id, audio_path, caption)`: Voice message bubble
- `play_tts(chat_id, audio_path)`: Auto-TTS playback
- `send_video(chat_id, video_path, caption)`: Native video
- `send_document(chat_id, file_path, caption)`: File attachment
- `send_image_file(chat_id, image_path, caption)`: Local image file

### 1.5 Message Handling Pipeline

```python
async def handle_message(self, event: MessageEvent) -> None:
    """Process incoming message. Returns quickly by spawning background tasks."""
```

**Flow**:
1. Build session key from event source
2. Check for active session:
   - If active → interrupt (or queue for photo bursts)
   - Commands `approve`, `deny`, `status`, `stop`, `new`, `reset`, `background`, `restart` bypass the guard and dispatch directly
3. Set interrupt event (synchronous guard to prevent race)
4. Spawn `_process_message_background()` task

### 1.6 Background Processing

```python
async def _process_message_background(self, event, session_key) -> None:
```

**Pipeline**:
1. Start continuous typing indicator (`_keep_typing` — refreshes every 2s)
2. Fire `on_processing_start` hook
3. Call `_message_handler(event)` → response string
4. Extract `MEDIA:<path>` tags (from TTS tool)
5. Extract image URLs (markdown `![alt](url)` and `<img src="...">`)
6. Extract local file paths (bare `/path/to/image.png`)
7. Auto-TTS for voice input (if not disabled via `/voice off`)
8. Play TTS audio before text (voice-first experience)
9. Send text with retry (`_send_with_retry`)
10. Human-like pacing delay between text and media
11. Send images/animations natively
12. Send media files (routed by extension)
13. Send local files natively
14. Fire `on_processing_complete` hook
15. Check for pending messages from interrupt → recurse
16. Cancel typing indicator in `finally`

### 1.7 Retry System

```python
async def _send_with_retry(chat_id, content, reply_to, metadata, max_retries=2, base_delay=2.0):
```

**Retryable errors**: `connecterror`, `connectionerror`, `connectionreset`, `connectionrefused`, `connecttimeout`, `network`, `broken pipe`, `remotedisconnected`, `eoferror`

**NOT retryable**: `timed out`, `readtimeout`, `writetimeout` — request may have reached server, retrying risks duplicate delivery.

**Fallback**: On formatting failure, sends plain-text version with "(Response formatting failed, plain text:)" prefix.

**Delivery failure notice**: After all retries exhausted, sends user a warning that their request was processed but response couldn't be delivered.

### 1.8 Typing Indicator

```python
async def _keep_typing(self, chat_id, interval=2.0, metadata=None):
    """Continuously send typing indicator until cancelled."""
```

- Refreshes every 2 seconds (platform typing expires after ~5s)
- Skips when chat is in `_typing_paused` (e.g., during approval waits)
- Critical for Slack's Assistant API where typing disables the compose box

### 1.9 Message Truncation

```python
@staticmethod
def truncate_message(content, max_length=4096, len_fn=None) -> List[str]:
    """Split long message into chunks, preserving code block boundaries."""
```

**Features**:
- Preserves fenced code block boundaries
- Reopens code fences with original language tag in next chunk
- Multi-chunk responses get `(1/3)` indicators
- Supports custom length function (`utf16_len` for Telegram)
- Avoids splitting inside inline code spans (prevents MarkdownV2 parse errors)

### 1.10 UTF-16 Length

```python
def utf16_len(s: str) -> int:
    """Count UTF-16 code units. Emoji consume 2 units each."""
    return len(s.encode("utf-16-le")) // 2
```

Telegram's 4,096 character limit is measured in UTF-16 code units, not Unicode codepoints.

### 1.11 Media Cache

**Image cache**: `~/.hermes/cache/images/`
**Audio cache**: `~/.hermes/cache/audio/`
**Document cache**: `~/.hermes/cache/documents/`

**Safety**:
- SSRF protection via `is_safe_url()` check
- Redirect guard re-validates each 302 target
- Image magic-byte validation before caching
- Document path traversal protection
- Auto-cleanup after 24 hours
- Retry with exponential backoff on transient CDN failures

### 1.12 Media Extraction

```python
@staticmethod
def extract_media(content) -> Tuple[List[Tuple[str, bool]], str]:
    """Extract MEDIA:<path> tags and [[audio_as_voice]] directives."""

@staticmethod
def extract_images(content) -> Tuple[List[Tuple[str, str]], str]:
    """Extract markdown and HTML image URLs."""

@staticmethod
def extract_local_files(content) -> Tuple[List[str], str]:
    """Detect bare local file paths in response text."""
```

**Local file detection**: Matches `/...` and `~/...` paths ending in image/video extensions, validates with `os.path.isfile()`, skips paths inside code blocks.

### 1.13 Proxy Support

```python
def resolve_proxy_url(platform_env_var=None) -> str | None:
    """Return proxy URL from env vars or macOS system proxy."""
```

**Check order**:
1. Platform-specific env var (e.g., `DISCORD_PROXY`)
2. `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` (case-insensitive)
3. macOS system proxy via `scutil --proxy`

**SOCKS support**: `aiohttp_socks` with `rdns=True` for remote DNS resolution (essential for bypassing DNS pollution behind GFW).

### 1.14 Network Accessibility

```python
def is_network_accessible(host: str) -> bool:
    """Return True if host would expose the server beyond loopback."""
```

Checks loopback addresses, IPv4-mapped addresses, and resolves hostnames.

### 1.15 Session State Management

```python
self._active_sessions: Dict[str, asyncio.Event]    # interrupt signals
self._pending_messages: Dict[str, MessageEvent]     # queued messages
self._background_tasks: set[asyncio.Task]           # in-flight processing
self._typing_paused: set                            # chats with paused typing
self._auto_tts_disabled_chats: set                  # /voice off chats
```

### 1.16 Fatal Error Tracking

```python
def _set_fatal_error(self, code, message, *, retryable) -> None:
    """Mark adapter as having a fatal error, update runtime status."""
```

Writes platform state to runtime status file for health monitoring.

### 1.17 Platform Locking

```python
def _acquire_platform_lock(self, scope, identity, resource_desc) -> bool:
    """Acquire a scoped lock for this adapter."""
```

Prevents multiple gateway instances from using the same bot token simultaneously.

### 1.18 Human-Like Pacing

```python
@staticmethod
def _get_human_delay() -> float:
    """Return random delay for human-like response pacing."""
```

**Env vars**:
- `HERMES_HUMAN_DELAY_MODE`: `off` (default) | `natural` | `custom`
- `HERMES_HUMAN_DELAY_MIN_MS`: 800 (default, custom mode)
- `HERMES_HUMAN_DELAY_MAX_MS`: 2500 (default, custom mode)

### 1.19 Command Bypass

Certain commands bypass the active-session guard and dispatch directly:

| Command | Purpose |
|---------|---------|
| `approve` / `deny` | Agent is blocked on `Event.wait()` for approval |
| `stop` | Interrupt running agent |
| `new` / `reset` | Start fresh session |
| `status` | Check agent status |
| `background` | Background process control |
| `restart` | Restart gateway |

---

## 2. Telegram Adapter

### Location

`gateway/platforms/telegram.py` (~2,825 lines)

### 2.1 Architecture

Uses `python-telegram-bot` library with optional fallback transport for networks where Telegram API is blocked.

### 2.2 Message Limits

- Max message length: 4,096 characters (UTF-16 code units)
- Split threshold: 4,000 (detects Telegram client-side message splits)

### 2.3 MarkdownV2 Handling

```python
_MDV2_ESCAPE_RE = re.compile(r'([_*\[\]()~`>#\+\-=|{}.!\\])')

def _escape_mdv2(text: str) -> str:
    """Escape Telegram MarkdownV2 special characters."""

def _strip_mdv2(text: str) -> str:
    """Strip MarkdownV2 escape backslashes for plain text fallback."""
```

### 2.4 Media Batch Handling

```python
self._media_batch_delay_seconds = 0.8    # photo burst aggregation window
self._text_batch_delay_seconds = 0.6     # text message aggregation
self._text_batch_split_delay_seconds = 2.0  # client-side split window
```

Telegram sends album photos as near-simultaneous updates. The adapter buffers these into a single `MessageEvent`.

### 2.5 Reply Mode

```python
self._reply_to_mode: str = 'first'  # reply to first message only
```

### 2.6 Network Fallback

Uses `TelegramFallbackTransport` with IP discovery for accessing Telegram API in restricted networks.

### 2.7 Forum Topics

Supports Telegram forum topics (thread_id) for group conversations.

---

## 3. Discord Adapter

### Location

`gateway/platforms/discord.py` (~3,082 lines)

### 3.1 Architecture

Uses `discord.py` library for server/DM message handling.

### 3.2 Voice Receiver

```python
class VoiceReceiver:
    """Captures and decodes voice audio from Discord voice channel."""
    
    SILENCE_THRESHOLD = 1.5    # seconds → end of utterance
    MIN_SPEECH_DURATION = 0.5  # skip noise shorter than this
    SAMPLE_RATE = 48000        # Discord native rate
    CHANNELS = 2               # stereo
```

**Features**:
- RTP packet decryption (NaCl transport + DAVE E2EE)
- Opus audio decoding per-user (separate decoder state per SSRC)
- SSRC → user_id mapping from SPEAKING events
- Per-user audio buffer with silence detection
- Pause during bot TTS playback to avoid echo

### 3.3 Thread Management

```python
VALID_THREAD_AUTO_ARCHIVE_MINUTES = {60, 1440, 4320, 10080}
```

### 3.4 Message Deduplication

Uses `MessageDeduplicator` helper to prevent processing duplicate messages.

### 3.5 Thread Participation Tracking

`ThreadParticipationTracker` monitors bot participation in threads for auto-archival awareness.

### 3.6 Discord ID Cleaning

```python
def _clean_discord_id(entry: str) -> str:
    """Strip prefixes like user:123, <@123>, <@!123> from Discord IDs."""
```

---

## 4. Shared Helpers

### Location

`gateway/platforms/helpers/`

### 4.1 Message Deduplicator

Prevents processing duplicate messages across platform event streams.

### 4.2 Thread Participation Tracker

Tracks bot's participation in Discord threads for context-aware behavior.

### 4.3 Crypto Helpers

Cryptographic utilities for platform-specific authentication (e.g., Signal, WhatsApp webhook verification).

### 4.4 Network Helpers

Network utilities for platform connectivity diagnostics.

---

## 5. Other Adapters

### 5.1 Feishu/Lark (`feishu.py`, ~3,986 lines)

Chinese enterprise messaging platform. Handles webhook events, card messages, and rich text formatting.

### 5.2 Slack (`slack.py`, ~1,670 lines)

Uses Slack Bolt framework. Supports thread replies, Assistant API integration, and channel_skill_bindings.

### 5.3 Matrix (`matrix.py`, ~2,023 lines)

Open federated messaging. Handles room events, encryption, and Matrix-specific message types.

### 5.4 QQBot (`qqbot.py`, ~1,960 lines)

Tencent QQ bot protocol. Handles group/DM messages and platform-specific formatting.

### 5.5 Weixin (`weixin.py`, ~1,829 lines)

WeChat bot adapter. Handles WeChat message types and media.

### 5.6 WeCom (`wecom.py`, ~1,430 lines)

WeChat Work (enterprise) adapter.

### 5.7 API Server (`api_server.py`, ~2,436 lines)

REST API endpoint for programmatic agent access. Supports streaming responses.

### 5.8 Remaining Adapters (covered in spec 28)

WhatsApp, Signal, BlueBubbles, Email, Webhook, Mattermost, SMS, DingTalk, HomeAssistant adapters — each implementing the `BasePlatformAdapter` interface with platform-specific message handling.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| base.py lines | ~2,087 |
| telegram.py lines | ~2,825 |
| discord.py lines | ~3,082 |
| feishu.py lines | ~3,986 |
| Total platforms/ lines | ~30,299 |
| Message types | 9 |
| Retryable error patterns | 9 |
| Command bypass commands | 8 |
| Max Telegram message length | 4,096 UTF-16 units |
| Typing refresh interval | 2 seconds |
| Voice silence threshold | 1.5 seconds |
| Voice sample rate | 48,000 Hz |
| Retry max attempts | 2 |
| Retry base delay | 2.0 seconds |
| Media cache directories | 3 (images, audio, documents) |
| Media cache TTL | 24 hours |
| Built-in skin count | 10 (poseidon, sisyphus, charizard added) |
| Human delay natural range | 800–2500ms |
| Thread auto-archive options | 4 (60, 1440, 4320, 10080 min) |

---

*Generated from source analysis of the Hermes Agent codebase.*
