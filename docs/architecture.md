# Flynt Architecture

## Product Identity

Flynt is a **single-user** knowledge management and task tracking desktop application for macOS, built in Rust with Dioxus 0.7. It combines Obsidian-style markdown note-taking with kanban project boards and a typed entity system.

- **Markdown project** as the canonical source of truth
- **SQLite** as a hot read index (disposable — rebuilt from markdown on launch)
- **Omegon** is an embedded AI capability that enhances Flynt, not the other way around
- **Publication** system for read-only external visibility when needed

## Boundaries

### What Flynt Is

- Single-user thick client (Dioxus macOS desktop, `aarch64-apple-darwin`)
- Markdown-first knowledge store with TOML frontmatter
- Kanban task tracker with per-project boards
- Entity type system (Document, Project, Task, Repo, Link, Custom)
- Git-backed project persistence for durability, portability, and audit trail
- MCP tool surface so Omegon can read/write project data
- Publication pipeline for static read-only output (markdown + HTML)

### What Flynt Is Not

- A collaboration platform — no shared boards, no real-time sync, no multi-user
- A web application — no server component for end users
- A team git-sync tool — git backing serves the single user's durability
- An Omegon dependency — Flynt is fully functional without Omegon installed

## Workspace Crates

| Crate | Role |
|---|---|
| `flynt-core` | Domain models, `ProjectStore` trait, `SyncBackend` trait, entity/datum type system, markdown/wikilink parser |
| `flynt-store` | `SqliteStore` (FTS5, WAL), `Project` (filesystem indexer), `ProjectWatcher` (FSEvents), task file serialization, `GitSync` |
| `flynt-agent` | Standalone MCP stdio binary; `omegon-extension` 0.15; 14 tools exposed to Omegon |
| `flynt-app` | Dioxus 0.7 desktop binary; views: notes, graph, kanban, search, settings, publication rules |

## Data Model

### Type Hierarchy

```
Datum            — atomic typed value (Bool, Int, Float, Text, Date, Timestamp, Ref, List, Map)
  |
Entity           — identified collection of Datum fields with a kind discriminator
  |
Document         — Entity + markdown body + file path
  |
Project, Task    — Documents/entities with known field contracts (typed projections)
```

### Entity Kinds

| Kind | Storage | Projection |
|---|---|---|
| `Document` | Markdown file in project | — |
| `Project` | Markdown file (kind="project") | `ProjectView` |
| `Task` | DB-only or markdown file under project sub-path | `TaskView` |
| `Repo` | Markdown file (kind="repo") | `RepoView` |
| `Link` | Markdown file (kind="link") | `LinkView` |
| `Custom(String)` | Markdown file | Generic entity accessors |

### Frontmatter Schema

Entity data lives in the `[data]` table within TOML frontmatter. The `kind` field discriminates entity type. Fields are schema-flexible by default.

```toml
+++
id = "uuid"
kind = "task"

[data]
title = "Fix the indexer"
board = "uuid"
column = "Backlog"
status = "todo"
priority = 2
+++
```

## Project & Task Persistence

### Two-tier storage

- **Hot tier**: SQLite — fast reads/writes during runtime. All queries go here.
- **Canonical tier**: Markdown files on disk — durable, portable, git-trackable.

### Project git backing

A project is its top-level directory. If that directory has a `.git`,
`GitSync` (top-level) handles auto-commit and push/pull. There is no
sub-path scoping or external-repo routing — one project per top-level
directory, full stop.

### Task lifecycle

Every task is a markdown file at `Tasks/<board-slug>/<title-slug>.md`,
mirrored into SQLite via `Project::persist_task` for query-time speed.
The file on disk is canonical; SQLite is rebuildable from a fresh
reindex.

## Publication Pipeline

Documents with `publication.enabled = true` export to a static output tree:

- `manifest.json` — index of all published documents
- `{slug}.md` — clean markdown (provenance stripped, wikilinks resolved)
- `{slug}.html` — self-contained HTML with inline styles

Visibility is layered: project-wide default policy, per-tag/per-path rules, per-document overrides. Only `public` and `unlisted` documents are exported; `private` is the default.

## Omegon Integration

Flynt is the primary product. Omegon is an embedded AI capability.

- `flynt-agent` exposes 14 MCP tools via stdio transport
- Omegon can search, read, create, and link documents
- Omegon stores durable memory facts (`ai/memory/`) and archives communications (`references/comms/`)
- Agent rail sidebar in the UI shows Omegon status and interaction
- Flynt launches and runs fully without Omegon installed

## Resolved Decisions

| Decision | Resolution |
|---|---|
| Source of truth | Markdown files on disk; DB is a disposable index |
| State DB | SQLite (`rusqlite`, bundled, WAL mode, FTS5) |
| Markdown parser | `comrak` — GFM + wikilink scanning via text-node walk |
| Frontmatter format | TOML `+++` primary; `---` accepted for Obsidian compat |
| User scope | **Single-user only** — no collaboration server, no shared boards |
| Git backing purpose | Durability, portability, audit trail — not team sync |
| Entity type system | Datum primitives, schema-flexible `[data]` tables, typed projections |
| Document identity | UUID embedded in frontmatter; stable across DB wipes |
| iCloud sync | Passive — project root in iCloud Drive folder; no API calls |
| Git sync | Auto-commit (debounced 30s), manual push; `git2` crate |
| MCP transport | stdio; Omegon connects via `command` transport |
| Omegon relationship | Omegon serves Flynt (embedded capability, not dependency) |

## Open Questions

1. **flynt-agent discovery**: how does Omegon find the binary? PATH? Explicit config in `~/.config/omegon/mcp.json`?
2. **Project/board publication**: extend the publication pipeline to render board state as static views (not just documents).
3. **S3 sync**: `object_store` crate is a dependency but sync backend is not yet implemented.
