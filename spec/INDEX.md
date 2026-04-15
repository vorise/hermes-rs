# Hermes Agent — Spec Index

> Quick-reference index across all spec documents.
> Total spec coverage: ~1,200+ KB across 15 markdown files.

---

## Spec Files

| # | File | What's Inside |
|---|------|---------------|
| — | [00_overview.md](00_overview.md) | Master architecture, repo structure, data flow, credential model, settings layers |
| 01 | [01_core_entry_query.md](01_core_entry_query.md) | `run_agent.py`, `cli.py`, `hermes_cli.main`, agent loop, iteration budget, context compression |
| 02 | [02_commands.md](02_commands.md) | All 60+ slash commands with args, options, and implementation |
| 03 | [03_tools.md](03_tools.md) | All 40+ tools: registry, schemas, handlers, toolsets, approval system |
| 04 | [04_components_cli_tui.md](04_components_cli_tui.md) | CLI TUI components, prompt_toolkit layout, Skin Engine, spinner, status bar |
| 05 | [05_components_messaging_platforms.md](05_components_messaging_platforms.md) | All 18+ platform adapters, message formatting, session routing |
| 06 | [06_services_context_state.md](06_services_context_state.md) | SQLite state store, session search, memory manager, prompt builder, model metadata |
| 07 | [07_gateway_cron.md](07_gateway_cron.md) | Gateway main loop, cron scheduler, session store, stream consumer |
| 08 | [08_environments_terminals.md](08_environments_terminals.md) | Terminal backends: local, Docker, SSH, Modal, Daytona, Singularity |
| 09 | [09_auth_providers.md](09_auth_providers.md) | Provider credential resolution, OAuth, API key management, 15+ providers |
| 10 | [10_utils_memory_skills.md](10_utils_memory_skills.md) | Memory system, skills system, skills Hub, skills sync, honcho integration |
| 11 | [11_special_systems.md](11_special_systems.md) | Soul system, web UI, ACP adapter, plugin system, model routing, trajectory |
| 12 | [12_constants_types.md](12_constants_types.md) | All constants, system prompts, model catalog, tool limits, config schema |
| 13 | [13_research_rl.md](13_research_rl.md) | RL training environments, batch runner, trajectory compression, Atropos |
| 14 | [14_mcp_acp.md](14_mcp_acp.md) | MCP client, ACP server, external tool integration |

---

## Quick Lookup

### "Where is X documented?"

| Topic | Spec File | Section |
|-------|-----------|---------|
| Main agent loop (`run_agent.py`) | 01 | §AIAgent |
| `run_conversation()` loop | 01 | §run_conversation |
| Iteration budget | 01 | §IterationBudget |
| Context compression | 01 | §ContextCompressor |
| CLI entry point (`hermes_cli.main`) | 01 | §hermes_cli.main |
| CLI TUI (`cli.py`) | 01 | §HermesCLI |
| Tool registry | 03 | §ToolRegistry |
| Terminal tool | 03 | §terminal_tool |
| File tools | 03 | §file_tools |
| Web tools | 03 | §web_tools |
| Browser tool | 03 | §browser_tool |
| MCP tool | 03 | §mcp_tool |
| Delegate tool | 03 | §delegate_tool |
| All slash commands | 02 | §per-command |
| `/model` command | 02 | §model |
| `/skills` command | 02 | §skills |
| `/tools` command | 02 | §tools |
| `/compress` command | 02 | §compress |
| Platform adapters | 05 | §per-platform |
| Telegram adapter | 05 | §telegram |
| Discord adapter | 05 | §discord |
| Slack adapter | 05 | §slack |
| WhatsApp adapter | 05 | §whatsapp |
| Gateway main loop | 07 | §run.py |
| Cron scheduler | 07 | §cron |
| Session store (gateway) | 07 | §session.py |
| SQLite state store | 06 | §hermes_state |
| Session search (FTS5) | 06 | §SessionDB |
| Memory manager | 06 | §memory_manager |
| Prompt builder | 06 | §prompt_builder |
| Model metadata | 06 | §model_metadata |
| Auxiliary client | 06 | §auxiliary_client |
| Credential pool | 06 | §credential_pool |
| Terminal backends | 08 | §environments |
| Modal backend | 08 | §modal |
| Docker backend | 08 | §docker |
| SSH backend | 08 | §ssh |
| Provider auth | 09 | §auth.py |
| Model switch | 09 | §model_switch |
| Provider resolution | 09 | §providers |
| Memory system | 10 | §memory |
| Skills system | 10 | §skills |
| Skills Hub | 10 | §skills_hub |
| Soul system | 11 | §soul |
| Web UI | 11 | §web_server |
| ACP adapter | 11 | §acp |
| Plugin system | 11 | §plugins |
| System prompts | 12 | §prompts |
| Model catalog | 12 | §models |
| RL environments | 13 | §environments |
| Batch runner | 13 | §batch_runner |
| Trajectory compression | 13 | §compressor |
| MCP client | 14 | §mcp_tool |
| ACP server | 14 | §acp_adapter |

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Total Python files | ~300+ |
| Total lines of code | ~500K+ |
| Number of slash commands | 60+ |
| Number of tools | 40+ |
| Number of platform adapters | 18+ |
| Number of terminal backends | 6 |
| Number of model providers | 15+ |
| Number of test files | ~100+ (~3,000 tests) |
| Optional extras | 20+ |
| Spec documentation size | ~1,200+ KB |

---

## Architecture in One Paragraph

Hermes Agent is a self-improving AI agent built by Nous Research. It runs as either a local CLI TUI (built on prompt_toolkit) or a persistent gateway process that connects to Telegram, Discord, Slack, WhatsApp, Signal, and 12+ other messaging platforms. The core agent loop (`run_agent.py` + `model_tools.py`) uses the OpenAI-compatible API protocol to call any LLM (Anthropic, OpenRouter, Nous Portal, OpenAI, or 15+ other providers) with tool calling. It has a decentralized tool registry with 40+ built-in tools (file I/O, terminal, web, browser, MCP, delegation), a skill system for procedural memory, an SQLite state store with FTS5 search, context compression, six terminal backends (local, Docker, SSH, Modal, Daytona, Singularity), a built-in cron scheduler, and a research pipeline for RL training with Atropos environments and trajectory generation.

---

*Generated 2026-04-15 from Hermes Agent source analysis.*
