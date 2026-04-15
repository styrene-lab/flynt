---
id: codex-root
title: "Codex — Notes & Task Tracker"
status: exploring
tags: [dioxus, macos, mcp, notes, kanban]
open_questions: []
dependencies: []
related: []
---

# Codex — Notes & Task Tracker

## Overview

Pure-Rust Dioxus 0.7 macOS desktop application combining Obsidian-style markdown note-taking with kanban task management, plus an MCP agent surface for Omegon integration. Vault root is a plain directory of markdown files; SQLite provides an indexed cache. Sync backends (iCloud Drive folder, Git, S3) are pluggable behind a trait. The codex-agent binary exposes vault tools to Omegon via stdio MCP transport.

Workspace: codex-core (models + traits) · codex-store (SQLite + filesystem) · codex-agent (MCP server binary) · codex-app (Dioxus UI binary)

## Decisions

### Document identity: UUID PK + path-slug secondary + UUID embedded in frontmatter

**Status:** accepted

**Rationale:** 

### Editor: Obsidian-style split pane — CodeMirror 6 via JS bridge + comrak preview

**Status:** accepted

**Rationale:** 

### Graph view: D3-force via JS bridge, all layout knobs surfaced and persisted

**Status:** accepted

**Rationale:** 

### Kanban DnD: Dioxus drag events first; pointer-event fallback if needed; click-to-move rejected

**Status:** accepted

**Rationale:** 

### Git sync: multi-user with auto-commit, background pull, conflict resolution panel

**Status:** accepted

**Rationale:** 

### Agent integration: Codex writes mcp.json on launch; embedded Omegon sidebar (Cmd+Shift+A)

**Status:** accepted

**Rationale:**
