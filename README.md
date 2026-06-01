<p align="center">
  <img src="docs/logo.svg" alt="WaitAgent" width="320">
</p>

# WaitAgent

[![CI](https://github.com/kikakkz/wait-agent/actions/workflows/ci.yaml/badge.svg)](https://github.com/kikakkz/wait-agent/actions/workflows/ci.yaml)
[![Rust](https://img.shields.io/badge/rust-1.86.0-orange?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue)](#license)
[![tmux](https://img.shields.io/badge/tmux-vendored-1e90ff?logo=tmux)](https://github.com/tmux/tmux)

> **A tmux-like multi-machine, multi-agent session manager — focused on developer project parallelism.**

WaitAgent is a terminal-native workspace manager that lets you run multiple AI agent sessions across machines from a single terminal interface. It does not replace your terminal, your agents, or your workflow — it gives you one place to manage them all.

---

## What WaitAgent Is

WaitAgent is a **tmux-first session manager** built for the reality of parallel AI-assisted development:

- **One workspace, many sessions** — create and switch between multiple agent sessions (Claude Code, Codex CLI, etc.) inside a single terminal workspace
- **Multi-machine aggregation** — connect remote machines over gRPC and interact with their sessions through the same unified catalog
- **Single-focus interaction** — exactly one session is visible and receives input at a time, so input never goes to the wrong place
- **Terminal-native, no login** — runs as a local binary with vendored tmux; no account, no cloud service, no registration required

WaitAgent is **not** an IDE, not an agent platform, and not an orchestration layer. It is the terminal multiplexing and session management layer that sits underneath your agents.

---

## WaitAgent vs Warp

| | WaitAgent | Warp |
|---|---|---|
| **Paradigm** | tmux-like session manager | Complete agentic IDE |
| **Surface** | Terminal-native (vendored tmux) | Custom GPU-accelerated terminal + web app |
| **Account** | None — local binary only | Login and registration required |
| **Focus** | Multi-machine multi-agent session parallelism | Full-stack agentic development environment |
| **Architecture** | One binary, vendored tmux, gRPC for remote | Proprietary terminal + cloud-backed agent platform |
| **Extensibility** | Vendor-neutral — works with any CLI agent | Warp-native agent ecosystem |
| **Target user** | Developers already using Claude Code / Codex CLI who need to parallelize | Developers looking for an all-in-one agentic IDE |

Warp is a complete development environment: it replaces your terminal, provides its own agent, and ties into a cloud platform. WaitAgent solves a narrower problem: when you are already running multiple agent sessions across machines, how do you manage them all from one place without changing your tools or signing up for a service.

---

## How It Works

WaitAgent embeds a vendored tmux and builds a persistent workspace layout on top of it:

```
┌────────────────────────────────────┐
│  Sidebar          │  Main Slot     │
│  ─────────        │                │
│  Session list     │  Active        │
│  Node list        │  session       │
│  Waiting badges   │  output        │
│                   │                │
├────────────────────────────────────┤
│  Footer / Status                   │
└────────────────────────────────────┘
```

- **Sidebar** — session catalog with waiting-state badges, shared across local and remote sessions
- **Main slot** — the active focused session; receives all input, renders raw PTY output
- **Footer** — status line with node identity and session info
- **Fullscreen** — zoom the main slot to full terminal size, restore cleanly

Switching sessions rebinds the main slot only — sidebar and footer stay fixed. This keeps the workspace stable while the user moves between sessions.

---

## Deployment Modes

### Local Mode

One machine, one `waitagent` workspace, multiple managed sessions:

```bash
waitagent
```

Creates a tmux-backed workspace. Sessions run as PTY-backed shell environments where you can launch Claude Code, Codex CLI, or any terminal workflow.

### Multi-Machine Mode

Connect remote machines over gRPC so their sessions appear in your local catalog:

**Server (listener):**

```bash
waitagent --port 7474
```

**Remote node (connects to server):**

```bash
waitagent --connect <server-ip>:7474
```

Remote sessions appear in the sidebar alongside local sessions. Input flows through the server control plane to the PTY-owning node; output synchronizes back to all attached consoles. The transport uses a single long-lived node-scoped gRPC connection with session-scoped routing, reconnect support, and replay on reconnect.

---

## Remote Protocol Status

| Feature | Status |
|---|---|
| gRPC node session protocol | Implemented |
| `--port` / `--connect` CLI | Implemented |
| Session-scoped routing and authority transport | Implemented |
| Reconnect with bounded replay | Implemented |
| Publication ownership and target discovery | Implemented |
| Remote terminal bootstrap and replay | Implemented |
| Live-mirror open/close protocol | Implemented |
| PTY-owner mirror lifecycle hardening | In progress |
| Cross-host visible parity validation | In progress |

---

## Quick Start

**One-line install (Linux x86_64 / macOS Apple Silicon):**

```bash
curl -fsSL https://raw.githubusercontent.com/kikakkz/wait-agent/main/scripts/install.sh | bash
```

**Build from source:**

```bash
git clone --recursive https://github.com/kikakkz/wait-agent
cd wait-agent
./scripts/install-build-deps.sh
cargo build --release
```

---

## Usage

```bash
# Start a workspace
waitagent

# List sessions
waitagent ls

# Attach to an existing session
waitagent attach <target>

# Detach
waitagent detach
```

Inside the workspace, create sessions, launch agents, and switch between them — all from one terminal.

---

## Why This Exists

Existing tools each solve part of the problem:

- **tmux / Zellij** — terminal multiplexing infrastructure, not interaction scheduling
- **Claude Code / Codex CLI** — single-agent CLI execution, not multi-session management
- **Warp / Cursor / Codex App** — vendor-owned agentic IDEs requiring accounts and cloud services

WaitAgent targets the missing layer:

> A terminal-native, vendor-neutral session manager that lets you run multiple agents across machines from one place — no account, no IDE, no platform lock-in.

---

## Documentation

- [Product PRD](docs/wait-agent-prd.md)
- [Architecture](docs/architecture.md)
- [Tmux-First Workspace Plan](docs/tmux-first-workspace-plan.md)
- [Tmux-First Runtime Architecture](docs/tmux-first-runtime-architecture.md)
- [Functional Design](docs/functional-design.md)
- [Remote Node Connection Architecture](docs/remote-node-connection-architecture.md)
- [Remote Network Completion Plan](docs/remote-network-completion-plan.md)
- [Remote Live Mirror Design](docs/remote-live-mirror-design.md)
- [Interaction Flows](docs/interaction-flows.md)
- [Protocol](docs/protocol.md)
- [Local Acceptance Checklist](docs/local-acceptance-checklist.md)
- [Execution Status Board](docs/execution-status-board.md)

---

## Topics

`tmux` `terminal-multiplexer` `multiplexer` `workspace-manager` `terminal` `rust` `cli` `tui` `multi-agent` `ai-agents` `multi-machine` `session-manager` `grpc`

*Add these topics on the [repo settings page](https://github.com/kikakkz/wait-agent/settings) → "Topics" for better discoverability on GitHub.*
