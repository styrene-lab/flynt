# Codex Architecture

## Workspace Crates

| Crate | Role |
|---|---|
| `codex-core` | Domain models, `VaultStore` trait, `SyncBackend` trait, markdown/wikilink parser |
| `codex-store` | `SqliteStore` (FTS5, WAL), `Vault` (filesystem indexer), `VaultWatcher` (FSEvents) |
| `codex-agent` | Standalone MCP stdio binary; `rmcp 1.x` tool_router; 7 tools exposed to Omegon |
| `codex-app` | Dioxus 0.7 desktop binary; macOS only (`aarch64-apple-darwin`) |

## Key Decisions (resolved)

- **Source of truth**: markdown files on disk — not the DB. DB is a read index + task store.
- **State DB**: SQLite (`rusqlite`, bundled, WAL) — relational queries for task filtering and backlinks beat a pure KV store.
- **Markdown**: `comrak` — GFM + wikilink scanning via text-node walk.
- **Frontmatter**: TOML `+++` delimiters (primary); `---` accepted for Obsidian compat.
- **Tasks stored in DB only** (not as markdown files) — reduces sync conflict surface; tasks link to documents by `DocumentId`.
- **iCloud**: passive — vault root placed in `~/Library/Mobile Documents/iCloud~com~blackmeridian~codex/Documents/`; no API calls needed.
- **Git sync**: hybrid — auto-commit (debounced 30 s after last change), manual push. `git2` crate.
- **S3 sync**: `object_store` crate (supports S3, GCS, Azure, local). Not yet implemented.
- **MCP transport**: stdio. Omegon connects via `command` transport in `mcp.json`.

## Open Questions

1. **Graph view rendering**: force-directed wikilink graph — HTML Canvas via Dioxus `eval()` JS bridge? SVG with Rust layout? Third option: embed a WASM force-graph library.
2. **Editor surface**: plain `<textarea>` for MVP vs CodeMirror via JS bridge vs fully native Rust editor widget.
3. **Drag-and-drop kanban**: Dioxus drag events are available in 0.7 but not battle-tested for reorder — need a proof of concept.
4. **Conflict resolution (Git)**: last-write-wins acceptable for solo use; two-way diff UI needed for team use. Define scope.
5. **codex-agent binary path**: how does Omegon discover the binary? PATH? Explicit config in `~/.config/omegon/mcp.json`?
6. **Document ID stability**: IDs are assigned at first index time and stored in DB. If DB is wiped, IDs regenerate — this breaks `document_refs` in tasks. Need a stable identity strategy (e.g. path-based slug as primary key).

## Design Node Hierarchy

```
codex-root (exploring)
├── codex-storage     — SqliteStore, Vault, VaultWatcher
├── codex-sync        — iCloud / Git / S3 backends
├── codex-agent-mcp   — MCP tool surface, rmcp server
├── codex-ui-shell    — App shell: sidebar, toolbar, routing
├── codex-ui-editor   — Markdown editor + preview pane
├── codex-ui-kanban   — Kanban board + drag-and-drop
└── codex-ui-graph    — Wikilink force-graph view
```
