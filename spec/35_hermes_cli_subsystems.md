# Hermes Agent — hermes_cli Subsystems

This document covers the hermes_cli/ modules that handle CLI commands beyond the main agent loop and gateway: OpenClaw migration, curses UI, clipboard, auth commands, web server, setup, doctor, plugins, skills hub, profiles, models, config, MCP config, runtime provider, Nous subscription, banner, status, memory setup, model switch, tools config, and model normalize.

---

## Table of Contents

1. [Claw Migration](#1-claw-migration)
2. [Curses UI](#2-curses-ui)
3. [Clipboard Image Extraction](#3-clipboard-image-extraction)
4. [Auth Commands](#4-auth-commands)
5. [Web Server](#5-web-server)
6. [Setup & Doctor](#6-setup--doctor)
7. [Plugins & Skills Hub](#7-plugins--skills-hub)
8. [Config, Models, and Providers](#8-config-models-and-providers)
9. [Profiles & Model Switch](#9-profiles--model-switch)
10. [Tools Config](#10-tools-config)
11. [Runtime Provider & Nous Subscription](#11-runtime-provider--nous-subscription)
12. [MCP Config & Memory Setup](#12-mcp-config--memory-setup)

---

## 1. Claw Migration

### Location

`hermes_cli/claw.py` (~734 lines)

### Purpose

One-time migration from OpenClaw to Hermes Agent. Detects running OpenClaw processes, previews migration changes, then applies configuration transfer.

### 1.1 Detection

```python
_OPENCLAW_DIR_NAMES = (".openclaw", ".clawdbot", ".moltbot")
```

Detects:
- **systemd service**: `systemctl --user is-active openclaw-gateway.service`
- **Processes**: `openclaw.exe`, `clawd.exe` (Windows tasklist), Node.js with "openclaw|clawd" in command line (PowerShell)
- **Legacy directories**: `.openclaw`, `.clawdbot`, `.moltbot`

### 1.2 Migration Script

Resolution order:
1. `optional-skills/migration/openclaw-migration/scripts/openclaw_to_hermes.py`
2. `$HERMES_HOME/skills/migration/openclaw-migration/scripts/openclaw_to_hermes.py` (Hub-installed)

### 1.3 Commands

| Command | Flags | Behavior |
|---------|-------|----------|
| `hermes claw migrate` | none | Preview then migrate (always shows preview first) |
| `hermes claw migrate` | `--dry-run` | Preview only, no changes |
| `hermes claw migrate` | `--yes` | Skip confirmation prompt |
| `hermes claw migrate` | `--preset full --overwrite` | Full migration, overwrite conflicts |
| `hermes claw cleanup` | none | Archive leftover OpenClaw directories |
| `hermes claw cleanup` | `--dry-run` | Preview what would be archived |

### 1.4 Cleanup

Archives known OpenClaw directories to a timestamped backup folder. Preserves data while removing clutter.

---

## 2. Curses UI

### Location

`hermes_cli/curses_ui.py` (~445 lines)

### Purpose

Shared curses-based UI components for interactive checklists. Used by `hermes tools` and `hermes skills` commands for keyboard-navigable multi-select.

### 2.1 curses_checklist

```python
def curses_checklist(
    title: str,
    items: List[str],
    selected: Set[int],
    *,
    cancel_returns: Set[int] | None = None,
    status_fn: Optional[Callable[[Set[int]], str]] = None,
) -> Set[int]:
```

**Features**:
- Color pairs: green (1), yellow (2), dim gray (3)
- Cursor navigation with arrow keys
- Space toggles selection
- Scroll offset for long lists
- `status_fn` callback renders live aggregate info on bottom row
- ESC/q returns `cancel_returns` (defaults to original selection)

**Safety**: Returns defaults immediately when `sys.stdin.isatty()` is False (subprocess pipe scenario).

### 2.2 curses_single_choice

```python
def curses_single_choice(
    title: str,
    items: List[str],
    default_index: int = 0,
) -> Optional[int]:
```

Single-select variant. Returns index or None on cancel.

### 2.3 Non-Curses Fallback

When curses is unavailable (Windows, missing ncurses), falls back to a text-based numbered list with `input()` selection.

### 2.4 flush_stdin

```python
def flush_stdin() -> None:
```

Drains OS input buffer after `curses.wrapper()` returns. Prevents leftover escape-sequence bytes (from arrow keys) from being silently consumed by subsequent `input()` or `getpass()` calls. Uses `termios.tcflush(sys.stdin, termios.TCIFLUSH)`. No-op on non-TTY stdin or Windows.

---

## 3. Clipboard Image Extraction

### Location

`hermes_cli/clipboard.py` (~432 lines)

### Purpose

Cross-platform clipboard image extraction. Uses only OS-level CLI tools — no external Python dependencies.

### 3.1 Platform Support

| Platform | Method |
|----------|--------|
| macOS | `pngpaste` (preferred), `osascript` (fallback) |
| Windows | PowerShell via `.NET System.Windows.Forms.Clipboard` |
| WSL2 | `powershell.exe` via `.NET System.Windows.Forms.Clipboard` |
| Linux (Wayland) | `wl-paste` |
| Linux (X11) | `xclip` |

### 3.2 Primary Functions

```python
def save_clipboard_image(dest: Path) -> bool:
    """Extract image from clipboard, save as PNG. Returns True on success."""

def has_clipboard_image() -> bool:
    """Quick check: does clipboard currently contain an image?"""
```

### 3.3 macOS Implementation

- **pngpaste**: `pngpaste dest.png` — fast, handles more formats
- **osascript**: AppleScript `clipboard info` checks for `«class PNGf»` or `«class TIFF»`, then extracts via AppleScript
- **Quick check**: `osascript -e "clipboard info"` → checks for image class markers

### 3.4 Windows/WSL2 Implementation

Uses PowerShell:
```powershell
[Windows.Forms.Clipboard]::GetImage() | ... | Out-File -Encoding UTF8
```
Checks `Image` format availability. Base64-encodes image data, decodes with Python `base64.b64decode()`.

### 3.5 Linux Implementation

- **Wayland**: `wl-paste --list-types` checks for image MIME types, then `wl-paste --type image/png > dest.png`
- **X11**: `xclip -selection clipboard -t image/png -o > dest.png`

---

## 4. Auth Commands

### Location

`hermes_cli/auth_commands.py` (~541 lines)

### Purpose

Credential-pool auth subcommands for managing API keys and OAuth tokens across multiple providers.

### 4.1 OAuth-Capable Providers

```python
_OAUTH_CAPABLE_PROVIDERS = {"anthropic", "nous", "openai-codex", "qwen-oauth"}
```

### 4.2 Provider Resolution

```python
def _normalize_provider(provider: str) -> str:
    # "or" → "openrouter", "open-router" → "openrouter"
    # Resolves custom provider names to pool keys
```

### 4.3 Key Operations

| Function | Purpose |
|----------|---------|
| `add_credential()` | Add API key to credential pool |
| `list_credentials()` | Show all credentials with status |
| `remove_credential()` | Remove a credential from pool |
| `login_oauth()` | OAuth flow for supported providers |
| `logout()` | Revoke OAuth token |
| `refresh_token()` | Refresh expiring access token |

### 4.4 Custom Provider Support

Resolves custom OpenAI-compatible endpoints from `config.yaml → custom_providers`. Maps display names to pool keys (`custom:<normalized_name>`).

### 4.5 Strategy Support

Supports credential pool strategies: `fill_first`, `round_robin`, `random`, `least_used`.

---

## 5. Web Server

### Location

`hermes_cli/web_server.py` (~2,108 lines)

### Purpose

Built-in web UI server for Hermes Agent. Provides a browser-based interface for chat, settings, and agent management.

### 5.1 Architecture

```
FastAPI app → static file serving + WebSocket + REST API
```

### 5.2 Key Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/` | GET | Serve web UI (HTML/JS/CSS) |
| `/api/chat` | POST | Send message to agent |
| `/api/chat/stream` | WebSocket | Stream agent responses |
| `/api/sessions` | GET | List conversations |
| `/api/settings` | GET/POST | Read/write config |
| `/api/files` | GET | Browse filesystem |
| `/health` | GET | Server health check |

### 5.3 WebSocket Streaming

```python
async def websocket_endpoint(websocket):
    await websocket.accept()
    # Forward deltas from agent to browser
    # Handle browser → agent message forwarding
```

### 5.4 Static File Serving

Serves built web assets from `hermes_cli/web/dist/`. Includes fallback for development mode.

### 5.5 Server Lifecycle

```python
def start_web_server(host="127.0.0.1", port=9876):
    uvicorn.run(app, host=host, port=port, log_level="warning")
```

Started as a background thread from the gateway process. Auto-detects available port if default is in use.

---

## 6. Setup & Doctor

### Location

- `hermes_cli/setup.py` (~3,199 lines)
- `hermes_cli/doctor.py` (~1,105 lines)

### 6.1 Setup (`setup.py`)

Initial installation wizard. Handles:
- Python dependency installation
- Environment variable configuration
- Provider API key setup
- Terminal environment initialization
- Skill installation
- Config.yaml generation

**Interactive prompts**: Uses `prompt_yes_no()`, `input()`, `getpass()` with colored output.

**Color system**:
```python
class Colors:
    GREEN = "\033[92m"
    YELLOW = "\033[93m"
    RED = "\033[91m"
    ...

def print_header(text): ...
def print_success(text): ...
def print_error(text): ...
```

### 6.2 Doctor (`doctor.py`)

Diagnostic tool that checks system health:

| Check | What It Validates |
|-------|-------------------|
| Python version | ≥ 3.10 required |
| Dependencies | All required packages installed |
| API keys | Configured and valid |
| Terminal backends | Docker/SSH/Modal reachable |
| Platform adapters | Messaging platform connectivity |
| Memory system | SQLite database integrity |
| Skills | Installed and importable |
| Config | YAML syntax and schema validation |

**Output**: Color-coded pass/fail/warning for each check with remediation suggestions.

---

## 7. Plugins & Skills Hub

### Location

- `hermes_cli/plugins.py` (~744 lines)
- `hermes_cli/plugins_cmd.py` (~1,128 lines)
- `hermes_cli/skills_hub.py` (~1,238 lines)

### 7.1 Plugin System (`plugins.py`)

Plugin discovery and management:
- Scans `plugins/` directory for installable plugins
- Handles installation, removal, and listing
- Validates plugin metadata from `plugin.yaml`

### 7.2 Plugin Commands (`plugins_cmd.py`)

CLI interface for plugin management:
- `hermes plugins list` — Show available plugins
- `hermes plugins install <name>` — Install a plugin
- `hermes plugins remove <name>` — Remove a plugin
- `hermes plugins status` — Show installed plugin status

### 7.3 Skills Hub (`skills_hub.py`)

Hub for discovering, installing, and managing skills:

**Remote Hub**: Fetches skill listings from a remote registry.

**Local Skills**: Scans `$HERMES_HOME/skills/` and `optional-skills/`.

**Operations**:
- Search skills by name/description
- Install skills from Hub
- Update installed skills
- Show skill details (metadata, dependencies)

---

## 8. Config, Models, and Providers

### Location

- `hermes_cli/config.py` (~3,415 lines)
- `hermes_cli/models.py` (~1,969 lines)
- `hermes_cli/providers.py` (~553 lines)

### 8.1 Config (`config.py`)

Central configuration management:

```python
def load_config() -> dict:
    """Load ~/.hermes/config.yaml with defaults."""

def save_config(config: dict) -> None:
    """Persist config to disk."""

def get_config_path() -> Path:
    """Return path to config.yaml."""

def get_hermes_home() -> Path:
    """Return HERMES_HOME path (default: ~/.hermes)."""
```

**Config sections**: providers, model, terminal, tts, memory, skills, cron, security, auxiliary, context, code_execution, website_policy.

**Layering**: Defaults → config.yaml → env vars → command-line flags.

### 8.2 Models (`models.py`)

Model catalog and metadata:

- Model definitions (name, context window, pricing, capabilities)
- Provider-specific model lists
- Vision/text/code-completion capability flags
- Token pricing information

### 8.3 Providers (`providers.py`)

Provider configuration and resolution:

- Provider definitions (API endpoint, auth type, features)
- Base URL resolution
- Provider capability detection

---

## 9. Profiles & Model Switch

### Location

- `hermes_cli/profiles.py` (~1,094 lines)
- `hermes_cli/model_switch.py` (~1,090 lines)

### 9.1 Profiles (`profiles.py`)

Per-profile configuration management:

```
~/.hermes/
├── profiles/
│   ├── default.yaml
│   ├── work.yaml
│   └── research.yaml
└── config.yaml
```

**Operations**:
- Create/delete/switch profiles
- Profile-specific API keys, models, terminal settings
- Inherit from base config with overrides

### 9.2 Model Switch (`model_switch.py`)

Runtime model switching:

- Lists available models for current provider
- Switches active model without restarting
- Validates model compatibility with current toolset
- Handles model-specific parameter adjustments

---

## 10. Tools Config

### Location

`hermes_cli/tools_config.py` (~1,722 lines)

### Purpose

Tool enable/disable configuration and management.

### 10.1 Features

- Per-tool enable/disable toggle
- Tool parameter customization
- Toolset presets (minimal, standard, full)
- Dependency resolution (tool A requires tool B)
- Validation of tool configurations

### 10.2 Integration

Read by `model_tools.py` at startup to determine which tools to include in the tool definitions sent to the LLM.

---

## 11. Runtime Provider & Nous Subscription

### Location

- `hermes_cli/runtime_provider.py` (~892 lines)
- `hermes_cli/nous_subscription.py` (~531 lines)

### 11.1 Runtime Provider (`runtime_provider.py`)

Runtime provider detection and configuration:

- Detects current execution environment (local, Docker, cloud)
- Resolves provider-specific settings
- Handles environment-specific feature flags

### 11.2 Nous Subscription (`nous_subscription.py`)

Nous Research subscription management:

- Subscription status checking
- Feature gate enforcement based on subscription tier
- Billing and usage tracking

---

## 12. MCP Config & Memory Setup

### Location

- `hermes_cli/mcp_config.py` (~716 lines)
- `hermes_cli/memory_setup.py` (~455 lines)

### 12.1 MCP Config (`mcp_config.py`)

MCP server configuration management:

- MCP server definitions (command, args, env)
- Tool discovery from MCP servers
- Server lifecycle (start/stop/restart)
- Health checking and auto-restart

### 12.2 Memory Setup (`memory_setup.py`)

Memory system initialization:

- SQLite database creation and migration
- Memory provider configuration
- Initial memory seeding
- Schema versioning

---

## Other CLI Modules

### Banner (`hermes_cli/banner.py`, ~535 lines)

ASCII art banner displayed on CLI startup. Includes version info, platform detection, and motd.

### Status (`hermes_cli/status.py`, ~476 lines)

System status display: running processes, active sessions, resource usage, gateway status.

### Model Normalize (`hermes_cli/model_normalize.py`, ~406 lines)

Model name normalization and aliasing. Maps provider-specific model names to canonical identifiers.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Largest CLI module | `main.py` at 6,121 lines |
| Config module | 3,415 lines |
| Setup module | 3,199 lines |
| Gateway module | 3,161 lines |
| Web server | 2,108 lines |
| Models catalog | 1,969 lines |
| Tools config | 1,722 lines |
| Skills Hub | 1,238 lines |
| Plugin commands | 1,128 lines |
| Doctor | 1,105 lines |
| Model switch | 1,090 lines |
| Profiles | 1,094 lines |
| OAuth-capable providers | 4 |
| Clipboard platforms | 5 |
| Credential pool strategies | 4 |
| Exhausted credential TTL | 1 hour (429/402) |

---

*Generated from source analysis of the Hermes Agent codebase.*
