+++
id = "obsidian-parity-milestone"
kind = "design_node"
title = "Obsidian parity milestone — implementation decomposition"
status = "active"
tags = ["obsidian-parity", "milestone", "planning"]

[data]
parent = "obsidian-feature-parity"
issue_type = "milestone"
priority = 1
+++

# Obsidian parity milestone

## Milestone

**Name:** Obsidian Parity Foundation

**Goal:** make Flynt credible for an Obsidian power user migrating a real vault
without compromising Flynt's own architecture: project/task orientation,
Git-backed history, deterministic drawings/flows, publication as static output,
and Omegon as the extension/agent plane.

**Release target:** next minor feature train after theme/logo stabilization.

**Release criteria:**

- Active-note context is navigable without opening the global graph.
- Git-backed recovery is visible and usable from the app.
- Sync status has an inspectable activity surface.
- Publication can be configured and previewed from the note workflow.
- Bookmarks and saved searches exist as portable project data.
- The first public Flynt extension API is documented with manifest,
  permissions, capabilities, and compatibility rules.
- No new Obsidian compatibility feature requires a server or breaks plain
  Markdown project portability.

## Workstreams

| Workstream | Feature ID | Priority | Depends on | Outcome |
| --- | --- | --- | --- | --- |
| Active note context | OP-01 | P0 | existing store backlinks/outgoing links | Right inspector with links, outline, properties. |
| Git recovery | OP-02 | P0 | GitSync/history helpers | Restore-as-copy and diff preview for active note. |
| Sync activity | OP-03 | P0 | GitSync status + conflict data | Inspectable sync diagnostics and config boundary docs. |
| Publication workflow | OP-04 | P1 | active properties inspector | Per-note publish controls and export report. |
| Bookmarks/searches | OP-05 | P1 | command palette + project config IO | Portable bookmarks and saved searches. |
| Project Lenses | OP-06 | P1 | metadata index + query engine | Saved table/list views over existing project data. |
| Page preview | OP-07 | P2 | active-note link handling | Hover previews for wikilinks/search/sidebar. |
| Canvas card parity | OP-08 | P2 | current canvas/flow/excalidraw surfaces | Note/media/web/folder canvas cards with graph references. |
| Extension API | OP-09 | P1 | Omegon/flynt-agent extension plane | Versioned manifest, capability schema, permission model. |
| Project sealing vertical slice | OP-10 | P2 | Styrene Identity + seal module | Selective sealed note workflow. |
| Publish site runtime | OP-11 | P2 | publication workflow | Search index, graph payload, theme tokens for static output. |
| Editor affordances | OP-12 | P3 | active inspector + CodeMirror bridge | Slash commands, word count, footnote navigation. |

## OP-01: Active Note Context

### Plan

Create a right-side inspector in Notes that follows the active tab. The panel
starts with three tabs: `Links`, `Outline`, and `Properties`.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-01.1 | Inspector shell and dock behavior | `crates/flynt-app/src/views/notes.rs`, `assets/styles/components.css` | Toggleable panel preserves editor layout and does not overlap agent rail. |
| OP-01.2 | Links tab | `notes.rs`, `flynt-core/src/store.rs`, `flynt-store/src/sqlite.rs` if outgoing helper is added | Shows backlinks and outgoing links for active note, click opens target. |
| OP-01.3 | Outline extraction | new helper in `flynt-core` or `flynt-app` | Extracts headings with level/anchor, handles duplicate headings deterministically. |
| OP-01.4 | Outline navigation | CodeMirror bridge in `notes.rs` | Clicking a heading scrolls the active editor/rendered view to that section. |
| OP-01.5 | Properties read-only view | `notes.rs`, `task_metadata_strip.rs` patterns | Shows title, tags, aliases, kind, status, publication, and `[data]` fields. |
| OP-01.6 | Palette commands | `command_palette.rs` | Commands exist for toggle inspector, show backlinks, show outline. |
| OP-01.7 | Tests | `flynt-core` tests + e2e notes test | Heading extraction and active-note panel smoke test pass. |

### Notes

This is the highest leverage migration feature. It uses existing index data and
turns Flynt's global graph into a daily navigation tool.

