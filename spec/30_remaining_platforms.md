# Hermes Agent — Remaining Platform Adapters (Detailed)

This document covers the remaining platform adapters not detailed in spec 27: Feishu/Lark, QQBot, DingTalk, Mattermost, WeCom, and Weixin.

---

## Table of Contents

1. [Feishu/Lark Adapter](#1-feishulark-adapter)
2. [QQBot Adapter](#2-qqbot-adapter)
3. [DingTalk Adapter](#3-dingtalk-adapter)
4. [Mattermost Adapter](#4-mattermost-adapter)
5. [WeCom Adapter](#5-wecom-adapter)
6. [Weixin Adapter](#6-weixin-adapter)

---

## 1. Feishu/Lark Adapter

### Location

`gateway/platforms/feishu.py` (~3990 lines)

### Purpose

Full-featured Feishu/Lark bot adapter supporting both WebSocket long-connection and HTTP webhook transports, rich message type normalization, per-chat serial processing, interactive card buttons for approvals, and QR-based bot onboarding.

### 1.1 Transport Modes

| Mode | Mechanism | Dependencies |
|------|-----------|--------------|
| WebSocket | `lark_oapi.ws.Client` runs in dedicated thread | `lark_oapi`, `websockets` |
| Webhook | `aiohttp.web` server on configurable host/port | `aiohttp` |

Configured via `FEISHU_CONNECTION_MODE` env var (default: `websocket`).

### 1.2 Dual-Domain Support

Supports both Feishu (China) and Lark (international) domains:
- Feishu: `open.feishu.cn`, `accounts.feishu.cn`
- Lark: `open.larksuite.com`, `accounts.larksuite.com`

Domain resolved via `FEISHU_DOMAIN` env var. Auto-detected during QR onboarding via `tenant_brand` field.

### 1.3 Connection Lifecycle

```
1. Check lark_oapi available + app_id/app_secret set
2. Acquire scoped lock on app_id (prevents duplicate connections)
3. Build lark SDK client + event handler
4. Hydrate bot identity (name, open_id) via /application/v6
5. Connect:
   a. WebSocket: FeishuWSClient in thread-local event loop
   b. Webhook: aiohttp AppRunner on host:port
6. Mark connected
```

**WebSocket thread isolation**: The official `lark_oapi.ws.client` runs in its own thread with a dedicated event loop. The adapter overrides `websockets.connect` and `_configure` at runtime to inject custom ping intervals. On disconnect, all pending tasks are cancelled and the loop is stopped.

**Reconnect**: 3 attempts with exponential backoff (1s, 2s, 4s). Auto-reconnect disabled during disconnect.

### 1.4 Message Normalization Pipeline

Feishu sends 10+ message types, all normalized via `normalize_feishu_message()`:

| Type | Processing |
|------|------------|
| `text` | Extract text field directly |
| `post` | Parse rich post payload: rows, elements, images, mentions |
| `image` | Extract image_key + alt text |
| `file`/`audio`/`media` | Extract file_key, build placeholder |
| `merge_forward` | Walk nested messages, build summary entries (max 8) |
| `share_chat` | Extract chat name + share ID |
| `interactive`/`card` | Walk card JSON, extract title, body lines, action labels |

**Post payload parsing**: Recursive walk through `content` arrays (rows → elements). Handles tags: `text`, `a`, `at`, `img`, `media`, `file`, `emotion`, `br`, `hr`, `code`, `code_block`. Renders to markdown with proper escaping.

**Card parsing**: Walks nested JSON for `_SUPPORTED_CARD_TEXT_KEYS` (title, text, content, label, value, etc.), skips structural keys (`tag`, `type`, `chat_id`, etc.). Collects button/action labels for display.

### 1.5 Inbound Event Routing

Event handler registered via `EventDispatcherHandler.builder()`:

| Event | Handler |
|-------|---------|
| `im.message.receive_v1` | `_on_message_event` → `_handle_message_event_data` |
| `im.message.message_read_v1` | Ignored (no action) |
| `im.message.reaction.created_v1` | `_on_reaction_event` → synthetic text |
| `im.message.reaction.deleted_v1` | `_on_reaction_event` → synthetic text |
| `card.action.trigger` | `_on_card_action_trigger` → approval or command |
| `im.chat.member.bot.added_v1` | `_on_bot_added_to_chat` |
| `im.chat.member.bot.deleted_v1` | `_on_bot_removed_from_chat` |

**Cross-thread dispatch**: SDK callbacks run on SDK thread → `asyncio.run_coroutine_threadsafe()` to main adapter loop.

### 1.6 Per-Chat Serial Processing

```python
def _get_chat_lock(self, chat_id: str) -> asyncio.Lock:
    lock = self._chat_locks.get(chat_id)
    if lock is None:
        lock = asyncio.Lock()
        self._chat_locks[chat_id] = lock
    return lock
```

Each chat gets its own lock. Messages in the same chat are processed one at a time, matching openclaw's `createChatQueue` serial queue behavior.

### 1.7 ACK Reaction

Before processing starts, the adapter adds a CHECK emoji reaction (`"OK"`) to the triggering message via `im.v1.message_reaction.create`. Serves as a persistent receipt marker.

### 1.8 Text Batching

Debounces rapid text bursts into single MessageEvents:

```python
text_batch_delay_seconds = 0.6        # normal flush delay
text_batch_split_delay_seconds = 2.0  # longer delay near split point
text_batch_max_messages = 8           # max merged messages
text_batch_max_chars = 4000           # max merged character count
_split_threshold = 4000               # triggers split delay
```

**Adaptive delay**: When the latest chunk is near the 4000-char split threshold, the delay extends to 2.0s since a continuation chunk is almost certain.

### 1.9 Media Batching

Similar to text batching but scoped by session key + message type:

```python
media_batch_key = f"{session_key}:media:{message_type.value}"
media_batch_delay_seconds = 0.8
```

Merges compatible events (same reply_to, same thread_id, same message_type). Accumulates `media_urls` and `media_types` lists.

### 1.10 Resource Download

**Images**: Downloaded via `im.v1.message_resource.get` with `resource_type="image"`, cached via `cache_image_from_bytes()`.

**Files/Audio/Video**: Downloaded via same API with `resource_type` matching the type. Audio files cached via `cache_audio_from_bytes()`, others via `cache_document_from_bytes()`.

**Text documents**: `.txt` and `.md` files under 100KB have their content injected directly into the message text as `[Content of filename]:\n...`.

### 1.11 Group Policy Gate

Per-group policy with 5 modes:

| Policy | Behavior |
|--------|----------|
| `open` | All messages accepted |
| `disabled` | All messages rejected |
| `admin_only` | Only admins accepted |
| `allowlist` | Only users in allowlist |
| `blacklist` | All except users in blacklist |

**Mention gating**: Group messages require `@bot` mention. Checked via `mentions` list from message object and parsed `mentioned_ids` from post content. `@_all` (Feishu's @everyone) always routes to bot.

**Bot identity hydration**: On connect, queries `/application/v6` to discover bot name/open_id for precise mention matching. Requires `admin:app.info:readonly` scope.

### 1.12 Webhook Security

Three-layer defense:

1. **Rate limiting**: 120 requests per 60s per `{app_id}:{path}:{remote_ip}` key. Max 4096 tracked keys (LRU eviction).
2. **Verification token**: `hmac.compare_digest` check against `FEISHU_VERIFICATION_TOKEN`.
3. **Signature verification**: `SHA256(timestamp + nonce + encrypt_key + body)` compared via `hmac.compare_digest`.

**Anomaly tracker**: Counts consecutive error responses per IP. WARNING logged every 25 hits. TTL: 6 hours.

**Body limits**: 1MB max body size, 30s read timeout.

### 1.13 Outbound Messaging

**Payload auto-detection**: `_build_outbound_payload()` scans content for markdown hints (`#`, `**`, ` ``` `, etc.) → sends as `post` type. Otherwise sends as `text`.

**Send retry**: 3 attempts with exponential backoff (1s, 2s, 4s).

**Reply fallback**: If replying to a message fails with code 230011 or 231003 (withdrawn/missing), falls back to posting a new message directly to the chat.

**Post fallback**: If `post` type is rejected with "content format of the post type is incorrect", falls back to plain `text` with markdown stripped.

**Message editing**: Supported via `im.v1.message.update` API. Same post→text fallback on edit.

### 1.14 Interactive Approval Cards

```python
async def send_exec_approval(self, chat_id, command, session_key, description):
    card = {
        "header": {"title": "⚠️ Command Approval Required", "template": "orange"},
        "elements": [
            {"tag": "markdown", "content": f"```\n{command}\n```"},
            {"tag": "action", "actions": [
                {"text": "Allow Once", "value": {"hermes_action": "approve_once", "approval_id": N}},
                {"text": "Allow Session", "value": {"hermes_action": "approve_session", "approval_id": N}},
                {"text": "Always Allow", "value": {"hermes_action": "approve_always", "approval_id": N}},
                {"text": "Deny", "value": {"hermes_action": "deny", "approval_id": N}},
            ]},
        ],
    }
```

Button clicks routed via `_on_card_action_trigger`:
1. Parse `hermes_action` and `approval_id` from button value
2. Build resolved card response (inline update showing decision)
3. Schedule async `_resolve_approval()` → `resolve_gateway_approval(session_key, choice)`
4. Card updated in-place via `CallBackCard` response

**Dedup**: Card action tokens deduplicated with 15-minute TTL.

### 1.15 Reaction Routing

User reactions on bot messages are routed as synthetic text events:
1. Verify reaction target was sent by this bot (`sender_type == "app"`)
2. Fetch target message to obtain chat context
3. Build synthetic text: `reaction:added:emoji_type` or `reaction:removed:emoji_type`
4. Dispatch via `_handle_message_with_guards`

Bot's own ACK reactions (`"OK"`) are filtered out to prevent feedback loops.

### 1.16 Deduplication

Persistent message ID cache saved to `~/.hermes/feishu_seen_message_ids.json`:

```python
_FEISHU_DEDUP_TTL_SECONDS = 86400  # 24 hours
dedup_cache_size = 2048  # configurable
```

Backward-compatible with old format (plain list of IDs → new format with timestamps).

### 1.17 QR Onboarding

Device-code flow for automatic bot creation:

```
1. _init_registration: verify client_secret support
2. _begin_registration: get device_code + QR URL
3. Render QR in terminal (via qrcode library)
4. _poll_registration: poll until user scans (interval-based, max expire_in)
5. probe_bot: verify connectivity via /bot/v3/info
6. Return {app_id, app_secret, domain, open_id, bot_name, bot_open_id}
```

**Domain auto-switch**: If user scans with Lark app, `tenant_brand == "lark"` triggers domain switch mid-poll.

### Key Numbers

| Metric | Value |
|--------|-------|
| Lines of code | ~3990 |
| Message types normalized | 10+ |
| Transport modes | 2 (WebSocket, Webhook) |
| Group policies | 5 |
| Approval button actions | 4 |
| Text batch delay | 0.6s / 2.0s (split) |
| Media batch delay | 0.8s |
| Dedup TTL | 24 hours |
| Dedup cache size | 2048 |
| Webhook rate limit | 120 per 60s per key |
| Webhook body limit | 1MB |
| Webhook anomaly threshold | 25 consecutive errors |
| Card action dedup TTL | 15 minutes |
| Sender name cache TTL | 10 minutes |
| Max message length | 8000 chars |
| Split threshold | 4000 chars |
| Send retry attempts | 3 |
| Connect retry attempts | 3 |

---

## 2. QQBot Adapter

### Location

`gateway/platforms/qqbot.py` (~1960 lines)

### Purpose

QQ Bot adapter using the official QQ Bot API v2. Connects to the QQ Bot WebSocket Gateway for inbound events and uses the REST API (`api.sgroup.qq.com`) for outbound messaging and media uploads.

### 2.1 Connection Lifecycle

```
1. Check aiohttp + httpx available
2. Check QQ_APP_ID + QQ_CLIENT_SECRET set
3. Acquire scoped lock on app_id
4. Get access token (POST to bots.qq.com/app/getAppAccessToken)
5. Get WebSocket gateway URL (GET to api.sgroup.qq.com/gateway)
6. Open WebSocket connection
7. Start _listen_loop + _heartbeat_loop
```

**Token management**: Token cached with expiry. Singleflight pattern via `asyncio.Lock` — concurrent token refreshes serialize. 60-second grace period before expiry.

### 2.2 WebSocket Protocol

QQ Bot uses a custom op-code protocol:

| Op | Name | Direction | Purpose |
|----|------|-----------|---------|
| 1 | Heartbeat | Client→Server | Send latest seq number |
| 2 | Identify | Client→Server | Authenticate with token + intents |
| 6 | Resume | Client→Server | Re-authenticate after reconnect |
| 10 | Hello | Server→Client | Heartbeat interval |
| 11 | Heartbeat ACK | Server→Client | Acknowledge heartbeat |
| 0 | Dispatch | Server→Client | Events (READY, RESUMED, messages) |

**Intents**: `(1 << 25) | (1 << 30) | (1 << 12)` = C2C_GROUP_AT_MESSAGES + PUBLIC_GUILD_MESSAGES + DIRECT_MESSAGE

**Heartbeat**: Server sends interval in Hello (op 10). Client sends heartbeats at 80% of that interval.

### 2.3 Reconnect Logic

Close code handling:

| Code | Meaning | Action |
|------|---------|--------|
| 4004 | Invalid token | Clear cached token, reconnect |
| 4006/4007/4009/4900-4913 | Session invalid | Clear session_id + last_seq, reconnect |
| 4008 | Rate limited | Wait 60s, reconnect |
| 4914 | Bot offline/sandbox | Stop reconnecting (fatal) |
| 4915 | Bot banned | Stop reconnecting (fatal) |

**Quick disconnect detection**: If connection drops within 5 seconds of connecting, 3 consecutive quick disconnects trigger a fatal error with guidance to check bot permissions.

**Backoff**: `[2, 5, 10, 30, 60]` seconds, max 100 reconnect attempts.

### 2.4 Event Types

| Event Type | Handler |
|------------|---------|
| `C2C_MESSAGE_CREATE` | `_handle_c2c_message` |
| `GROUP_AT_MESSAGE_CREATE` | `_handle_group_message` |
| `DIRECT_MESSAGE_CREATE` | `_handle_dm_message` |
| `GUILD_MESSAGE_CREATE` | `_handle_guild_message` |
| `GUILD_AT_MESSAGE_CREATE` | `_handle_guild_message` |
| `READY` | Store session_id for resume |
| `RESUMED` | Log only |

### 2.5 Attachment Processing

Mirrors OpenClaw's `processAttachments`:

1. **Voice attachments**: Priority chain:
   - QQ's built-in `asr_refer_text` (Tencent ASR — free)
   - Self-hosted STT on `voice_wav_url` (pre-converted WAV)
   - Self-hosted STT on original URL (requires SILK→WAV conversion)

2. **SILK audio decoding**: QQ voice messages are typically SILK format.
   - Try `pilk` library first (handles SILK natively)
   - Fall back to `ffmpeg`
   - Last resort: write raw PCM as 16-bit mono 16kHz WAV

3. **Magic byte detection**: `#!SILK_V3`, `#!SILK`, `\x02!`, `RIFF` (WAV), `fLaC`, `\xff\xfb` (MP3), etc.

4. **Images**: Downloaded with `QQBot {token}` Authorization header, cached locally.

5. **Other files**: Recorded as text descriptions.

### 2.6 STT Configuration

Three configuration sources (priority order):
1. `channels.qqbot.stt` in config.yaml (with provider mapping: zai→GLM, openai→Whisper)
2. `QQ_STT_API_KEY` / `QQ_STT_BASE_URL` / `QQ_STT_MODEL` env vars
3. None configured → QQ built-in ASR still works via `asr_refer_text`

**API compatibility**: Supports both Zhipu/GLM format (`choices[0].message.content`) and OpenAI/Whisper format (`text` field).

### 2.7 Outbound Messaging

**Message types**:
- `msg_type=0`: Plain text
- `msg_type=2`: Markdown (when `markdown_support=true`)
- `msg_type=6`: Input notify (typing indicator)
- `msg_type=7`: Media

**Media upload**: `/v2/users/{openid}/files` or `/v2/groups/{group_openid}/files`
- Supports URL or base64 `file_data`
- Returns `file_info` which is used in the media message body
- 3 retries with exponential backoff

**Send flow**:
1. Format message (markdown or stripped)
2. Truncate to 4000 chars
3. For each chunk: `_send_chunk` with 3 retries
4. Route by chat type (c2c, group, guild)

**Reply fallback**: Permanent errors (invalid, forbidden, not found) are not retried.

### 2.8 ACL Policies

| Policy Type | Values |
|-------------|--------|
| `dm_policy` | `open`, `allowlist`, `disabled` |
| `group_policy` | `open`, `allowlist`, `disabled` |

Allowlist supports wildcard `*` and pattern matching.

### Key Numbers

| Metric | Value |
|--------|-------|
| Lines of code | ~1960 |
| Message types | 5 (text, markdown, input_notify, media, reply) |
| Media types | 4 (image, video, voice, file) |
| Reconnect backoff | [2, 5, 10, 30, 60]s |
| Max reconnect attempts | 100 |
| Quick disconnect threshold | 5s, 3 consecutive |
| Heartbeat interval | 80% of server interval (~33s default) |
| Message dedup window | 300s |
| Dedup max size | 1000 |
| Max message length | 4000 chars |
| Send retry attempts | 3 |
| Upload retry attempts | 3 |
| Rate limit delay | 60s (code 4008) |
| Token expiry grace | 60s before expiry |
| File upload timeout | 120s |

---

## 3. DingTalk Adapter

### Location

`gateway/platforms/dingtalk.py` (~334 lines)

### Purpose

DingTalk Stream Mode adapter using the `dingtalk-stream` SDK for WebSocket-based long-lived connections.

### 3.1 Connection

Uses `dingtalk-stream` SDK which manages the WebSocket connection lifecycle:

```python
client = CredentialClient(access_key_id, access_key_secret)
gateway = ChatBotMessageGateway(client)
gateway.register_callback(handler.handle)
gateway.start_forever()
```

**Reconnect backoff**: `[2, 5, 10, 30, 60]` seconds.

### 3.2 Message Handling

Incoming messages processed via `ChatbotHandler` callback:
1. Dedup check via `MessageDeduplicator(max_size=1000)`
2. Build `MessageEvent` with session source
3. Cross-thread dispatch: `asyncio.run_coroutine_threadsafe()` from dingtalk-stream thread to main event loop

### 3.3 Reply Mechanism

Replies sent via session webhook URL (provided by DingTalk in each message):

```python
async def _reply_via_webhook(webhook_url, content):
    # Validate URL against SSRF
    if not re.match(r"^https://api\.dingtalk\.com/", webhook_url):
        raise ValueError(f"Invalid DingTalk webhook URL")
    # POST JSON to webhook
```

**SSRF protection**: URL validated against `^https://api\.dingtalk\.com/` regex.

**Session webhook cache**: Capped at 500 entries (FIFO eviction) to avoid unbounded memory growth.

### 3.4 Format

Outbound messages sent in Markdown format.

### Key Numbers

| Metric | Value |
|--------|-------|
| Lines of code | ~334 |
| Dedup cache size | 1000 |
| Webhook cache max | 500 |
| Reconnect backoff | [2, 5, 10, 30, 60]s |
| Outbound format | Markdown |

---

## 4. Mattermost Adapter

### Location

`gateway/platforms/mattermost.py` (~734 lines)

### Purpose

Mattermost adapter using pure aiohttp WebSocket + REST API v4. No external Mattermost library required.

### 4.1 Connection

```python
# WebSocket URL
wss://{host}/api/v4/websocket

# Authentication
{"action": "authentication_challenge", "data": {"token": "xxx"}}
```

**Connection flow**:
1. Create aiohttp ClientSession
2. Open WebSocket to `/api/v4/websocket`
3. Send `authentication_challenge` with personal access token
4. Wait for `server_version` event (confirms auth)
5. Start WebSocket event reader

### 4.2 Event Processing

Only `posted` events are processed. Other events are ignored.

```python
event = json.loads(msg.data)
if event.get("event") == "posted":
    post = event.get("data", {}).get("post")
    # Process post
```

### 4.3 Mention Gating

- `MATTERMOST_REQUIRE_MENTION` (default: true) — channel messages require @mention
- `MATTERMOST_FREE_RESPONSE_CHANNELS` — channels where mention not required
- DM messages always accepted

### 4.4 Reply Modes

- `"thread"` (default): Replies in thread via `root_id`
- `"off"`: Flat replies (no threading)

### 4.5 File Handling

**Upload**: Multipart form POST to `/api/v4/files`, then post message with `file_ids`.

**Download**: File attachments downloaded immediately (URLs require auth headers downstream, so ephemeral URLs are resolved to cached files).

### 4.6 Channel Types

| Mattermost Type | Mapped Type |
|-----------------|-------------|
| `D` | dm |
| `G` | group |
| `P` | group (private channel) |
| `O` | channel (open channel) |

### 4.7 Reconnect

Exponential backoff with jitter:
- Base: 2s → 60s
- Jitter: 20% random
- Max attempts: 20

**Permanent auth failure**: 401/403 responses stop the reconnect loop immediately.

### Key Numbers

| Metric | Value |
|--------|-------|
| Lines of code | ~734 |
| Reconnect max attempts | 20 |
| Reconnect backoff | 2s → 60s + 20% jitter |
| External dependencies | None (pure aiohttp) |

---

## 5. WeCom Adapter

### Location

`gateway/platforms/wecom.py` (~1431 lines)

### Purpose

WeCom (WeChat Work) AI Bot adapter using the official WebSocket gateway (`openws.work.weixin.qq.com`). Implements a subscribe/callback/send command protocol with chunked media upload.

### 5.1 Connection

```python
# WebSocket URL
wss://openws.work.weixin.qq.com

# Auth: subscribe command
{
    "cmd": "aibot_subscribe",
    "bot_id": "xxx",
    "secret": "xxx"
}

# Wait for handshake ack before proceeding
```

**Heartbeat**: `ping` command every 30 seconds.

### 5.2 Command Protocol

| Command | Direction | Purpose |
|---------|-----------|---------|
| `aibot_subscribe` | Client→Server | Authenticate |
| `aibot_msg_callback` | Server→Client | Inbound message |
| `aibot_send_msg` | Client→Server | Send message |
| `aibot_respond_msg` | Client→Server | Reply to specific message |
| `ping` | Client↔Server | Heartbeat |

**Request/response correlation**: Via `req_id` headers. Pending futures stored in `_pending_responses` dict.

### 5.3 Text Batching

Handles 4000-char client-side splits:

```python
text_batch_delay = 0.6          # normal flush
text_batch_split_delay = 2.0    # for split messages
split_threshold = 3900          # chars that indicate a split
```

### 5.4 Media Handling

**Inbound**: Base64 inline or URL download with AES decryption.

**Outbound — Chunked Upload**:
1. `init`: Get upload session with 512KB chunk size
2. `chunk`: Upload chunks sequentially
3. `finish`: Complete upload, get `media_id`
4. Send message with `media_id`

**File size limits**:

| Type | Limit | Auto-downgrade |
|------|-------|----------------|
| Image | 10MB | → file |
| Video | 10MB | → file |
| Voice | 2MB (AMR only) | → file |
| File | 20MB | error |

### 5.5 ACL Policies

**DM policy**:
- `open`: Accept all DMs
- `allowlist`: Only specified users
- `disabled`: Reject all DMs
- `pairing`: Accept DMs only after user pairs

**Group policy**:
- `open`: Accept all group messages
- `allowlist`: Only specified groups
- `disabled`: Reject all group messages

Per-group configuration supported.

### 5.6 Format

Outbound: Markdown. Replies: stream reply format.

### Key Numbers

| Metric | Value |
|--------|-------|
| Lines of code | ~1431 |
| Heartbeat interval | 30 seconds |
| Chunk size | 512KB |
| Image limit | 10MB |
| Video limit | 10MB |
| Voice limit | 2MB |
| File limit | 20MB |
| Text batch delay | 0.6s / 2.0s (split) |
| Split threshold | 3900 chars |

---

## 6. Weixin Adapter

### Location

`gateway/platforms/weixin.py` (~1830 lines)

### Purpose

WeChat (personal) iLink Bot API adapter. Uses long-poll for inbound messages, AES-128-ECB encrypted CDN for media, and QR login flow for authentication.

### 6.1 Connection

**Long-poll**: `getupdates` endpoint with 35-second timeout for inbound messages.

**Context token**: Every outbound message must echo `context_token` for peer identification. Stored in `ContextTokenStore` — disk-backed cache per account + peer.

### 6.2 QR Login Flow

```
1. Fetch QR code
2. Poll status until user scans
3. Confirm login
4. Save credentials to ~/.hermes/weixin/accounts/{account_id}.json
```

### 6.3 AES-128-ECB CDN

**Download**:
1. Get `encrypted_query_param` URL or `full_url`
2. Download encrypted data
3. Decrypt with AES-128-ECB using the account's AES key

**Upload**:
1. Get upload URL
2. Encrypt data with AES-128-ECB
3. POST ciphertext
4. Receive `encrypted_param` for reference

**Critical**: AES key encoding is `base64(hex_string)` — NOT `base64(raw_bytes)`. Wrong encoding causes silent decryption failures.

### 6.4 Media Types

| Type | ID |
|------|-----|
| Image | 1 |
| Video | 2 |
| File | 3 |
| Voice | 4 |

### 6.5 Markdown Normalization

WeChat display requires markdown normalization:

| Markdown | WeChat Display |
|----------|----------------|
| Tables | Key-value lists |
| H1 (`# title`) | 【title】 |
| Links `[text](url)` | `text (url)` |

### 6.6 Delivery Unit Splitting

Long messages split into chat-like blocks (delivery units), each sent as a separate bubble. Compact mode is the default.

### 6.7 Typing Tickets

`TypingTicketCache`: 600-second TTL cache for typing tickets received from `getconfig` events.

### 6.8 Streaming

`SUPPORTS_MESSAGE_EDITING = False` — streaming uses send-final-only path (no cursor left visible). Per-chunk retry with backoff for send operations.

### Key Numbers

| Metric | Value |
|--------|-------|
| Lines of code | ~1830 |
| Long-poll timeout | 35 seconds |
| Typing ticket TTL | 600 seconds |
| AES cipher | AES-128-ECB |
| AES key encoding | base64(hex_string) |
| Media types | 4 (image, video, file, voice) |
| Message editing | Not supported |

---

*Generated from source analysis of the Hermes Agent codebase.*
