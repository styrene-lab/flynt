# Flynt

**Local-first knowledge management. Markdown is the source of truth.**

A desktop + mobile app for notes, tasks, drawings, and knowledge graphs — built in Rust, synced with Git, powered by an AI agent. No cloud account, no subscription, no vendor lock-in.

[![docs](https://img.shields.io/badge/docs-flynt.styrene.io-2ab4c8)](https://flynt.styrene.io)
[![demo](https://img.shields.io/badge/demo-demo.flynt.styrene.io-1ab878)](https://demo.flynt.styrene.io)
[![license](https://img.shields.io/badge/license-BSL%201.1-344858)](LICENSE)

---

## What it does

Your project is a folder of `.md` files. Flynt indexes them, links them, and gets out of the way.

- **Wikilinks & backlinks** — `[[note]]` creates connections. The knowledge graph shows how ideas relate.
- **Live markdown preview** — Obsidian-style live editing with CodeMirror 6. Headings, tables, bold, links render inline; click to reveal raw syntax.
- **Kanban boards** — task management with decay-based relevance scoring. Untouched tasks fade naturally.
- **Excalidraw drawings** — visual thinking embedded directly in notes.
- **Query blocks** — `TABLE`, `LIST`, `TASK` queries inline in your documents (like Dataview).
- **Daily notes & templates** — date-indexed journals with variable expansion.
- **Git sync** — auto-commit + push/pull in the background. Multi-device, no server.
- **AI agent** — Omegon in the sidebar with full project read/write access.
- **iOS Share Extension** — share links, text, and images from any iOS app into your project.
- **Cross-platform** — macOS (DMG + TestFlight), iOS (TestFlight), Linux amd64/aarch64.

---

## Install

### macOS

Download the latest DMG from [Releases](https://github.com/styrene-lab/flynt/releases/latest). Open it, drag to Applications.

### iOS

TestFlight beta — contact the team for access. Includes the Share Extension for saving links, text, and images from any app.

### Linux

CI builds for `x86_64` and `aarch64` are available from [Releases](https://github.com/styrene-lab/flynt/releases). Requires `webkit2gtk-4.1` and GTK 3.

```sh
# Ubuntu/Debian
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev
```

### Build from source

```sh
# Prerequisites: Rust toolchain + dioxus-cli
cargo install dioxus-cli

# Desktop
cd crates/flynt-app && dx build --platform desktop --release

# iOS
cd crates/flynt-mobile && IPHONEOS_DEPLOYMENT_TARGET=17.0 dx build --platform ios --device --release
```

---

## Quick start

1. Open Flynt
2. Choose **Clone remote project**
3. Enter `https://github.com/styrene-lab/flynt-demo-project.git`, branch `main`
4. The demo project opens with documentation and a live knowledge graph

Or choose **Create local project** to start fresh.

---

## Architecture

```
flynt-core     Pure Rust models, query engine, parser, templates, graph layout
flynt-store    Project I/O, SQLite index, git/iCloud sync, file watching
flynt-app      macOS/Linux desktop UI (Dioxus + wry + muda)
flynt-mobile   iOS companion app (Dioxus mobile)
flynt-agent    MCP extension for Omegon (project tools)
```

All crates share a workspace at the repo root. The desktop and mobile apps depend on `flynt-core` and `flynt-store`. The agent extension is a standalone binary.

### Key design decisions

- **Markdown is canonical.** No database is the source of truth — the `.md` files are. The SQLite index is derived and rebuilds from disk on every launch.
- **Local-first.** Everything works offline. Sync is optional and Git-based.
- **No Node.js.** All JS (CodeMirror, Excalidraw) is vendored as static bundles. No npm, no node_modules, no build step for frontend code.
- **No MCP for integration.** The agent extension provides project tools via ACP (Agent Client Protocol), not MCP.

---

## Project structure

```
my-project/
  .flynt/
    config.toml          # project settings (name, sync, appearance)
    templates/           # note templates (Note.md, Daily.md, Meeting.md)
    notifications/       # git-synced notification queue
  .flynt-local/          # SQLite index (auto-generated, gitignored)
  notes.md
  guides/
  daily/
  drawings/
```

### Frontmatter

Flynt uses TOML frontmatter (enclosed in `+++`). YAML (`---`) is also read.

```toml
+++
title = "My Note"
tags = ["project", "idea"]
+++
```

### Sync

```toml
# .flynt/config.toml
[sync]
backend = "git"
remote = "origin"
branch = "main"
auto_commit_seconds = 60
```

Flynt auto-commits, pulls, and pushes on a timer. Merge conflicts are detected and reported. SSH keys and Git credential helpers are supported.

---

## Tests

```sh
cargo test -p flynt-core -p flynt-store
```

269 tests covering: document parsing, query DSL, task decay math, project lifecycle, tag operations, notifications, git sync (status, commit, pull, push, clone, conflicts), and integration tests.

---

## Sites

| URL | What |
|-----|------|
| [flynt.styrene.io](https://flynt.styrene.io) | Landing page |
| [demo.flynt.styrene.io](https://demo.flynt.styrene.io) | Demo project (clone this to get started) |
| [demo.flynt.styrene.io/graph](https://demo.flynt.styrene.io/graph/) | Interactive knowledge graph |

---

## Ecosystem

Flynt is part of the [Styrene](https://styrene.io) stack:

- **[Omegon](https://omegon.styrene.io)** — terminal-native AI agent harness (powers the Flynt agent sidebar)
- **[Styrene Identity](https://github.com/styrene-lab/styrene-rs)** — cross-device identity, key derivation, and project encryption (planned integration)

---

## License

Business Source License 1.1 — see [LICENSE](LICENSE).

Free for non-production use (evaluation, development, testing, personal use). Change date: 2031. Change license: MIT.

For commercial licensing: licensing@styrene.io
