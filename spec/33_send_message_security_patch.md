# Hermes Agent — send_message Tool, Security Scanning & Patch Parsing

This document covers three tool modules: the cross-platform send_message tool (18+ platforms), the Tirith security scanner (command-level threat detection), and the V4A patch parser (codex/cline-compatible multi-file patches).

---

## Table of Contents

1. [send_message Tool](#1-send_message-tool)
2. [Per-Platform Send Implementations](#2-per-platform-send-implementations)
3. [Tirith Security Scanner](#3-tirith-security-scanner)
4. [V4A Patch Parser](#4-v4a-patch-parser)
5. [Fuzzy Match Engine](#5-fuzzy-match-engine)

---

## 1. send_message Tool

### Location

`tools/send_message_tool.py` (~1,172 lines)

### Purpose

Cross-platform messaging via REST/API calls. Allows the agent to send messages to any connected messaging platform (Telegram, Discord, Slack, WhatsApp, Signal, Matrix, and 12+ others) without requiring a running gateway adapter — each send is a one-shot API call.

### 1.1 Tool Schema

```python
SEND_MESSAGE_SCHEMA = {
    "name": "send_message",
    "description": "Send a message to a connected messaging platform, or list available targets...",
    "parameters": {
        "type": "object",
        "properties": {
            "action": {"type": "string", "enum": ["send", "list"]},
            "target": {"type": "string",
                "description": "Format: 'platform', 'platform:#channel-name', 'platform:chat_id', or 'platform:chat_id:thread_id'"},
            "message": {"type": "string", "description": "The message text to send"}
        },
        "required": []
    }
}
```

### 1.2 Target Resolution

**Format**: `platform:reference[:thread_id]`

- Bare platform → sends to home channel
- `platform:#channel-name` → resolves via `resolve_channel_name()` from channel_directory
- `platform:chat_id` → direct numeric ID
- `platform:chat_id:thread_id` → for Telegram topics and Discord threads

**Platform-specific target regexes**:
| Platform | Pattern | Example |
|----------|---------|---------|
| Telegram | `^(-?\d+)(?:(\d+))?$` | `-1001234567890:17585` |
| Feishu | `^((?:oc\|ou\|on\|chat\|open)_[-A-Za-z0-9]+)(?:([-A-Za-z0-9_]+))?$` | `oc_xxx123` |
| Weixin | `^((?:wxid\|gh\|v\d+\|wm\|wb)_[A-Za-z0-9_-]+\|[A-Za-z0-9._-]+@chatroom\|filehelper)$` | `wxid_abc123` |
| Discord | Same as Telegram (numeric snowflake) | `999888777:555444333` |
| Matrix | Starts with `!` (room) or `@` (user) | `!roomid:server.org` |

### 1.3 Message Processing

1. Extract `MEDIA:<path>` tags and `[[audio_as_voice]]` directives via `BasePlatformAdapter.extract_media()`
2. Long messages auto-chunked via `BasePlatformAdapter.truncate_message()` preserving code block boundaries
3. Telegram: measures length in UTF-16 code units, not Unicode codepoints
4. Media files sent one-by-one after the text message

### 1.4 Cron Duplicate Skip

When `send_message` is called from a cron job that has `deliver=origin` configured to the same target, the tool detects this and returns a skip notice:

```python
# Env vars set by cron scheduler:
HERMES_CRON_AUTO_DELIVER_PLATFORM
HERMES_CRON_AUTO_DELIVER_CHAT_ID
HERMES_CRON_AUTO_DELIVER_THREAD_ID
```

Returns `{"success": True, "skipped": True, "reason": "cron_auto_delivery_duplicate_target"}`.

### 1.5 Session Mirroring

After successful send, the message is mirrored into the target's gateway session via `gateway.mirror.mirror_to_session()`. This ensures the sent message appears in the conversation history of the target session.

### 1.6 Availability Gate

`_check_send_message()` — requires gateway to be running (for CLI mode). Always available on messaging platforms.

---

## 2. Per-Platform Send Implementations

### 2.1 Telegram

**Method**: `python-telegram-bot` Bot API (one-shot)

**Format detection**: Auto-detects HTML tags via `<[a-zA-Z/][^>]*>` regex. If present → `ParseMode.HTML`; otherwise → `ParseMode.MARKDOWN_V2` via gateway adapter's `format_message()`.

**Fallback**: If parse mode fails, strips MarkdownV2 and retries as plain text.

**Media**: Uploaded via `send_photo`, `send_video`, `send_voice`, `send_audio`, `send_document` based on file extension. Voice extensions (`.ogg`, `.opus`) with `[[audio_as_voice]]` flag → `send_voice()`.

### 2.2 Discord

**Method**: Discord REST API v10 (`/channels/{id}/messages`)

**Transport**: aiohttp with proxy support

**Media**: Multipart/form-data uploads via `aiohttp.FormData()` with `files[0]` field.

**Thread support**: When thread_id provided, sends to `/channels/{thread_id}/messages` directly.

### 2.3 Slack

**Method**: Slack Web API (`chat.postMessage`)

**Format**: Auto-converts markdown to mrkdwn via `SlackAdapter.format_message()`.

### 2.4 WhatsApp

**Method**: Local bridge HTTP API (`localhost:{bridge_port}/send`)

**Config**: `bridge_port` from platform extra config (default 3000).

### 2.5 Signal

**Method**: signal-cli JSON-RPC API

**Endpoint**: `{http_url}/api/v1/rpc` with `{"jsonrpc": "2.0", "method": "send", ...}`

**Routing**: `groupId` for groups, `recipient` list for individuals.

### 2.6 SMS (Twilio)

**Method**: Twilio REST API with HTTP Basic auth

**Markdown stripping**: Full markdown removal (bold, italic, code, headers, links) since SMS renders markdown as literal characters.

**Auth**: `Authorization: Basic {base64(account_sid:auth_token)}`

### 2.7 Email

**Method**: SMTP (one-shot, stdlib `smtplib`)

**Config**: `EMAIL_ADDRESS`, `EMAIL_PASSWORD`, `EMAIL_SMTP_HOST`, `EMAIL_SMTP_PORT` (default 587).

**TLS**: `starttls()` with `ssl.create_default_context()`.

### 2.8 Matrix

**Method**: Matrix Client-Server API (`/_matrix/client/v3/rooms/{room}/send/m.room.message/{txn_id}`)

**Format**: Converts markdown to HTML via `markdown` library with fenced_code and tables extensions. H1-H6 converted to `<strong>` for Element X compatibility.

**Fallback**: Plain text if `markdown` library not installed.

### 2.9 Other Platforms

| Platform | Method | Required Config |
|----------|--------|----------------|
| Mattermost | REST API (`/api/v4/posts`) | `MATTERMOST_URL`, `MATTERMOST_TOKEN` |
| Home Assistant | REST API (`/api/services/notify/notify`) | `HASS_URL`, `HASS_TOKEN` |
| DingTalk | Robot webhook (POST JSON) | `DINGTALK_WEBHOOK_URL` |
| WeCom | WeComAdapter (WebSocket send) | `WECOM_BOT_ID`, `WECOM_SECRET` |
| Weixin | `send_weixin_direct()` (iLink) | `WEIXIN_TOKEN`, `WEIXIN_ENCODING_AES_KEY` |
| BlueBubbles | BlueBubblesAdapter (REST API) | `BLUEBUBBLES_SERVER_URL`, `BLUEBUBBLES_PASSWORD` |
| Feishu | FeishuAdapter (lark_oapi SDK) | `FEISHU_APP_ID`, `FEISHU_APP_SECRET` |
| QQBot | QQ Bot REST API (token + POST) | `QQ_APP_ID`, `QQ_CLIENT_SECRET` |

---

## 3. Tirith Security Scanner

### Location

`tools/tirith_security.py` (~670 lines)

### Purpose

Pre-execution security scanning of shell commands. Runs the `tirith` binary (Rust CLI) as a subprocess to detect content-level threats: homograph URLs, pipe-to-interpreter attacks, terminal injection, and other command injection patterns.

### 3.1 Verdict Model

| Exit Code | Action | Meaning |
|-----------|--------|---------|
| 0 | `allow` | Command is safe |
| 1 | `block` | Threat detected |
| 2 | `warn` | Suspicious but not blocked |
| Other | respects `fail_open` | Unknown → config determines action |

**JSON stdout** enriches findings/summary but **never overrides the exit code verdict**.

### 3.2 Configuration

```python
defaults = {
    "tirith_enabled": True,
    "tirith_path": "tirith",
    "tirith_timeout": 5,
    "tirith_fail_open": True,
}
```

**Env var overrides**: `TIRITH_ENABLED`, `TIRITH_BIN`, `TIRITH_TIMEOUT`, `TIRITH_FAIL_OPEN`.

**Config source**: `config.yaml → security:` section.

### 3.3 Auto-Install System

**Resolution order**:
1. `shutil.which("tirith")` — PATH lookup
2. `$HERMES_HOME/bin/tirith` — previously auto-installed
3. Auto-download from GitHub releases → `$HERMES_HOME/bin/tirith`

**Platform detection**: Maps `platform.system()` + `platform.machine()` to Rust targets:
- `x86_64-apple-darwin`, `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`

**Verification** (two layers):
1. **Cosign provenance** (preferred): Verifies `checksums.txt` was signed by the expected GitHub Actions workflow (`https://github.com/sheeki03/tirith/.github/workflows/release.yml@refs/tags/v...`). Uses `cosign verify-blob` with certificate identity and OIDC issuer validation.
2. **SHA-256 checksum**: Always performed. If cosign unavailable, SHA-256 + HTTPS still provides integrity.

**Download flow**:
```
1. Detect platform target triple
2. Download archive, checksums.txt, (.sig, .pem if cosign available)
3. Verify cosign provenance (if cosign on PATH) → must succeed, not optional
4. Verify SHA-256 checksum → must match
5. Extract tirith binary from archive (path traversal guard: reject "..")
6. Install to $HERMES_HOME/bin/tirith with executable permissions
```

**Failure persistence**:
- In-memory: `_resolved_path = _INSTALL_FAILED` sentinel (cached for process lifetime)
- On-disk: `.tirith-install-failed` marker file (24h TTL)
- Retryable failures: `cosign_missing` — auto-cleared when cosign becomes available on PATH
- Background install: daemon thread, never blocks startup

### 3.4 Fail-Open/Fail-Closed

```python
if fail_open:
    return {"action": "allow", ...}  # command proceeds without scanning
else:
    return {"action": "block", ...}  # command blocked when scanner unavailable
```

**Operational failures** (spawn error, timeout, unknown exit code) respect `fail_open`. Programming errors propagate.

### 3.5 Scan Execution

```python
result = subprocess.run(
    [tirith_path, "check", "--json", "--non-interactive",
     "--shell", "posix", "--", command],
    capture_output=True, text=True, timeout=timeout,
)
```

**JSON enrichment**: Parses `findings` (max 50) and `summary` (max 500 chars) from stdout. If JSON parse fails, degrades to action-only verdict.

---

## 4. V4A Patch Parser

### Location

`tools/patch_parser.py` (~580 lines)

### Purpose

Parses V4A patch format used by codex, cline, and other coding agents. Supports multi-file patches with add, update, delete, and move operations.

### 4.1 V4A Format

```
*** Begin Patch
*** Update File: path/to/file.py
@@ optional context hint @@
 context line (space prefix)
-removed line (minus prefix)
+added line (plus prefix)
*** Add File: path/to/new.py
+new file content
+line 2
*** Delete File: path/to/old.py
*** Move File: old/path.py -> new/path.py
*** End Patch
```

### 4.2 Data Model

```python
class OperationType(Enum):
    ADD = "add"
    UPDATE = "update"
    DELETE = "delete"
    MOVE = "move"

@dataclass
class HunkLine:
    prefix: str   # ' ', '-', or '+'
    content: str

@dataclass
class Hunk:
    context_hint: Optional[str] = None
    lines: List[HunkLine] = field(default_factory=list)

@dataclass
class PatchOperation:
    operation: OperationType
    file_path: str
    new_path: Optional[str] = None  # For MOVE
    hunks: List[Hunk] = field(default_factory=list)
    content: Optional[str] = None   # For ADD
```

### 4.3 Parsing

- Markers: `*** Begin Patch`, `*** End Patch` (also accepts `***Begin Patch` without space)
- File operations: `*** Update File:`, `*** Add File:`, `*** Delete File:`, `*** Move File: X -> Y`
- Hunk lines: `+` (add), `-` (remove), ` ` (context), `\` (no newline marker — skipped)
- Lines without prefix → treated as context (implicit space)

**Validation**:
- Empty file path → error
- UPDATE with no hunks → error
- MOVE without destination → error
- Empty patch → `[]` (not an error)

### 4.4 Two-Phase Apply

**Phase 1: Validate** — `_validate_operations()`

- UPDATE: Simulates each hunk in order using `fuzzy_find_and_replace()` against current file content. Later hunks validate against post-earlier-hunk content.
- DELETE: Checks file exists
- MOVE: Checks source exists, destination does NOT exist
- ADD: No pre-check (parent dirs created by write_file)

If any validation error → returns immediately with **no filesystem changes**.

**Phase 2: Apply** — `_apply_update()`, `_apply_add()`, `_apply_delete()`, `_apply_move()`

- UPDATE: Applies hunks sequentially via fuzzy match, generates unified diff
- ADD: Extracts `+` lines from hunks as file content
- DELETE: Reads file first (for diff), then deletes, generates unified diff to `/dev/null`
- MOVE: Uses `file_ops.move_file()`, generates `# Moved: src -> dst` marker

**Post-apply**: Runs lint check on all modified/created files via `file_ops._check_lint()`.

### 4.5 Context Hint Handling (UPDATE)

**Addition-only hunks** (no context or removed lines, only `+` lines):
1. If context hint present → find it in content, insert after the line containing it
2. If hint ambiguous (>1 occurrence) → error
3. If hint not found → append at end of file (safe fallback)

**Context hint window fallback**: When fuzzy match fails for a hunk but a context hint exists, searches in a 500-2000 character window around the hint position.

---

## 5. Fuzzy Match Engine

### Location

`tools/fuzzy_match.py` (~566 lines)

### Purpose

Multi-strategy matching chain to robustly find and replace text, accommodating variations in whitespace, indentation, escaping, and Unicode common in LLM-generated code.

### 5.1 9-Strategy Chain (tried in order)

| # | Strategy | What It Handles |
|---|----------|----------------|
| 1 | `exact` | Direct `str.find()` match |
| 2 | `line_trimmed` | Strip leading/trailing whitespace per line |
| 3 | `whitespace_normalized` | Collapse multiple spaces/tabs to single space |
| 4 | `indentation_flexible` | Strip all leading whitespace from lines |
| 5 | `escape_normalized` | Convert `\n` literals → actual newlines, `\t` → tabs |
| 6 | `trimmed_boundary` | Trim whitespace from first and last lines only |
| 7 | `unicode_normalized` | Smart quotes, em/en-dashes, ellipsis, NBSP → ASCII |
| 8 | `block_anchor` | Match first+last lines exactly, require ≥50%/≥70% middle similarity |
| 9 | `context_aware` | ≥50% of lines must have ≥80% similarity (SequenceMatcher) |

### 5.2 Multi-Occurrence Handling

When `replace_all=False` and a strategy finds >1 match → error: "Found N matches. Provide more context to make it unique, or use replace_all=True."

When `replace_all=True` → all occurrences replaced.

### 5.3 Unicode Normalization (Strategy 7)

```python
UNICODE_MAP = {
    "\u201c": '"', "\u201d": '"',  # smart double quotes
    "\u2018": "'", "\u2019": "'",  # smart single quotes
    "\u2014": "--", "\u2013": "-", # em/en dashes
    "\u2026": "...", "\u00a0": " ", # ellipsis and non-breaking space
}
```

**Position mapping**: Because some replacements expand a single character into multiple (em-dash → `--`, ellipsis → `...`), builds `_build_orig_to_norm_map()` to map normalized positions back to original positions.

### 5.4 Block Anchor (Strategy 8)

```
1. Normalize unicode in both content and pattern
2. Match first and last lines (stripped) exactly
3. If 1 candidate → 50% middle similarity threshold
4. If >1 candidate → 70% middle similarity threshold
5. Middle similarity: SequenceMatcher(None, content_middle, pattern_middle).ratio()
```

**Previous thresholds were 10%/30%** — dangerously loose. Raised to 50%/70% to prevent matching unrelated blocks.

### 5.5 Context Aware (Strategy 9)

Sliding window over content lines. For each window of `len(pattern_lines)`:
- Compare each line pair with `SequenceMatcher`
- Count lines with ≥80% similarity
- Require ≥50% of lines to meet threshold

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Platforms supported by send_message | 18+ |
| Secret patterns in error redaction | 2 regex patterns |
| Media extension sets | 4 (image, video, audio, voice) |
| Tirith exit codes | 3 (0=allow, 1=block, 2=warn) |
| Tirith default timeout | 5 seconds |
| Tirith install failure TTL | 24 hours |
| Tirith max findings | 50 |
| Tirith max summary length | 500 chars |
| V4A operation types | 4 (add, update, delete, move) |
| Fuzzy match strategies | 9 |
| Unicode characters normalized | 8 |
| Block anchor single-candidate threshold | 50% |
| Block anchor multi-candidate threshold | 70% |
| Context-aware line similarity threshold | 80% |
| Context-aware required line ratio | 50% |
| Context hint search window | 500 chars before, 2000 chars after |

---

*Generated from source analysis of the Hermes Agent codebase.*
