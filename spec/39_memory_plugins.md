# Hermes Agent — Memory Plugins

This document covers the plugins/memory/ subsystem: the MemoryProvider ABC, discovery system, and all 7 memory backend plugins (Holographic, Honcho, OpenViking, RetainDB, Supermemory, Hindsight, Byterover).

---

## Table of Contents

1. [MemoryProvider ABC](#1-memoryprovider-abc)
2. [Plugin Discovery](#2-plugin-discovery)
3. [Holographic Memory](#3-holographic-memory)
4. [Honcho Memory](#4-honcho-memory)
5. [Other Memory Plugins](#5-other-memory-plugins)

---

## 1. MemoryProvider ABC

### Location

`plugins/memory/__init__.py` (discovery) + interface defined per-plugin

### Purpose

Standard interface that all memory backends implement. Only ONE can be active at a time, selected via `memory.provider` in `config.yaml`.

### 1.1 Interface

Memory plugins are separate from the general plugin system. They live in the repo and are always available without user installation. Each plugin directory must contain `__init__.py` with a class implementing the MemoryProvider protocol.

### 1.2 Selection

```yaml
# ~/.hermes/config.yaml
memory:
  provider: "holographic"  # or "honcho", "openviking", etc.
```

### 1.3 Configuration Resolution

Config chain (profile-scoped):
1. `$HERMES_HOME/config.yaml -> plugins:<plugin-name>:`
2. Plugin-specific config files (e.g., `$HERMES_HOME/honcho.json`)
3. Environment variables

---

## 2. Plugin Discovery

### Location

`plugins/memory/__init__.py` (~317 lines)

### Purpose

Scans `plugins/memory/<name>/` directories for memory provider plugins.

### 2.1 Discovery

```python
def discover_memory_providers() -> List[Tuple[str, str, bool]]:
    """Scan plugins/memory/ for available providers.

    Returns list of (name, description, is_available) tuples.
    Does NOT import providers — reads plugin.yaml for metadata
    and does lightweight availability check.
    """
```

### 2.2 Metadata Reading

Reads `plugin.yaml` for description:
```python
yaml_file = child / "plugin.yaml"
if yaml_file.exists():
    meta = yaml.safe_load(f) or {}
    desc = meta.get("description", "")
```

### 2.3 Availability Check

```python
provider = _load_provider_from_dir(child)
if provider:
    available = provider.is_available()
```

Attempts import and calls `is_available()` to check dependencies and connectivity.

### 2.4 Provider Loading

```python
def load_memory_provider(name: str) -> Optional["MemoryProvider"]:
    """Load and return a MemoryProvider instance by name."""
```

Dynamically imports the plugin module and extracts the MemoryProvider instance.

---

## 3. Holographic Memory

### Location

`plugins/memory/holographic/` (~1,400 lines total)
- `__init__.py` (~407 lines) — Plugin entry point + tool schemas
- `store.py` (~574 lines) — SQLite fact store with entity resolution
- `retrieval.py` (~593 lines) — Hybrid keyword/BM25/HRR retrieval
- `holographic.py` (~203 lines) — HRR (Holographic Reduced Representation) math

### 3.1 Architecture

```
hermes-memory-store — structured fact storage with:
  - Entity resolution (named entity extraction)
  - Trust scoring (0.0-1.0)
  - HRR-based compositional retrieval
  - Temporal decay (configurable)
```

**Original plugin by**: dusterbloom (PR #2351), adapted to MemoryProvider ABC.

### 3.2 Tool Interface (2 tools)

**fact_store** — 9 actions:
| Action | Purpose |
|--------|---------|
| `add` | Store a fact the user expects you to remember |
| `search` | Keyword lookup |
| `probe` | Entity recall: ALL facts about a person/thing |
| `related` | Structural adjacency: what connects to an entity? |
| `reason` | Compositional: facts connected to MULTIPLE entities |
| `contradict` | Memory hygiene: find conflicting claims |
| `update` | Modify fact trust score or content |
| `remove` | Delete a fact |
| `list` | List all facts |

**fact_feedback** — Rate facts after use:
| Action | Purpose |
|--------|---------|
| `helpful` | Mark fact as accurate (trust +0.05) |
| `unhelpful` | Mark fact as outdated (trust -0.10) |

### 3.3 Database Schema

```sql
CREATE TABLE facts (
    fact_id         INTEGER PRIMARY KEY AUTOINCREMENT,
    content         TEXT NOT NULL UNIQUE,
    category        TEXT DEFAULT 'general',
    tags            TEXT DEFAULT '',
    trust_score     REAL DEFAULT 0.5,
    retrieval_count INTEGER DEFAULT 0,
    helpful_count   INTEGER DEFAULT 0,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    hrr_vector      BLOB
);

CREATE TABLE entities (
    entity_id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    entity_type TEXT DEFAULT 'unknown',
    aliases     TEXT DEFAULT ''
);

CREATE TABLE fact_entities (
    fact_id   INTEGER REFERENCES facts(fact_id),
    entity_id INTEGER REFERENCES entities(entity_id),
    PRIMARY KEY (fact_id, entity_id)
);

CREATE VIRTUAL TABLE facts_fts USING fts5(content, tags, content=facts);
```

**Triggers**: Automatic FTS5 index updates on INSERT/UPDATE/DELETE.

### 3.4 Retrieval Pipeline

**Hybrid search** (3-stage):

1. **FTS5 search**: Get `limit * 3` candidates from SQLite full-text search
2. **Jaccard rerank**: Token overlap between query and fact content
3. **Trust weighting**: `final_score = relevance * trust_score`
4. **Temporal decay** (optional): `decay = 0.5^(age_days / half_life)`

### 3.5 Scoring Weights

| Component | Weight | Purpose |
|-----------|--------|---------|
| FTS5 | 0.4 | Full-text search relevance |
| Jaccard | 0.3 | Token overlap similarity |
| HRR | 0.3 | Compositional vector similarity |

**Fallback**: If numpy unavailable, HRR weight redistributes: FTS=0.6, Jaccard=0.4, HRR=0.0.

### 3.6 Trust System

| Action | Delta |
|--------|-------|
| Mark helpful | +0.05 |
| Mark unhelpful | -0.10 |

Default trust: 0.5. Min trust threshold: 0.3.

### 3.7 Configuration

```yaml
plugins:
  hermes-memory-store:
    db_path: $HERMES_HOME/memory_store.db
    auto_extract: false
    default_trust: 0.5
    min_trust_threshold: 0.3
    temporal_decay_half_life: 0  # days, 0 = disabled
```

### 3.8 HRR (Holographic Reduced Representation)

Mathematical framework for compositional vector representations. Enables:
- Entity binding (associating facts with named entities)
- Compositional queries (facts connected to multiple entities)
- Approximate similarity search in vector space

**Dimension**: 1024 (configurable). Requires numpy for full functionality.

### 3.9 Memory Banks

```sql
CREATE TABLE memory_banks (
    bank_id    INTEGER PRIMARY KEY AUTOINCREMENT,
    bank_name  TEXT NOT NULL UNIQUE,
    vector     BLOB NOT NULL,
    dim        INTEGER NOT NULL,
    fact_count INTEGER DEFAULT 0,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

Separate memory banks for different categories or contexts.

---

## 4. Honcho Memory

### Location

`plugins/memory/honcho/` (~1,648 lines total)
- `__init__.py` (~722 lines) — Plugin entry point + tool schemas
- `client.py` (~565 lines) — Honcho API client
- `session.py` (~1,083 lines) — Session management
- `cli.py` (~1,305 lines) — CLI commands

### 4.1 Purpose

AI-native cross-session user modeling with:
- Dialectic Q&A
- Semantic search
- Peer cards
- Persistent conclusions

### 4.2 Tool Interface (4 tools)

| Tool | Purpose |
|------|---------|
| `honcho_profile` | Retrieve user's peer card — factual snapshot (name, role, preferences, patterns) |
| `honcho_search` | Semantic search over stored context. Returns raw excerpts ranked by relevance |
| `honcho_context` | Full context retrieval with LLM synthesis |
| `honcho_conclude` | Save conclusions from current session |

### 4.3 Config Chain

| Priority | Location |
|----------|----------|
| 1 | `$HERMES_HOME/honcho.json` (profile-scoped) |
| 2 | `~/.honcho/config.json` (legacy global) |
| 3 | Environment variables |

### 4.4 Architecture

```
Hermes -> Honcho SDK -> Honcho API
       -> session tracking
       -> user modeling
       -> semantic memory
```

---

## 5. Other Memory Plugins

### 5.1 OpenViking

**Location**: `plugins/memory/openviking/__init__.py` (~637 lines)

**Purpose**: Memory backend using OpenViking service. Provides vector-based semantic memory with REST API access.

### 5.2 RetainDB

**Location**: `plugins/memory/retaindb/__init__.py` (~766 lines)

**Purpose**: Persistent memory database with structured storage. Focuses on long-term fact retention with configurable TTL.

### 5.3 Supermemory

**Location**: `plugins/memory/supermemory/__init__.py` (~791 lines)

**Purpose**: Memory backend using Supermemory API. Provides AI-powered memory with automatic organization and retrieval.

### 5.4 Hindsight

**Location**: `plugins/memory/hindsight/__init__.py` (~883 lines)

**Purpose**: Retrospective memory system. Learns from past conversations to build user models and patterns over time.

### 5.5 Byterover

**Location**: `plugins/memory/byterover/__init__.py` (~383 lines)

**Purpose**: Memory backend using Byterover service. Provides lightweight persistent memory with fast retrieval.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Memory plugins | 7 |
| Holographic tools | 2 (fact_store, fact_feedback) |
| Holographic actions | 9 |
| Holographic DB tables | 4 (facts, entities, fact_entities, memory_banks) |
| Holographic FTS | SQLite FTS5 virtual table |
| Holographic retrieval weights | FTS: 0.4, Jaccard: 0.3, HRR: 0.3 |
| Holographic HRR dimension | 1024 |
| Holographic default trust | 0.5 |
| Holographic min trust threshold | 0.3 |
| Trust helpful delta | +0.05 |
| Trust unhelpful delta | -0.10 |
| Honcho tools | 4 (profile, search, context, conclude) |
| Holographic total lines | ~1,400 |
| Honcho total lines | ~1,648 |
| Total memory plugin lines | ~9,822 |
| Active memory providers | 1 (only one at a time) |
| Retrieval stages (holographic) | 4 (FTS5 -> Jaccard -> Trust -> Decay) |

---

*Generated from source analysis of the Hermes Agent codebase.*
