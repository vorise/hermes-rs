# Hermes Agent — Remaining Platform Adapters

This document covers the remaining platform adapters beyond Telegram/Discord/Slack: WhatsApp, Signal, Matrix, BlueBubbles, Email, HomeAssistant, Webhook, and SMS (Twilio).

---

## Table of Contents

1. [WhatsApp Adapter](#1-whatsapp-adapter)
2. [Signal Adapter](#2-signal-adapter)
3. [Matrix Adapter](#3-matrix-adapter)
4. [BlueBubbles Adapter](#4-bluebubbles-adapter)
5. [Email Adapter](#5-email-adapter)
6. [HomeAssistant Adapter](#6-homeassistant-adapter)
7. [Webhook Adapter](#7-webhook-adapter)
8. [SMS Adapter](#8-sms-adapter-twilio)
9. [Adapter Comparison Matrix](#9-adapter-comparison-matrix)

---

## 1. WhatsApp Adapter

### Location

`gateway/platforms/whatsapp.py` (~990 lines)

### Purpose

WhatsApp integration via a Node.js bridge subprocess. No official bot API for personal accounts; uses `whatsapp-web.js` or `Baileys` libraries running in a separate Node.js process.

### 1.1 Architecture

**Bridge pattern** — Python adapter communicates with a Node.js subprocess over HTTP:

```
Python Gateway ←HTTP→ Node.js Bridge ←WhatsApp Web→ WhatsApp Servers
```

| Component | Role |
|-----------|------|
| Node.js bridge | WhatsApp Web client, QR auth, message send/recv |
| Python adapter | MessageEvent dispatch, markdown formatting, media caching |
| HTTP bridge | REST endpoints: `/health`, `/send`, `/edit`, `/send-media`, `/typing`, `/messages` |

### 1.2 Connection Lifecycle

```
1. Check Node.js availability (node --version)
2. Verify bridge script exists (scripts/whatsapp-bridge/bridge.js)
3. Auto-install npm dependencies if node_modules missing
4. Kill orphaned bridge process on configured port (fuser/netstat)
5. Launch bridge: node bridge.js --port 3000 --session ~/.hermes/platforms/whatsapp/session --mode self-chat
6. Phase 1: Wait for HTTP server (up to 15s)
7. Phase 2: Wait for WhatsApp connection status=connected (up to 15s more)
8. Start message polling task (_poll_messages, 1s interval)
9. Mark connected
```

**Crash resilience**: Checks for existing running bridge via `/health` endpoint. Reuses external bridge if status=connected. Kills orphaned processes on startup.

### 1.3 Message Handling

**Inbound** — HTTP polling (`GET /messages` every 1s):

1. Fetch pending messages from bridge
2. `_should_process_message()` filtering:
   - DMs: always process
   - Groups: check `require_mention`, `free_response_chats`, reply-to-bot, @mention, custom patterns
3. `_build_message_event()`:
   - Detect media type (hasMedia → mediaType: image/video/audio/ptt)
   - Download media URLs to local cache (image → cache_image_from_url, voice → cache_audio_from_url)
   - Local file paths from bridge used directly
   - Text document injection: reads .txt/.md/.csv/.json/.py/.js etc. files inline (cap 100KB)
4. Dispatch to `handle_message()`

**Mention detection** (groups):
- `require_mention` config (default: false)
- `free_response_chats` — comma-separated group IDs exempt from mention
- `mention_patterns` — regex patterns for custom triggers
- Reply-to-bot detection (`quotedParticipant` matches bot ID)
- Bot ID normalization: `1234567890:1234567890` → `1234567890@1234567890`

### 1.4 Formatting

**Markdown → WhatsApp conversion** (`format_message()`):

| Markdown | WhatsApp |
|----------|----------|
| `**bold**` / `__bold__` | `*bold*` |
| `~~strikethrough~~` | `~strikethrough~` |
| `# Header` | `*Header*` |
| `[link](url)` | `text (url)` |
| `` `code` `` | `` `code` `` (protected) |
| ```fenced``` | ```fenced``` (protected) |

Placeholder protection pattern: code blocks and inline code replaced with `\x00FENCE\x00` / `\x00CODE\x00` tokens during conversion, restored afterward.

### 1.5 Media Sending

All media routed through `_send_media_to_bridge()` → `POST /send-media`:

```python
payload = {
    "chatId": chat_id,
    "filePath": file_path,
    "mediaType": "image" | "video" | "document",
    "caption": caption,
    "fileName": file_name,
}
```

### 1.6 Message Limits

- Max message length: 4096 chars (practical UX limit, WhatsApp allows ~65K)
- Chunk delay: 0.3s between chunks to avoid rate limiting

---

## 2. Signal Adapter

### Location

`gateway/platforms/signal.py` (~826 lines)

### Purpose

Signal messenger integration via `signal-cli` daemon running in HTTP mode. Inbound messages via SSE (Server-Sent Events), outbound via JSON-RPC 2.0 over HTTP.

### 2.1 Connection

```
Python Gateway ←SSE→ signal-cli daemon ←Signal Protocol→ Signal Servers
                      (HTTP mode)
```

**Requirements**: `signal-cli daemon --http 127.0.0.1:8080`

**Config**: `SIGNAL_HTTP_URL`, `SIGNAL_ACCOUNT`

**Lifecycle**:
1. Acquire scoped platform lock (`signal-phone`, account)
2. Health check: `GET /api/v1/check`
3. Start SSE listener task (`_sse_listener`)
4. Start health monitor task (`_health_monitor`)

### 2.2 SSE Streaming

```
GET /api/v1/events?account=<encoded>
Headers: Accept: text/event-stream
```

**Retry backoff**: 2s initial, 60s max, with 20% jitter (prevents thundering herd).

**Health monitor**: Checks every 30s. If SSE idle >120s, probes daemon health. If daemon alive but SSE dead → force reconnect by closing response stream.

### 2.3 Message Handling

**Envelope processing** (`_handle_envelope()`):

1. Unwrap nested envelope structure
2. **Note to Self handling**: `syncMessage.sentMessage` with `destinationNumber == account` → promoted to `dataMessage`
3. Self-message filtering: `sender == account` and not Note to Self → drop
4. Story filtering: `storyMessage` → drop (configurable via `ignore_stories`)
5. Edit message support: `editMessage.dataMessage`
6. Group filtering: derived from `SIGNAL_GROUP_ALLOWED_USERS`:
   - Empty → groups disabled (default safe)
   - `"*"` → all groups allowed
   - Specific group IDs → whitelist only
7. Mention rendering: `\uFFFC` (Unicode object replacement char) → `@identifier`
8. Attachment download via JSON-RPC `getAttachment` → base64 decode → magic byte detection → cache

### 2.4 JSON-RPC Communication

```python
payload = {
    "jsonrpc": "2.0",
    "method": "send" | "sendTyping" | "getContact" | "getAttachment",
    "params": {"account": self.account, ...},
    "id": "method_timestamp"
}
POST /api/v1/rpc
```

### 2.5 Echo Prevention

**`_recent_sent_timestamps`** — set tracking outbound message timestamps (max 50 entries). When a syncMessage arrives with a matching timestamp, it's recognized as our own echo and discarded. Prevents reply loops in Note to Self mode.

### 2.6 Sending

| Method | RPC Method | Notes |
|--------|-----------|-------|
| `send()` | `send` | Text message |
| `send_image()` | `send` | With `attachments: [file_path]` |
| `send_document()` | `send` | Generic file attachment |
| `send_voice()` | `send` | No special voice API — routes as attachment |
| `send_video()` | `send` | Video attachment |
| `send_typing()` | `sendTyping` | Single-shot (no loop) |

Max attachment size: 100MB

---

## 3. Matrix Adapter

### Location

`gateway/platforms/matrix.py` (~2024 lines)

### Purpose

Matrix protocol adapter using `mautrix` Python SDK. Connects to any homeserver (self-hosted or matrix.org). Full E2EE support with libolm.

### 3.1 Connection

**Auth methods**:
- Access token (preferred): `MATRIX_ACCESS_TOKEN`
- Password login: `MATRIX_USER_ID` + `MATRIX_PASSWORD`

**E2EE setup** (when `MATRIX_ENCRYPTION=true`):
1. SQLite crypto store: `~/.hermes/platforms/matrix/store/crypto.db`
2. `PgCryptoStore` (SQLite-backed, despite the name)
3. `OlmMachine` from `mautrix.crypto`
4. Device key verification against homeserver
5. Cross-signing via `MATRIX_RECOVERY_KEY` (optional)
6. Trust level: `TrustState.UNVERIFIED` (accepts all devices)

**Legacy cleanup**: Removes `crypto_store.pickle` (legacy pickle format) on startup.

### 3.2 Sync Loop

```
1. Initial sync: client.sync(timeout=10000, full_state=True)
2. Store next_batch token
3. Dispatch sync events via client.handle_sync()
4. Build DM room cache from m.direct account data
5. Share keys (E2EE)
6. Start incremental sync loop (timeout=30000)
7. After each sync: retry pending undecrypted events
```

**Auth error detection**: Sync returns `SyncError` objects (not exceptions) for `M_UNKNOWN_TOKEN`. Detected via `sync_data.message` containing `"m_unknown_token"` → stop immediately.

### 3.3 E2EE Decryption Buffer

**`_pending_megolm`** — buffer for undecrypted events (max 100 entries, 300s TTL):

1. Encrypted event arrives → buffer with timestamp
2. After each sync → `_retry_pending_decryptions()`
3. Try decrypt with new Megolm session keys
4. Success → dispatch to `_on_room_message` (remove from dedup set)
5. Still undecrypted → keep in buffer (or drop if TTL expired)

### 3.4 Message Handling

**Event types**: `ROOM_MESSAGE`, `REACTION`, `INVITE` (auto-join), `ROOM_ENCRYPTED` (buffer)

**`_resolve_message_context()`** — shared mention/thread/DM gating:

1. DM detection: `m.direct` account data cache → 2-member room fallback
2. Thread extraction: `m.relates_to.rel_type == "m.thread"`
3. Mention detection (MSC3952):
   - `m.mentions.user_ids` — authoritative signal (Matrix v1.7)
   - Bot user ID in body text
   - Localpart match (case-insensitive word boundary)
   - `matrix.to/#{user_id}` in formatted_body
4. Mention gating: DMs bypass, free rooms bypass, bot threads bypass
5. DM mention-threads: creates thread on @mention in DM (configurable)
6. Auto-thread: non-DM messages start threads when `MATRIX_AUTO_THREAD=true`
7. Mention stripping from body

**Text batch aggregation**: Matrix clients split messages around 4000 chars. Custom `_enqueue_text_event()` / `_flush_text_batch()` with 0.6s normal delay, 2.0s for split messages (≥3900 chars).

**Reply fallback stripping**: Matrix clients include `> quoted text` fallback in body → stripped after blank line delimiter.

### 3.5 Sending

**`send()`** — truncates, converts markdown to HTML, sends via `send_message_event()`:

```python
msg_content = {
    "msgtype": "m.text",
    "body": chunk,
    "format": "org.matrix.custom.html",       # if HTML differs
    "formatted_body": html,
    "m.relates_to": {                        # thread support
        "rel_type": "m.thread",
        "event_id": thread_id,
        "is_falling_back": True,
    }
}
```

**E2EE send retry**: On encrypted send errors → `crypto.share_keys()` → retry once.

**Media sending**: Upload to homeserver via `upload_media()` → get MXC URL → send media event with `m.image`, `m.audio`, `m.video`, `m.file` msgtypes.

**Voice messages**: MSC3245 native voice via `"org.matrix.msc3245.voice": {}` in content.

### 3.6 Markdown → HTML Conversion

Two-tier:
1. **Primary**: `markdown` library with `fenced_code`, `tables`, `nl2br`, `sane_lists` extensions
2. **Fallback**: Comprehensive regex converter handling:
   - Fenced code blocks (with language class)
   - Inline code
   - Headers (h1-h6)
   - Bold/italic/strikethrough
   - Links with URL sanitization (javascript/data/vbscript blocked)
   - Blockquotes, ordered/unordered lists
   - Horizontal rules
   - HTML escaping of non-protected text

### 3.7 Reactions

**Processing lifecycle**:
- `on_processing_start()` → 👀 eyes reaction
- `on_processing_complete()` → replace eyes with ✅ (success) or ❌ (failure)
- `_pending_reactions` dict tracks (room_id, msg_id) → reaction_event_id

**Incoming reactions**: `_on_reaction()` — logs sender, event, key. Configurable via `MATRIX_REACTIONS=false`.

### 3.8 Room Management

| Method | Purpose |
|--------|---------|
| `create_room()` | Create new room with preset (private/public/trusted_private) |
| `invite_user()` | Invite user to room |
| `set_presence()` | Set online/offline/unavailable |
| `redact_message()` | Delete/redact event |
| `send_read_receipt()` | m.read marker |
| `emote()` | Send /me-style emote (m.emote) |
| `notice()` | Send bot notice (m.notice — ignored by other bots) |

### 3.9 Media Download & Caching

For photos, voice, and encrypted media:
1. Download via `client.download_media(ContentURI(url))`
2. E2EE media: decrypt via `mautrix.crypto.attachments.decrypt_attachment` (needs key, iv, hash from `file` field)
3. Cache via `cache_image_from_bytes()`, `cache_audio_from_bytes()`, `cache_document_from_bytes()`

---

## 4. BlueBubbles Adapter

### Location

`gateway/platforms/bluebubbles.py` (~919 lines)

### Purpose

iMessage integration via BlueBubbles macOS server. Local webhook server for inbound events, REST API for outbound.

### 4.1 Architecture

```
Python Gateway ←aiohttp webhook← BlueBubbles macOS Server ←iMessage
     ↓ httpx REST
BlueBubbles API
```

**Config**: `BLUEBUBBLES_SERVER_URL`, `BLUEBUBBLES_PASSWORD`

### 4.2 Webhook Lifecycle

1. Start local aiohttp webhook server (`DEFAULT: 127.0.0.1:8645/bluebubbles-webhook`)
2. Register webhook with BlueBubbles server via `POST /api/v1/webhook`
   - Events: `["new-message", "updated-message"]`
   - Password embedded in webhook URL (no custom header support)
3. Crash resilience: checks for existing registration before creating duplicate
4. Unregister on disconnect: `DELETE /api/v1/webhook/{id}`

**Authentication**: Inbound webhooks validated via `password` query param or `x-password`/`x-guid`/`x-bluebubbles-guid` headers.

### 4.3 Chat GUID Resolution

BlueBubbles uses GUID format like `iMessage;-;user@example.com`:

1. Check in-memory `_guid_cache`
2. Query chat list: `POST /api/v1/chat/query` with participants
3. Match on `chatIdentifier` or participant `address`
4. If not found + private API enabled → create new chat: `POST /api/v1/chat/new`

### 4.4 Sending

**Text**: `POST /api/v1/message/text` with `chatGuid`, `message`, `tempGuid`

**Attachments**: `POST /api/v1/message/attachment` (multipart upload)

**Reply quoting**: When `private_api_enabled` and `helper_connected`:
- `method: "private-api"`
- `selectedMessageGuid: reply_to`

**Markdown**: Stripped via `strip_markdown()` — no rich formatting support.

### 4.5 Inbound Attachments

Download via `GET /api/v1/attachment/{guid}/download`:
- Images: MIME-based extension mapping (HEIC/HEIF/TIFF → .jpg)
- Audio: X-CAF → .mp3 conversion hint
- Documents: UUID-based filenames

### 4.6 Tapback Reactions

Reaction codes mapped but currently no-op (webhook silently acknowledges):

| Code | Meaning |
|------|---------|
| 2000/3000 | love added/removed |
| 2001/3001 | like added/removed |
| 2002/3002 | dislike added/removed |
| 2003/3003 | laugh added/removed |
| 2004/3004 | emphasize added/removed |
| 2005/3005 | question added/removed |

### 4.7 Read Receipts

`mark_read()` → `POST /api/v1/chat/{guid}/read` (requires private API + helper). Sent automatically on message receipt when `send_read_receipts=true` (default).

### 4.8 PII Redaction

Log helper `_redact()` strips phone numbers (`\+?\d{7,15}`) and email addresses from log output.

---

## 5. Email Adapter

### Location

`gateway/platforms/email.py` (~626 lines)

### Purpose

Email interaction via IMAP (receive) and SMTP (send). Poll-based, no push/IMAP IDLE.

### 5.1 Connection

```
1. Test IMAP connection (IMAP4_SSL)
2. Mark all existing messages as seen (UID SEARCH ALL)
3. Test SMTP connection (STARTTLS)
4. Start poll loop (default: 15s interval)
```

### 5.2 Polling

**`_fetch_new_messages()`** — runs in executor thread (blocking IMAP):

1. IMAP login + INBOX select
2. `UID SEARCH UNSEEN`
3. For each unseen UID (not in `_seen_uids`):
   - Fetch RFC822
   - Decode headers (RFC 2047)
   - Extract text body (multipart traversal: text/plain → text/html with tag stripping)
   - Extract attachments (image → cache_image_from_bytes, document → cache_document_from_bytes)
   - Automated sender detection → skip

**Automated sender detection**:
- Address patterns: `noreply`, `no-reply`, `mailer-daemon`, `postmaster`, `bounce`, `notifications@`, `auto-reply`, etc.
- RFC headers: `Auto-Submitted` (non-"no"), `Precedence` (bulk/list/junk), `X-Auto-Response-Suppress`, `List-Unsubscribe`

**UID tracking**: Bounded set (max 2000 entries). Trimmed by keeping top half (monotonically increasing UIDs).

### 5.3 Threading

**`_thread_context`** — maps sender email → `{subject, message_id}`:
- Outbound emails include `In-Reply-To` and `References` headers
- Subject prepended with `Re:` for replies
- Message-ID format: `<hermes-{uuid}@domain>`

### 5.4 Sending

**`_send_email()`** — executor thread (blocking SMTP):
1. Create `MIMEMultipart`
2. Set threading headers (In-Reply-To, References)
3. Attach plain text body
4. SMTP STARTTLS → login → send_message

**`send_document()`** — adds `MIMEBase` attachment with base64 encoding.

### 5.5 Message Limits

- Max message length: 50,000 chars (Gmail-safe)
- Image URLs: included as plain text links in email body
- No inline HTML formatting — plain text only

---

## 6. HomeAssistant Adapter

### Location

`gateway/platforms/homeassistant.py` (~450 lines)

### Purpose

Monitors Home Assistant `state_changed` events via WebSocket API. Outbound messages delivered as persistent notifications.

### 6.1 Connection

```
1. WS connect to {hass_url}/api/websocket
2. Auth handshake: receive auth_required → send access_token → wait auth_ok
3. Subscribe to state_changed events
4. Start listen loop with reconnection backoff (5s → 10s → 30s → 60s)
```

**REST session**: Dedicated `aiohttp.ClientSession` for outbound `send()` calls (avoids WS race with event listener).

### 6.2 Event Filtering

**Three filter modes** (closed by default — require explicit config):

| Config | Behavior |
|--------|----------|
| `watch_domains: ["light", "sensor"]` | Forward events from listed domains |
| `watch_entities: ["sensor.temp"]` | Forward specific entity IDs |
| `watch_all: true` | Forward all state_changed events |

**`ignore_entities`** — blacklist of entity IDs to always skip.

**Cooldown**: Per-entity cooldown (default 30s) prevents event floods from rapidly-changing entities.

### 6.3 Event Formatting

Domain-specific human-readable formatting:

| Domain | Format |
|--------|--------|
| `climate` | "HVAC mode changed from 'X' to 'Y' (current: 22, target: 24)" |
| `sensor` | "changed from 20°C to 25°C" (with unit) |
| `binary_sensor` | "triggered/cleared (was triggered/cleared)" |
| `light/switch/fan` | "turned on/off" |
| `alarm_control_panel` | "alarm state changed from 'X' to 'Y'" |
| Generic | "changed from 'X' to 'Y'" |

State unchanged → dropped.

### 6.4 Outbound

**`send()`** → `POST /api/services/persistent_notification/create`:

```json
{
    "title": "Hermes Agent",
    "message": "content..."
}
```

Uses REST API instead of WebSocket to avoid race condition with event listener.

---

## 7. Webhook Adapter

### Location

`gateway/platforms/webhook.py` (~673 lines)

### Purpose

Generic webhook receiver for external services (GitHub, GitLab, JIRA, Stripe). Validates HMAC signatures, transforms payloads into agent prompts, routes responses back.

### 7.1 Configuration

Routes defined in `config.yaml` under `platforms.webhook.extra.routes`:

```yaml
routes:
  github-pr:
    events: ["pull_request", "pull_request_review_comment"]
    secret: "hmac-secret-here"
    prompt: "A PR event occurred: {pull_request.title} by {pull_request.user.login}"
    skills: ["github"]
    deliver: "github_comment"
    deliver_extra:
      repo: "owner/repo"
      pr_number: "{pull_request.number}"
```

**Dynamic routes**: Agent-created subscriptions persisted to `~/.hermes/webhook_subscriptions.json`. Reloaded on each POST (mtime-gated).

### 7.2 Security

**HMAC validation** (auth-before-body):
- GitHub: `X-Hub-Signature-256` → `sha256=<hex>` (HMAC-SHA256)
- GitLab: `X-Gitlab-Token` → plain secret comparison
- Generic: `X-Webhook-Signature` → hex HMAC-SHA256
- `INSECURE_NO_AUTH` — skip validation (testing only)

**Rate limiting**: Per-route fixed window (default 30/minute).

**Idempotency**: TTL cache (1 hour) of `delivery_id` values. Prevents duplicate agent runs on webhook retries.

**Body size limit**: 1MB default (`max_body_bytes`). Content-Length checked before reading.

### 7.3 Prompt Rendering

Template supports dot-notation access into nested JSON:

```
{pull_request.title}     → payload["pull_request"]["title"]
{pull_request.user.login} → payload["pull_request"]["user"]["login"]
{__raw__}                → entire payload as indented JSON (truncated to 4000 chars)
```

Nested dicts/lists rendered as JSON (truncated to 2000 chars).

### 7.4 Response Delivery

| Deliver Type | Mechanism |
|-------------|-----------|
| `log` | Logger output only |
| `github_comment` | `gh pr comment` CLI |
| Cross-platform (telegram, discord, etc.) | Route through gateway's connected adapters |

**Cross-platform delivery**: Looks up target adapter by `Platform` enum, resolves chat_id from `deliver_extra` or home channel. Supports thread_id for Telegram forum topics.

**Delivery info lifecycle**: Stored on POST, read by every `send()` for that session chat_id. TTL cleanup on each POST (not on send) so interim status messages don't consume the entry.

### 7.5 Session Key

`webhook:{route_name}:{delivery_id}` — unique delivery_id ensures concurrent webhooks on same route get independent agent runs (no queuing/interrupt).

---

## 8. SMS Adapter (Twilio)

### Location

`gateway/platforms/sms.py` (~374 lines)

### Purpose

SMS via Twilio REST API. Inbound messages received through webhook server, outbound via Twilio Messages API.

### 8.1 Connection

```
1. Validate TWILIO_PHONE_NUMBER configured
2. Validate SMS_WEBHOOK_URL (required for signature validation)
3. Start aiohttp webhook server on port 8080
4. Route: POST /webhooks/twilio
```

**Multi-tenant**: Each inbound phone number gets its own Hermes session.

### 8.2 Twilio Signature Validation

HMAC-SHA1 algorithm per Twilio spec:

```
data_to_sign = url + sorted(params)
computed = base64(hmac_sha1(auth_token, data_to_sign))
```

**Port variant handling**: Twilio may sign with or without default port (443/80). Tries both variants:
- `https://example.com:443/path` → strip port → `https://example.com/path`
- `https://example.com/path` → add port → `https://example.com:443/path`
- Non-standard ports → no variant attempted

**`SMS_INSECURE_NO_SIGNATURE=true`**: Disables validation (dev only, warned in logs).

### 8.3 Message Handling

**Inbound webhook** → `POST /webhooks/twilio` (form-encoded):

1. Parse form data (`From`, `To`, `Body`, `MessageSid`)
2. Echo prevention: ignore messages from own number
3. Build `MessageEvent` with phone number as chat_id/user_id
4. Return empty TwiML `<Response></Response>` (replies via REST API, not inline)

**Outbound** → `POST /2010-04-01/Accounts/{sid}/Messages.json`:
- Basic auth: `Basic base64(sid:auth_token)`
- Form data: `From`, `To`, `Body`
- Truncated to 1600 chars (~10 SMS segments)
- Chunked sending with sequential delivery

### 8.4 Formatting

`strip_markdown()` — SMS renders markdown as literal characters. All formatting stripped.

---

## 9. Adapter Comparison Matrix

| Feature | WhatsApp | Signal | Matrix | BlueBubbles | Email | HomeAssistant | Webhook | SMS |
|---------|----------|--------|--------|-------------|-------|---------------|---------|-----|
| Protocol | HTTP bridge | SSE + JSON-RPC | Matrix SDK (mautrix) | REST + webhook | IMAP/SMTP | WebSocket | HTTP webhook | Twilio REST |
| Inbound | Poll (1s) | SSE stream | Sync loop | Webhook POST | Poll (15s) | WS events | Webhook POST | Webhook POST |
| E2EE | No | Yes (Signal Protocol) | Yes (Olm/Megolm) | No (iMessage E2EE at Apple) | Optional (PGP external) | No | No | No (TLS only) |
| Media | Image/Video/Doc/Voice | Image/Audio/Doc/Video | Image/Audio/Video/Doc | Image/Audio/Video/Doc | Image/Doc | N/A | Via delivery target | Text only |
| Typing | Yes | Yes (single-shot) | Yes | Yes | No | No | No | No |
| Threading | No | No | Yes (m.thread) | No | Subject-based | N/A | Per-delivery | No |
| Reactions | No | No | Yes (👀→✅/❌) | Tapback (no-op) | No | No | No | No |
| Format | WhatsApp markdown | Plain text | Markdown + HTML | Plain text (stripped) | Plain text | Plain text | Template-driven | Plain text (stripped) |
| Max length | 4096 | 8000 | 4000 | 4000 | 50000 | 4096 | Unlimited | 1600 |
| Group support | Yes (mention gating) | Yes (allowlist) | Yes (auto-thread) | Yes (via GUID) | No | N/A | N/A | No |
| Auth | Bridge subprocess | signal-cli daemon | Access token/password | Server password | IMAP/SMTP creds | Long-lived token | HMAC secret per route | Twilio SID/token |
| Dependencies | Node.js + npm | signal-cli | mautrix[encryption] | httpx + aiohttp | stdlib only | aiohttp | aiohttp | aiohttp |
| Proxy support | Via bridge | Via httpx | Via mautrix | Via httpx | N/A | Via aiohttp | N/A | Via aiohttp |

---

## Key Numbers

| Metric | Value |
|--------|-------|
| WhatsApp bridge port | 3000 (default) |
| WhatsApp poll interval | 1 second |
| WhatsApp bridge startup timeout | 15s HTTP + 15s WS = 30s total |
| Signal max attachment | 100 MB |
| Signal SSE retry backoff | 2s → 60s (20% jitter) |
| Signal health check interval | 30 seconds |
| Signal health stale threshold | 120 seconds |
| Matrix crypto store | SQLite (~/.hermes/platforms/matrix/store/crypto.db) |
| Matrix pending megolm buffer | 100 events, 300s TTL |
| Matrix DM cache | m.direct account data |
| Matrix text batch split threshold | 3900 chars |
| BlueBubbles webhook port | 8645 (default) |
| BlueBubbles text limit | 4000 chars |
| Email poll interval | 15 seconds (default) |
| Email UID cache max | 2000 entries |
| Email max message length | 50,000 chars |
| HA reconnection backoff | 5s → 10s → 30s → 60s |
| HA event cooldown | 30 seconds (default) |
| Webhook max body | 1 MB (default) |
| Webhook idempotency TTL | 1 hour |
| Webhook rate limit | 30/minute (default) |
| Webhook default port | 8644 |
| SMS max length | 1600 chars (~10 segments) |
| SMS webhook port | 8080 (default) |
| SMS signature algorithm | HMAC-SHA1 |

---

*Generated from source analysis of the Hermes Agent codebase.*
