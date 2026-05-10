---
id: flynt-root
title: "Flynt — Knowledge & Task Tracker"
status: active
tags: [dioxus, macos, mcp, notes, kanban, single-user]
open_questions: []
dependencies: []
related: []
---

# Flynt — Knowledge & Task Tracker

## Overview

Single-user Rust/Dioxus 0.7 macOS desktop application. Obsidian-style markdown knowledge management with kanban task tracking, a typed entity system, and an MCP agent surface for Omegon AI integration. Project root is a plain directory of markdown files; SQLite provides an indexed cache. Git backing provides durability and portability for project data. Publication pipeline renders read-only static output for external visibility.

Workspace: flynt-core (models + entities + traits) · flynt-store (SQLite + filesystem + git) · flynt-agent (MCP server binary) · flynt-app (Dioxus UI binary)

## Decisions

### Document identity: UUID PK + path-slug secondary + UUID embedded in frontmatter

**Status:** accepted

**Rationale:** UUIDs in frontmatter survive DB wipes and file moves. Path-based slugs provide human-readable secondary lookup for wikilinks.

### Editor: Obsidian-style split pane — CodeMirror 6 via JS bridge + comrak preview

**Status:** accepted

**Rationale:** Native Rust editor widgets are immature. CodeMirror via Dioxus `eval()` JS bridge gives us syntax highlighting, vim mode, and markdown-aware editing with minimal custom code.

### Graph view: D3-force via JS bridge, all layout knobs surfaced and persisted

**Status:** accepted

**Rationale:** Force-directed layout via JS bridge leverages mature D3 ecosystem. Graph renders documents, entities (repos, links), tasks, and boards with typed node colors.

### Kanban DnD: Dioxus drag events first; pointer-event fallback if needed

**Status:** accepted

**Rationale:** Dioxus 0.7 drag events work for column reordering. Task cards use drag-and-drop for column transitions.

### Git sync: single-user auto-commit for durability

**Status:** accepted

**Rationale:** Git backing serves durability, portability, and audit trail for the single user. Multi-user git sync was evaluated and rejected — it breaks down beyond a handful of developers, and a coordination server is premature. Projects can back to the project repo (ProjectRepo) or an external repo (ExternalRepo).

### Agent integration: Flynt exposes MCP tools; Omegon is an embedded capability

**Status:** accepted

**Rationale:** Flynt is the primary product. Omegon enhances it via 14 MCP tools (stdio transport). Flynt functions fully without Omegon. The flynt-agent binary runs as a standalone MCP server that Omegon connects to.

### Scope: single-user, no collaboration

**Status:** accepted

**Rationale:** Collaboration requires either git merge semantics that break with concurrent edits, or a coordination server that is premature to build. Flynt stays single-user. External visibility is served by the publication pipeline (static markdown + HTML output).
