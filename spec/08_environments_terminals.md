# Hermes Agent — Terminal Environments

This document covers all terminal backends (environments) that Hermes uses for tool execution.

---

## Table of Contents

1. [Environment Architecture](#1-environment-architecture)
2. [Base Environment](#2-base-environment)
3. [Local Environment](#3-local-environment)
4. [Docker Environment](#4-docker-environment)
5. [SSH Environment](#5-ssh-environment)
6. [Modal Environment](#6-modal-environment)
7. [Daytona Environment](#7-daytona-environment)
8. [Singularity Environment](#8-singularity-environment)
9. [File Sync](#9-file-sync)
10. [Terminal Tool Integration](#10-terminal-tool-integration)

---

## 1. Environment Architecture

### Location

`tools/environments/` — 12 files

### Purpose

Terminal backends that provide isolated execution environments for the terminal tool. Hermes supports 6 different backends, allowing it to run anywhere from a local machine to cloud GPUs.

### Environment Selection

The backend is configured in `~/.hermes/config.yaml`:
```yaml
terminal:
  backend: "local"  # or "docker", "ssh", "modal", "daytona", "singularity"
```

### Architecture

```
Terminal Tool
     ↓
Environment Router
     ↓
┌────────┬────────┬─────┬───────┬────────┬────────────┐
│ Local  │ Docker │ SSH │ Modal │ Daytona│ Singularity│
└────────┴────────┴─────┴───────┴────────┴────────────┘
```

---

## 2. Base Environment

### Location

`tools/environments/base.py`

### Purpose

Abstract base class for all terminal environments.

### Interface

| Method | Purpose |
|--------|---------|
| `setup()` | Initialize environment |
| `teardown()` | Clean up environment |
| `run_command()` | Execute a command |
| `run_command_interactive()` | Execute an interactive command (PTY) |
| `upload_file()` | Upload file to environment |
| `download_file()` | Download file from environment |
| `get_working_dir()` | Get current working directory |
| `is_persistent()` | Check if environment persists between turns |

### Key Concepts

- **Task ID** — Each conversation turn gets a unique task ID for isolation
- **Working directory** — Per-environment working directory
- **File sync** — Synchronize files between local and remote environments
- **Persistence** — Whether environment state persists between turns

---

## 3. Local Environment

### Location

`tools/environments/local.py`

### Purpose

Execute commands on the local machine.

### Features

- No isolation (runs on host)
- Full system access
- No setup required
- Fastest execution
- No file sync needed

### Configuration

```yaml
terminal:
  backend: "local"
```

### Use Cases

- Personal development machines
- Already-isolated systems (containers, VMs)
- When maximum performance is needed

---

## 4. Docker Environment

### Location

`tools/environments/docker.py`

### Purpose

Execute commands in Docker containers.

### Features

- Container-based isolation
- Configurable base image
- Volume mounting for file sync
- Automatic container lifecycle
- Network isolation
- Resource limits

### Configuration

```yaml
terminal:
  backend: "docker"
  docker:
    image: "python:3.12-slim"
    volumes: ["/path/to/project:/workspace"]
```

### Lifecycle

1. **Setup** — Pull/create container
2. **Per turn** — exec into container, run command
3. **Teardown** — Stop/remove container

---

## 5. SSH Environment

### Location

`tools/environments/ssh.py`

### Purpose

Execute commands on remote machines via SSH.

### Features

- Remote machine access
- Key-based or password authentication
- File sync via SCP/SFTP
- Working directory management
- Session persistence

### Configuration

```yaml
terminal:
  backend: "ssh"
  ssh:
    host: "remote.example.com"
    user: "username"
    key_path: "~/.ssh/id_rsa"
    port: 22
```

### Use Cases

- Cloud VMs
- Remote development machines
- GPU servers
- On-premises servers

---

## 6. Modal Environment

### Location

`tools/environments/modal.py`, `managed_modal.py`, `modal_utils.py`

### Purpose

Execute commands on Modal cloud infrastructure with serverless persistence.

### Features

- Serverless execution
- Serverless persistence (hibernate when idle)
- GPU access
- No idle costs
- Automatic resource provisioning
- Cloud storage integration

### Key Concepts

- **Sandbox** — Modal's isolated execution environment
- **Persistent volume** — State persistence between runs
- **Hibernation** — Environment sleeps when idle, wakes on demand
- **GPU** — Access to cloud GPUs for ML workloads

### Configuration

```yaml
terminal:
  backend: "modal"
  modal:
    token_id: "..."
    token_secret: "..."
    # Optional
    image: "..."
    gpu: "T4"  # or "A100", "L40S", etc.
    memory: "2048MB"
```

### Serverless Mode

The managed Modal backend provides serverless persistence:
1. Environment hibernates when idle
2. Wakes on demand when message received
3. Costs nearly nothing between sessions
4. Maintains state across wake cycles

---

## 7. Daytona Environment

### Location

`tools/environments/daytona.py`

### Purpose

Execute commands on Daytona cloud infrastructure with serverless persistence.

### Features

- Serverless execution
- Serverless persistence
- Automatic resource provisioning
- Cloud-native execution

### Configuration

```yaml
terminal:
  backend: "daytona"
  daytona:
    api_key: "..."
```

---

## 8. Singularity Environment

### Location

`tools/environments/singularity.py`

### Purpose

Execute commands in Singularity/Apptainer containers.

### Features

- HPC-grade container isolation
- No root required
- GPU support
- Shared filesystem access

### Configuration

```yaml
terminal:
  backend: "singularity"
  singularity:
    image: "/path/to/image.sif"
```

### Use Cases

- HPC clusters
- Shared computing environments
- When Docker is not available

---

## 9. File Sync

### Location

`tools/environments/file_sync.py`

### Purpose

Synchronize files between local and remote environments.

### Features

- SCP/SFTP file transfer
- Directory synchronization
- Change detection
- Bidirectional sync

### Key Methods

| Method | Purpose |
|--------|---------|
| `sync_up()` | Upload local files to remote |
| `sync_down()` | Download remote files to local |
| `sync_changes()` | Sync only changed files |

---

## 10. Terminal Tool Integration

### Location

`tools/terminal_tool.py` (~74K lines)

### Purpose

The terminal tool orchestrates command execution across all environments.

### Environment Detection

```python
def get_active_env() -> BaseEnvironment:
    """Get the currently configured environment."""
    # Reads config.yaml, returns appropriate environment instance
```

### Persistence Check

```python
def is_persistent_env() -> bool:
    """Check if the current environment persists between turns."""
    # Modal and Daytona with serverless persistence return True
```

### Cleanup

```python
def cleanup_vm(task_id: str) -> None:
    """Clean up environment resources for a task."""
    # Stops containers, closes SSH connections, hibernates Modal sandboxes
```

---

## Key Numbers

| Metric | Value |
|--------|-------|
| Total environments | 6 |
| Environment files | 12 |
| Serverless options | 2 (Modal, Daytona) |
| GPU support | Modal, Singularity |
| File sync | SCP/SFTP, volume mounts |

---

*Generated from source analysis of the Hermes Agent codebase.*
