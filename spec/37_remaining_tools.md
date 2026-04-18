# Hermes Agent — Remaining Tools

This document covers the tool modules not covered by earlier specs: Camofox browser backend, website policy, managed tool gateway, OSV malware check, and remaining tool infrastructure.

---

## Table of Contents

1. [Camofox Browser Backend](#1-camofox-browser-backend)
2. [Website Policy](#2-website-policy)
3. [Managed Tool Gateway](#3-managed-tool-gateway)
4. [OSV Malware Check](#4-osv-malware-check)
5. [Tool Infrastructure](#5-tool-infrastructure)

---

## 1. Camofox Browser Backend

### Location

`tools/browser_camofox.py` (~592 lines)

### Purpose

Anti-detection browser backend via REST API. Routes browser tool calls through a self-hosted Node.js server wrapping Camoufox (Firefox fork with C++ fingerprint spoofing).

### 1.1 Architecture

```
Hermes Agent → Camofox REST API → Camoufox (Firefox fork)
                   (port 9377)        C++ fingerprint spoofing
```

**1:1 mapping** to browser tool interface:
- Accessibility snapshots with element refs
- Click/type/scroll by ref
- Screenshots

### 1.2 Configuration

```
CAMOFOX_URL=http://localhost:9377  # Set in ~/.hermes/.env
```

When `CAMOFOX_URL` is set, browser tools route through this module instead of `agent-browser` CLI.

### 1.3 Setup Options

| Method | Command |
|--------|---------|
| npm | `git clone https://github.com/jo-inc/camofox-browser && cd camofox-browser && npm install && npm start` |
| Docker | `docker run -p 9377:9377 -e CAMOFOX_PORT=9377 jo-inc/camofox-browser` |

**First run**: Downloads Camoufox (~300MB).

### 1.4 Health Check

```python
def check_camofox_available() -> bool:
    resp = requests.get(f"{url}/health", timeout=5)
    # Extracts VNC port from response for interactive viewing
```

Health response includes `vncPort` for VNC viewer access to the browser.

### 1.5 Key Operations

| Operation | REST Endpoint | Purpose |
|-----------|---------------|---------|
| Snapshot | `/snapshot` | Accessibility tree with element refs |
| Click | `/click` | Click element by ref |
| Type | `/type` | Type text into element by ref |
| Scroll | `/scroll` | Scroll viewport |
| Screenshot | `/screenshot` | Full page screenshot |
| Navigate | `/navigate` | Go to URL |
| Health | `/health` | Server status + VNC port |

### 1.6 Snapshot Pagination

```python
_SNAPSHOT_MAX_CHARS = 80_000  # camofox paginates at this limit
```

Large pages are truncated at 80K characters. Caller can request specific viewport regions.

### 1.7 VNC Integration

```python
_vnc_url: Optional[str] = None  # Cached from /health response
_vnc_url_checked = False        # Only probe once per process
```

On first health check, extracts VNC port and constructs VNC URL for interactive viewing: `http://{host}:{vnc_port}`.

### 1.8 Identity/Browser Fingerprinting

```python
from tools.browser_camofox_state import get_camofox_identity
```

Manages browser identity (fingerprint, timezone, locale, screen resolution) for consistent anti-detection profiles across sessions.

---

## 2. Website Policy

### Location

`tools/website_policy.py` (~282 lines)

### Purpose

URL blocklist enforcement for web-capable tools (web_search, web_extract, browser).

### 2.1 Configuration

```yaml
# ~/.hermes/config.yaml
website_blocklist:
  enabled: false
  domains:
    - "example.com"
    - "*.example.com"
  shared_files:
    - "/path/to/blocklist.txt"
```

### 2.2 Policy Cache

```python
_CACHE_TTL_SECONDS = 30.0  # Config re-read interval
```

Avoids re-parsing YAML on every URL check. A web_crawl with 50 pages would otherwise mean 51 YAML parses.

### 2.3 Rule Normalization

```python
def _normalize_rule(rule: Any) -> Optional[str]:
    # Strips whitespace, comments, protocols
    # Removes www. prefix
    # Splits on / (domain only)
    # Lowercases
```

Handles:
- Full URLs: `https://www.example.com/path` → `example.com`
- Wildcards: `*.example.com` → `example.com`
- Comments: Lines starting with `#` are skipped

### 2.4 Blocklist File Format

Plain text, one domain per line. Missing/unreadable files log a warning rather than disabling all web tools.

### 2.5 Check Function

```python
def is_url_allowed(url: str) -> Tuple[bool, Optional[str]]:
    """Returns (allowed, reason_if_blocked)."""
```

Uses `fnmatch` for wildcard matching. Checks against both inline domains and shared file rules.

---

## 3. Managed Tool Gateway

### Location

`tools/managed_tool_gateway.py` (~167 lines)

### Purpose

Generic passthrough for Nous-hosted vendor APIs. Allows Hermes to access vendor tools (OpenAI, Anthropic, etc.) via the Nous Research proxy gateway.

### 3.1 Configuration

```python
_DEFAULT_TOOL_GATEWAY_DOMAIN = "nousresearch.com"
_DEFAULT_TOOL_GATEWAY_SCHEME = "https"
_NOUS_ACCESS_TOKEN_REFRESH_SKEW_SECONDS = 120
```

### 3.2 Token Resolution Chain

```
1. TOOL_GATEWAY_USER_TOKEN env var → direct override
2. ~/.hermes/auth.json → read Nous Subscriber OAuth token
3. Check token expiry (with 120s skew)
4. If expiring → trigger refresh
```

### 3.3 Auth State Reading

```python
def _read_nous_provider_state() -> Optional[dict]:
    """Read from ~/.hermes/auth.json → providers → nous"""
```

Parses `auth.json` for Nous provider OAuth state including access token, refresh token, and expiry.

### 3.4 Timestamp Parsing

```python
def _parse_timestamp(value: object) -> Optional[datetime]:
    # Handles ISO format with Z suffix
    # Converts to UTC
```

### 3.5 Expiry Detection

```python
def _access_token_is_expiring(expires_at: object, skew_seconds: int) -> bool:
    remaining = (expires - datetime.now(timezone.utc)).total_seconds()
    return remaining <= max(0, int(skew_seconds))
```

### 3.6 Gateway Config

```python
@dataclass(frozen=True)
class ManagedToolGatewayConfig:
    vendor: str               # "openai", "anthropic", etc.
    gateway_origin: str       # Full gateway URL
    nous_user_token: str      # Subscriber OAuth token
    managed_mode: bool        # Whether managed mode is active
```

### 3.7 Integration

Used by individual tools (OpenAI TTS, etc.) when they need to access vendor APIs through the Nous gateway:

```python
if managed_nous_tools_enabled():
    config = resolve_managed_tool_gateway("openai-audio")
    # Use config.gateway_origin + config.nous_user_token
```

---

## 4. OSV Malware Check

### Location

`tools/osv_check.py` (~155 lines)

### Purpose

Pre-installation malware scanning for MCP extension packages. Queries Google's OSV (Open Source Vulnerabilities) API before launching MCP servers via `npx`/`uvx`.

### 4.1 Threat Model

Only blocks **confirmed malware** (MAL-* advisories). Regular CVEs are ignored — they indicate vulnerabilities, not malicious intent.

**Inspired by**: Block/goose's extension malware check.

### 4.2 API

```python
def check_package_for_malware(command: str, args: list) -> Optional[str]:
    """Returns error message if malware found, None if clean/unknown."""
```

### 4.3 Ecosystem Detection

| Command | Ecosystem |
|---------|-----------|
| `npx`, `npx.cmd` | npm |
| `uvx`, `uvx.cmd`, `pipx` | PyPI |

Other commands are skipped (return None).

### 4.4 Package Parsing

Handles:
- `npx package-name` → `("package-name", None)`
- `npx package-name@1.2.3` → `("package-name", "1.2.3")`
- `uvx package-name` → `("package-name", None)`

### 4.5 OSV Query

```python
_OSV_ENDPOINT = "https://api.osv.dev/v1/query"
_TIMEOUT = 10  # seconds
```

Query format:
```json
{"package": {"name": "pkg", "ecosystem": "npm"}, "version": "1.2.3"}
```

### 4.6 Fail-Open Policy

```python
except Exception as exc:
    logger.debug("OSV check failed (allowing): %s", exc)
    return None  # Network errors → allow
```

Network errors, timeouts, and parse failures all allow the package to proceed.

### 4.7 Response Formatting

```python
ids = ", ".join(m["id"] for m in malware[:3])
summaries = "; ".join(m.get("summary", m["id"])[:100] for m in malware[:3])
return f"BLOCKED: Package '{package}' ({ecosystem}) has known malware advisories: {ids}"
```

Limits to first 3 advisories, truncates summaries to 100 chars.

---

## 5. Tool Infrastructure

### 5.1 Process Registry

**Location**: `tools/process_registry.py` (~1,172 lines)

Tracks running processes across tool calls. Maintains process metadata, stdout/stderr capture, and lifecycle management.

### 5.2 Approval System

**Location**: `tools/approval.py` (~926 lines)

User approval gate for destructive tool operations. Supports:
- Auto-approve patterns (regex-based)
- Always-block patterns
- Per-session approval memory
- Timeout-based approval expiry

### 5.3 Terminal Tool

**Location**: `tools/terminal_tool.py` (~1,749 lines)

Terminal command execution with:
- Multiple backend support (local, Docker, SSH, Modal, Daytona, Singularity)
- PTY mode for interactive commands
- Background process management
- Output streaming
- Working directory management

### 5.4 File Operations

**Location**: `tools/file_operations.py` (~1,216 lines)

Low-level file operations with:
- Atomic writes (write to temp, rename)
- Path security (symlink following prevention)
- Permission handling
- Lint checking post-write

### 5.5 Tool Backend Helpers

**Location**: `tools/tool_backend_helpers.py`

Shared utilities for tool backends:
- `managed_nous_tools_enabled()` — check if managed tool gateway is active
- Provider-specific configuration helpers

### 5.6 Tool Result Storage

**Location**: `tools/tool_result_storage.py`

Persistent storage for tool results across turns. Enforces turn budget limits and handles result persistence for long-running operations.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Camofox snapshot limit | 80,000 characters |
| Camofox default port | 9377 |
| Website policy cache TTL | 30 seconds |
| OSV check timeout | 10 seconds |
| OSV max advisories shown | 3 |
| OSV summary truncation | 100 chars |
| Nous token refresh skew | 120 seconds |
| Tool sandbox allowed tools | 7 |
| Approval patterns | regex-based |
| Turn budget enforcement | per-tool |

---

*Generated from source analysis of the Hermes Agent codebase.*
