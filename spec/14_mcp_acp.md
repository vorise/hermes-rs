# Hermes Agent — MCP & ACP Integration

This document covers the MCP (Model Context Protocol) client and ACP (Agent Communication Protocol) server integration.

---

## Table of Contents

1. [MCP Client](#1-mcp-client)
2. [MCP OAuth](#2-mcp-oauth)
3. [MCP Configuration](#3-mcp-configuration)
4. [MCP Serve](#4-mcp-serve)
5. [ACP Adapter](#5-acp-adapter)

---

## 1. MCP Client

### Location

`tools/mcp_tool.py` (~88K lines)

### Purpose

MCP (Model Context Protocol) client that connects to external MCP servers and exposes their tools to the agent.

### Architecture

```
Hermes Agent → MCP Client → MCP Server → Tools/Resources/Prompts
```

### Transports

| Transport | Description |
|-----------|-------------|
| Stdio | Spawn MCP server process, communicate via stdin/stdout |
| SSE | Connect to MCP server via Server-Sent Events |

### Key Features

- Dynamic tool discovery
- Server lifecycle management
- Tool result size limiting
- Resource support
- Prompt support
- OAuth for authenticated servers
- Error handling and recovery

### Server Management

| Operation | Description |
|-----------|-------------|
| `connect()` | Connect to MCP server |
| `disconnect()` | Disconnect from MCP server |
| `list_tools()` | List available tools from server |
| `call_tool()` | Call a tool on the server |
| `list_resources()` | List available resources |
| `read_resource()` | Read a resource |
| `list_prompts()` | List available prompts |
| `get_prompt()` | Get a prompt |

### Dynamic Discovery

MCP tools are registered/deregistered dynamically:
- Server connects → tools registered via `registry.register()`
- Server sends `notifications/tools/list_changed` → old tools deregistered, new tools registered
- Server disconnects → all tools deregistered

### Shadow Prevention

The registry prevents MCP tools from shadowing built-in tools:
- MCP-to-MCP overwrites are allowed
- Built-in vs MCP shadowing is rejected

---

## 2. MCP OAuth

### Location

`tools/mcp_oauth.py` (~17K lines)

### Purpose

OAuth 2.0 flow for MCP servers that require user authentication.

### Flow

1. MCP server requires authentication
2. Hermes generates OAuth authorization URL
3. User authorizes in browser
4. OAuth callback receives authorization code
5. Hermes exchanges code for access token
6. Token stored for future requests
7. Token refresh on expiration

### Token Storage

OAuth tokens are stored securely in the Hermes config.

---

## 3. MCP Configuration

### Location

`hermes_cli/mcp_config.py` (~24K lines)

### Purpose

MCP server configuration via CLI.

### Configuration Format

```yaml
mcp:
  servers:
    filesystem:
      command: "npx"
      args: ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/project"]
    github:
      command: "npx"
      args: ["-y", "@modelcontextprotocol/server-github"]
    postgres:
      command: "npx"
      args: ["-y", "@modelcontextprotocol/server-postgres", "postgresql://localhost/mydb"]
```

### CLI Commands

| Command | Purpose |
|---------|---------|
| `hermes mcp list` | List configured MCP servers |
| `hermes mcp add` | Add a new MCP server |
| `hermes mcp remove` | Remove an MCP server |
| `hermes mcp status` | Show MCP server status |

---

## 4. MCP Serve

### Location

`mcp_serve.py` (~31K lines)

### Purpose

Standalone MCP server that exposes Hermes tools via the Model Context Protocol.

### Use Cases

- Expose Hermes tools to other AI assistants
- Use Hermes as a tool server for Claude Desktop, Cursor, etc.
- Share tools across multiple agents

### Features

- All Hermes tools available via MCP
- Stdio transport
- Tool schema compatibility
- Result size limiting

---

## 5. ACP Adapter

### Location

`acp_adapter/` — ACP (Agent Communication Protocol) server

### Purpose

IDE integration via the Agent Communication Protocol. Used by VS Code, Zed, and JetBrains extensions.

### Entry Point

```python
[project.scripts]
hermes-acp = "acp_adapter.entry:main"
```

### Architecture

```
IDE Extension → ACP Protocol → Hermes Agent → Tool Execution
```

### ACP Protocol

The Agent Communication Protocol defines:
- Session management
- Message exchange
- Tool execution
- File context awareness
- Selection-based operations

### ACP Adapter Components

| Component | Purpose |
|-----------|---------|
| `acp_adapter/entry.py` | Entry point |
| `acp_adapter/server.py` | ACP server implementation |
| `acp_adapter/handlers.py` | Request handlers |
| `acp_adapter/` (other files) | Protocol support files |

### Features

- IDE-native agent experience
- File context awareness
- Selection-based operations
- Terminal integration
- Git integration

### ACP Registry

`acp_registry/` — ACP registry for tool and command discovery.

---

## Key Numbers

| Metric | Value |
|--------|-------|
| MCP client size | ~88K lines |
| MCP OAuth size | ~17K lines |
| MCP config size | ~24K lines |
| MCP serve size | ~31K lines |
| MCP transports | 2 (stdio, SSE) |
| ACP entry points | 1 (hermes-acp) |

---

*Generated from source analysis of the Hermes Agent codebase.*
