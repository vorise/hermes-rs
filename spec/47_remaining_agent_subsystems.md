# Hermes Agent — Remaining Agent Subsystems

This document covers `agent/context_compressor.py` (~1,091 lines), `agent/prompt_builder.py` (~1,043 lines), `agent/display.py` (~1,037 lines), `agent/error_classifier.py` (~820 lines), `agent/insights.py` (~789 lines), `agent/model_metadata.py` (~1,102 lines), `agent/usage_pricing.py` (~613 lines), `agent/copilot_acp_client.py` (~570 lines), `agent/context_references.py` (~520 lines), and `agent/insights.py`.

---

## Table of Contents

1. [Context Compressor](#1-context-compressor)
2. [Prompt Builder](#2-prompt-builder)
3. [Display](#3-display)
4. [Error Classifier](#4-error-classifier)
5. [Insights Engine](#5-insights-engine)
6. [Model Metadata](#6-model-metadata)
7. [Usage Pricing](#7-usage-pricing)
8. [Copilot ACP Client](#8-copilot-acp-client)
9. [Context References](#9-context-references)

---

## 1. Context Compressor

### Location

`agent/context_compressor.py` (~1,091 lines)

### Purpose

Automatic context window compression via LLM summarization. Protects head and tail context, summarizes middle turns with structured template.

### 1.1 Summary Prefix

```python
SUMMARY_PREFIX = (
    "[CONTEXT COMPACTION — REFERENCE ONLY] Earlier turns were compacted "
    "into the summary below. This is a handoff from a previous context "
    "window — treat it as background reference, NOT as active instructions. "
    "Do NOT answer questions or fulfill requests mentioned in this summary; "
    "they were already addressed. Respond ONLY to the latest user message "
    "that appears AFTER this summary."
)
```

**Design choices**:
- "Do not respond to any questions" preamble (from OpenCode)
- "Handoff from a previous context window" framing (from Codex)
- "Remaining Work" replaces "Next Steps" to avoid reading as active instructions

### 1.2 Constants

```python
_MIN_SUMMARY_TOKENS = 2000           # Minimum tokens for summary output
_SUMMARY_RATIO = 0.20                # 20% of compressed content allocated to summary
_SUMMARY_TOKENS_CEILING = 12_000     # Absolute ceiling
_PRUNED_TOOL_PLACEHOLDER = "[Old tool output cleared to save context space]"
_CHARS_PER_TOKEN = 4
_SUMMARY_FAILURE_COOLDOWN_SECONDS = 600  # 10 min
```

### 1.3 Tool Result Summarization

```python
def _summarize_tool_result(tool_name, tool_args, tool_content) -> str:
```

**Per-tool summaries**:

| Tool | Summary Format |
|------|---------------|
| terminal | `[terminal] ran \`npm test\` -> exit 0, 47 lines output` |
| read_file | `[read_file] read config.py from line 1 (1,200 chars)` |
| write_file | `[write_file] wrote to config.py (42 lines)` |
| search_files | `[search_files] content search for 'compress' in agent/ -> 12 matches` |
| patch | `[patch] replace in config.py (1,200 chars result)` |
| browser_* | `[browser_navigate] https://example.com (4,200 chars)` |
| web_search | `[web_search] query='X' (2,400 chars result)` |
| web_extract | `[web_extract] https://example.com (+2 more) (8,100 chars)` |
| delegate_task | `[delegate_task] 'fix the login bug' (1,500 chars result)` |
| execute_code | `[execute_code] \`print(hello)\` (12 lines output)` |

### 1.4 Compression Algorithm

1. **Prune old tool results** (cheap pre-pass, no LLM call) — replaces large outputs with 1-line summaries via `_summarize_tool_result()`
2. **Protect head messages** — system prompt + first user/assistant exchange
3. **Protect tail messages** by token budget (most recent ~20K tokens)
4. **Summarize middle turns** with structured LLM prompt tracking Resolved/Pending questions
5. **On subsequent compactions** — iteratively updates previous summary instead of re-summarizing from scratch

### 1.5 Scaled Summary Budget

```python
summary_tokens = max(_MIN_SUMMARY_TOKENS, min(compressed_tokens * _SUMMARY_RATIO, _SUMMARY_TOKENS_CEILING))
```

### 1.6 Context Engine Interface

```python
class ContextCompressor(ContextEngine):
    @property
    def name(self) -> str: return "compressor"

    def on_session_reset(self) -> None:
        # Clears compaction state

    def compress(self, messages, model, provider, config) -> List[Dict]:
        # Returns compressed message list
```

### 1.7 Failure Cooldown

If summarization fails, enters 10-minute cooldown. During cooldown, falls back to simple truncation (protect head + tail, drop middle).

---

## 2. Prompt Builder

### Location

`agent/prompt_builder.py` (~1,043 lines)

### Purpose

Stateless system prompt assembly — identity, platform hints, skills index, context files, memory guidance.

### 2.1 Core Prompt Pieces

| Constant | Purpose |
|----------|---------|
| `DEFAULT_AGENT_IDENTITY` | "You are Hermes Agent, an intelligent AI assistant created by Nous Research..." |
| `MEMORY_GUIDANCE` | How to use the memory tool — save facts that reduce future user steering |
| `SESSION_SEARCH_GUIDANCE` | Use session_search for past conversation references |
| `SKILLS_GUIDANCE` | Save approaches as skills after complex tasks; patch outdated skills |
| `TOOL_USE_ENFORCEMENT_GUIDANCE` | MUST use tools to act, not describe intentions |

### 2.2 Model-Specific Execution Guidance

```python
TOOL_USE_ENFORCEMENT_MODELS = ("gpt", "codex", "gemini", "gemma", "grok")
```

Models matching these substrings get additional steering to prevent work abandonment.

**OpenAI-specific guidance** (`OPENAI_MODEL_EXECUTION_GUIDANCE`):
- Do not stop early when another tool call would improve the result
- Always look up missing information before reasoning about it
- Run tests to confirm changes work correctly
- Use tools instead of hallucinating file contents or command outputs

### 2.3 Context File Scanning

```python
_CONTEXT_THREAT_PATTERNS = [
    (r'ignore\s+(previous|all|above|prior)\s+instructions', "prompt_injection"),
    (r'do\s+not\s+tell\s+the\s+user', "deception_hide"),
    (r'system\s+prompt\s+override', "sys_prompt_override"),
    (r'disregard\s+(your|all|any)\s+(instructions|rules|guidelines)', "disregard_rules"),
    (r'act\s+as\s+(if|though)\s+you\s+(have\s+no|don\'t\s+have)\s+(restrictions|limits|rules)', "bypass_restrictions"),
    (r'<!--[^>]*(?:ignore|override|system|secret|hidden)[^>]*-->', "html_comment_injection"),
    (r'<\s*div\s+style\s*=\s*["\'][\s\S]*?display\s*:\s*none', "hidden_div"),
    (r'translate\s+.*\s+into\s+.*\s+and\s+(execute|run|eval)', "translate_execute"),
    (r'curl\s+[^\n]*\$\{?\w*(KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL|API)', "exfil_curl"),
    (r'cat\s+[^\n]*(\.env|credentials|\.netrc|\.pgpass)', "read_secrets"),
]

_CONTEXT_INVISIBLE_CHARS = {
    '\u200b', '\u200c', '\u200d', '\u2060', '\ufeff',
    '\u202a', '\u202b', '\u202c', '\u202d', '\u202e',
}
```

When a context file (AGENTS.md, .cursorrules, SOUL.md) contains threat patterns or invisible Unicode, the file is replaced with:
`[BLOCKED: filename contained potential prompt injection (...). Content not loaded.]`

### 2.4 HERMES.md Discovery

```python
def _find_hermes_md(cwd: Path) -> Optional[Path]:
```

Search order: cwd first, then each parent up to git repository root. Looks for `.hermes.md` or `HERMES.md`.

### 2.5 YAML Frontmatter Stripping

```python
def _strip_yaml_frontmatter(content: str) -> str:
```

Removes `---` delimited YAML frontmatter from context files before injecting into system prompt. Structured config in frontmatter is handled separately.

### 2.6 Skills Index Building

```python
def _build_skills_index(platform: str, disabled_skills: set) -> str:
```

Builds a table of available skills with name, description, and trigger conditions. Excludes disabled and platform-incompatible skills.

### 2.7 System Prompt Assembly

```python
def build_system_prompt(identity, platform_hints, memory_guidance, skills_index, context_files, ephemeral_prompts):
```

**Order**:
1. Agent identity
2. Platform-specific hints (terminal backend, display config)
3. Memory guidance
4. Session search guidance
5. Skills guidance
6. Tool-use enforcement (model-specific)
7. Context files (HERMES.md, AGENTS.md, etc.)
8. Skills index
9. Ephemeral prompts (clarify responses, compression summaries)

---

## 3. Display

### Location

`agent/display.py` (~1,037 lines)

### Purpose

CLI presentation — spinner, kawaii faces, tool preview formatting, diff display. Skin-aware theming.

### 3.1 Diff Display

```python
def _diff_ansi() -> dict:
    # Resolves ANSI escapes from active skin colors
    # Falls back to dark terminal defaults
```

**Limits**: Max 6 inline diff files, max 80 diff lines.

### 3.2 Local Edit Snapshot

```python
@dataclass
class LocalEditSnapshot:
    paths: list[Path]
    before: dict[str, str | None]  # path → content (None = new file)
```

Captures filesystem state before tool execution for post-tool diff rendering.

### 3.3 Tool Preview

```python
def build_tool_preview(tool_name, args, max_len=None) -> str | None:
```

Builds one-line preview of tool call's primary argument:
- terminal → command
- web_search → query
- read_file/write_file/patch → path
- browser_navigate → URL
- image_generate → prompt
- execute_code → code preview (first 60 chars)
- delegate_task → goal (first 60 chars)

### 3.4 Skin-Aware Helpers

```python
def get_skin_tool_prefix() -> str:    # Returns "┊" or skin override
def get_tool_emoji(tool_name, default="⚡") -> str:
    # Resolution: skin.tool_emojis → registry.get_emoji() → default
```

### 3.5 Tool Preview Length

```python
def set_tool_preview_max_len(n: int) -> None:  # 0 = unlimited
def get_tool_preview_max_len() -> int:
```

Configured at startup from `display.tool_preview_length` in config.yaml.

### 3.6 Kawaii Faces

Spinner face sets for different states (waiting, thinking) configurable per skin.

---

## 4. Error Classifier

### Location

`agent/error_classifier.py` (~820 lines)

### Purpose

Centralized API error taxonomy and classification pipeline. Determines recovery action (retry, rotate credential, fallback, compress, abort).

### 4.1 Error Taxonomy

```python
class FailoverReason(enum.Enum):
    # Authentication
    auth = "auth"              # Transient (401/403) — refresh/rotate
    auth_permanent = "auth_permanent"  # After refresh failed — abort

    # Billing / quota
    billing = "billing"        # 402 or credit exhaustion — rotate
    rate_limit = "rate_limit"  # 429 or throttling — backoff then rotate

    # Server-side
    overloaded = "overloaded"  # 503/529 — backoff
    server_error = "server_error"  # 500/502 — retry

    # Transport
    timeout = "timeout"        # Connection/read timeout — rebuild + retry

    # Context / payload
    context_overflow = "context_overflow"  # Compress, not failover
    payload_too_large = "payload_too_large"  # 413 — compress

    # Model
    model_not_found = "model_not_found"  # Fallback to different model

    # Request format
    format_error = "format_error"  # 400 — abort or strip + retry

    # Provider-specific
    thinking_signature = "thinking_signature"  # Anthropic thinking block sig invalid
    long_context_tier = "long_context_tier"    # Anthropic "extra usage" tier gate

    # Catch-all
    unknown = "unknown"        # Retry with backoff
```

### 4.2 ClassifiedError

```python
@dataclass
class ClassifiedError:
    reason: FailoverReason
    status_code: Optional[int]
    provider: Optional[str]
    model: Optional[str]
    message: str
    error_context: Dict[str, Any]

    # Recovery hints
    retryable: bool = True
    should_compress: bool = False
    should_rotate_credential: bool = False
    should_fallback: bool = False
```

### 4.3 Pattern Matching

**Billing patterns**: "insufficient credits", "insufficient_quota", "credit balance", "credits have been exhausted", "top up your credits", "payment required", "billing hard limit", "exceeded your current quota", "account is deactivated", "plan does not include"

**Rate limit patterns**: "rate limit", "too many requests", "throttled", "requests per minute", "tokens per minute", "requests per day", "try again in", "resource_exhausted", "rate increased too quickly" (Alibaba/DashScope)

**Usage limit patterns** (need disambiguation): "usage limit", "quota", "limit exceeded", "key limit exceeded"
**Transient signals**: "try again", "retry", "resets at", "reset in", "wait", "requests remaining", "periodic", "window"

**Context overflow patterns**: "context length", "context size", "maximum context", "token limit", "too many tokens", "prompt is too long", "exceeds the max_model_len" (vLLM), "context length exceeded" (Ollama), "slot context" (llama.cpp), Chinese: "超过最大长度", "上下文长度"

**Model not found**: "is not a valid model", "invalid model", "model not found", "does not exist", "unknown model", "unsupported model"

**Auth patterns**: "invalid api key", "authentication", "unauthorized", "forbidden", "invalid token", "token expired", "token revoked", "access denied"

**Payload too large**: "request entity too large", "payload too large", "error code: 413"

### 4.4 Classification Pipeline

```python
def classify_error(exception, status_code=None, message="", provider="", model="", error_context=None) -> ClassifiedError:
```

**Priority order**:
1. Status code → direct mapping (401→auth, 402→billing, 429→rate_limit, 500→server_error, 503→overloaded, 413→payload_too_large, 404→model_not_found)
2. Message pattern matching → billing, rate_limit, context_overflow, auth, model_not_found, payload_too_large
3. Provider-specific handling → Anthropic thinking signature, long context tier
4. Fallback → unknown (retryable)

### 4.5 Recovery Action Mapping

| Reason | retryable | compress | rotate | fallback |
|--------|-----------|----------|--------|----------|
| auth | True | False | True | False |
| auth_permanent | False | False | False | True |
| billing | True | False | True | True |
| rate_limit | True | False | True | False |
| overloaded | True | False | False | True |
| server_error | True | False | False | False |
| timeout | True | False | False | False |
| context_overflow | True | True | False | False |
| payload_too_large | True | True | False | False |
| model_not_found | True | False | False | True |
| format_error | False | False | False | False |
| thinking_signature | True | False | False | False |
| long_context_tier | True | True | False | False |
| unknown | True | False | False | False |

---

## 5. Insights Engine

### Location

`agent/insights.py` (~789 lines)

### Purpose

Analyzes historical session data from SQLite state database to produce usage insights — token consumption, cost estimates, tool usage patterns, activity trends.

### 5.1 Report Generation

```python
class InsightsEngine:
    def generate(self, days=30) -> Dict[str, Any]:
    def format_terminal(self, report) -> str:
```

### 5.2 Report Sections

| Section | Content |
|---------|---------|
| Overview | Total sessions, messages, tokens, estimated cost |
| Token consumption | Input/output/cache/reasoning breakdown |
| Cost estimation | Per-model cost using `usage_pricing` |
| Tool usage | Most-used tools, success/failure rates |
| Activity trends | Sessions per day, messages per session |
| Model breakdown | Usage by model/provider |
| Platform breakdown | CLI vs Telegram vs Discord vs etc. |

### 5.3 Cost Estimation

Uses `usage_pricing.estimate_usage_cost()` with `CanonicalUsage` dataclass. Falls back to `_DEFAULT_PRICING` for unknown models.

---

## 6. Model Metadata

### Location

`agent/model_metadata.py` (~1,102 lines)

### Purpose

Model context length detection, token estimation, and metadata caching. Primary source: `models.dev` API, fallback: OpenRouter models API, then thin local defaults.

### 6.1 Constants

```python
MINIMUM_CONTEXT_LENGTH = 64_000       # Models below this are rejected
DEFAULT_FALLBACK_CONTEXT = 128_000    # Safe default for modern models
_MODEL_CACHE_TTL = 3600               # 1 hour
_ENDPOINT_MODEL_CACHE_TTL = 300       # 5 minutes for custom endpoints
```

### 6.2 Context Probe Tiers

```python
CONTEXT_PROBE_TIERS = [128_000, 64_000, 32_000, 16_000, 8_000]
```

Descending tiers for context length probing when model is unknown. Steps down on context-length errors until one works.

### 6.3 Provider Prefix Stripping

```python
def _strip_provider_prefix(model: str) -> str:
    # "local:my-model" → "my-model"
    # "qwen3.5:27b" → "qwen3.5:27b"  (preserved — Ollama model:tag)
    # "deepseek:latest" → "deepseek:latest"  (preserved — Ollama tag)
```

Uses `_OLLAMA_TAG_PATTERN` to detect Ollama-style tags: `7b`, `latest`, `stable`, `q4`, `fp16`, `instruct`, `chat`, `coder`, `vision`, `text`.

### 6.4 Metadata Fetching

```python
def fetch_model_metadata(model: str) -> Dict[str, Any]:
    # 1. models.dev API → context_length, max_output_tokens, pricing
    # 2. OpenRouter models API → context_length
    # 3. Anthropic API docs → known Claude models
    # 4. Local DEFAULT_CONTEXT_LENGTHS dict → pattern matching

def fetch_endpoint_model_metadata(base_url: str, model: str) -> Dict[str, Any]:
    # Queries /v1/models endpoint for context window info
    # Cached for 5 minutes per endpoint
```

### 6.5 Token Estimation

```python
def estimate_messages_tokens_rough(messages: List[Dict]) -> int:
    # Rough estimate: chars / _CHARS_PER_TOKEN (4)
    # Accounts for tool call overhead
```

---

## 7. Usage Pricing

### Location

`agent/usage_pricing.py` (~613 lines)

### Purpose

Model pricing lookup and cost estimation. Sources: provider cost APIs, generation APIs, official docs snapshots, custom contracts.

### 7.1 Data Types

```python
@dataclass(frozen=True)
class CanonicalUsage:
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: int = 0
    cache_write_tokens: int = 0
    reasoning_tokens: int = 0
    request_count: int = 1

@dataclass(frozen=True)
class PricingEntry:
    input_cost_per_million: Optional[Decimal]
    output_cost_per_million: Optional[Decimal]
    cache_read_cost_per_million: Optional[Decimal]
    cache_write_cost_per_million: Optional[Decimal]
    request_cost: Optional[Decimal]
    source: CostSource  # "provider_cost_api", "official_docs_snapshot", "none", ...

@dataclass(frozen=True)
class CostResult:
    amount_usd: Optional[Decimal]
    status: CostStatus  # "actual", "estimated", "included", "unknown"
    source: CostSource
    label: str
```

### 7.2 Official Docs Pricing

Built-in pricing for stable models:
- **Anthropic Claude**: Opus ($15/$75 per 1M), Sonnet ($3/$15), Haiku ($0.80/$4.00), with cache read/write pricing
- **OpenAI GPT**: Per-model pricing from published docs

### 7.3 Cost Estimation

```python
def estimate_usage_cost(model: str, usage: CanonicalUsage, provider=None, base_url=None) -> CostResult:
```

**Lookup order**:
1. Provider cost API (real-time)
2. Provider generation API
3. Provider models API
4. Official docs snapshot (built-in)
5. User override (custom pricing)
6. Custom contract pricing
7. Unknown (returns `status="unknown"`)

### 7.4 Duration Formatting

```python
def format_duration_compact(seconds: float) -> str:
    # "2h 15m", "45s", "1d 3h", etc.
```

---

## 8. Copilot ACP Client

### Location

`agent/copilot_acp_client.py` (~570 lines)

### Purpose

Client for GitHub Copilot's Agent Communication Protocol (ACP). Spawns `copilot --acp --stdio` subprocess and communicates via JSON-RPC over stdio.

### 8.1 Lifecycle

1. Spawn `copilot --acp --stdio` subprocess
2. Initialize via JSON-RPC `initialize` handshake
3. Send prompts via `session/new` and `session/turn`
4. Receive streaming response deltas
5. Handle tool call requests from Copilot side
6. Clean shutdown on session end

### 8.2 Token Management

Reads Copilot OAuth tokens from `~/.codex/auth.json` or `gh auth` fallback.

---

## 9. Context References

### Location

`agent/context_references.py` (~520 lines)

### Purpose

Parses and resolves context references in user messages: `@file/path`, `@folder/path`, `@git/commit`, `@url`.

### 9.1 Reference Types

| Type | Syntax | Resolution |
|------|--------|------------|
| File | `@path/to/file.py` or `@path/to/file.py:10-20` | Reads file content, optional line range |
| Folder | `@path/to/folder/` | Lists directory contents |
| Git | `@git/abc1234` or `@git/abc1234..def5678` | Git commit diff |
| URL | `@url https://example.com` | Fetches URL content |

### 9.2 Resolution

```python
def resolve_references(text: str, cwd: Path) -> tuple[str, List[ResolvedRef]]:
    # Returns (text with references replaced, list of resolved references)
```

References are resolved before message processing. File contents are inlined into the message with appropriate formatting.

### 9.3 Line Range Parsing

```python
# @file.py:10 → line 10
# @file.py:10-20 → lines 10-20
# @file.py:10:5 → 5 lines starting at 10
```

---

## Key Numbers

| Metric | Value |
|--------|-------|
| context_compressor.py lines | ~1,091 |
| prompt_builder.py lines | ~1,043 |
| display.py lines | ~1,037 |
| error_classifier.py lines | ~820 |
| insights.py lines | ~789 |
| model_metadata.py lines | ~1,102 |
| usage_pricing.py lines | ~613 |
| copilot_acp_client.py lines | ~570 |
| context_references.py lines | ~520 |
| Summary ratio | 20% of compressed content |
| Summary token ceiling | 12,000 |
| Summary failure cooldown | 600 seconds (10 min) |
| Min context length | 64,000 tokens |
| Default fallback context | 128,000 tokens |
| Context probe tiers | 5 (128K → 64K → 32K → 16K → 8K) |
| Model metadata cache TTL | 3,600 seconds (1 hour) |
| Endpoint model cache TTL | 300 seconds (5 min) |
| Threat patterns for injection scan | 10 |
| Invisible Unicode chars blocked | 9 |
| Failover reasons | 13 |
| Billing patterns | 10 |
| Rate limit patterns | 10 |
| Context overflow patterns | 18 |
| Diff max inline files | 6 |
| Diff max lines | 80 |
| Tool-use enforcement model families | 5 (gpt, codex, gemini, gemma, grok) |

---

*Generated from source analysis of the Hermes Agent codebase.*
