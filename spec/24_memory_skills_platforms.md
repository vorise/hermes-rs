# Hermes Agent — Memory & Skills Systems

This document covers the file-backed memory system (MEMORY.md/USER.md), the skills progressive disclosure architecture, and platform adapter details.

---

## Table of Contents

1. [Memory System](#1-memory-system)
2. [Skills System](#2-skills-system)
3. [Platform Adapters](#3-platform-adapters)

---

## 1. Memory System

### Location

`tools/memory_tool.py` (~500 lines)

### Purpose

Bounded, file-backed persistent memory that survives across sessions. Two stores: MEMORY.md (agent notes, environment facts, project conventions) and USER.md (user preferences, communication style, workflow habits). Uses a frozen snapshot pattern to preserve prefix cache stability.

### 1.1 Frozen Snapshot Pattern

```
Session Start:
  load_from_disk() → read MEMORY.md + USER.md
  → Capture _system_prompt_snapshot (frozen, never mutated mid-session)
  → System prompt injected with frozen snapshot

Mid-session:
  memory tool calls → mutate live entries → write to disk
  → Tool responses show LIVE state
  → System prompt still references FROZEN state (prefix cache preserved)

Next Session:
  load_from_disk() → reads updated files → new frozen snapshot
```

This is critical: mid-session writes update files but do NOT change the system prompt, preserving the Anthropic prompt cache prefix for the entire session.

### 1.2 MemoryStore

```python
class MemoryStore:
    memory_entries: List[str]      # Live state
    user_entries: List[str]        # Live state
    memory_char_limit = 2200       # Character limit (not tokens)
    user_char_limit = 1375         # Character limit (not tokens)
    _system_prompt_snapshot: Dict  # Frozen at load time
```

Character limits (not tokens) because char counts are model-independent.

### 1.3 Entry Format

Delimiter: `\n§\n` (section sign with newlines). Entries can be multiline.

File content:
```
Entry one text here

§

Entry two text here
```

### 1.4 Atomic File Writes

```python
def _write_file(path, entries):
    fd, tmp_path = tempfile.mkstemp(dir=parent, suffix=".tmp", prefix=".mem_")
    # Write content + fsync
    os.replace(tmp_path, path)  # Atomic rename on same filesystem
```

Readers always see either the old complete file or the new complete file — no empty-file race window.

### 1.5 File Locking

Separate `.lock` file for read-modify-write safety:
- Unix: `fcntl.flock(fd, LOCK_EX)`
- Windows: `msvcrt.locking(fd.fileno(), LK_LOCK, 1)`

Lock acquired before reload + mutate + save cycle.

### 1.6 Operations

| Action | Behavior |
|--------|----------|
| `add` | Append new entry, reject if over char limit or duplicate |
| `replace` | Find entry containing `old_text` substring, replace entire entry |
| `remove` | Find entry containing `old_text` substring, delete it |
| `read` | Return current entries with usage info |

**Substring matching**: `replace`/`remove` use short unique substring matching (not full text or IDs). If multiple entries match:
- If all matches are identical text → operate on first one
- If matches differ → error: "Multiple entries matched. Be more specific."

### 1.7 Threat Scanning

All content scanned for injection/exfiltration before acceptance:

**Prompt injection patterns**:
- `ignore previous/all/above/prior instructions`
- `you are now `
- `do not tell the user`
- `system prompt override`
- `disregard your/all/any instructions/rules/guidelines`
- `act as if/though you have no restrictions/limits/rules`

**Exfiltration patterns**:
- `curl ... ${KEY|TOKEN|SECRET|PASSWORD|...}`
- `wget ... ${KEY|TOKEN|SECRET|...}`
- `cat ... .env|credentials|.netrc|.pgpass|.npmrc|.pypirc`

**Persistence patterns**:
- `authorized_keys`
- `$HOME/.ssh` or `~/.ssh`
- `$HOME/.hermes/.env` or `~/.hermes/.env`

**Invisible unicode**: Zero-width spaces, direction overrides (`\u200b`–`\u200d`, `\u2060`, `\ufeff`, `\u202a`–`\u202e`).

### 1.8 System Prompt Block Rendering

```
══════════════════════════════════════════════
MEMORY (your personal notes) [65% — 1,430/2,200 chars]
══════════════════════════════════════════════
Entry one
§
Entry two
```

Returns `None` if snapshot is empty (no entries at load time).

### 1.9 Deduplication

On load: `list(dict.fromkeys(entries))` — preserves order, keeps first occurrence.

---

## 2. Skills System

### Location

`tools/skills_tool.py` (~800+ lines), `agent/skill_utils.py`

### Purpose

Progressive disclosure architecture for skill-based tool instructions. Inspired by Anthropic's Claude Skills system. Skills are directories containing a SKILL.md file with YAML frontmatter.

### 2.1 Directory Structure

```
~/.hermes/skills/
├── mlops/
│   └── axolotl/
│       ├── SKILL.md           # Main instructions (required)
│       ├── references/        # Supporting documentation
│       ├── templates/         # Templates for output
│       └── assets/            # Supplementary files
└── category/
    └── another-skill/
        └── SKILL.md
```

External skill directories configurable via `skills.external_dirs` in config.yaml.

### 2.2 SKILL.md Format

```yaml
---
name: skill-name                    # Required, max 64 chars
description: Brief description       # Required, max 1024 chars
version: 1.0.0                      # Optional
license: MIT                        # Optional
platforms: [macos]                  # Optional — macos, linux, windows
prerequisites:                      # Optional — legacy requirements
  env_vars: [API_KEY]
  commands: [curl, jq]
compatibility: Requires X           # Optional
metadata:                           # Optional, arbitrary key-value
  hermes:
    tags: [fine-tuning, llm]
    related_skills: [peft, lora]
setup:                              # Optional — setup guidance
  help: "Visit example.com to get a key"
  collect_secrets:                  # Secret collection prompts
    - env_var: API_KEY
      prompt: "Enter your API key"
      provider_url: "https://example.com"
      secret: true
required_environment_variables:     # Required env vars with prompts
  - name: API_KEY
    prompt: "Enter your API key"
    help: "Get it from example.com"
    required_for: "Authentication"
    optional: false
---

# Skill Title

Full instructions and content here...
```

### 2.3 Progressive Disclosure

**Tier 1 — skills_list**: Returns only name + description + category. Minimal token usage.

**Tier 2 — skill_view(name)**: Loads full SKILL.md content, tags, linked files.

**Tier 3 — skill_view(name, "references/file.md")**: Loads specific linked file within a skill.

### 2.4 Platform Matching

```python
_PLATFORM_MAP = {
    "macos": "darwin",
    "linux": "linux",
    "windows": "win32",
}

def skill_matches_platform(frontmatter) -> bool:
    # If no 'platforms' field → matches all platforms
    # Otherwise checks sys.platform against mapped prefixes
```

### 2.5 Skill Discovery

`_find_all_skills()` — recursive scan:
1. Scan `~/.hermes/skills/` (local skills)
2. Scan external dirs from `get_external_skills_dirs()`
3. Parse frontmatter from each SKILL.md
4. Filter by platform compatibility
5. Filter out disabled skills
6. Exclude `.git`, `.github`, `.hub` directories
7. Local skills take precedence over external (by name)

### 2.6 Disabled Skills

Two-level disabling:
- **Global**: `skills.disabled: [skill-name]` in config.yaml
- **Per-platform**: `skills.platform_disabled.linux: [skill-name]`

Resolved via `_is_skill_disabled(name, platform)` → checks `HERMES_PLATFORM` env var.

### 2.7 Environment Variable Management

`_get_required_environment_variables()` collects from multiple sources:
1. `required_environment_variables` frontmatter field
2. `setup.collect_secrets` entries
3. Legacy `prerequisites.env_vars`

**Secret capture flow** (CLI only):
```python
_capture_required_environment_variables(skill_name, missing_entries)
  → _secret_capture_callback(env_var, prompt, metadata)
  → Interactive prompt for user to enter value
  → Stored in ~/.hermes/.env
```

Gateway surface: returns missing names without prompting (secret capture unsupported over messaging).

### 2.8 Readiness Status

```python
class SkillReadinessStatus(str, Enum):
    AVAILABLE = "available"
    SETUP_NEEDED = "setup_needed"      # Missing env vars
    UNSUPPORTED = "unsupported"        # Platform mismatch
```

### 2.9 Prompt Injection Detection

Skills scanned for injection patterns before serving:
```python
_INJECTION_PATTERNS = [
    "ignore previous instructions", "ignore all previous",
    "you are now", "disregard your", "forget your instructions",
    "new instructions:", "system prompt:", "<system>", "]]>",
]
```

Local skills: logged but still served. Plugin skills: logged but still served.

### 2.10 Plugin Skills

Skills served by plugins (namespaced as `namespace:skill_name`):
- Plugin must be enabled: `hermes plugins enable {namespace}`
- Bundle context banner injected: lists sibling skills in the plugin
- Agent can invoke sibling skills via qualified form: `namespace:sibling`

### 2.11 Category Descriptions

Category directories may contain `DESCRIPTION.md` with frontmatter:
```yaml
---
description: Category description here
---
```

Parsed and included in `skills_list` output when browsing categories.

### 2.12 Tag Parsing

Handles multiple formats:
- YAML list: `[tag1, tag2]` (already parsed by yaml.safe_load)
- Bracket string: `"[tag1, tag2]"`
- Comma string: `"tag1, tag2"`

---

## 3. Platform Adapters

### Location

`gateway/platforms/` (24 files)

### Purpose

Platform-specific adapters for messaging integrations. Each adapter implements a common interface defined by `base.py`.

### 3.1 Supported Platforms

| Platform | File(s) | Notes |
|----------|---------|-------|
| Telegram | `telegram.py`, `telegram_network.py` | Primary platform |
| Discord | `discord.py` | Native GIF animation support |
| Slack | `slack.py` | |
| WhatsApp | `whatsapp.py` | JID/LID normalization |
| Signal | `signal.py` | Group + DM support |
| Matrix | `matrix.py` | Typing indicator handling |
| BlueBubbles | `bluebubbles.py` | iMessage bridge |
| Email | `email.py` | IMAP/SMTP |
| Home Assistant | `homeassistant.py` | Smart home integration |
| Webhook | `webhook.py` | Generic HTTP webhook |
| API Server | `api_server.py` | REST API |
| Feishu | `feishu.py` | Lark/Feishu |
| DingTalk | `dingtalk.py` | Alibaba |
| WeCom | `wecom.py`, `wecom_callback.py`, `wecom_crypto.py` | WeChat Work |
| Weixin | `weixin.py` | WeChat |
| QQ Bot | `qqbot.py` | QQ platform |
| SMS | `sms.py` | SMS gateway |

### 3.2 Base Adapter Interface

```python
class BasePlatform(ABC):
    @abstractmethod
    async def connect(self): ...
    @abstractmethod
    async def disconnect(self): ...
    @abstractmethod
    async def send_message(self, chat_id, message, **kwargs): ...

    @property
    def platform(self) -> Platform: ...
    @property
    def fatal_error_retryable(self) -> bool: ...
```

### 3.3 PII-Safe Platforms

For WhatsApp, Signal, Telegram, and BlueBubbles, user/chat IDs are hashed before being shown to the agent:

```python
def _hash_sender_id(value: str) -> str:
    return f"user_{sha256(value)[:12]}"
```

Discord excluded — needs raw IDs for `<@user_id>` mentions.

### 3.4 Platform Helpers

`helpers.py` — shared utilities across platforms:
- Message formatting
- Mention parsing
- Media handling
- Rate limiting

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Memory stores | 2 (MEMORY.md, USER.md) |
| Memory char limit (memory) | 2,200 |
| Memory char limit (user) | 1,375 |
| Entry delimiter | `\n§\n` |
| Memory threat patterns | 12 |
| Invisible unicode chars blocked | 9 |
| Skill name max length | 64 chars |
| Skill description max length | 1,024 chars |
| Skill content read limit | 4,000 chars (for listing) |
| Excluded skill directories | 3 (.git, .github, .hub) |
| Platform identifiers | 3 (macos, linux, windows) |
| Injection patterns | 9 |
| Supported platforms | 17+ |
| PII-safe platforms | 4 |
| File write method | Atomic rename (tempfile + os.replace) |

---

*Generated from source analysis of the Hermes Agent codebase.*
