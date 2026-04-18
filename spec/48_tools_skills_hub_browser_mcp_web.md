# Hermes Agent — Tools: Skills Hub, Browser, MCP, Web

This document covers the `tools/` subsystem: `skills_hub.py` (~3,053 lines), `browser_tool.py` (~2,393 lines), `mcp_tool.py` (~2,273 lines), and `web_tools.py` (~2,100 lines).

---

## Table of Contents

1. [Skills Hub](#1-skills-hub)
2. [Browser Tool](#2-browser-tool)
3. [MCP Client](#3-mcp-client)
4. [Web Tools](#4-web-tools)

---

## 1. Skills Hub

### Location

`tools/skills_hub.py` (~3,053 lines)

### Purpose

Source adapters and hub state management for the Hermes Skills Hub. Provides a library interface (not an agent tool) for discovering, fetching, and installing skills from multiple remote registries.

### 1.1 Directory Layout

```
~/.hermes/skills/
  .hub/
    lock.json          # Installed skill provenance
    audit.log          # Install/uninstall audit trail
    taps.json          # GitHub tap registry
    quarantine/        # Unscanned skill downloads
    index-cache/       # Cached remote indices
  <skill-name>/        # Installed skill directories
```

### 1.2 Data Models

**SkillMeta** (search results):
```python
@dataclass
class SkillMeta:
    name: str
    description: str
    source: str           # "official" | "github" | "clawhub" | "claude-marketplace" | "lobehub" | "skills.sh" | "well-known"
    identifier: str       # source-specific ID
    trust_level: str      # "builtin" | "trusted" | "community"
    repo: Optional[str]
    path: Optional[str]
    tags: List[str]
    extra: Dict[str, Any]
```

**SkillBundle** (downloaded skill ready for install):
```python
@dataclass
class SkillBundle:
    name: str
    files: Dict[str, Union[str, bytes]]  # relative_path -> content
    source: str
    identifier: str
    trust_level: str
    metadata: Dict[str, Any]
```

### 1.3 Path Validation

All skill names, category names, and bundle file paths go through `_normalize_bundle_path()`:
- Rejects absolute paths, `..` traversal, Windows drive letters
- Allows single-level names (skills, categories) or nested paths (bundle files)
- Normalizes backslashes to forward slashes

### 1.4 GitHubAuth

Authentication to GitHub API with 4-method fallback:

| Priority | Method | Details |
|----------|--------|---------|
| 1 | PAT env var | `GITHUB_TOKEN` or `GH_TOKEN` |
| 2 | gh CLI | `gh auth token` subprocess |
| 3 | GitHub App | JWT + installation token (RS256, ~58 min expiry) |
| 4 | Anonymous | 60 req/hr, public repos only |

### 1.5 SkillSource ABC

```python
class SkillSource(ABC):
    @abstractmethod
    def search(self, query: str, limit: int = 10) -> List[SkillMeta]: ...
    @abstractmethod
    def fetch(self, identifier: str) -> Optional[SkillBundle]: ...
    @abstractmethod
    def inspect(self, identifier: str) -> Optional[SkillMeta]: ...
    @abstractmethod
    def source_id(self) -> str: ...
    def trust_level_for(self, identifier: str) -> str:  # defaults to "community"
```

### 1.6 GitHubSource

Default taps (repos searched for skills):

| Repo | Path |
|------|------|
| `openai/skills` | `skills/` |
| `anthropics/skills` | `skills/` |
| `VoltAgent/awesome-agent-skills` | `skills/` |
| `garrytan/gstack` | (root) |

**Search**: Iterates all taps, reads `SKILL.md` frontmatter for metadata, deduplicates by name preferring higher trust levels.

**Download**: Uses Git Trees API (single request per repo) to avoid per-directory rate limiting. Falls back to recursive Contents API when tree endpoint is unavailable.

**Tree cache**: `_get_repo_tree()` caches `(default_branch, tree_entries)` per repo to avoid redundant API calls across `_download_directory_via_tree()` and `_find_skill_in_repo_tree()`.

**Index cache**: File-based cache in `~/.hermes/skills/.hub/index-cache/` with 1-hour TTL.

**Rate limit detection**: Flags `_rate_limited = True` when GitHub returns 403 with `X-RateLimit-Remaining: 0`.

### 1.7 WellKnownSkillSource

Reads skills from domains exposing `/.well-known/skills/index.json`:
- Index format: `{"skills": [{"name": "...", "description": "...", "files": ["SKILL.md"]}]}`
- Identifier: `well-known:<base_url>/<skill_name>`
- Fetches individual skill files listed in the index
- Trust level: always "community"

### 1.8 SkillsShSource

Discovers skills via skills.sh aggregator:
- Search: `GET https://skills.sh/api/search?q=...`
- Featured skills: Scraped from skills.sh homepage
- Fetch: Delegates to `GitHubSource` after resolving canonical identifiers
- Parses detail pages for install counts, weekly stats
- Caches search results and featured list

### 1.9 HubLockFile

Tracks provenance of installed skills:
```python
@dataclass
class HubLockFile:
    skills: Dict[str, LockEntry]
    # LockEntry tracks: source, identifier, version, installed_at, files[]
```

### 1.10 Quarantine & Audit

- **Quarantine**: New skill downloads go to `.hub/quarantine/` before installation (scanned by `skills_guard`)
- **Audit log**: `.hub/audit.log` records all install/uninstall operations with timestamps

---

## 2. Browser Tool

### Location

`tools/browser_tool.py` (~2,393 lines)

### Purpose

Browser automation via `agent-browser` CLI with multiple backends: local Chromium, Browserbase, Browser Use cloud, Firecrawl, and Camofox.

### 2.1 Backend Modes

| Mode | Trigger | Mechanism |
|------|---------|-----------|
| Local (default) | No cloud provider configured | Headless Chromium via `agent-browser --session` |
| Browserbase | `BROWSERBASE_API_KEY` + `BROWSERBASE_PROJECT_ID` | Cloud browser via `--cdp <ws_url>` |
| Browser Use | `BROWSER_USE_API_KEY` | Cloud via managed Nous gateway or direct API |
| Firecrawl | `browser.cloud_provider: firecrawl` | Cloud via Firecrawl provider |
| Camofox | `CAMOFOX_URL` set | REST API delegate (anti-detection) |
| CDP override | `BROWSER_CDP_URL` set | Direct connect to Chrome DevTools endpoint |

### 2.2 CDP Endpoint Resolution

`_resolve_cdp_override()` normalizes user-supplied endpoints:
- Full websocket URLs pass through
- HTTP discovery endpoints fetch `/json/version` → `webSocketDebuggerUrl`
- Bare `host:port` values get protocol prepended

### 2.3 Cloud Provider Registry

```python
_PROVIDER_REGISTRY = {
    "browserbase": BrowserbaseProvider,
    "browser-use": BrowserUseProvider,
    "firecrawl": FirecrawlProvider,
}
```

Fallback order when `cloud_provider` unset: Browser Use (managed gateway or direct) → Browserbase (direct only).

### 2.4 Session Management

```python
_active_sessions: Dict[str, Dict[str, str]]  # task_id -> {session_name, bb_session_id, cdp_url}
_recording_sessions: set  # task_ids with active recordings
```

Each task gets its own socket directory: `/tmp/agent-browser-{session_name}/` (mode 0o700).

### 2.5 Inactivity Cleanup

**Timeout**: `BROWSER_INACTIVITY_TIMEOUT` env var (default 300s / 5 min).

**Background thread**: Runs every 30 seconds, checks `_session_last_activity` dict.

**Orphan reaping** (one-time on startup):
- Scans `/tmp/agent-browser-h_*` and `agent-browser-cdp_*` socket dirs
- Reads PID files, kills untracked daemons via `SIGTERM`
- Cleans up stale socket directories

**Emergency cleanup**: `atexit` handler closes all active sessions on process exit.

**Signal handling**: Previous SIGINT/SIGTERM handlers were removed — they conflicted with prompt_toolkit's async event loop. Only `atexit` is used now.

### 2.6 PATH Resolution

Browser-specific PATH merging for finding `agent-browser` and `npx`:
1. Hermes-managed node (`~/.hermes/node/bin`)
2. Homebrew versioned Node.js (`/opt/homebrew/opt/node@2*/bin`)
3. System fallbacks: Termux, `/usr/local/bin`, `/usr/bin`, etc.

### 2.7 macOS Socket Path Limit

`_socket_safe_tmpdir()` returns `/tmp` on macOS (not `$TMPDIR`) to avoid exceeding 104-byte `AF_UNIX` socket path limit.

### 2.8 Tool Schemas (9 tools)

| Tool | Parameters | Purpose |
|------|-----------|---------|
| `browser_navigate` | `url` | Load page, returns compact snapshot |
| `browser_snapshot` | `full` (bool) | Accessibility tree snapshot |
| `browser_click` | `ref` | Click element by ref ID (@e1, @e2) |
| `browser_type` | `ref`, `text` | Type into input field |
| `browser_scroll` | `direction` (up/down) | Scroll viewport |
| `browser_back` | — | Navigate back in history |
| `browser_press` | `key` | Keyboard key press |
| `browser_get_images` | — | List all images on page |
| `browser_vision` | `question`, `annotate` | Screenshot + AI analysis |
| `browser_console` | `clear`, `expression` | JS console / eval |

### 2.9 SSRF Protection

Two-layer protection for cloud backends:
1. **Pre-navigation**: `_is_safe_url()` blocks private/internal URLs
2. **Post-redirect**: Checks final URL after redirects; navigates to `about:blank` if it landed on a private address

**Skipped for local backends**: Agent already has full local network access via terminal tool.

**Opt-out**: `browser.allow_private_urls` in config.yaml.

### 2.10 URL Secret Exfiltration

`browser_navigate` blocks URLs containing API key/token patterns (checked via `agent.redact._PREFIX_RE`), including URL-decoded form to catch `%2D` encoding tricks.

### 2.11 Website Policy

All navigation and extraction goes through `check_website_access()` for domain-level allow/block rules.

### 2.12 Bot Detection Warning

Detects blocked page patterns in title: "access denied", "bot detected", "captcha", "cloudflare", "just a moment", etc. Returns `bot_detection_warning` in response.

### 2.13 Snapshot Summarization

Snapshots > 8,000 chars go through `_extract_relevant_content()`:
- Uses auxiliary LLM with task-aware prompt
- Redacts secrets before sending to auxiliary model
- Falls back to `_truncate_snapshot()` (structure-aware line-boundary truncation)

### 2.14 browser_vision

Takes annotated screenshot, sends to vision model:
- `annotate=true` overlays numbered `[N]` labels on interactive elements (maps to `@eN` refs)
- Returns both AI analysis and `screenshot_path` for sharing with user via `MEDIA:` tag
- Uses `AUXILIARY_VISION_MODEL` env var for model selection

### 2.15 Command Timeout

Configurable via `config["browser"]["command_timeout"]` (default 30s, floor 5s). Cached after first resolution.

### 2.16 `_run_browser_command`

Uses temp files (not pipes) for stdout/stderr to prevent daemon FD inheritance blocking. Per-task socket directories prevent concurrency conflicts.

### 2.17 Camofox Backend

When `CAMOFOX_URL` is set, all browser operations delegate to `tools/browser_camofox.py` REST API instead of agent-browser CLI.

---

## 3. MCP Client

### Location

`tools/mcp_tool.py` (~2,273 lines)

### Purpose

Connects to external MCP (Model Context Protocol) servers via stdio or HTTP/StreamableHTTP transport, discovers their tools, and registers them into the hermes-agent tool registry.

### 3.1 Architecture

```
Background daemon thread: asyncio event loop (_mcp_loop)
    ↓
MCPServerTask (one per server, long-lived asyncio.Task)
    ↓
ClientSession (MCP SDK) → stdio_client or streamablehttp_client
    ↓
Tool discovery → registry.register()
    ↓
Agent calls MCP tools like built-in tools
```

### 3.2 Transport Support

| Transport | Config | SDK Requirement |
|-----------|--------|----------------|
| Stdio | `command` + `args` + `env` | mcp >= 1.0 |
| HTTP/StreamableHTTP | `url` + `headers` | mcp >= 1.0 (new API >= 1.24.0) |

### 3.3 Sampling Support

MCP servers can request LLM completions via `sampling/createMessage`:

**SamplingHandler** (per-server):
```python
@dataclass
class SamplingConfig:
    max_rpm: int = 10              # Rate limit per minute
    timeout: float = 30            # LLM call timeout
    max_tokens_cap: int = 4096     # Token ceiling
    max_tool_rounds: int = 5       # Tool loop limit (0 = disabled)
    model: Optional[str]           # Model override
    allowed_models: List[str]      # Model whitelist
    log_level: str = "info"        # Audit verbosity
```

**Message conversion**: MCP `SamplingMessages` → OpenAI format via `content_as_list` dispatch on block type (TextContent, ToolUseContent, ToolResultContent).

**Tool loop governance**: Tracks `_tool_loop_count`, errors when `max_tool_rounds` exceeded.

**Rate limiting**: Sliding 60-second window on `_rate_timestamps`.

**Model resolution**: Config override → server hint → default.

### 3.4 Dynamic Tool Discovery

When MCP SDK supports `message_handler` and notification types:
- Listens for `tools/list_changed` notifications
- Calls `_refresh_tools()`: deregisters old tools, re-registers new ones
- Logs added/removed tool names
- Stub handlers for `prompts/list_changed` and `resources/list_changed`

### 3.5 Security

**Environment filtering**: Only passes `PATH`, `HOME`, `USER`, `LANG`, `LC_ALL`, `TERM`, `SHELL`, `TMPDIR`, and `XDG_*` variables to stdio subprocesses.

**Credential stripping**: `_sanitize_error()` removes `ghp_*`, `sk-*`, `Bearer ...`, `token=...`, `key=...`, `password=...`, `secret=...` patterns from error messages.

**MCP injection scanning**: `_scan_mcp_description()` checks tool descriptions for 10 prompt injection patterns (ignore previous instructions, identity override, system prompt injection, role tags, concealment instructions, network commands, base64 decode, eval/exec, dangerous imports).

**OSV malware check**: `check_package_for_malware()` runs before spawning stdio MCP servers.

**Command resolution**: `_resolve_stdio_command()` resolves bare `npx`/`npm`/`node` against filtered PATH, including Hermes-managed node bin.

### 3.6 Connection Lifecycle

**Initial connection**: Retries up to 3 times with exponential backoff (1s, 2s, 4s, max 60s).

**Reconnection**: Up to 5 retries after established connection drops.

**Shutdown**: Signals `_shutdown_event`, waits up to 10s for task to exit, deregisters tools.

**Stdio PID tracking**: `_snapshot_child_pids()` (via `/proc` or `psutil`) tracks spawned subprocess PIDs for force-kill on shutdown.

### 3.7 OAuth 2.1 PKCE

HTTP transport supports OAuth via `tools.mcp_oauth.build_oauth_auth()`. Failures are re-raised (not swallowed) so individual server failures don't block others.

### 3.8 Error Formatting

`_format_connect_error()` unwraps nested exceptions to find missing executables, renders actionable messages with install hints for `npx`/`npm`/`node`.

### 3.9 Config Loading

Reads `mcp_servers` from `~/.hermes/config.yaml`. Supports `${ENV_VAR}` placeholder interpolation from `os.environ` (including `.env` file).

### 3.10 Tool Call Execution

`_make_tool_handler()` returns sync handler `handler(args_dict, **kwargs) -> str`:
- Schedules `session.call_tool()` on MCP event loop via `run_coroutine_threadsafe()`
- Polls with 0.1s intervals to honor user interrupts
- Returns JSON with `result` field (or `result` + `structuredContent` when both present)
- Honors per-server `tool_timeout` (default 120s)

### 3.11 Constants

| Constant | Value |
|----------|-------|
| `_DEFAULT_TOOL_TIMEOUT` | 120s |
| `_DEFAULT_CONNECT_TIMEOUT` | 60s |
| `_MAX_RECONNECT_RETRIES` | 5 |
| `_MAX_INITIAL_CONNECT_RETRIES` | 3 |
| `_MAX_BACKOFF_SECONDS` | 60 |
| Safe env keys | PATH, HOME, USER, LANG, LC_ALL, TERM, SHELL, TMPDIR |

---

## 4. Web Tools

### Location

`tools/web_tools.py` (~2,100 lines)

### Purpose

Generic web search and extraction tools with multi-backend support (Firecrawl, Exa, Parallel, Tavily) and LLM-powered content summarization.

### 4.1 Backend Selection

```python
def _get_backend() -> str:
```

Reads `web.backend` from config.yaml. Falls back to first available API key:
1. Firecrawl (`FIRECRAWL_API_KEY` or `FIRECRAWL_API_URL` or managed gateway)
2. Parallel (`PARALLEL_API_KEY`)
3. Tavily (`TAVILY_API_KEY`)
4. Exa (`EXA_API_KEY`)

Default: `firecrawl` (backward compatible).

### 4.2 Firecrawl Client

**Direct config precedence**: `FIRECRAWL_API_KEY` + `FIRECRAWL_API_URL` env vars take priority.

**Managed tool gateway fallback**: For Nous subscribers, resolves via `resolve_managed_tool_gateway("firecrawl")` with Nous user token.

**Caching**: Client singleton cached by config tuple `(source, api_url, api_key)`.

### 4.3 Exa Client

Lazy-initialized `Exa(api_key)`, adds `x-exa-integration: hermes-agent` header. Search returns results with highlights. Extract returns full text content.

### 4.4 Parallel Client

Both sync (`Parallel`) and async (`AsyncParallel`) clients. Search supports modes: `fast`, `one-shot`, `agentic` (default). Extract returns `full_content` with excerpts fallback.

### 4.5 Tavily Client

Direct HTTP to `https://api.tavily.com/{endpoint}`. Auth via `api_key` in JSON body (no header auth). Supports `/search` and `/extract` endpoints.

### 4.6 Result Normalization

**Firecrawl**: `_extract_web_search_results()` handles multiple response shapes (SDK object, gateway response, direct API) — tries `data.web`, `data.results`, `web`, `results` at multiple nesting levels.

**Tavily**: `_normalize_tavily_search_results()` maps `{results: [{title, url, content, score}]}` → `{data: {web: [{title, url, description, position}]}}`.

**Parallel**: Extracts `excerpts` from search results, `full_content` from extract.

### 4.7 LLM Content Summarization

```python
async def process_content_with_llm(content, url, title, model, min_length) -> Optional[str]
```

**Size thresholds**:
| Threshold | Value | Behavior |
|-----------|-------|----------|
| Refusal | > 2M chars | Refuses entirely |
| Chunked | > 500K chars | Parallel chunk processing |
| Processing | > min_length (default 5K) | LLM summarization |
| Output cap | 5,000 chars | Hard limit on final output |

**Chunked processing** (for > 500K chars):
1. Split into 100K-char chunks
2. Summarize each chunk in parallel via `asyncio.gather()`
3. Synthesize summaries into single cohesive summary
4. Fallback to concatenated summaries if synthesis fails

**LLM call**: Uses `async_call_llm()` with `task="web_extract"`, model from `_resolve_web_extract_auxiliary()`, temperature 0.1.

**Fallback**: On LLM failure, returns truncated raw content (first 5,000 chars) with explanatory note.

### 4.8 Auxiliary Model Resolution

`_resolve_web_extract_auxiliary()` → `(client, model, extra_body)`:
- Reads `AUXILIARY_WEB_EXTRACT_MODEL` env var override
- For Nous backends: adds `extra_body` with `{"tags": ["product=hermes-agent"]}`

### 4.9 SSRF & Secret Protection

- URLs checked via `is_safe_url()` before any backend fetch
- URLs containing embedded secrets (API keys, tokens) blocked via `agent.redact._PREFIX_RE`
- URL-decoded form also checked to catch percent-encoded secrets

### 4.10 Website Policy

All extraction goes through `check_website_access()` for domain-level rules before fetching.

### 4.11 Debug Mode

`WEB_TOOLS_DEBUG=true` enables `DebugSession` logging:
- Captures tool calls, results, compression metrics
- Saves to `./logs/web_tools_debug_UUID.json`

### 4.12 Base64 Image Cleanup

`clean_base64_images()` strips `data:image/...;base64,...` patterns (with and without parentheses) to reduce token count.

### 4.13 Interrupt Handling

All tool functions check `tools.interrupt.is_interrupted()` before making API calls, returning `{"error": "Interrupted", "success": false}` when user sent new message.

### 4.14 Exported Tools

| Tool | Sync/Async | Parameters |
|------|-----------|------------|
| `web_search_tool` | Sync | `query`, `limit` (default 5) |
| `web_extract_tool` | Async | `urls`, `format`, `use_llm_processing`, `model`, `min_length` |
| `web_crawl_tool` | Async | `url`, `instruction`, `limit` |

---

## Key Numbers

| Metric | Value |
|--------|-------|
| skills_hub.py lines | ~3,053 |
| browser_tool.py lines | ~2,393 |
| mcp_tool.py lines | ~2,273 |
| web_tools.py lines | ~2,100 |
| Skill sources | 4 (GitHub, well-known, skills.sh, official) |
| Default GitHub taps | 4 repos |
| Browser tool schemas | 10 (navigate, snapshot, click, type, scroll, back, press, images, vision, console) |
| Cloud browser providers | 3 (Browserbase, Browser Use, Firecrawl) |
| Browser session timeout | 300s (5 min, configurable) |
| Browser command timeout | 30s default (configurable, 5s floor) |
| Browser cleanup interval | 30s |
| MCP transport types | 2 (stdio, HTTP/StreamableHTTP) |
| MCP max reconnect retries | 5 |
| MCP max initial connect retries | 3 |
| MCP default tool timeout | 120s |
| MCP default connect timeout | 60s |
| MCP sampling rate limit | 10 RPM default |
| MCP sampling max tool rounds | 5 default |
| MCP injection patterns scanned | 10 |
| MCP safe env keys | 8 + XDG_* |
| Web tool backends | 4 (Firecrawl, Exa, Parallel, Tavily) |
| LLM summarization threshold | 5,000 chars default |
| Content refusal limit | 2,000,000 chars |
| Chunked processing threshold | 500,000 chars |
| Chunk size | 100,000 chars |
| Output cap | 5,000 chars |
| Snapshot summarization threshold | 8,000 chars |
| Index cache TTL | 3,600s (1 hour) |
| GitHub rate limit (unauth) | 60 req/hr |
| GitHub App token expiry | ~58 min |
| Credential stripping patterns | 8 (PAT, API key, Bearer, token=, key=, API_KEY=, password=, secret=) |

---

*Generated from source analysis of the Hermes Agent codebase.*
