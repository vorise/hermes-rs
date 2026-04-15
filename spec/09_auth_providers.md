# Hermes Agent — Authentication & Providers

This document covers provider credential resolution, API key management, model switching, and the 15+ supported providers.

---

## Table of Contents

1. [Auth Architecture](#1-auth-architecture)
2. [Credential Resolution](#2-credential-resolution)
3. [Provider Resolution](#3-provider-resolution)
4. [Model Switching](#4-model-switching)
5. [Model Catalog](#5-model-catalog)
6. [Supported Providers](#6-supported-providers)
7. [OAuth Support](#7-oauth-support)
8. [Nous Subscription](#8-nous-subscription)

---

## 1. Auth Architecture

### Key Files

| File | Purpose |
|------|---------|
| `hermes_cli/auth.py` (~126K) | Provider credential resolution |
| `hermes_cli/providers.py` (~17K) | Provider abstraction layer |
| `hermes_cli/model_switch.py` (~41K) | Model switching pipeline |
| `hermes_cli/models.py` (~72K) | Model catalog and provider lists |
| `hermes_cli/model_normalize.py` (~13K) | Model name normalization |
| `agent/credential_pool.py` (~58K) | Credential pool with failover |
| `hermes_cli/copilot_auth.py` (~10K) | GitHub Copilot authentication |

### Credential Sources

Credentials can come from multiple sources (in priority order):
1. Command-line flags
2. Environment variables (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, etc.)
3. Config file (`~/.hermes/.env`)
4. Default values (for free providers)

---

## 2. Credential Resolution

### Location

`hermes_cli/auth.py` (~126K lines)

### Purpose

Resolve provider credentials from environment variables, config file, and defaults.

### Resolution Flow

```
1. Check command-line flag
2. Check environment variable
3. Check ~/.hermes/.env
4. Check ~/.hermes/config.yaml
5. Use default (if applicable)
6. Fail if no credential found
```

### Environment Variables

| Variable | Provider | Purpose |
|----------|----------|---------|
| `OPENAI_API_KEY` | OpenAI | OpenAI API key |
| `ANTHROPIC_API_KEY` | Anthropic | Anthropic API key |
| `OPENROUTER_API_KEY` | OpenRouter | OpenRouter API key |
| `NOUS_API_KEY` | Nous Portal | Nous Portal API key |
| `XIAOMI_MIMO_API_KEY` | Xiaomi MiMo | Xiaomi API key |
| `ZHIPU_API_KEY` | z.ai/GLM | GLM API key |
| `MOONSHOT_API_KEY` | Kimi/Moonshot | Moonshot API key |
| `MINIMAX_API_KEY` | MiniMax | MiniMax API key |
| `HUGGINGFACE_API_KEY` | HuggingFace | HuggingFace API key |
| `MISTRAL_API_KEY` | Mistral | Mistral API key |
| `ELEVEN_LABS_API_KEY` | ElevenLabs | ElevenLabs TTS key |
| `BROWSERBASE_API_KEY` | Browserbase | Browser automation key |
| `BROWSER_USE_API_KEY` | Browser Use | Browser automation key |
| `EXA_API_KEY` | Exa | Web search key |
| `FIRECRAWL_API_KEY` | Firecrawl | Web extraction key |
| `FAL_API_KEY` | FAL | Image generation key |

---

## 3. Provider Resolution

### Location

`hermes_cli/providers.py` (~17K lines)

### Purpose

Abstraction layer for provider-specific operations.

### Provider Properties

| Property | Purpose |
|----------|---------|
| `base_url` | API endpoint URL |
| `api_key_env` | Environment variable name |
| `display_name` | Human-readable name |
| `default_model` | Default model for provider |
| `supports_tools` | Whether provider supports tool calling |
| `supports_vision` | Whether provider supports vision |

---

## 4. Model Switching

### Location

`hermes_cli/model_switch.py` (~41K lines)

### Purpose

Shared model switching pipeline for both CLI and messaging platforms.

### Pipeline Flow

```
1. Parse provider:model string
2. Validate provider exists
3. Resolve credentials
4. Fetch model metadata
5. Update config.yaml
6. Show model info to user
7. Rebuild API client on next turn
```

### Model String Formats

| Format | Example | Parsed As |
|--------|---------|-----------|
| `provider:model` | `anthropic/claude-sonnet-4-6` | provider=anthropic, model=claude-sonnet-4-6 |
| `model` | `claude-sonnet-4-6` | provider=default, model=claude-sonnet-4-6 |
| `openrouter/provider/model` | `openrouter/anthropic/claude-3-opus` | provider=openrouter, model=... |

---

## 5. Model Catalog

### Location

`hermes_cli/models.py` (~72K lines)

### Purpose

Model catalog and provider model lists.

### Features

- All supported models per provider
- Default models per provider
- Model context lengths
- Model capabilities (tools, vision, reasoning)
- Provider model lists for autocomplete

---

## 6. Supported Providers

### Provider Details

| Provider | Base URL | Models | Free Tier |
|----------|----------|--------|-----------|
| **Nous Portal** | `api.nousresearch.com` | Hermes 3, 2.5, etc. | Yes (limited) |
| **OpenRouter** | `openrouter.ai` | 200+ models | Varies |
| **Anthropic** | `api.anthropic.com` | Claude 3/4 family | No |
| **OpenAI** | `api.openai.com` | GPT-4/4o/o-series | No |
| **Xiaomi MiMo** | `platform.xiaomimimo.com` | MiMo models | Free |
| **z.ai/GLM** | `open.bigmodel.cn` | GLM-4, etc. | Free tier |
| **Kimi/Moonshot** | `platform.moonshot.ai` | Kimi models | Free tier |
| **MiniMax** | `api.minimax.chat` | MiniMax models | Free tier |
| **HuggingFace** | `huggingface.co` | Various | Free tier |
| **Ollama** | `localhost:11434` | Local models | Free |
| **Mistral** | `api.mistral.ai` | Mistral models | Free tier |
| **Local endpoint** | Any URL | Custom | N/A |
| **GitHub Copilot** | `api.github.com` | GPT models | With subscription |
| **Novita** | `api.novita.ai` | Various | Free tier |
| **Fireworks** | `api.fireworks.ai` | Various | Free tier |

---

## 7. OAuth Support

### MCP OAuth

`tools/mcp_oauth.py` (~17K lines) — OAuth flow for MCP servers that require user authentication.

### Features

- OAuth 2.0 authorization code flow
- Token refresh
- Token storage
- Authorization URL generation

---

## 8. Nous Subscription

### Purpose

Special handling for Nous Research subscribers.

### Features

- Nous Portal API integration
- Browser Use cloud mode for subscribers
- Reduced rate limits for free tier
- Priority access for paid tier

### Subscription Prompt

`agent/prompt_builder.py` — `build_nous_subscription_prompt()` adds Nous-specific hints to the system prompt for subscribers.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Supported providers | 15+ |
| Models via OpenRouter | 200+ |
| Credential sources | 4 (CLI, env, .env, config) |
| Auth methods | API key, OAuth, JWT |
| Free providers | 8+ |

---

*Generated from source analysis of the Hermes Agent codebase.*
