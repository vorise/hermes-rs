# Hermes Agent — CLI Setup, Model Catalogs, Tools Config & Command Registry

This document covers `hermes_cli/setup.py` (~3,199 lines), `hermes_cli/models.py` (~1,969 lines), `hermes_cli/tools_config.py` (~1,722 lines), and `hermes_cli/commands.py` (~1,217 lines).

---

## Table of Contents

1. [Setup Wizard](#1-setup-wizard)
2. [Model Catalogs](#2-model-catalogs)
3. [Tools Configuration](#3-tools-configuration)
4. [Command Registry](#4-command-registry)

---

## 1. Setup Wizard

### Location

`hermes_cli/setup.py` (~3,199 lines)

### Purpose

Interactive setup wizard for configuring Hermes Agent. Supports full, quick, and section-specific setup. Delegates provider/model selection to `hermes model` flow for a single code path.

### 1.1 Wizard Sections

```python
SETUP_SECTIONS = [
    ("model",   "Model & Provider",             setup_model_provider),
    ("tts",     "Text-to-Speech",               setup_tts),
    ("terminal","Terminal Backend",              setup_terminal_backend),
    ("gateway", "Messaging Platforms (Gateway)", setup_gateway),
    ("tools",   "Tools",                        setup_tools),
    ("agent",   "Agent Settings",               setup_agent_settings),
]
```

**Returning-user menu** (omits TTS — already in model setup):
`model`, `terminal`, `gateway`, `tools`, `agent`

### 1.2 Wizard Modes

| Mode | Trigger | Behavior |
|------|---------|----------|
| First-time quick | No provider configured | Provider + model only, defaults for rest |
| First-time full | No provider configured | All 6 sections |
| Returning quick | Provider already set | Configure missing items only |
| Returning full | Provider already set | Reconfigure everything |
| Section-specific | `hermes setup <section>` | Single section |

### 1.3 Quick Setup (First-Time)

```python
def _run_first_time_quick_setup(config, hermes_home, is_existing):
```

**Steps**:
1. Model & Provider (delegates to `select_provider_and_model()`) — skips credential rotation, vision, TTS
2. Applies defaults: TTS=Edge, terminal=local, agent settings recommended
3. Tools with first-install flow
4. Messaging platforms checklist
5. Summary and offer to launch chat

### 1.4 Quick Setup (Returning)

```python
def _run_quick_setup(config, hermes_home):
```

Runs each section's `_skip_configured_section()` check — if already configured, shows summary and offers to skip. Only runs sections that need configuration.

### 1.5 Model & Provider Setup

```python
def setup_model_provider(config, *, quick=False):
```

**Flow**:
1. Delegates to `select_provider_and_model()` (shared with `hermes model`)
2. Re-syncs config from disk (critical — `cmd_model` writes its own config)
3. **Same-provider fallback & rotation** (full setup only): prompts to add additional credentials to the credential pool, then selects rotation strategy (fill_first, round_robin, random)
4. **Vision setup** (full setup only, if no vision backends available): offers OpenRouter (Gemini) or OpenAI-compatible endpoint with vision model selection
5. **Nous subscription defaults**: applies defaults if `nous` provider selected
6. **TTS setup** (full setup only, non-nous): delegates to `_setup_tts_provider()`

### 1.6 TTS Provider Setup

```python
def _setup_tts_provider(config):
```

**Provider choices**:
| Provider | Setup |
|----------|-------|
| Nous Subscription (managed OpenAI) | No key needed, bills to subscription |
| Edge TTS | Free, no setup |
| ElevenLabs | Prompts for API key |
| OpenAI TTS | Prompts for API key |
| MiniMax TTS | Prompts for API key |
| Mistral Voxtral | Prompts for API key |
| NeuTTS | Installs `espeak-ng` + `neutts[all]` (~300MB model) |

**NeuTTS installation** (`_install_neutts_deps()`):
1. Checks `espeak-ng` (phonemizer) — auto-installs via brew/choco/apt
2. Installs `neutts[all]` via pip (~50MB install + ~300MB model download)

### 1.7 Terminal Backend Setup

```python
def setup_terminal_backend(config):
```

**Backend choices** (platform-dependent):

| Backend | Config | Platform |
|---------|--------|----------|
| Local | CWD, sudo password | All |
| Docker | Image, CPU, memory, disk | All |
| Modal | Mode (managed/direct), tokens, resources | All |
| SSH | Host, user, port, key path, test connection | All |
| Daytona | API key, image, resources | All |
| Singularity/Apptainer | Image, resources | Linux only |

**Modal managed mode**: When Nous subscription is active, offers "Use my Nous subscription" (managed gateway, bills to subscription) vs "Use my own Modal account" (direct Modal tokens).

**Container resources** (`_prompt_container_resources()`):
- Persistence (yes/no)
- CPU cores (default: 1)
- Memory in MB (default: 5120 = 5GB)
- Disk in MB (default: 51200 = 50GB)

### 1.8 Agent Settings

```python
def setup_agent_settings(config):
```

**Configurable settings**:

| Setting | Default | Description |
|---------|---------|-------------|
| Max iterations | 90 | Tool-calling iterations per conversation |
| Tool progress mode | `all` | `off` / `new` / `all` / `verbose` |
| Compression threshold | 0.50 | Context compression trigger (0.5-0.95) |
| Session reset mode | `both` | `idle` / `daily` / `both` / `none` |
| Inactivity timeout | 1440 min | 24 hours |
| Daily reset hour | 4:00 | Local time |

**Recommended defaults** (`_apply_default_agent_settings()`):
```python
config["agent"]["max_turns"] = 90
config["display"]["tool_progress"] = "all"
config["compression"]["enabled"] = True
config["compression"]["threshold"] = 0.50
config["session_reset"] = {"mode": "both", "idle_minutes": 1440, "at_hour": 4}
```

### 1.9 Messaging Platforms (Gateway Setup)

```python
_GATEWAY_PLATFORMS = [
    ("Telegram",      "TELEGRAM_BOT_TOKEN",     _setup_telegram),
    ("Discord",       "DISCORD_BOT_TOKEN",      _setup_discord),
    ("Slack",         "SLACK_BOT_TOKEN",        _setup_slack),
    ("Signal",        "SIGNAL_HTTP_URL",        _setup_signal),
    ("Email",         "EMAIL_ADDRESS",          _setup_email),
    ("SMS (Twilio)",  "TWILIO_ACCOUNT_SID",     _setup_sms),
    ("Matrix",        "MATRIX_ACCESS_TOKEN",    _setup_matrix),
    ("Mattermost",    "MATTERMOST_TOKEN",       _setup_mattermost),
    ("WhatsApp",      "WHATSAPP_ENABLED",       _setup_whatsapp),
    ("DingTalk",      "DINGTALK_CLIENT_ID",     _setup_dingtalk),
    ("Feishu / Lark", "FEISHU_APP_ID",          _setup_feishu),
    ("WeCom",         "WECOM_BOT_ID",           _setup_wecom),
    ("WeCom Callback","WECOM_CALLBACK_CORP_ID", _setup_wecom_callback),
    ("Weixin",        "WEIXIN_ACCOUNT_ID",      _setup_weixin),
    ("BlueBubbles",   "BLUEBUBBLES_SERVER_URL", _setup_bluebubbles),
    ("QQ Bot",        "QQ_APP_ID",              _setup_qqbot),
    ("Webhooks",      "WEBHOOK_ENABLED",        _setup_webhooks),
]
```

**Flow**:
1. Multi-select checklist (pre-selects configured platforms)
2. Runs setup for each selected platform
3. After all platforms: checks for missing home channels
4. Offers to install gateway as systemd/launchd service
5. Offers to start/restart gateway

**Per-platform setup pattern**:
1. Check if already configured → offer to reconfigure
2. Show setup instructions/URLs
3. Prompt for credentials (password=True for secrets)
4. Save to `.env` via `save_env_value()`
5. Prompt for user allowlist (security)
6. Prompt for home channel (cron job delivery)

**Discord ID cleaning** (`_clean_discord_user_ids()`):
Strips `<@123>`, `<@!123>`, `user:123` prefixes from comma-separated IDs.

**Gateway service installation**: After platforms configured, detects systemd (Linux) or launchd (macOS), offers to install as background service with auto-start on boot.

### 1.10 OpenClaw Migration

```python
def _offer_openclaw_migration(hermes_home: Path) -> bool:
```

**Detection**: Checks for `~/.openclaw` directory and migration script at `optional-skills/migration/openclaw-migration/scripts/openclaw_to_hermes.py`.

**Two-phase process**:
1. **Dry-run preview**: Creates `Migrator(execute=False, overwrite=True)` to show what would be imported, overwritten, or skipped
2. **Confirm & execute**: Creates `Migrator(execute=True, overwrite=False)` — preserves existing Hermes configs

**High-impact warnings** for:
- Gateway/messaging tokens (platform hijack risk)
- Config values (different semantics between OpenClaw and Hermes)
- Instruction/context files (incompatible procedures)

### 1.11 Post-Migration Section Skip Logic

```python
def _skip_configured_section(config, section_key, label) -> bool:
```

After OpenClaw migration, shows summary of what was imported for each section and offers to skip reconfiguration.

### 1.12 Setup Summary

```python
def _print_setup_summary(config, hermes_home):
```

**Tool availability summary** — checks each tool category:
- Vision (via `get_available_vision_backends()`)
- Mixture of Agents (requires OpenRouter)
- Web Search & Extract (Exa/Parallel/Firecrawl/Tavily/Nous)
- Browser Automation (local/Browserbase/Browser Use/Camofox/Firecrawl/Nous)
- Image Generation (FAL/Nous)
- TTS (provider-specific)
- Modal Execution (managed/direct/Nous)
- RL Training (Tinker + WandB)
- Home Assistant
- Skills Hub (GitHub)
- Terminal (always)
- Task planning (always)
- Skills (always)

### 1.13 UI Primitives

| Function | Purpose |
|----------|---------|
| `prompt(text, default)` | Styled input with getpass for passwords |
| `prompt_choice(question, choices, default)` | Curses-based arrow key navigation |
| `prompt_yes_no(question, default)` | Yes/no with default |
| `prompt_checklist(title, items, pre_selected)` | Multi-select with Space toggle |
| `print_header(text)` | Colored section header |
| `print_info(text)` | Info message |
| `print_success(text)` | Green success |
| `print_warning(text)` | Yellow warning |
| `print_error(text)` | Red error |

---

## 2. Model Catalogs

### Location

`hermes_cli/models.py` (~1,969 lines)

### Purpose

Per-provider model catalogs, Nous Portal filtering, account tier detection, and canonical provider registry.

### 2.1 Provider Model Catalogs

```python
_PROVIDER_MODELS: dict[str, list[str]]
```

**Models per provider** (key selections):

| Provider | Models |
|----------|--------|
| openrouter | ~30 curated models (Claude, GPT, Gemini, etc.) |
| nous | Portal-curated list |
| copilot | From `codex_models.py` |
| gemini | Gemini 3 Pro, 3 Flash, 2.5 Pro, 2.5 Flash |
| zai | GLM-5, GLM-4.7, GLM-4.6 |
| kimi-coding | Kimi-K2.5, Kimi-K2-Thinking, Kimi-K2 |
| minimax | MiniMax-M2.7, M2.5, M2.5-free, M2.1 |
| anthropic | Claude Opus 4.6/4.5/4.1, Sonnet 4.6/4.5/4, Haiku 4.5/3.5 |
| xai | Grok models |
| xiaomi | MiMo-V2-Pro, Omni, Flash |
| arcee | Trinity-Large-Thinking, Large-Preview, Mini |
| opencode-zen | 35+ curated models (GPT, Claude, Gemini, etc.) |
| opencode-go | GLM-5, Kimi-K2.5, MiMo-V2, MiniMax-M2 |
| ai-gateway | 20+ models via Vercel |
| kilocode | Claude Opus/Sonnet, GPT, Gemini |
| alibaba | Qwen3.5-Plus, Qwen3-Coder, third-party (GLM, Kimi, MiniMax) |
| huggingface | 8 open models (Qwen, DeepSeek, Kimi, MiniMax, GLM, MiMo) |
| deepseek | deepseek-chat, deepseek-reasoner |

### 2.2 Nous Portal Free-Model Filtering

```python
_NOUS_ALLOWED_FREE_MODELS = frozenset({
    "xiaomi/mimo-v2-pro",
    "xiaomi/mimo-v2-omni",
})
```

**Rules**:
- Paid models NOT in allowlist → keep (normal case)
- Free models NOT in allowlist → drop (prevents promotional clutter)
- Allowlist models that ARE free → keep
- Allowlist models that are NOT free → drop

### 2.3 Nous Account Tier Detection

```python
def fetch_nous_account_tier(access_token, portal_base_url) -> dict:
    # GET <portal>/api/oauth/account
    # Returns {"subscription": {"plan", "tier", "monthly_charge", "credits_remaining"}}

def is_nous_free_tier(account_info) -> bool:
    # subscription.monthly_charge == 0
```

**TTL cache**: 180 seconds (3 minutes) — short enough to reflect upgrades quickly, long enough to avoid repeated API calls.

```python
def check_nous_free_tier() -> bool:
    # Refreshes credentials if needed, fetches account info, caches result
    # Returns False (paid) on any error — never blocks paying users
```

### 2.4 Nous Model Partitioning

```python
def partition_nous_models_by_tier(model_ids, pricing, free_tier):
    # Paid tier: all models selectable
    # Free tier: only free models selectable, paid models shown grayed out
```

### 2.5 Canonical Provider Registry

```python
class ProviderEntry(NamedTuple):
    slug: str       # Internal ID (config.yaml, --provider flag)
    label: str      # Short display name
    tui_desc: str   # Detailed description for `hermes model` TUI

CANONICAL_PROVIDERS: list[ProviderEntry] = [
    # 25 providers: nous, openrouter, anthropic, openai-codex, xiaomi,
    # qwen-oauth, copilot, copilot-acp, huggingface, gemini, deepseek,
    # xai, zai, kimi-coding, kimi-coding-cn, minimax, minimax-cn,
    # alibaba, arcee, kilocode, opencode-zen, opencode-go, ai-gateway
]
```

### 2.6 Provider Aliases

```python
_PROVIDER_ALIASES = {
    "glm": "zai", "z-ai": "zai", "z.ai": "zai", "zhipu": "zai",
    "github": "copilot", "github-copilot": "copilot",
    "google": "gemini", "google-gemini": "gemini",
    "kimi": "kimi-coding", "moonshot": "kimi-coding",
    "claude": "anthropic", "claude-code": "anthropic",
    "opencode": "opencode-zen", "zen": "opencode-zen",
    "aigateway": "ai-gateway", "vercel": "ai-gateway",
    "kilo": "kilocode", "dashscope": "alibaba", "aliyun": "alibaba",
    "qwen": "alibaba", "hf": "huggingface",
    "mimo": "xiaomi", "grok": "xai", "x-ai": "xai",
    # ... 50+ aliases total
}
```

### 2.7 Default Model Resolution

```python
def get_default_model_for_provider(provider: str) -> str:
    # Uses first entry in _PROVIDER_MODELS
```

### 2.8 OpenRouter Model Fallback

```python
OPENROUTER_MODELS: list[str]
```

Snapshot of popular OpenRouter models as fallback when API fetch fails. Includes Claude, GPT, Gemini, Kimi, GLM, MiniMax, DeepSeek, Qwen models.

---

## 3. Tools Configuration

### Location

`hermes_cli/tools_config.py` (~1,722 lines)

### Purpose

Unified tool configuration shared between `hermes tools` and `hermes setup tools`. Handles platform selection, toolset toggles, and provider/API key configuration.

### 3.1 Platform Registry

```python
PLATFORMS: dict[str, dict] = {
    "cli":       {"name": "CLI",             "default_toolset": "hermes-cli"},
    "telegram":  {"name": "Telegram",        "default_toolset": "hermes-messaging"},
    "discord":   {"name": "Discord",         "default_toolset": "hermes-messaging"},
    "slack":     {"name": "Slack",           "default_toolset": "hermes-messaging"},
    "whatsapp":  {"name": "WhatsApp",        "default_toolset": "hermes-messaging"},
    "qqbot":     {"name": "QQ Bot",          "default_toolset": "hermes-messaging"},
}
```

### 3.2 Configurable Toolsets

```python
CONFIGURABLE_TOOLSETS = [
    (ts_key, ts_name, ts_description)
    # 18 toolsets: web, browser, terminal, file, code_execution,
    # vision, image_gen, moa, tts, skills, todo, memory,
    # session_search, clarify, delegation, cronjob, messaging,
    # rl, homeassistant
]

_DEFAULT_OFF_TOOLSETS = {"moa", "homeassistant", "rl"}
```

### 3.3 Tool Categories (Provider Configuration)

```python
TOOL_CATEGORIES: dict[str, dict] = {
    "web": {
        "name": "Web Search & Extract",
        "providers": [
            {"name": "Nous Subscription", "managed_nous_feature": "web", ...},
            {"name": "Exa", "env_vars": [{"key": "EXA_API_KEY", ...}]},
            {"name": "Parallel", "env_vars": [{"key": "PARALLEL_API_KEY", ...}]},
            {"name": "Tavily", "env_vars": [{"key": "TAVILY_API_KEY", ...}]},
            {"name": "Firecrawl Self-Hosted", ...},
        ],
    },
    "image_gen": {
        "name": "Image Generation",
        "providers": [
            {"name": "Nous Subscription", "managed_nous_feature": "image_gen", ...},
            {"name": "FAL.ai", "env_vars": [{"key": "FAL_KEY", ...}]},
        ],
    },
    "browser": {
        "name": "Browser Automation",
        "providers": [
            {"name": "Nous Subscription (Browser Use cloud)", ...},
            {"name": "Local Browser", "browser_provider": "local", ...},
            {"name": "Browserbase", ...},
            {"name": "Browser Use", ...},
            {"name": "Firecrawl", ...},
            {"name": "Camofox", ...},
        ],
    },
    "homeassistant": {...},
    "rl": {
        "name": "RL Training",
        "providers": [{
            "name": "Tinker / Atropos",
            "env_vars": [
                {"key": "TINKER_API_KEY", ...},
                {"key": "WANDB_API_KEY", ...},
            ],
            "post_setup": "rl_training",
        }],
    },
}
```

### 3.4 Simple Env Requirements (Fallback)

```python
TOOLSET_ENV_REQUIREMENTS = {
    "vision": [("OPENROUTER_API_KEY", "https://openrouter.ai/keys")],
    "moa":    [("OPENROUTER_API_KEY", "https://openrouter.ai/keys")],
}
```

### 3.5 Post-Setup Hooks

```python
def _run_post_setup(post_setup_key: str):
```

| Key | Action |
|-----|--------|
| `agent_browser` / `browserbase` | `npm install` for agent-browser |
| `camofox` | `npm install @askjo/camofox-browser` |
| `rl_training` | Install `tinker-atropos` submodule via `uv pip install -e` |

### 3.6 Platform Toolset Resolution

```python
def _get_platform_tools(config, platform, *, include_default_mcp_servers=True) -> Set[str]:
```

**Resolution logic**:
1. Look up `platform_toolsets[platform]` in config
2. If no explicit config → resolve composite toolset names (e.g., "hermes-cli") to individual tools, then reverse-map to configurable keys
3. **Plugin toolsets**: enabled by default unless explicitly disabled (tracked via `known_plugin_toolsets`)
4. **MCP servers**: included by default unless `"no_mcp"` sentinel or explicit allowlist
5. Non-configurable toolset entries pass through (custom toolsets, MCP server names)

**Explicit config detection**: If saved toolset list contains any configurable keys directly, uses direct membership (avoids subset-inference bug where composite toolsets cause disabled toolsets to re-appear).

### 3.7 Platform Toolset Saving

```python
def _save_platform_tools(config, platform, enabled_toolset_keys: Set[str]):
```

Preserves non-configurable entries (MCP server names). Handles known_plugin_toolsets tracking.

### 3.8 Tool Availability Checking

```python
def check_tool_availability(config, toolset_key) -> dict:
    # Returns {"available": bool, "missing": list, "message": str}
    # Checks env vars, Python imports, system binaries
```

### 3.9 Unified Tools Command

```python
def tools_command(first_install=False, config=None):
```

**Flow**:
1. Load config, detect enabled platforms
2. Show platform checklist (multi-select)
3. For each selected platform:
   - Show enabled toolsets as checklist
   - Toggle toolsets on/off
   - For newly enabled toolsets: prompt for API keys/providers
4. Save config
5. Run post-setup hooks for installed tools

**First-install mode**: Simplified flow — no platform menu, prompts for all unconfigured API keys.

### 3.10 Nous Subscription Feature Flags

```python
def get_nous_subscription_features(config) -> SubscriptionFeatures:
    # Returns dataclass with: web, browser, image_gen, tts, modal, nous_auth_present
    # Each feature has: managed_by_nous, available, current_provider, direct_override
```

---

## 4. Command Registry

### Location

`hermes_cli/commands.py` (~1,217 lines)

### Purpose

Central registry for all slash commands. Defines `CommandDef` dataclass, registers commands with metadata (args, aliases, categories, CLI/gateway scoping), and provides platform-specific command exports.

### 4.1 CommandDef Dataclass

```python
@dataclass
class CommandDef:
    name: str                        # Canonical command name (without /)
    description: str                 # Short description
    category: str                    # Grouping for help display
    aliases: tuple[str, ...] = ()    # Alternative names
    args_hint: str = ""              # Tab-completion hint (e.g., "[on|off|status]")
    subcommands: tuple[str, ...] = ()  # Explicit subcommands
    cli_only: bool = False           # Only available in CLI TUI
    gateway_only: bool = False       # Only available in gateway
    gateway_config_gate: str = ""    # Dot-separated config key that gates availability
```

### 4.2 Command Categories

| Category | Commands |
|----------|----------|
| Session | new, reset, stop, approve, deny, status, restart, background, sessions |
| Configuration | model, skin, voice, compress, set, config |
| Tools & Skills | tools, skills, skill_manage, memory, memory_manage, todo, web_research, browser, clarify, cron |
| Info | help, about, usage, provider |
| Exit | quit, exit |

### 4.3 Command Registry Operations

```python
COMMAND_REGISTRY: list[CommandDef]
```

**Derived lookups**:

| Lookup | Purpose |
|--------|---------|
| `COMMANDS: dict[str, str]` | Flat `"/command" -> description` (backwards compat) |
| `COMMANDS_BY_CATEGORY: dict` | Categorized command descriptions |
| `SUBCOMMANDS: dict[str, list]` | `"/cmd" -> ["sub1", "sub2"]` |
| `GATEWAY_KNOWN_COMMANDS: frozenset` | All dispatchable command names + aliases |

**Subcommand auto-extraction**: Parses `args_hint` for pipe-separated patterns (e.g., `[on|off|tts|status]`) when no explicit subcommands defined.

### 4.4 Gateway Helpers

```python
def _resolve_config_gates() -> set[str]:
    # Reads config.yaml, walks dot-separated key path for each gated command
    # Returns canonical names of commands whose config gate is truthy

def _is_gateway_available(cmd, config_overrides=None) -> bool:
    # Unconditionally available when cli_only=False
    # When cli_only=True but gateway_config_gate set, checks config value

def gateway_help_lines() -> list[str]:
    # Generates gateway help text from registry

def telegram_bot_commands() -> list[tuple[str, str]]:
    # (command_name, description) for Telegram setMyCommands
    # Hyphens replaced with underscores (Telegram requirement)
```

### 4.5 Telegram Command Sanitization

```python
def _sanitize_telegram_name(raw: str) -> str:
    # Telegram: 1-32 chars, lowercase a-z, digits 0-9, underscores only
    # Steps: lowercase → replace hyphens with underscores → strip invalid chars
    #        → collapse consecutive underscores → strip leading/trailing underscores
```

### 4.6 Command Name Clamping

```python
def _clamp_command_names(entries, reserved) -> list[tuple[str, str]]:
    """Enforce 32-char command name limit with collision avoidance.
    
    Both Telegram and Discord cap slash command names at 32 characters.
    Truncated names that collide get a digit suffix (0-9).
    If all 10 digit slots exhausted, entry is silently dropped.
    """
```

### 4.7 Gateway Skill/Plugin Collection

```python
def _collect_gateway_skill_entries(platform, max_slots, reserved_names, desc_limit, sanitize_name=None):
    """Collect plugin + skill entries for a gateway platform.
    
    Priority order:
      1. Plugin slash commands (take precedence over skills, never trimmed)
      2. Built-in skill commands (fill remaining slots, alphabetical)
    
    Filtering:
      - Hub-installed skills excluded (accessible via /skills)
      - Per-platform disabled skills excluded
      - Names clamped to platform limits
    """
```

### 4.8 Platform-Specific Wrappers

```python
def telegram_menu_commands(max_commands=100) -> tuple[list[tuple[str, str]], int]:
    # Returns (menu_commands, hidden_count)
    # Core commands + plugins + skills (trimmed at cap)

def discord_skill_commands(max_slots, reserved_names) -> tuple[list[tuple[str, str, str]], int]:
    # Same as telegram but: hyphens allowed, desc limit 100 chars

def discord_skill_commands_by_category(reserved_names) -> tuple[dict, list, int]:
    # Groups skills by top-level directory category for Discord subcommand groups
    # Root-level skills returned as uncategorized
```

### 4.9 Command Resolution

```python
def resolve_command(name: str) -> Optional[CommandDef]:
    # Looks up by name or alias
    # Returns None if not found

def get_command_description(cmd: CommandDef) -> str:
    # Includes arg hint and alias info
```

---

## Key Numbers

| Metric | Value |
|--------|-------|
| setup.py lines | ~3,199 |
| models.py lines | ~1,969 |
| tools_config.py lines | ~1,722 |
| commands.py lines | ~1,217 |
| Setup wizard sections | 6 (model, tts, terminal, gateway, tools, agent) |
| Messaging platforms in setup | 17 |
| Providers in canonical registry | 25 |
| Provider aliases | 50+ |
| Configurable toolsets | 18 |
| Default-off toolsets | 3 (moa, homeassistant, rl) |
| Tool categories with providers | 5 (web, image_gen, browser, homeassistant, rl) |
| Built-in skins | 10 |
| Command categories | 5 |
| Telegram/Discord command name limit | 32 chars |
| Telegram menu command max | 100 |
| Nous free-tier cache TTL | 180 seconds |
| Nous allowed free models | 2 (xiaomi/mimo-v2-pro, mimo-v2-omni) |
| NeuTTS model size | ~300MB |
| Container default memory | 5120 MB (5GB) |
| Container default disk | 51200 MB (50GB) |
| Default max iterations | 90 |
| Default compression threshold | 0.50 |
| Default session idle timeout | 1440 minutes (24 hours) |
| Default daily reset hour | 4:00 |

---

*Generated from source analysis of the Hermes Agent codebase.*
