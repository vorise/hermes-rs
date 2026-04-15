# Hermes Agent — Utilities, Memory & Skills

This document covers the memory system, skills system, Skills Hub, skills sync, and honcho integration.

---

## Table of Contents

1. [Memory System Overview](#1-memory-system-overview)
2. [Memory Tool](#2-memory-tool)
3. [Memory Manager](#3-memory-manager)
4. [Memory Provider](#4-memory-provider)
5. [Skills System](#5-skills-system)
6. [Skills Tool](#6-skills-tool)
7. [Skills Hub](#7-skills-hub)
8. [Skills Config](#8-skills-config)
9. [Skills Sync](#9-skills-sync)
10. [Skills Guard](#10-skills-guard)
11. [Skill Manager Tool](#11-skill-manager-tool)
12. [Skill Commands](#12-skill-commands)
13. [Skill Utilities](#13-skill-utilities)
14. [Honcho Integration](#14-honcho-integration)

---

## 1. Memory System Overview

### Architecture

Hermes uses a layered memory system:

```
┌─────────────────────────────────────────────────────────────┐
│                    SHORT-TERM MEMORY                          │
│  SQLite Session Store — Current conversation messages        │
└─────────────────────────────────────────────────────────────┘
                             │
┌─────────────────────────────────────────────────────────────┐
│                    MEDIUM-TERM MEMORY                         │
│  Session Search (FTS5) — Past conversation search            │
└─────────────────────────────────────────────────────────────┘
                             │
┌─────────────────────────────────────────────────────────────┐
│                    LONG-TERM MEMORY                           │
│  Memory files (markdown) — Consolidated memories             │
│  Honcho — Dialectic user modeling                            │
└─────────────────────────────────────────────────────────────┘
```

### Memory Directory

Memories are stored in `~/.hermes/memory/` as markdown files. The structure follows the same pattern as Claude Code's memory system.

### Memory Flow

1. **Write** — Agent creates memories during conversation
2. **Consolidate** — Periodic consolidation merges related memories
3. **Retrieve** — Relevant memories retrieved at start of each turn
4. **Inject** — Retrieved memories injected into system prompt
5. **Nudge** — Periodic nudges remind agent to use/curate memory

---

## 2. Memory Tool

### Location

`tools/memory_tool.py` (~23K lines)

### Purpose

Provide the agent with memory CRUD operations.

### Tools Provided

| Tool | Purpose |
|------|---------|
| `memory` | Create/update/delete/query memories |

### Operations

| Operation | Description |
|-----------|-------------|
| `create` | Create a new memory entry |
| `update` | Update an existing memory |
| `delete` | Delete a memory entry |
| `list` | List all memories |
| `search` | Search memories by content |

---

## 3. Memory Manager

### Location

`agent/memory_manager.py` (~14K lines)

### Purpose

High-level memory management with honcho integration.

### Key Methods

| Method | Purpose |
|--------|---------|
| `prefetch_all()` | Fetch all relevant memories for a query |
| `save_memory()` | Save a new memory |
| `get_memories()` | Retrieve memories |
| `clear_memories()` | Clear all memories |

### Memory Nudge

The agent is periodically nudged to:
- Review and consolidate memories
- Create new memories from significant turns
- Update stale memories

### Nudge Configuration

| Setting | Default | Purpose |
|---------|---------|---------|
| `memory_nudge_interval` | Every N turns | How often to nudge for memory review |
| `skill_nudge_interval` | Every N tool iterations | How often to nudge for skill creation |

---

## 4. Memory Provider

### Location

`agent/memory_provider.py` (~10K lines)

### Purpose

Abstract interface for memory backends.

### Interface

| Method | Purpose |
|--------|---------|
| `query()` | Query memories |
| `save()` | Save memory |
| `delete()` | Delete memory |
| `clear()` | Clear all memories |

---

## 5. Skills System

### Overview

Skills are procedural memory units that extend the agent's capabilities. They are:
- Created autonomously by the agent after completing complex tasks
- Self-improved during use
- Searchable and installable from agentskills.io
- Compatible with the agentskills.io open standard

### Skill Lifecycle

1. **Creation** — Agent creates skill after completing novel/complex task
2. **Storage** — Skill saved as markdown in `~/.hermes/skills/`
3. **Registration** — Skill registered in skill registry
4. **Execution** — Skill available via `/skill-name` or `<skill-name>` slash command
5. **Improvement** — Skill self-improves during use (updates its own instructions)
6. **Sharing** — Skill can be published to agentskills.io

### Skill Structure

Each skill is a markdown file with:
- Name and description
- Instructions (system prompt injection)
- Tool requirements
- Version information

---

## 6. Skills Tool

### Location

`tools/skills_tool.py` (~51K lines)

### Purpose

Execute installed skills.

### Key Features

- Skill execution with arguments
- Skill system prompt injection
- Skill version support
- Per-platform enable/disable

### Tools Provided

| Tool | Purpose |
|------|---------|
| `skill_view` | View skill details |
| `skills_list` | List installed skills |
| `skill_manage` | Manage skills (create/update/delete) |

---

## 7. Skills Hub

### Location

`tools/skills_hub.py` (~112K lines) and `hermes_cli/skills_hub.py` (~47K lines)

### Purpose

Search, browse, and install skills from the agentskills.io registry.

### Key Features

- GitHub API integration for skill registry
- Skill search and filtering
- Installation management
- Version checking
- Update support
- Skill preview before install

### Integration

- GitHub App JWT authentication for bot identity
- Skill repository browsing
- Community skill installation

---

## 8. Skills Config

### Location

`hermes_cli/skills_config.py` (~7K lines)

### Purpose

Enable/disable skills per platform.

### Key Features

- Per-platform skill enable/disable
- Skill configuration in config.yaml
- Skill dependency checking

---

## 9. Skills Sync

### Location

`tools/skills_sync.py` (~11K lines)

### Purpose

Synchronize installed skills with the agentskills.io registry.

### Key Features

- Automatic skill updates
- Version checking
- Conflict resolution
- Background sync

---

## 10. Skills Guard

### Location

`tools/skills_guard.py` (~37K lines)

### Purpose

Safety checks for skill execution.

### Key Features

- Skill validation before execution
- Dangerous skill detection
- Skill sandbox enforcement
- Tool access control

---

## 11. Skill Manager Tool

### Location

`tools/skill_manager_tool.py` (~28K lines)

### Purpose

Manage skills from within the agent conversation.

### Key Features

- Skill creation from conversation context
- Skill update during use
- Skill deletion
- Skill listing

---

## 12. Skill Commands

### Location

`agent/skill_commands.py` (~14K lines)

### Purpose

Shared skill slash commands for CLI and gateway.

### Commands

| Command | Purpose |
|---------|---------|
| `/skills` | Browse/search skills |
| `/<skill-name>` | Execute a skill |

---

## 13. Skill Utilities

### Location

`agent/skill_utils.py` (~16K lines)

### Purpose

Skill utility functions for creation, loading, and validation.

### Key Functions

| Function | Purpose |
|----------|---------|
| `load_skill()` | Load skill from file |
| `save_skill()` | Save skill to file |
| `validate_skill()` | Validate skill format |
| `render_skill()` | Render skill for prompt injection |

---

## 14. Honcho Integration

### Purpose

Hermes integrates with the Honcho dialectic user modeling system for deep user understanding.

### How It Works

1. **User modeling** — Honcho builds a model of the user over time
2. **Dialectic approach** — Honcho asks clarifying questions to refine understanding
3. **Memory integration** — Honcho insights are integrated into Hermes's memory system
4. **Cross-session recall** — User model persists across sessions

### Configuration

```yaml
memory:
  honcho:
    enabled: true
    # Honcho API configuration
```

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Memory files | Markdown in ~/.hermes/memory/ |
| Skill files | Markdown in ~/.hermes/skills/ |
| Memory operations | 5 (create, update, delete, list, search) |
| Skill tools | 3 (view, list, manage) |
| Skills guard checks | Multiple (validation, safety, sandbox) |
| Nudge types | 2 (memory, skill) |

---

*Generated from source analysis of the Hermes Agent codebase.*