## OP-02: Git Recovery

### Plan

Expose a note history browser backed by Git. Start with safe restore semantics:
read-only diff preview and `Restore as Copy`; defer destructive restore.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-02.1 | Git history API | `crates/flynt-store/src/sync/git.rs` | Done — can list commits touching a project-relative path. |
| OP-02.2 | Blob read API | `git.rs` | Done — can load a file's content at a commit. |
| OP-02.3 | Diff summary model | `notes.rs` | Done — active-note history renders selected snapshot vs current body. |
| OP-02.4 | History modal | `notes.rs`, `markdown.css` | Done — active note history shows timestamp, commit, message, author. |
| OP-02.5 | Restore as copy | `Project::save_document_content` path | Done — creates `Recovered/<title> <commit>.md` and opens it. |
| OP-02.6 | Snapshot integration | `command_palette.rs`, settings/docs | Done — Show Note History and Create Snapshot open the history surface. |
| OP-02.7 | Tests | `flynt-store` git tests + `flynt-app` unit tests | Partial — history API and diff helpers covered; UI smoke remains. |

## OP-03: Sync Activity

### Plan

Add an inspectable sync panel instead of relying on a compact status pill.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-03.1 | Sync diagnostic model | `flynt-store/src/sync/git.rs` | Done — reports backend, remote, branch, local dirty files, ahead/behind when available. |
| OP-03.2 | Last run state | app runtime state | Done — current phase, last start/finish, outcome, and success/failure counts persist for the app session. |
| OP-03.3 | Conflict list | existing conflict detection | Done — panel lists conflict files and opens indexed markdown conflicts. |
| OP-03.4 | Sync Activity UI | toolbar popover | Done — operator can inspect status and run manual sync. |
| OP-03.5 | Config boundary docs | `docs/onboarding.md`, parity docs | Done — project-synced vs device-local settings are explicit. |
| OP-03.6 | E2E smoke | `tests/e2e` | Remaining — add Playwright smoke for non-Git and Git-backed toolbar popover. |

## OP-04: Publication Workflow

### Plan

Move publication out of advanced settings only and into the note authoring flow.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-04.1 | Publication fields in Properties | OP-01 properties tab | Done — active note can toggle enabled, visibility, slug, collections. |
| OP-04.2 | Safe frontmatter writer | `flynt-store/src/project.rs` | Done — publication updates preserve unrelated frontmatter and body. |
| OP-04.3 | Publish preview command | `command_palette.rs`, `bootstrap.rs` | Done — palette command exports preview and shows report. |
| OP-04.4 | Export report view | settings or modal | Done — modal shows exported/skipped/error counts and output path. |
| OP-04.5 | Adapter contract design | `design/publication-adapters.md` | Done — static folder, GitHub Pages, Astro adapter boundaries defined. |
| OP-04.6 | Tests | `flynt-store` publication tests | Done — frontmatter update and report output verified. |

## OP-05: Bookmarks And Saved Searches

### Plan

Add portable navigation anchors stored under `.flynt/bookmarks.toml`.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-05.1 | Bookmark model | `flynt-core/src/models.rs` | Done — supports note, heading, search, graph filter, canvas, and drawing targets. |
| OP-05.2 | Project IO | `flynt-store/src/project.rs` | Done — load/save/add/remove bookmarks without DB migration. |
| OP-05.3 | UI list | `sidebar.rs`, `components.css` | Done — sidebar bookmark section lists project bookmarks. |
| OP-05.4 | Add current item commands | `command_palette.rs` | Partial — current note and current search are command-palette actions; graph filter remains deferred until graph filters are persisted. |
| OP-05.5 | Saved search route | `search.rs`, `app.rs`, `sidebar.rs` | Done — clicking a saved search opens Search with the query populated. |
| OP-05.6 | Tests | `flynt-store` tests | Done — bookmark round-trip, target serialization, dedupe, and remove covered. |

## OP-06: Project Lenses

### Plan

Make Flynt's query engine discoverable as saved project lenses.

