# Agent Journal

Append-only record of agent sessions. Read recent entries for context.

## 2026-04-15 — main (4t 6tc 2m34s)

**Task:** let's begin designing Codex, a pure-rust Dioxus app entirely designed to track work tasking and documents/notes. Think "obsidian and kanban" with full agent-surface tooling exposed for Omegon.  This w

**Model:** unknown — 65784 in / 2750 out tokens across 4 turns
## 2026-04-15 — master (47t 55tc 8m16s)

**Task:** let's begin designing Codex, a pure-rust Dioxus app entirely designed to track work tasking and documents/notes. Think "obsidian and kanban" with full agent-surface tooling exposed for Omegon.  This w

**Outcome:** Clean. Run the parser unit tests:

**Model:** anthropic:claude-sonnet-4-6 — 2045202 in / 31033 out tokens across 51 turns
## 2026-04-15 — main (15t 69tc 2m58s)

**Task:** let's begin designing Codex, a pure-rust Dioxus app entirely designed to track work tasking and documents/notes. Think "obsidian and kanban" with full agent-surface tooling exposed for Omegon.  This w

**Outcome:** Here's where we stand:

## ✅ Scaffold committed (`0cdb1cc` + `ba41820`)

**4-crate workspace compiles clean, 4 parser tests passing:**

| Crate | Status | Contents |
|---|---|---|
| `codex-core` | ✅ | Domain models, `VaultStore`/`SyncBackend` traits, wikilink+frontmatter parser |
| `codex-store` | ✅

**Model:** anthropic:claude-sonnet-4-6 — 3217126 in / 35782 out tokens across 66 turns

**Commits:**
  ba41820 docs: add architecture overview with decisions and open questions
  0cdb1cc feat: initial workspace scaffold
## 2026-04-15 — main (11t 79tc 2m41s)

**Task:** let's begin designing Codex, a pure-rust Dioxus app entirely designed to track work tasking and documents/notes. Think "obsidian and kanban" with full agent-surface tooling exposed for Omegon.  This w

**Outcome:** Here's the current state of the design and where to go next.

---

## Decisions recorded (all 6 resolved)

| # | Decision | Status |
|---|---|---|
| 1 | UUID PK + path-slug secondary + UUID embedded in frontmatter | ✅ **Implemented** |
| 2 | Obsidian split-pane, CodeMirror 6 via JS bridge + comrak p

**Model:** anthropic:claude-sonnet-4-6 — 4161592 in / 39695 out tokens across 77 turns

**Commits:**
  31aa618 feat(core): stable document identity via frontmatter UUID
  ba41820 docs: add architecture overview with decisions and open questions
  0cdb1cc feat: initial workspace scaffold
## 2026-04-15 — main (37t 114tc 7m28s)

**Task:** let's begin designing Codex, a pure-rust Dioxus app entirely designed to track work tasking and documents/notes. Think "obsidian and kanban" with full agent-surface tooling exposed for Omegon.  This w

**Outcome:** Working tree is clean — everything is already committed. The last commit `c21b698` captured all three tracks (launch integration, UI shell, git sync backend). Nothing to commit.

**Model:** anthropic:claude-sonnet-4-6 — 7478582 in / 57602 out tokens across 114 turns

**Commits:**
  c21b698 feat: three-track parallel implementation
  09f9153 chore: stub files for cleave scope
  31aa618 feat(core): stable document identity via frontmatter UUID
## 2026-04-15 — main (9t 13tc 2m6s)

**Task:** Let's proceed with codex design

**Outcome:** **What just happened and where we are:**

The vault/watcher/context was completely disconnected from the UI — `bootstrap_from_env` was defined but never called. Fixed. Now every view has a live path to the SQLite store.

**Next design decision needed:** The views are still placeholders. The three op

**Model:** anthropic:claude-sonnet-4-6 — 301328 in / 5551 out tokens across 9 turns

**Commits:**
  a341258 feat(app): wire AppContext into Dioxus context tree at launch
  c21b698 feat: three-track parallel implementation
  09f9153 chore: stub files for cleave scope
## 2026-04-15 — main (8t 20tc 4m12s)

**Task:** Let's proceed with codex design

**Outcome:** **What's working now:**

- Sidebar loads the real document list from SQLite on mount
- Clicking a document sets the reactive `selected_doc` signal
- `NotesView` re-fetches the document automatically when the signal changes (Dioxus reactive dep tracking in the `use_resource` closure)
- comrak renders

**Model:** anthropic:claude-sonnet-4-6 — 747311 in / 19847 out tokens across 17 turns

