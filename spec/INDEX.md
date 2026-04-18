# Hermes Agent — Spec Index

> Quick-reference index across all spec documents.
> Total spec coverage: ~1,600+ KB across 51 markdown files (00-49, numbered non-contiguously).

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
| 15 | [15_detailed_behaviors.md](15_detailed_behaviors.md) | Detailed behavior spec: implementation-level behaviors from run_agent.py, model_tools.py |
| 16 | [16_sandbox_checkpoint_clarify.md](16_sandbox_checkpoint_clarify.md) | Code execution sandbox, checkpoint manager (shadow git repos), clarify tool |
| 17 | [17_delegation_browser_mcp.md](17_delegation_browser_mcp.md) | Delegate tool (subagent), thread-scoped interrupts, env passthrough, browser automation, MCP client |
| 18 | [18_approval_file_web.md](18_approval_file_web.md) | Dangerous command approval system, file tools module, web tool architecture |
| 19 | [19_credential_pool_rate_limits.md](19_credential_pool_rate_limits.md) | Persistent multi-credential pool for provider failover, rate limit tracking |
| 20 | [20_redaction_context_compression.md](20_redaction_context_compression.md) | Regex-based secret redaction system, context compression engine |
| 21 | [21_prompt_caching_title.md](21_prompt_caching_title.md) | Anthropic prompt caching system, automatic session title generation |
| 22 | [22_gateway_session_delivery.md](22_gateway_session_delivery.md) | Gateway runner, session management, delivery routing, platform adapter lifecycle |
| 23 | [23_terminal_environments.md](23_terminal_environments.md) | Execution sandbox backends (Local, Docker, SSH, Modal, Daytona, Singularity), file sync |
| 24 | [24_memory_skills_platforms.md](24_memory_skills_platforms.md) | File-backed memory (MEMORY.md/USER.md), skills progressive disclosure, platform adapter details |
| 25 | [25_cron_process_web.md](25_cron_process_web.md) | Cron job scheduler, background process registry, FastAPI web UI server |
| 26 | [26_error_classification_context.md](26_error_classification_context.md) | API error classification, smart model routing, Anthropic adapter, context references (@file/@folder/@git/@url) |
| 27 | [27_platform_adapters.md](27_platform_adapters.md) | Base adapter interface, shared helpers, Telegram, Discord, Slack adapters |
| 28 | [28_remaining_adapters.md](28_remaining_adapters.md) | WhatsApp, Signal, Matrix, BlueBubbles, Email, HomeAssistant, Webhook, SMS adapters |
| 29 | [29_gateway_internals.md](29_gateway_internals.md) | Gateway runner, startup lifecycle, message pipeline, session management, hooks, delivery |
| 30 | [30_remaining_platforms.md](30_remaining_platforms.md) | Feishu/Lark, QQBot, DingTalk, Mattermost, WeCom, Weixin adapters |
| 31 | [31_tts_voice_audio.md](31_tts_voice_audio.md) | TTS (6 providers), voice mode, audio capture/playback, Whisper hallucination filter |
| 32 | [32_code_execution_sandbox.md](32_code_execution_sandbox.md) | execute_code tool, UDS and file-based RPC, 7-tool sandbox, output truncation |
| 33 | [33_send_message_security_patch.md](33_send_message_security_patch.md) | send_message (18 platforms), Tirith scanner, V4A patch parser, fuzzy match engine |
| 34 | [34_rl_training_tool.md](34_rl_training_tool.md) | RL training tool: environment discovery, config management, 3-process training, WandB |
| 35 | [35_hermes_cli_subsystems.md](35_hermes_cli_subsystems.md) | Claw migration, curses UI, clipboard, auth commands, web server, setup, doctor, plugins, config |
| 36 | [36_environments_rl_infra.md](36_environments_rl_infra.md) | Base env, agent loop engine, ToolContext, OPD, web research, benchmarks, tool call parsers |
| 37 | [37_remaining_tools.md](37_remaining_tools.md) | Camofox browser, website policy, managed tool gateway, OSV malware check |
| 38 | [38_agent_subsystems.md](38_agent_subsystems.md) | Error classifier, insights, credential pool, context engine, auxiliary client, Copilot ACP |
| 39 | [39_memory_plugins.md](39_memory_plugins.md) | MemoryProvider ABC, 7 plugins: Holographic, Honcho, OpenViking, RetainDB, Supermemory, Hindsight, Byterover |
| 40 | [40_core_agent_runtime.md](40_core_agent_runtime.md) | AIAgent (11K lines), conversation loop, parallel tools, API modes, CLI TUI (10K lines) |
| 41 | [41_gateway_batch_infra.md](41_gateway_batch_infra.md) | Gateway service mgmt, batch runner, trajectory compressor, MCP serve, toolset distributions |
| 42 | [42_gateway_runner.md](42_gateway_runner.md) | GatewayRunner (9.6K lines), startup lifecycle, agent caching, message routing, shutdown/restart |
| 43 | [43_cli_auth_skin_runtime.md](43_cli_auth_skin_runtime.md) | CLI entry point, OAuth authentication (25+ providers), skin engine, runtime provider resolution |
| 44 | [44_gateway_platform_adapters.md](44_gateway_platform_adapters.md) | Base adapter ABC, Telegram, Discord, media cache, retry system, message pipeline |
| 45 | [45_cli_setup_models_tools_commands.md](45_cli_setup_models_tools_commands.md) | Setup wizard (6 sections, 17 platforms), model catalogs (25 providers), tools config (18 toolsets), command registry |
| 46 | [46_agent_subsystems.md](46_agent_subsystems.md) | Auxiliary client (5 backends), Anthropic adapter (OAuth, thinking, limits), credential pool (rotation, sync, exhaustion) |
| 47 | [47_remaining_agent_subsystems.md](47_remaining_agent_subsystems.md) | Context compressor, prompt builder (injection scanning), display, error classifier (13 reasons), insights engine, model metadata, usage pricing, Copilot ACP |
| 48 | [48_tools_skills_hub_browser_mcp_web.md](48_tools_skills_hub_browser_mcp_web.md) | Skills Hub (4 sources, GitHub auth), Browser tool (5 backends, 10 schemas), MCP client (stdio/HTTP, sampling), Web tools (4 backends, LLM summarization) |
| 49 | [49_tools_terminal_skills_codeexec_rl.md](49_tools_terminal_skills_codeexec_rl.md) | Terminal tool (6 backends, sudo, guards), Skills tool (progressive disclosure, secret capture), Code execution (UDS/file RPC, 7-tool sandbox), RL training (3-process pipeline, 18 locked fields) |

