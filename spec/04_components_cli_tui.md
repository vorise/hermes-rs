# Hermes Agent — CLI TUI Components

This document covers the CLI TUI components built on prompt_toolkit.

---

## Table of Contents

1. [TUI Architecture](#1-tui-architecture)
2. [prompt_toolkit Framework](#2-prompt_toolkit-framework)
3. [Fixed Input Area Layout](#3-fixed-input-area-layout)
4. [SlashCommandCompleter](#4-slashcommandcompleter)
5. [Skin Engine](#5-skin-engine)
6. [Banner & Version Display](#6-banner--version-display)
7. [Spinner & Status Display](#7-spinner--status-display)
8. [Key Bindings](#8-key-bindings)
9. [Cursor Control](#9-cursor-control)
10. [Output Handling](#10-output-handling)
11. [Callbacks System](#11-callbacks-system)

---

## 1. TUI Architecture

### Overview

The Hermes CLI TUI is built on `prompt_toolkit`, a Python library for building interactive terminal applications.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Terminal Output                          │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  Welcome message / Banner                                ││
│  │  Model info / Status                                     ││
│  │  ─────────────────────────────────────────────────────   ││
│  │  User: hello!                                            ││
│  │  Assistant: Hi! How can I help you today?               ││
│  │  [tool execution output...]                             ││
│  │                                                          ││
│  └─────────────────────────────────────────────────────────┘│
│  ─────────────────────────────────────────────────────────── │
│  > user input here...                          [spinner]     │
│                      Fixed Input Area                        │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Decision

Unlike simple readline, the Hermes TUI uses a **fixed input area** layout:
- Output region scrolls independently
- Input area stays at bottom
- Multiline editing supported
- Cursor control in input area

---

## 2. prompt_toolkit Framework

### Location

`cli.py` — imports and usage

### Key Imports

```python
from prompt_toolkit import Application
from prompt_toolkit.layout import Layout, HSplit, Window, FormattedTextControl
from prompt_toolkit.layout import ConditionalContainer
from prompt_toolkit.layout.processors import Processor, Transformation
from prompt_toolkit.layout.dimension import Dimension
from prompt_toolkit.layout.menus import CompletionsMenu
from prompt_toolkit.widgets import TextArea
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.styles import Style
from prompt_toolkit.history import FileHistory
from prompt_toolkit.completion import Completer, Completion
from prompt_toolkit.filters import Condition
from prompt_toolkit.patch_stdout import patch_stdout
from prompt_toolkit.formatted_text import ANSI
from prompt_toolkit.cursor_shapes import CursorShape
```

### Key Classes

| Class | Purpose |
|-------|---------|
| `Application` | Main application container |
| `Layout` | Terminal layout management |
| `HSplit` | Horizontal split container |
| `Window` | Terminal window widget |
| `FormattedTextControl` | Rich text display |
| `TextArea` | Text input widget |
| `KeyBindings` | Keyboard shortcut manager |
| `CompletionsMenu` | Autocomplete dropdown |

---

## 3. Fixed Input Area Layout

### Location

`cli.py` — TUI layout

### Layout Structure

```python
layout = Layout(
    HSplit([
        # Output region (scrollable)
        Window(
            content=FormattedTextControl(get_text=get_output_text),
            scroll_offset=...,
        ),
        # Separator
        Window(height=1, char='─'),
        # Input area (fixed at bottom)
        Window(
            content=TextArea(...),
            height=Dimension(min=1, max=3),  # Up to 3 lines
        ),
    ])
)
```

### Features

- Output scrolls, input stays fixed
- Multiline input (Shift+Enter for newline)
- Input area height limited (max 3 lines)
- Cursor style (block, steady)

---

## 4. SlashCommandCompleter

### Location

`hermes_cli/commands.py` — class `SlashCommandCompleter`

### Purpose

Provides autocomplete suggestions as the user types `/` in the CLI TUI.

### Interface

```python
class SlashCommandCompleter(Completer):
    def get_completions(self, document, complete_event):
        # Returns Completion objects for matching commands
```

### Features

- Detects `/` prefix
- Fuzzy matching against command names
- Description shown alongside suggestions
- Sorted by relevance
- Context-aware (some commands only available when relevant)

---

## 5. Skin Engine

### Location

`hermes_cli/skin_engine.py` (~40K lines)

### Purpose

CLI visual customization system. Allows users to theme the CLI with different colors, styles, and layouts.

### Features

- Color scheme customization
- Banner style selection
- Prompt format customization
- Status bar customization
- Emoji toggle
- Layout preferences

### Skin Configuration

```yaml
skin:
  name: "default"  # or "dark", "light", "kawaii", etc.
  # Custom overrides
  prompt_prefix: "> "
  colors:
    user: "cyan"
    assistant: "green"
    tool: "yellow"
```

---

## 6. Banner & Version Display

### Location

`hermes_cli/banner.py` (~22K lines)

### Purpose

Display version banner and branding information.

### Features

- ASCII art branding
- Version number display
- Model info
- Platform info
- Update availability check

### Banner Elements

| Element | Description |
|---------|-------------|
| Logo | ASCII art Hermes logo |
| Version | Current version number |
| Model | Current model and provider |
| Platform | Current platform (cli, telegram, etc.) |
| Home | Hermes home directory path |
| Update | Update availability notification |

---

## 7. Spinner & Status Display

### Location

`agent/display.py` — `KawaiiSpinner` class

### Spinner Frames

```python
_COMMAND_SPINNER_FRAMES = ("⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏")
```

### Features

- Animated spinner during tool execution
- Tool name display
- Tool argument preview
- Cute/kawaii tool messages
- Failure detection
- Emoji support

### Status Display

Status messages displayed above the input area:
- API call progress
- Tool execution status
- Error messages
- Cost/token summaries

---

## 8. Key Bindings

### Location

`cli.py` — KeyBindings setup

### Key Bindings

| Key | Action |
|-----|--------|
| Enter | Submit input |
| Shift+Enter | Newline in input |
| Up arrow | Previous message in history |
| Down arrow | Next message in history |
| Ctrl+C | Interrupt current work |
| Ctrl+D | Exit |
| Ctrl+L | Clear screen |
| Tab | Autocomplete command |
| Escape | Cancel autocomplete |

---

## 9. Cursor Control

### Features

- Block cursor (non-blinking)
- Input area highlighting
- Cursor visibility management

### Cursor Shape

```python
try:
    from prompt_toolkit.cursor_shapes import CursorShape
    _STEADY_CURSOR = CursorShape.BLOCK  # Non-blinking block cursor
except (ImportError, AttributeError):
    _STEADY_CURSOR = None
```

---

## 10. Output Handling

### Safe Writer

`run_agent.py` — `_SafeWriter` class

Wraps stdout/stderr to catch OSError/ValueError from broken pipes:
- systemd services
- Docker containers
- Headless daemons
- ThreadPoolExecutor thread teardown

### Output Routing

- CLI output routed through `_print_fn` (default: builtins.print)
- CLI replaces with `_cprint` so ANSI sequences go through prompt_toolkit
- Gateway output routed through platform adapters

---

## 11. Callbacks System

### Location

`hermes_cli/callbacks.py` (~8K lines)

### Purpose

Terminal callbacks for agent events.

### Callbacks

| Callback | Purpose |
|----------|---------|
| `clarify` | Interactive user questions (clarify tool) |
| `sudo` | Elevated permission requests |
| `approval` | Dangerous command approval |

### Clarify Callback

The clarify tool asks the agent to pose interactive questions to the user:
```
Agent: "I need to know your preferred deployment environment. Choose one:"
  1. Local machine
  2. Docker container
  3. Cloud VM
User selects → answer returned to agent
```

---

## Key Numbers

| Metric | Value |
|--------|-------|
| TUI file size | ~447K lines (cli.py) |
| Skin engine size | ~40K lines |
| Banner file size | ~22K lines |
| Display file size | ~40K lines |
| Key bindings | 8+ |
| Spinner frames | 10 |

---

*Generated from source analysis of the Hermes Agent codebase.*