Project Lenses are Dataview-style saved views over existing indexed project
data. They persist query and display definitions only; they must not persist
query results, document snapshots, or duplicated metadata.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-06.1 | `.flynt/lenses/*.toml` schema | `flynt-core/src/models.rs` | Done — source, filters, columns, sort, layout, and limit definitions. |
| OP-06.2 | Store/query execution | `flynt-core/src/query.rs`, `flynt-store/src/project.rs` | Done — executes document/task lenses against existing store APIs. |
| OP-06.3 | Lenses view | `views/lenses.rs` | Done — renders table and list layouts. |
| OP-06.4 | Lens creation MVP | `command_palette.rs` | Partial — Save Search as Lens creates a search-backed lens; full builder deferred. |
| OP-06.5 | Inline query compatibility | `notes.rs` renderer | Deferred — lenses can later emit/query-block equivalents. |
| OP-06.6 | Tests | core/store tests | Done — lens execution and definition-file round-trip covered. |

## OP-07: Page Preview

### Plan

Add delayed hover previews for note links, then extend to search and graph.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-07.1 | Preview renderer | `note_preview.rs` | Done — produces capped text preview without heavy embeds. |
| OP-07.2 | Wikilink hover bridge | `notes.rs` JS bridge | Done — CodeMirror and rendered markdown wikilinks show delayed previews and dismiss on Escape/mouseout. |
| OP-07.3 | Search/sidebar preview | `search.rs`, sidebar component | Done — same preview card reused for search results and sidebar notes. |
| OP-07.4 | Graph preview | `graph.rs` tooltip | Graph hover can show title/excerpt. |
| OP-07.5 | Tests | unit + e2e visual smoke | Partial — preview excerpt unit tests covered; e2e visual smoke remains. |

## OP-08: Canvas Card Parity

### Plan

Unify Flynt's canvas story around JSON Canvas-compatible note/media/web/folder
cards while preserving Excalidraw for drawings and Flow for structured systems.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-08.1 | Capability matrix | `design/canvas-card-parity.md` | Text, note, media, web, folder, group, edge behavior defined. |
| OP-08.2 | JSON Canvas round-trip tests | `flynt-core/src/canvas.rs` | Unknown fields survive load/save. |
| OP-08.3 | Note card UI | `views/canvas.rs` | Add existing note as card, click opens note. |
| OP-08.4 | Media/web cards | `views/canvas.rs` | Add image/web URL card, render stable preview shell. |
| OP-08.5 | Graph indexing | `flynt-core/src/graph.rs` | Canvas note references create graph edges. |
| OP-08.6 | Agent tool update | `flynt-agent` canvas/drawing tools | Agents can create cards semantically. |

## OP-09: Flynt Extension API

### Plan

Productize the existing Omegon/flynt-agent extension plane as the first public
Flynt extension API. Keep it capability-first and permissioned.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-09.1 | Extension API design doc | `docs/extension-api.md` | Manifest, lifecycle, permissions, capabilities, compatibility documented. |
| OP-09.2 | Manifest schema | `crates/flynt-core` or `flynt-agent` | `flynt.extension.json` validates id/name/version/runtime/capabilities. |
| OP-09.3 | Capability registry | `flynt-agent` | Documents/tasks/boards/graph/drawings/flow/publication/sync capabilities are versioned. |
| OP-09.4 | Permission checks | `flynt-agent` tool dispatch | Tools enforce declared read/write/network/secret permissions. |
| OP-09.5 | Settings integration | Omegon extension manager | Compatibility and disabled-state reasons are visible. |
| OP-09.6 | Sample extension | `examples/` or `crates/` | Minimal native extension exercises manifest and one command/tool. |
| OP-09.7 | Tests | manifest parser + tool dispatch | Invalid manifests fail clearly; undeclared capability denied. |

## OP-10: Selective Project Sealing

### Plan

Ship the smallest useful identity/encryption vertical slice: selective sealed
notes, not full-project sealing.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-10.1 | Key unlock flow reuse | `identity.rs`, settings | Keychain/passphrase unlock can provide project seal key. |
| OP-10.2 | Seal/unseal body operations | `flynt-core/src/seal.rs`, `flynt-store/src/project.rs` | Body encrypts/decrypts; frontmatter remains clear. |
| OP-10.3 | Index sealed note | indexer | Sealed note indexes title/tags/properties, not body. |
| OP-10.4 | UI actions | context menu/sidebar/editor | Seal/unseal note and lock project commands. |
| OP-10.5 | Sync behavior tests | store tests | Encrypted body round-trips through Git-safe text. |