---

## Quick Lookup

### "Where is X documented?"

| Topic | Spec File | Section |
|-------|-----------|---------|
| Master architecture overview | 00 | §architecture |
| Repo structure | 00 | §repo-structure |
| High-level architecture diagram | 00 | §high-level-architecture |
| Core subsystems | 00 | §core-subsystems |
| Data flow | 00 | §data-flow |
| Credential model | 00 | §credential-model |
| Settings layers | 00 | §settings-layers |
| Model support | 00 | §model-support |
| File dependency chain | 00 | §file-dependency-chain |
| Main agent loop (`run_agent.py`) | 01 | §AIAgent |
| Conversation loop (two-level) | 15 | §conversation-loop |
| Per-turn initialization | 15 | §per-turn-initialization |
| Error classification and recovery | 15 | §error-classification |
| Fallback chain | 15 | §fallback-chain |
| Budget system | 15 | §budget-system |
| Context compression | 01 | §ContextCompressor |
| Message sanitization | 15 | §message-sanitization |
| API call execution | 15 | §api-call-execution |
| Session persistence | 15 | §session-persistence |
| Interrupt system | 15 | §interrupt-system |
| Plugin hooks | 15 | §plugin-hooks |
| Memory provider integration | 15 | §memory-provider-integration |
| Ollama context injection | 15 | §ollama-context-injection |
| Prompt caching | 15 | §prompt-caching |
| Model switching | 15 | §model-switching |
| Background review system | 15 | §background-review-system |
| Trajectory format | 15 | §trajectory-format |
| Rate limit tracking | 15 | §rate-limit-tracking |
| Activity monitoring | 15 | §activity-monitoring |
| Response normalization | 15 | §response-normalization |
| Thinking block handling | 15 | §thinking-block-handling |
| Stream consumer system | 15 | §stream-consumer-system |
| Quiet mode / spinner | 15 | §quiet-mode-spinner |
| Status emission | 15 | §status-emission |
| `run_conversation()` loop | 01 | §run_conversation |
| Iteration budget | 01 | §IterationBudget |
| Context compression | 01 | §ContextCompressor |
| CLI entry point (`hermes_cli.main`) | 01 | §hermes_cli.main |
| CLI TUI (`cli.py`) | 01 | §HermesCLI |
| CLI TUI components | 04 | §tui-architecture |
| prompt_toolkit framework | 04 | §prompt_toolkit-framework |
| Fixed input area layout | 04 | §fixed-input-area-layout |
| SlashCommandCompleter | 04 | §slashcommandcompleter |
| Skin Engine (TUI) | 04 | §skin-engine |
| Banner & version display | 04 | §banner--version-display |
| Spinner & status display | 04 | §spinner--status-display |
| Key bindings | 04 | §key-bindings |
| Cursor control | 04 | §cursor-control |
| Output handling | 04 | §output-handling |
| Callbacks system | 04 | §callbacks-system |
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
| Delegate tool (subagent) | 17 | §delegate-tool |
| Depth limit 2 | 17 | §depth-limit |
| Credential pool sharing | 17 | §credential-pool-sharing |
| Heartbeat | 17 | §heartbeat |
| Thread-scoped interrupts | 17 | §thread-scoped-interrupts |
| Env passthrough registry | 17 | §env-passthrough |
| Browser tool (5 backends) | 17 | §browser-tool |
| Provider architecture | 17 | §provider-architecture |
| MCP client (stdio/HTTP) | 17 | §mcp-client |
| MCP sampling | 17 | §mcp-sampling |
| Dynamic discovery | 17 | §dynamic-discovery |
| Dangerous command approval | 18 | §approval-system |
| 35 danger patterns | 18 | §danger-patterns |
| Smart LLM approval | 18 | §llm-approval |
| YOLO mode | 18 | §yolo-mode |
| Gateway blocking approval | 18 | §gateway-blocking |
| File tools (read/write/search/patch) | 18 | §file-tools |
| Security guards | 18 | §security-guards |
| Staleness detection | 18 | §staleness-detection |
| Web tools (search/extract) | 18 | §web-tools |
| URL safety | 18 | §url-safety |
| Credential pool (PooledCredential) | 19 | §pooledcredential |
| Selection strategies | 19 | §selection-strategies |
| Exhaustion / rotation | 19 | §exhaustion-rotation |
| OAuth refresh | 19 | §oauth-refresh |
| Lease system | 19 | §lease-system |
| Persistence | 19 | §persistence |
| Custom providers | 19 | §custom-providers |
| Rate limit tracker | 19 | §rate-limit-tracker |
| 12 rate limit headers | 19 | §12-headers |
| RateLimitBucket / State | 19 | §ratelimitbucket-state |
| Display formatting | 19 | §display-formatting |
| Secret redaction | 20 | §secret-redaction |
| 35+ token prefix patterns | 20 | §token-prefix-patterns |
| Masking strategy | 20 | §masking-strategy |
| RedactingFormatter | 20 | §redactingformatter |
| Context compression algorithm | 20 | §compression-algorithm |
| Summary constants | 20 | §summary-constants |
| Tool result summarization | 20 | §tool-result-summarization |
| Pruning | 20 | §pruning |
| Iterative updates | 20 | §iterative-updates |
| Anthropic prompt caching | 21 | §prompt-caching |
| system_and_3 strategy | 21 | §system-and-3-strategy |
| 4 breakpoints max | 21 | §4-breakpoints |
| Cache TTL | 21 | §cache-ttl |
| Session title generation | 21 | §session-title |
| Auto-title from first exchange | 21 | §auto-title |
| Daemon thread | 21 | §daemon-thread |
| Cleanup | 21 | §cleanup |
| Gateway runner | 22 | §gateway-runner |
| Startup sequence | 22 | §startup-sequence |
| Config bridge | 22 | §config-bridge |
| Agent caching | 22 | §agent-caching |
| Interrupt handling | 22 | §interrupt-handling |
| Memory flush | 22 | §memory-flush |
| Session management | 22 | §session-management |
| SessionSource | 22 | §sessionsource |
| SessionContext | 22 | §sessioncontext |
| PII redaction | 22 | §pii-redaction |
| Delivery router | 22 | §delivery-router |
| Local / platform delivery | 22 | §local-platform-delivery |
| Platform adapter lifecycle | 22 | §adapter-lifecycle |
| States / runtime status | 22 | §states-runtime-status |
| Fatal errors | 22 | §fatal-errors |
| Terminal environments (7 backends) | 23 | §execution-backends |
| Local / Docker / SSH / Modal / Daytona / Singularity / Managed Modal | 23 | §7-backends |
| ProcessHandle protocol | 23 | §processhandle-protocol |
| Session snapshots | 23 | §session-snapshots |
| CWD persistence | 23 | §cwd-persistence |
| FileSyncManager | 23 | §filesyncmanager |
| Credential file mounts | 23 | §credential-file-mounts |
| Memory system (frozen snapshot) | 24 | §memory-system |
| MemoryStore | 24 | §memorystore |
| Atomic writes | 24 | §atomic-writes |
| Threat scanning | 24 | §threat-scanning |
| Skills progressive disclosure | 24 | §progressive-disclosure |
| SKILL.md format | 24 | §skillmd-format |
| Platform matching | 24 | §platform-matching |
| Readiness status | 24 | §readiness-status |
| Plugin skills | 24 | §plugin-skills |
| Platform adapters (17+ supported) | 24 | §platform-adapters |
| Cron scheduler | 25 | §cron-scheduler |
| 3 schedule types | 25 | §3-schedule-types |
| Tick loop | 25 | §tick-loop |
| Delivery targets | 25 | §delivery-targets |
| Media detection | 25 | §media-detection |
| Silent marker | 25 | §silent-marker |
| Process registry | 25 | §process-registry |
| ProcessSession | 25 | §processsession |
| Spawn modes | 25 | §spawn-modes |
| Watch patterns | 25 | §watch-patterns |
| Crash recovery | 25 | §crash-recovery |
| LRU pruning | 25 | §lru-pruning |
| Web server (FastAPI) | 25 | §web-server |
| Auth middleware | 25 | §auth-middleware |
| CORS | 25 | §cors |
| Endpoints | 25 | §endpoints |
| Error classification (14 FailoverReasons) | 26 | §error-classification |
| Pattern sets | 26 | §pattern-sets |
| Classification pipeline | 26 | §classification-pipeline |
| Smart model routing | 26 | §smart-model-routing |
| Complexity detection | 26 | §complexity-detection |
| Routing resolution | 26 | §routing-resolution |
| Anthropic adapter | 26 | §anthropic-adapter |
| Auth modes | 26 | §auth-modes |
| Beta headers | 26 | §beta-headers |
| Claude Code identity | 26 | §claude-code-identity |
| Max output limits | 26 | §max-output-limits |
| Thinking support | 26 | §thinking-support |
| OAuth refresh | 26 | §oauth-refresh-26 |
| Context references | 26 | §context-references |
| @file/@folder/@git/@url/@diff/@staged | 26 | §context-refs |
| Subdirectory hints | 26 | §subdirectory-hints |
| AGENTS.md / CLAUDE.md discovery | 26 | §agents-claude-discovery |
| Retry / jitter | 26 | §retry-jitter |
| Exponential backoff | 26 | §exponential-backoff |
| Insights engine | 26 | §insights-engine |
| Session mirror | 26 | §session-mirror |
| Hook system | 26 | §hook-system |
| Base adapter interface | 27 | §base-adapter |
| MessageEvent | 27 | §messageevent |
| SendResult | 27 | §sendresult |
| Shared helpers | 27 | §shared-helpers |
| MessageDeduplicator | 27 | §messagededuplicator |
| TextBatchAggregator | 27 | §textbatchaggregator |
| strip_markdown | 27 | §strip-markdown |
| ThreadParticipationTracker | 27 | §threadparticipationtracker |
| Telegram adapter (aiogram) | 27 | §telegram-adapter |
| Sticker cache | 27 | §sticker-cache |
| Text batch aggregation | 27 | §text-batch-aggregation |
| Discord adapter (discord.py) | 27 | §discord-adapter |
| GIF animation | 27 | §gif-animation |
| Reactions | 27 | §reactions |
| Slack adapter (slack-bolt) | 27 | §slack-adapter |
| Block Kit | 27 | §block-kit |
| Multi-workspace | 27 | §multi-workspace |
| Media cache system | 27 | §media-cache |
| Proxy support | 27 | §proxy-support |
| WhatsApp adapter | 28 | §whatsapp-28 |
| Node.js bridge | 28 | §nodejs-bridge |
| Polling | 28 | §polling |
| Signal adapter | 28 | §signal-adapter |
| signal-cli SSE+JSON-RPC | 28 | §signal-cli |
| Matrix adapter | 28 | §matrix-adapter |
| mautrix SDK | 28 | §mautrix-sdk |
| E2EE | 28 | §e2ee |
| megolm buffer | 28 | §megolm-buffer |
| BlueBubbles adapter | 28 | §bluebubbles-adapter |
| Webhook+REST | 28 | §webhook-rest |
| GUID resolution | 28 | §guid-resolution |
| Email adapter | 28 | §email-adapter |
| IMAP/SMTP | 28 | §imap-smtp |
| HomeAssistant adapter | 28 | §homeassistant-adapter |
| WebSocket events | 28 | §websocket-events |
| Webhook adapter | 28 | §webhook-adapter |
| HMAC validation | 28 | §hmac-validation |
| Prompt rendering | 28 | §prompt-rendering |
| SMS/Twilio adapter | 28 | §sms-twilio |
| Signature validation | 28 | §signature-validation |
| Chunked sending | 28 | §chunked-sending |
| Adapter comparison matrix | 28 | §comparison-matrix |
| GatewayRunner architecture | 29 | §gatewayrunner |
| Core state | 29 | §core-state |
| Pending sentinel | 29 | §pending-sentinel |
| AIAgent caching | 29 | §aiagent-caching |
| Startup lifecycle | 29 | §startup-lifecycle-29 |
| Import order | 29 | §import-order |
| SSL cert detection | 29 | §ssl-cert-detection |
| Adapter sequence | 29 | §adapter-sequence |
| Session suspension | 29 | §session-suspension |
| Stuck-loop detection | 29 | §stuck-loop-detection |
| Message processing pipeline | 29 | §message-processing |
| Authorization | 29 | §authorization |
| Running agent intercept | 29 | §running-agent-intercept |
| Command dispatch | 29 | §command-dispatch |
| Agent execution | 29 | §agent-execution |
| Event hook system | 29 | §event-hook-system |
| Delivery router | 29 | §delivery-router-29 |
| ContextVars | 29 | §contextvars |
| PID file & scoped locks | 29 | §pid-file-locks |
| Shutdown & restart | 29 | §shutdown-restart |
| Background tasks | 29 | §background-tasks |
| Config loading | 29 | §config-loading |
| Session mirroring | 29 | §session-mirroring |
| BOOT.md hook | 29 | §bootmd-hook |
| Feishu/Lark adapter | 30 | §feishu |
| QQBot adapter | 30 | §qqbot |
| DingTalk adapter | 30 | §dingtalk |
| Mattermost adapter | 30 | §mattermost |
| WeCom adapter | 30 | §wecom |
| Weixin adapter | 30 | §weixin |
| TTS providers | 31 | §per-provider |
| Edge TTS | 31 | §edge |
| ElevenLabs TTS | 31 | §elevenlabs |
| OpenAI TTS | 31 | §openai |
| MiniMax TTS | 31 | §minimax |
| Mistral Voxtral | 31 | §mistral |
| NeuTTS | 31 | §neutts |
| Streaming TTS | 31 | §streaming-tts-pipeline |
| Voice mode | 31 | §voice-mode-cli |
| AudioRecorder | 31 | §audiorecorder |
| TermuxAudioRecorder | 31 | §termuxaudiorecorder |
| Whisper hallucinations | 31 | §whisper-hallucination-filter |
| Audio playback | 31 | §audio-playback |
| execute_code | 32 | §main-entry |
| UDS transport | 32 | §local-backend-uds |
| File-based RPC | 32 | §remote-backend-file-based |
| hermes_tools.py generator | 32 | §hermes_toolspy-module-generator |
| Sandbox security | 32 | §child-process-security |
| Output truncation | 32 | §output-handling |
| send_message tool | 33 | §send_message-tool |
| Image generation (FLUX 2 Pro) | 03 | §image-generation-tool |
| FAL.ai integration | 03 | §model-upscaler |
| Clarity upscaler (2x) | 03 | §upscaler-configuration |
| Managed FAL gateway | 03 | §managed-gateway |
| Tirith scanner | 33 | §tirith-security-scanner |
| V4A patch parser | 33 | §v4a-patch-parser |
| Fuzzy match engine | 33 | §fuzzy-match-engine |
| RL training tool | 34 | §training-run-lifecycle |
| Claw migration | 35 | §claw-migration |
| Curses UI | 35 | §curses-ui |
| Clipboard | 35 | §clipboard-image-extraction |
| Auth commands | 35 | §auth-commands |
| Web server | 35 | §web-server |
| Base environment | 36 | §base-environment-architecture |
| Agent loop engine | 36 | §agent-loop-engine |
| ToolContext | 36 | §toolcontext |
| Agentic OPD | 36 | §agentic-opd-environment |
| Web research env | 36 | §web-research-environment |
| Benchmark envs | 36 | §benchmark-environments |
| Tool call parsers | 36 | §tool-call-parsers |
| Camofox browser | 37 | §camofox-browser-backend |
| Website policy | 37 | §website-policy |
| Managed tool gateway | 37 | §managed-tool-gateway |
| OSV malware check | 37 | §osv-malware-check |
| Error classifier | 38 | §error-classifier |
| Insights engine | 38 | §insights-engine |
| Credential pool | 38 | §credential-pool |
| Context engine | 38 | §context-engine--compressor |
| Auxiliary client | 38 | §auxiliary-client |
| Copilot ACP | 38 | §copilot-acp-client |
| Usage pricing | 38 | §usage-pricing |
| Memory plugins | 39 | §memoryprovider-abc |
| Holographic memory | 39 | §holographic-memory |
| Honcho memory | 39 | §honcho-memory |
| AIAgent class | 40 | §aiagent-class |
| Conversation loop | 40 | §conversation-loop |
| Parallel tool exec | 40 | §parallel-tool-execution |
| API mode detection | 40 | §api-mode-detection |
| CLI TUI | 40 | §cli-tui-cli-py |
| Model tools dispatch | 40 | §model-tools-dispatch |
| Gateway service mgmt | 41 | §gateway-service-management |
| Batch runner | 41 | §batch-runner |
| Trajectory compressor | 41 | §trajectory-compressor |
| MCP serve | 41 | §mcp-serve |
| Toolset distributions | 41 | §toolset-distributions |
| GatewayRunner | 42 | §gatewayrunner-architecture |
| Startup lifecycle | 42 | §startup-lifecycle |
| Agent factory cache | 42 | §agent-factory--caching |
| Message routing | 42 | §message-routing |
| Shutdown & restart | 42 | §shutdown--restart |
| CLI entry point | 43 | §cli-entry-point |
| Profile override system | 43 | §profile-override |
| TTY guard | 43 | §tty-guard |
| Command routing | 43 | §command-routing |
| Session browser | 43 | §session-browser |
| Auth system (25+ providers) | 43 | §auth-system |
| 4 auth types | 43 | §4-auth-types |
| OAuth device code flow | 43 | §oauth-device-code |
| Auth store | 43 | §auth-store |
| Credential pool | 43 | §credential-pool-43 |
| Endpoint probing | 43 | §endpoint-probing |
| Secret validation | 43 | §secret-validation |
| Skin engine (YAML skins) | 43 | §skin-engine-43 |
| 10 built-in skins | 43 | §10-built-in-skins |
| Inheritance | 43 | §inheritance |
| prompt_toolkit integration | 43 | §prompt_toolkit-integration |
| Runtime provider resolution | 43 | §runtime-provider-resolution |
| Resolution pipeline | 43 | §resolution-pipeline |
| API mode detection | 43 | §api-mode-detection-43 |
| Custom providers | 43 | §custom-providers-43 |
| Nous credential resolution | 43 | §nous-credential-resolution |
| Base platform adapter | 44 | §base-platform-adapter |
| MessageEvent | 44 | §messageevent-44 |
| SendResult | 44 | §sendresult-44 |
| Message handling pipeline | 44 | §message-handling |
| Background processing | 44 | §background-processing |
| Retry system | 44 | §retry-system |
| Typing indicator | 44 | §typing-indicator |
| Message truncation | 44 | §message-truncation |
| UTF-16 length | 44 | §utf16-length |
| Media cache | 44 | §media-cache-44 |
| Media extraction | 44 | §media-extraction |
| Proxy support | 44 | §proxy-support-44 |
| Network accessibility | 44 | §network-accessibility |
| Session state management | 44 | §session-state |
| Fatal error tracking | 44 | §fatal-error-tracking |
| Platform locking | 44 | §platform-locking |
| Human-like pacing | 44 | §human-like-pacing |
| Command bypass | 44 | §command-bypass |
| Telegram adapter | 44 | §telegram-adapter-44 |
| MarkdownV2 handling | 44 | §markdownv2 |
| Media batch handling | 44 | §media-batch |
| Reply mode | 44 | §reply-mode |
| Network fallback | 44 | §network-fallback |
| Forum topics | 44 | §forum-topics |
| Discord adapter | 44 | §discord-adapter-44 |
| Voice receiver | 44 | §voice-receiver |
| Thread management | 44 | §thread-management |
| Message deduplication | 44 | §message-deduplication |
| Thread participation tracking | 44 | §thread-participation |
| Discord ID cleaning | 44 | §discord-id-cleaning |
| Shared helpers | 44 | §shared-helpers-44 |
| Other adapters | 44 | §other-adapters |
| Setup wizard | 45 | §setup-wizard |
| Setup sections | 45 | §wizard-sections |
| Model catalogs | 45 | §model-catalogs |
| Provider registry | 45 | §canonical-provider-registry |
| Tools config | 45 | §tools-configuration |
| Command registry | 45 | §command-registry |
| Telegram sanitization | 45 | §telegram-command-sanitization |
| Auxiliary client | 46 | §auxiliary-client |
| Codex adapter | 46 | §codex-responses-adapter |
| Anthropic adapter | 46 | §anthropic-adapter |
| Credential pool | 46 | §credential-pool |
| OAuth token sync | 46 | §oauth-token-sync |
| Context compressor | 47 | §context-compressor |
| Prompt builder | 47 | §prompt-builder |
| Injection scanning | 47 | §prompt-builder |
| Display module | 47 | §display |
| Error classifier | 47 | §error-classifier |
| Insights engine | 47 | §insights-engine |
| Model metadata | 47 | §model-metadata |
| Usage pricing | 47 | §usage-pricing |
| Copilot ACP | 47 | §copilot-acp |
| Skills Hub | 48 | §skills-hub |
| GitHubSource | 48 | §githubsource |
| WellKnownSkillSource | 48 | §wellknownskillsource |
| SkillsShSource | 48 | §skillsssource |
| Browser tool | 48 | §browser-tool |
| Browser backends | 48 | §backend-modes |
| Session management | 48 | §session-management |
| SSRF protection | 48 | §ssrf-protection |
| MCP client | 48 | §mcp-client |
| MCP sampling | 48 | §sampling-support |
| Dynamic tool discovery | 48 | §dynamic-tool-discovery |
| Web tools | 48 | §web-tools |
| Web backends | 48 | §backend-selection |
| LLM summarization | 48 | §llm-content-summarization |
| Terminal tool | 49 | §terminal-tool |
| Terminal backends | 49 | §environment-selection |
| Sudo handling | 49 | §sudo-password-handling |
| Command guards | 49 | §command-guard-system |
| Skills tool | 49 | §skills-tool |
| Progressive disclosure | 49 | §progressive-disclosure |
| Secret capture | 49 | §secret-capture |
| Code execution | 49 | §code-execution-tool |
| UDS transport | 49 | §uds-transport |
| File-based RPC | 49 | §file-based-rpc-transport |
| Sandbox tools | 49 | §allowed-tools-7 |
| RL training | 49 | §rl-training-tool |
| Training pipeline | 49 | §3-process-training-pipeline |
| Locked config | 49 | §locked-configuration |

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
| Number of spec files | 51 (00-49, non-contiguous 15-26 exist as separate docs) |
| Optional extras | 20+ |
| Spec documentation size | ~1,500+ KB |

---

## Architecture in One Paragraph

Hermes Agent is a self-improving AI agent built by Nous Research. It runs as either a local CLI TUI (built on prompt_toolkit) or a persistent gateway process that connects to Telegram, Discord, Slack, WhatsApp, Signal, and 12+ other messaging platforms. The core agent loop (`run_agent.py` + `model_tools.py`) uses the OpenAI-compatible API protocol to call any LLM (Anthropic, OpenRouter, Nous Portal, OpenAI, or 15+ other providers) with tool calling. It has a decentralized tool registry with 40+ built-in tools (file I/O, terminal, web, browser, MCP, delegation), a skill system for procedural memory, an SQLite state store with FTS5 search, context compression, six terminal backends (local, Docker, SSH, Modal, Daytona, Singularity), a built-in cron scheduler, and a research pipeline for RL training with Atropos environments and trajectory generation.

---

*Generated 2026-04-15 from Hermes Agent source analysis.*
