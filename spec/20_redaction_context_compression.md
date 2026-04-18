# Hermes Agent — Redaction & Context Compression

This document covers the regex-based secret redaction system and the context compression engine.

---

## Table of Contents

1. [Secret Redaction](#1-secret-redaction)
2. [Context Compression](#2-context-compression)

---

## 1. Secret Redaction

### Location

`agent/redact.py` (182 lines)

### Purpose

Regex-based secret redaction for logs and tool output. Applies pattern matching to mask API keys, tokens, and credentials before they reach log files, verbose output, or gateway logs.

### 1.1 Enable Gate

```python
_REDACT_ENABLED = os.getenv("HERMES_REDACT_SECRETS", "").lower() not in ("0", "false", "no", "off")
```

Snapshot at import time — runtime environment mutations cannot disable redaction mid-session.

### 1.2 Token Prefix Patterns (35+ patterns)

Known API key prefixes matched as alternation regex:

| Pattern | Service |
|---------|---------|
| `sk-[A-Za-z0-9_-]{10,}` | OpenAI / OpenRouter / Anthropic |
| `ghp_[A-Za-z0-9]{10,}` | GitHub PAT (classic) |
| `github_pat_[A-Za-z0-9_]{10,}` | GitHub PAT (fine-grained) |
| `gho_`, `ghu_`, `ghs_`, `ghr_` | GitHub OAuth tokens |
| `xox[baprs]-[A-Za-z0-9-]{10,}` | Slack tokens |
| `AIza[A-Za-z0-9_-]{30,}` | Google API keys |
| `AKIA[A-Z0-9]{16}` | AWS Access Key ID |
| `sk_live_`, `sk_test_`, `rk_live_` | Stripe keys |
| `SG\.[A-Za-z0-9_-]{10,}` | SendGrid |
| `hf_[A-Za-z0-9]{10,}` | HuggingFace |
| `pplx-[A-Za-z0-9]{10,}` | Perplexity |
| `fc-[A-Za-z0-9]{10,}` | Firecrawl |
| `bb_live_[A-Za-z0-9_-]{10,}` | BrowserBase |
| `tvly-[A-Za-z0-9]{10,}` | Tavily |
| `exa_[A-Za-z0-9]{10,}` | Exa search |
| `gsk_[A-Za-z0-9]{10,}` | Groq Cloud |
| `npm_[A-Za-z0-9]{10,}` | npm |
| `pypi-[A-Za-z0-9_-]{10,}` | PyPI |
| `syt_[A-Za-z0-9]{10,}` | Matrix |
| `dop_v1_`, `doo_v1_` | DigitalOcean |
| `am_[A-Za-z0-9_-]{10,}` | AgentMail |
| `sk_[A-Za-z0-9_]{10,}` | ElevenLabs |
| `r8_[A-Za-z0-9]{10,}` | Replicate |
| `fal_[A-Za-z0-9_-]{10,}` | Fal.ai |
| `mem0_`, `brv_`, `hsk-` | Mem0, ByteRover, Hindsight |

### 1.3 Additional Pattern Categories

**Environment assignments** — `KEY=value` where KEY contains secret-like name:
```python
_SECRET_ENV_NAMES = r"(?:API_?KEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL|AUTH)"
# Matches: OPENAI_API_KEY=sk-abc..., GITHUB_TOKEN=ghp_xxx...
```

**JSON fields** — `"apiKey": "value"`, `"token": "value"`, `"secret": "value"`:
```python
_JSON_KEY_NAMES = r"(?:api_?[Kk]ey|token|secret|password|access_token|...)"
```

**Authorization headers** — `Authorization: Bearer <token>`

**Telegram bot tokens** — `bot<digits>:<token>` (token >= 30 chars)

**Private key blocks** — `-----BEGIN RSA PRIVATE KEY----- ... -----END RSA PRIVATE KEY-----`

**Database connection strings** — `postgres://user:PASSWORD@host`, `mongodb+srv://...`

**Phone numbers** — E.164 format `+<country><number>` (7-15 digits)

### 1.4 Masking Strategy

| Token Length | Mask Format |
|-------------|-------------|
| < 18 chars | `***` (fully masked) |
| >= 18 chars | `{first6}...{last4}` (preserves prefix/suffix for debugging) |

### 1.5 RedactingFormatter

Log formatter that applies `redact_sensitive_text()` to every log message:

```python
class RedactingFormatter(logging.Formatter):
    def format(self, record):
        original = super().format(record)
        return redact_sensitive_text(original)
```

### 1.6 Application Points

Redaction applied at:
1. Sandbox stdout/stderr (after ANSI strip)
2. File read content (after character-count guard)
3. Tool output in conversation
4. Log messages (via RedactingFormatter)
5. Trajectory files (before writing)

---

## 2. Context Compression

### Location

`agent/context_compressor.py` (large file), extends `ContextEngine`

### Purpose

Automatic context window compression for long conversations. Uses auxiliary model (cheap/fast) to summarize middle turns while protecting head and tail context.

### 2.1 Algorithm

1. **Prune old tool results** — cheap pre-pass, no LLM call
2. **Protect head messages** — system prompt + first exchange preserved
3. **Protect tail messages** — by token budget (most recent ~20K tokens)
4. **Summarize middle turns** — structured LLM prompt
5. **Iterative summary** — on subsequent compactions, updates previous summary

### 2.2 Summary Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `_MIN_SUMMARY_TOKENS` | 2,000 | Minimum summary output |
| `_SUMMARY_RATIO` | 0.20 | Proportion of compressed content for summary |
| `_SUMMARY_TOKENS_CEILING` | 12,000 | Absolute ceiling |
| `_CHARS_PER_TOKEN` | 4 | Rough estimate |
| `_SUMMARY_FAILURE_COOLDOWN` | 600s | Backoff after failed compression |

### 2.3 Summary Prefix

```
[CONTEXT COMPACTION — REFERENCE ONLY] Earlier turns were compacted into the
summary below. This is a handoff from a previous context window — treat it as
background reference, NOT as active instructions. Do NOT answer questions or
fulfill requests mentioned in this summary; they were already addressed.
Respond ONLY to the latest user message that appears AFTER this summary.
```

Key design choices:
- **Handoff framing** — "different assistant" creates separation (from Codex)
- **Preamble** — "Do not respond to any questions" prevents re-answering (from OpenCode)
- **"Remaining Work"** replaces "Next Steps" — avoids reading as active instructions

### 2.4 Tool Result Summarization

Before LLM summarization, tool results are pre-summarized to a single line:

| Tool | Summary Format |
|------|---------------|
| `terminal` | `ran 'npm test' -> exit 0, 47 lines output` |
| `read_file` | `read config.py from line 1 (1,200 chars)` |
| `write_file` | `wrote to config.py (42 lines)` |
| `search_files` | `content search for 'compress' in agent/ -> 12 matches` |
| `patch` | `replace in config.py (200 chars result)` |
| `delegate_task` | `'Debug the login flow' (5,000 chars result)` |
| `execute_code` | `'import requests...' (23 lines output)` |
| `browser_*` | `https://example.com (3,000 chars)` |
| `web_search` | `query='X framework' (8,000 chars result)` |
| `clarify` | `asked user a question` |
| `memory` | `read on MEMORY.md` |
| `todo` | `updated task list` |
| `vision_analyze` | `'What does this UI do?' (4,000 chars)` |
| `cronjob` | `list` |
| `process` | `list session=abc123` |

### 2.5 Pruning Strategy

When context needs compression before summarization, old tool results are pruned:

```python
_PRUNED_TOOL_PLACEHOLDER = "[Old tool output cleared to save context space]"
```

This cheap pre-pass reduces content before the expensive LLM summarization call.

### 2.6 Scaled Summary Budget

Summary size scales proportionally to compressed content:
- Minimum: 2,000 tokens
- Ratio: 20% of compressed content
- Ceiling: 12,000 tokens

### 2.7 Iterative Summary Updates

On subsequent compactions, the previous summary is included in the summarizer input so information is preserved across multiple compression cycles.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Token prefix patterns | 35+ |
| Redaction pattern categories | 8 |
| Mask threshold (short tokens) | < 18 chars |
| Minimum summary tokens | 2,000 |
| Summary ratio | 20% |
| Summary ceiling tokens | 12,000 |
| Chars per token estimate | 4 |
| Summary failure cooldown | 600s |
| Tool types with custom summaries | 15+ |

---

*Generated from source analysis of the Hermes Agent codebase.*