**Commits:**
  bb5b808 feat(notes): Obsidian-style markdown preview with live sidebar doc list
  a341258 feat(app): wire AppContext into Dioxus context tree at launch
  c21b698 feat: three-track parallel implementation
## 2026-04-15 — main (15t 34tc 6m1s)

**Task:** Let's proceed with codex design

**Outcome:** Here's what you have:

**Theme engine architecture:**
- **One `data-theme` attribute** on the root div — switching themes means writing to a `Signal<ThemeName>`, nothing else
- **80+ design tokens** in `alpharius.css`: every surface, text colour, border, spacing step, radius, shadow, transition, and

**Model:** anthropic:claude-sonnet-4-6 — 1834224 in / 43991 out tokens across 32 turns

**Commits:**
  48f566d feat(theme): parameterized theme engine with Alpharius token set
  bb5b808 feat(notes): Obsidian-style markdown preview with live sidebar doc list
  a341258 feat(app): wire AppContext into Dioxus context tree at launch
## 2026-04-15 — main (16t 49tc 9m4s)

**Task:** Let's proceed with codex design

**Outcome:** Done. Here's what you have:

**Settings view features:**
- **Theme picker** — swatch cards render a mini preview using the theme's own raw hex tokens (the one justified use of inline style in the codebase). Clicking applies immediately via the `ThemeName` signal; the entire app repaints. The `THEMES

**Model:** anthropic:claude-sonnet-4-6 — 3265769 in / 60423 out tokens across 48 turns

**Commits:**
  4946dd2 feat(settings): fully-wired settings view with theme + sync config
  48f566d feat(theme): parameterized theme engine with Alpharius token set
  bb5b808 feat(notes): Obsidian-style markdown preview with live sidebar doc list
## 2026-04-15 — main (14t 62tc 6m14s)

**Task:** Let's proceed with codex design

**Outcome:** Done. What you have:

**Kanban board:**
- **Multi-board** — tab bar, create new board inline (+ New board), auto-selects first on load
- **Columns** — default sprint layout (Backlog → In Progress → Review → Done), WIP limits shown as `count/limit` with warning colour when exceeded
- **Drag & drop** 

**Model:** anthropic:claude-sonnet-4-6 — 4871324 in / 87666 out tokens across 62 turns

**Commits:**
  83f9ad5 feat(kanban): full Kanban board — columns, drag-and-drop, inline add, archive
  4946dd2 feat(settings): fully-wired settings view with theme + sync config
  48f566d feat(theme): parameterized theme engine with Alpharius token set
## 2026-04-15 — main (19t 80tc 5m24s)

**Task:** Let's proceed with codex design

**Outcome:** The Toolbar now requires `selected_doc` and `active_route` — fix the call site in `app.rs`:

**Model:** anthropic:claude-sonnet-4-6 — 7175175 in / 103146 out tokens across 80 turns

**Commits:**
  83f9ad5 feat(kanban): full Kanban board — columns, drag-and-drop, inline add, archive
  4946dd2 feat(settings): fully-wired settings view with theme + sync config
  48f566d feat(theme): parameterized theme engine with Alpharius token set
## 2026-04-15 — main (3t 81tc 37s)

**Task:** Let's proceed with codex design

**Outcome:** **What was committed:**

- **`Vault::save_document_content`** — the write path from UI to filesystem. Writes the file, re-indexes it (preserving frontmatter UUID), and returns.
- **Notes editor** — edit/preview toggle. Edit mode shows a raw markdown textarea; Save button and Cmd+S both write to disk

**Model:** anthropic:claude-sonnet-4-6 — 7644397 in / 104255 out tokens across 83 turns

**Commits:**
  5af3a71 feat: notes editor write mode, toolbar search, vault write methods
  83f9ad5 feat(kanban): full Kanban board — columns, drag-and-drop, inline add, archive
  4946dd2 feat(settings): fully-wired settings view with theme + sync config
## 2026-04-15 — main (7t 87tc 55s)

**Task:** Let's proceed with codex design

**Outcome:** Stop. I have enough to answer without more reads.

**MCP is the wrong move.** Omegon does not use MCP — it has its own extension protocol:

- Extensions are **native binaries** installed to `~/.omegon/extensions/{name}/`
- Communication is **JSON-RPC 2.0 over stdin/stdout** using the `omegon-extensi

**Model:** anthropic:claude-sonnet-4-6 — 8756095 in / 105822 out tokens across 90 turns

**Commits:**
  5af3a71 feat: notes editor write mode, toolbar search, vault write methods
  83f9ad5 feat(kanban): full Kanban board — columns, drag-and-drop, inline add, archive
  4946dd2 feat(settings): fully-wired settings view with theme + sync config