## OP-11: Publish Site Runtime

### Plan

Make exported static sites feel intentional: searchable, themed, and graph-aware.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-11.1 | Search index export | `Project::export_publication_tree` | Emits JSON search index with title/path/body excerpt. |
| OP-11.2 | Published graph export | graph payload filter | Emits graph JSON for public/unlisted docs only. |
| OP-11.3 | Theme token export | publication renderer | Generated HTML includes stable theme CSS variables. |
| OP-11.4 | `publish.css` override | publication exporter | Optional project stylesheet is copied/included. |
| OP-11.5 | SEO fields | frontmatter + renderer | Description/canonical/image/noindex render into HTML. |

## OP-12: Editor Affordances

### Plan

Polish parity once navigation and recovery exist.

### Implementation tasks

| Task ID | Scope | Files likely touched | Acceptance |
| --- | --- | --- | --- |
| OP-12.1 | Slash command menu | CodeMirror bundle/init bridge | `/` inserts headings, task lists, callouts, links, drawings, templates. |
| OP-12.2 | Word count | notes view/status strip | Active note and selection word/char count visible. |
| OP-12.3 | Footnote inspector | active note inspector | Footnotes list and navigate to definitions/references. |
| OP-12.4 | Note composer commands | command palette/editor | Split at heading and merge note into current preserve frontmatter. |

## GitHub Issue Breakdown

GitHub milestone:
[Obsidian Parity Foundation](https://github.com/styrene-lab/flynt/milestone/1)

Milestone-level feature issues, not one issue per task:

| Issue | Feature |
| --- | --- |
| [#7](https://github.com/styrene-lab/flynt/issues/7) | OP-01 Active note context inspector |
| [#8](https://github.com/styrene-lab/flynt/issues/8) | OP-02 Git-backed file recovery UI |
| [#9](https://github.com/styrene-lab/flynt/issues/9) | OP-03 Sync activity and config boundary |
| [#10](https://github.com/styrene-lab/flynt/issues/10) | OP-04 Publication authoring workflow |
| [#11](https://github.com/styrene-lab/flynt/issues/11) | OP-05 Bookmarks and saved searches |
| [#12](https://github.com/styrene-lab/flynt/issues/12) | OP-06 Project Lenses over metadata/query engine |
| [#13](https://github.com/styrene-lab/flynt/issues/13) | OP-07 Page preview |
| [#14](https://github.com/styrene-lab/flynt/issues/14) | OP-08 Canvas card parity |
| [#15](https://github.com/styrene-lab/flynt/issues/15) | OP-09 Flynt extension API foundation |
| [#16](https://github.com/styrene-lab/flynt/issues/16) | OP-10 Selective project sealing |
| [#17](https://github.com/styrene-lab/flynt/issues/17) | OP-11 Publish site runtime |
| [#18](https://github.com/styrene-lab/flynt/issues/18) | OP-12 Editor affordances |

Implementation issues can be split from these when the owning feature enters
active development. This keeps GitHub manageable while preserving enough task
resolution in this document for agent handoff.

## Dependency Order

1. OP-01 Active note context
2. OP-02 Git recovery and OP-03 Sync activity in parallel
3. OP-04 Publication workflow and OP-05 Bookmarks
4. OP-09 Extension API foundation
5. OP-06 Project Lenses
6. OP-07 Page preview
7. OP-08 Canvas card parity
8. OP-10 Sealing vertical slice
9. OP-11 Publish site runtime
10. OP-12 Editor affordances

## First Sprint Recommendation

Start with these tasks:

1. OP-01.1 Inspector shell and dock behavior
2. OP-01.2 Links tab
3. OP-01.3 Outline extraction
4. OP-02.1 Git history API
5. OP-03.1 Sync diagnostic model
6. OP-09.1 Extension API design doc

This produces immediate UX lift while opening the lower-level APIs needed for
history, sync diagnostics, and extension productization.
