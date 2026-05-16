# Flynt UI Guide

> Interface map for QA, user documentation, and feature verification.
> Version: 0.6.2

---

## 1. First Launch (Welcome Screen)

**Route:** `Welcome` | **Trigger:** No project configured, or `Cmd+P → Welcome`

### Primary Actions

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| **Start writing** | Click | Creates project at `~/Documents/Flynt/`, writes Welcome.md, switches to Notes view |
| **Open your notebook** (project exists) | Click | Switches to last used project, navigates to Notes |
| **Sync across devices** | Click | Expands sync options panel |
| **Join a shared project** | Click | Opens clone dialog modal |

### Sync Options (expanded)

#### Cloud Storage (auto-detected, only shows installed providers)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| **iCloud Drive** | Click | Creates project in `~/Library/Mobile Documents/com~apple~CloudDocs/Flynt/`. If already exists, opens it. |
| **Google Drive** | Click | Creates project in Google Drive's local sync folder |
| **Dropbox** | Click | Creates project in `~/Dropbox/Flynt/` |
| **OneDrive** | Click | Creates project in OneDrive's local sync folder |
| No providers detected | | Cloud section hidden entirely. Git section auto-expands with label "Online sync". |

**Error case:** Provider folder missing or permission denied → error logged but no UI feedback. **Known gap.**

#### Git Hosting (collapsed when cloud providers exist, auto-expanded otherwise)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Toggle label | "More options ▾" (with cloud) or "Online sync" (without cloud) | Expands/collapses git section |
| **Connect a notebook** | Click | Opens clone dialog |
| **Codeberg** button | Click | Opens `codeberg.org/repo/create` in default browser |
| **GitHub** button | Click | Opens `github.com/new` in default browser |
| Explainer text | Display | "Don't have an account? Git hosting keeps a complete history..." |

### Clone Dialog (modal)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Notebook URL | Text input | Placeholder: `https://github.com/you/your-project.git`. On blur, auto-fills token from stored credentials if available. |
| Branch | Text input, default "main" | |
| Access token | Password input, optional | "Only needed for private notebooks". Token is persisted to `auth.json` on successful clone. |
| Clone button | Click | Async (`spawn_blocking`), button shows "Cloning..." while working |
| Clone success | | Project opened, navigate to Notes, launcher profile updated |
| Clone failure | | Error inline in dialog: network error, auth failure, path conflict |
| Cancel button | Click | Closes dialog |
| Overlay click | Click | Closes dialog |
| Folder already exists | | Error: "Folder already exists: /path" |

### Advanced Options (collapsed)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| **Open an existing folder** | Click | File picker → project opened with `write_frontmatter: false` |
| **Clone from Git** | Click | Opens clone dialog |
| **Import markdown files** | Click | File picker → copies files into current project |

---

## 2. Notes View

**Route:** `Notes` | **Sidebar:** Notes icon

### Note List (Sidebar)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Note item | Click | Opens in main pane, adds to tab bar |
| Note item | Right-click | Context menu: Rename, Delete |
| Folder group | Click header | Expand/collapse |
| `+` button | Click | Inline new-note input |
| New note input | Enter | Creates `<name>.md`, opens in tab |
| New note input | Escape | Cancels |

**Filtered from sidebar:** `ai/delegations/`, `ai/memory/`, `references/comms/` — these are agent-internal documents, searchable but not shown in the note list.

### Note Editor

| Mode | Description | Limitations |
|------|-------------|-------------|
| **Live** (default) | Rendered markdown with CodeMirror overlay for inline editing | Code blocks, tables, and embeds are not directly editable in-place — click triggers their specific action (open drawing, navigate wikilink). Switch to Source for full editing. |
| **Source** | Raw markdown with syntax highlighting | Full frontmatter visible and editable |

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Mode toggle | Click | Switches between Live and Source |
| Title (top bar) | Double-click | Inline rename, Enter saves, Escape cancels |
| Auto-save | 2s debounce | Saves to disk via JS bridge, re-renders |
| `Cmd+S` | Keydown | Immediate save |
| `![[file.excalidraw]]` | Rendered | Inline SVG from `.svg` sidecar. Click opens Excalidraw editor. If no SVG: "[Drawing: file — open to render]" placeholder, click opens editor. |
| `![[diagram.d2]]` | Rendered | Inline SVG from D2 render. If d2 CLI unavailable: "[Diagram: file — rendering not available]" placeholder. |
| `![[image.png]]` | Rendered | Inline image via `project://localhost/` protocol. Searches root, `assets/`, `images/`, `drawings/`. |
| `[[wikilink]]` | Click | Navigates to linked note, opens in new tab |

### Conflict Resolution Banner

**Trigger:** Document content contains `<<<<<<<` + `=======` + `>>>>>>>` markers (git merge conflict).

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Banner | Display | Yellow bar: "⚠ This file has merge conflicts." |
| Keep mine | Click | Resolves all regions with local ("ours") content, auto-saves, re-renders |
| Keep theirs | Click | Resolves all regions with remote ("theirs") content, auto-saves, re-renders |
| Edit manually | Click | Switches to Source mode — operator edits conflict markers by hand |

**Note:** Resolution operates on the loaded `edit_body` signal. If the file has been modified by sync since loading, the resolution may apply to stale content. Switching to Source mode and back re-reads from disk.

---

## 3. Excalidraw Drawing

**Trigger:** Open a `.excalidraw` wrapper document via sidebar, or `Cmd+Shift+D` (new drawing via menu)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Canvas | Full Excalidraw editor | Dark theme, transparent background |
| Auto-save | 2s interval | Saves `.excalidraw` JSON to disk |
| SVG auto-export | After each save | Writes `.svg` sidecar next to `.excalidraw` file |
| Export SVG button | Click | Manual SVG export, opens parent folder in Finder (macOS) |
| Export PNG button | Click | 2x retina PNG via canvas→blob, opens in Finder |
| Tab bar | Hidden during drawing | Reclaimed for canvas space |
| Navigate away | Close tab or switch view | Tab bar restored, React root unmounted, JS state cleaned up |

**Headless SVG export:** When the agent creates a `.excalidraw` file via MCP tool, the desktop watcher detects the file change and triggers SVG export via the webview's Excalidraw bundle — no editor needs to be open. Concurrent exports are serialized via a JS promise queue.

**Semantic agent authoring:** Agents should prefer the `drawing_*_spec`
tools for architecture and system diagrams. These tools write a
`drawings/<name>.drawing.json` sidecar containing semantic components and
connections, then render that spec deterministically into Excalidraw JSON.
This keeps future edits patchable by component id instead of forcing agents
to generate raw Excalidraw element arrays.

---

## 4. Kanban Board

**Route:** `Kanban` | **Sidebar:** Kanban icon

### Board Tabs

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Board tab | Click | Switches active board |
| `+ New board` | Click | Inline name input, Enter creates with default columns (Backlog/In Progress/Review/Done) |
| Delete button (×) | Click | Confirmation: "Delete this board and its tasks? This cannot be undone." Confirm deletes board + all tasks. Cancel dismisses. |

### Columns

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Column name | Double-click | Inline rename. Enter saves (renames column + updates all tasks). Escape cancels. |
| Column × button | Hover header to reveal | Removes column. **Only visible on empty columns.** |
| `+ Add column` | Click (end of board) | Inline name input. Enter creates, Escape cancels. |
| WIP counter | Display | `count/limit` badge. Turns orange when over limit. |
| Drop zone | Drag card over | Column border highlights with primary color + shadow |

### Task Cards

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Card | Drag | Drag between columns. Drop updates task's column. |
| Priority dot | Display | Colored circle: green (low), amber (medium), red (high), purple (critical) |
| Tags | Display | Small badges below title |
| `+` button | Hover to reveal, click | Expands inline detail editor |
| Detail: Title | Text input | |
| Detail: Description | Textarea | Markdown body |
| Detail: Priority | Select dropdown | Low/Medium/High/Critical |
| Detail: Due date | Date input | |
| Save | Click | Persists all changes |
| Archive | Click | Sets status to Archived, card hidden from column |
| `+ Add task` | Per-column button | Inline title input, Enter creates, Escape cancels |

**Error case:** Empty column name on rename → saves empty string. **Known gap** — no validation.

---

## 5. Graph View

**Route:** `Graph` | **Sidebar:** Graph icon

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Graph | Static SVG | Force-directed layout rendered in pure Rust, displayed as inline SVG |
| Node labels | Always visible | Truncated to 20 chars with ellipsis |
| Filter row | Button per kind | Document, Task, Board, Repo, Link, Communication, Memory, Design Node |
| Node colors | By kind | Document (teal), Task (amber), Board (blue), Repo (violet), Link (gray), Design Node (green), Workspace Lease (purple) |
| Design node status | Icon overlay | seed ◌, exploring ◐, resolved ◉, decided ●, implementing ⚙, implemented ✓ |
| Edge opacity | By kind | Wikilinks 0.4, Tasks 0.3, Semantic 0.2, Dependencies 0.5, Parent-Child 0.6 |

**Note:** The graph is a static render — no hover tooltips, no click-to-navigate, no drag-to-rearrange. It's a structural overview, not an interactive explorer.

---

## 6. Command Palette

### Command Mode (`Cmd+P`)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Input | Type | Fuzzy-searches all commands + note titles |
| Arrow keys | Up/Down | Moves selection highlight, wraps around |
| Enter | Executes selected | Opens note or runs command |
| Escape | Closes palette | |

#### Available Commands

| Command | Category | Action |
|---------|----------|--------|
| Notes / Board / Graph / Settings / Welcome | Navigate | Switch view |
| New Note | Create | Timestamped note, opens in tab |
| New Drawing | Create | Excalidraw `.excalidraw` + wrapper `.md` |
| Insert Drawing Here | Create | Inserts `![[drawing.excalidraw]]` at cursor (Notes view only, requires open note) |
| Today's Note | Create | Daily note from template, or opens existing |
| New from: `<template>` | Template | Creates timestamped note from project template |
| Create Snapshot | Action | **Git sync only.** Auto-commits + creates `snapshot-YYYYMMDD-HHMMSS` tag + pushes tags. **Silently no-ops on non-Git projects.** |
| Sync Now | Action | **Git sync only.** Manual commit + pull + push. |
| Create Project in iCloud | Create | Creates iCloud project + opens new instance |
| `<note title>` | Open | Opens matching note in tab |

### Agent Mode (`Cmd+K`)

**Only available when the agent panel has been opened and an ACP session is active.** If no session exists, `Cmd+K` is silently ignored and the Agent tab is hidden.

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Input | Type | Natural language prompt |
| Enter | Submit | Submits to shared ACP session (fire-and-forget). Shows green "✓ Delegated" for 400ms, then closes palette. |
| Context | Automatic | Active note title + current view injected as prefix: `[Currently viewing: "Title"] [On: Board view]` |
| Audit trail | Automatic | Prompt saved to `ai/delegations/<YYYYMMDD-HHMMSS>.md` |
| No session | | Placeholder: "Agent not connected — open the agent panel first" (input disabled) |

---

## 7. Settings

**Route:** `Settings` | **Sidebar:** Settings icon

### Basic (always visible)

| Section | Field | Type | Notes |
|---------|-------|------|-------|
| **Appearance** | Theme | Card grid | Currently: Alpharius only |
| | Font size | Button group | Small / Medium / Large / XLarge |
| **Project** | Name | Text input | |
| | Location | Read-only path | |
| **Sync** | Backend | Radio group | None / iCloud / Git |
| | Remote (Git) | Text input | Only shown for Git backend |
| | Branch (Git) | Text input | |
| | Auto-commit (Git) | Number input | Seconds, minimum 30, 0 = manual only |

**Sync backend change on Save:** Triggers project migration.
- **None → iCloud:** Copies all project files to iCloud Drive folder, updates config, switches runtime. **Synchronous — UI blocks during copy, no progress indicator.** Large projects may appear to freeze.
- **None → Git:** Stays in current location, initializes git repo + adds remote.
- **Any → None:** Copies to `~/Documents/<project_name>/`, iCloud/git copy remains (not deleted).

### Advanced ("Show advanced settings ▾")

| Section | Field | Type | Notes |
|---------|-------|------|-------|
| **Visualization** | Excalidraw auto-export | Checkbox | SVG sidecar on save |
| | D2 auto-render | Checkbox | SVG via `d2` CLI on change |
| | D2 theme | Number | 200 = dark (Alpharius), 0 = default |
| | D2 layout | Radio | ELK / Dagre / TALA |
| | D2 binary | Text input | Override path if not on PATH |
| **Indexing** | Write frontmatter | Checkbox | Disable for shared repos |
| **Publication** | Default visibility | Radio | Private / Public / Unlisted |
| | Rules | Editor component | Tag/path matching |
| **Local Runtime** | State root | Text path | Must be absolute |
| | Index DB | Text path | |
| | Omegon runtime root | Text path | |
| | Omegon mind DB | Text path | |
| | Styrene Identity profile | Text | |
| | Omegon channel | Radio | Stable / RC / Nightly |
| | Omegon binary | Text | Override path, bypasses channel resolution |
| **Omegon Profile** | Model provider | Text input | e.g. "anthropic" |
| | Model ID | Text input | e.g. "claude-sonnet-4-6" |
| | Thinking level | Select | None / Low / Medium / High |
| | Max turns | Number input | |
| **Identity** | Status | Indicator | Green dot = active, gray = none |
| | Create | Passphrase + confirm | Async (argon2id is slow), "Creating..." state |
| | Unlock | Passphrase | Async, 2s deliberate delay on failure |
| | SSH auth key | Code + copy | For adding to git hosting SSH keys |
| | Git signing key | Code display | Separate key from auth key |
| | Enable git signing | Button | Configures `.git/config` for SSH signing |
| **Providers** | Per-provider row | Status + action | 11 providers: Anthropic, OpenAI API, OpenAI ChatGPT, OpenRouter, Groq, xAI, Mistral, Google, GitHub, Forgejo/Codeberg, GitLab |
| | API key providers | "Add key" → masked input | |
| | OAuth providers | "Login" button | Spawns `omegon auth login <provider>` |
| | All providers | "Remove" button | Clears from auth.json |
| **Operator** | Active persona | Select | Off / Scribe / Omegon |
| | Rail extension | Select | None / Vox / Flynt |
| | Vox enabled | Checkbox | |
| | Vox TTS | Checkbox | |
| | Vox voice | Text input | System TTS voice name |
| **Agent Daemon** | Status | Indicator + text | Disabled (gray) / Stopped / Starting (yellow) / Running (green, port) / Unhealthy (red) |
| | Enable | Checkbox | |
| | Auto-start | Checkbox | Start on app launch |
| | Controls | Buttons | Start / Stop / Restart (disabled based on state) |
| | Model | Text input | |
| | Posture | Radio | Default / Fabricator / Architect / Explorator / Devastator |
| | Persona | Text input | |
| | Port | Number | Default 7842 |
| | Capabilities | Checkbox grid (2-col) | 8 capabilities |
| | Signal | Enable + phone | |
| | Email | Enable + address | |
| | Webhook | Enable + path | |

### Save Bar

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Save changes | Click | Persists to `.flynt/config.toml` + `.flynt/operator-settings.json` + `.omegon/profile.json` |
| Export local preview | Advanced only | Exports publication HTML |
| Success message | Inline green text | "Settings saved" or "Project migrated and sync updated." |
| Error message | Inline red text | Validation failures, save errors |

---

## 8. Toolbar

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Project name | Display | Current project name |
| Build hash | Display | Tiny monospace, click-to-copy version hash |
| Update badge | Button | Appears when GitHub Releases has a newer Flynt version; labels direct/Homebrew/Nix/dev installs, opens the appropriate artifact or release page, and can skip a specific version |
| Sync badge | ✓ (green) | Synced — Git auto-sync idle |
| | ↻ (spinning blue) | Syncing — committing, pulling, or pushing |
| | ⚠ (amber) | Conflict — count shown, tooltip describes resolution |
| | Not shown | No Git sync configured |
| Search input | Type | Live full-text search, results grouped by folder |
| Search result | Click | Opens note in tab |
| Agent toggle | Click | Shows/hides agent rail |

---

## 9. Sidebar — Project Switcher

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Current project | Display | Name + path |
| Other cloned projects | Click | Switches runtime, navigates to Notes |
| Project × button | Hover to reveal | Confirmation with "Your notes are not deleted" hint |
| Uncloned manifest projects | ⤓ icon + role badge | Click clones to device (async) |
| Add project | Click | Inline form: name + repo URL, Enter adds to manifest + clones |
| Open folder | Click | File picker → opens as project |
| Error banner | Red inline | Shows clone/add errors with × dismiss |

**Error case:** "Add project" when no manifest configured → error: "No manifest configured. Connect a manifest first."

---

## 10. Agent Rail (sidebar panel)

**Trigger:** Toolbar agent toggle

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Status badge | connected (green) | ACP session active |
| | connecting (yellow) | Establishing connection |
| | thinking (blue pulsing) | Processing prompt |
| | tool running (blue pulsing) | Executing a tool |
| Omegon not ready | Setup panel | Checks the Omegon binary, ACP session, Flynt extension, and provider auth; offers user-local upstream install, Homebrew install, binary selection, session retry, extension install/enable, provider settings, runtime settings, and recheck actions. |
| Chat messages | User: plain text | Assistant: markdown rendered with smart URL badges |
| Tool calls | Inline blocks | Kind badge + title + status (InProgress/Complete) |
| Slash commands | Type `/` | Popup menu — only shows if agent advertises commands |
| Config bar | If agent provides options | Model/thinking/posture dropdowns — **only shown when advertised by the ACP session** |
| Input | Enter | Sends prompt |
| Input | Shift+Enter | Newline |
| Input | Up/Down arrows | Navigate input history |
| `/login` | Special command | Triggers OAuth login, reconnects session |

---

## 11. Keyboard Shortcuts

| Shortcut | Action | Context |
|----------|--------|---------|
| `Cmd+P` | Command palette (command mode) | Global |
| `Cmd+K` | Command palette (agent mode) | Global, only if agent connected |
| `Cmd+N` | New note | Menu |
| `Cmd+Shift+D` | New drawing | Menu |
| `Cmd+S` | Save current note | Notes view |
| `Cmd+W` | Close current tab | Notes view |
| `Escape` | Close palette / dialog / cancel edit | Context-dependent |
| Up/Down arrows | Navigate palette results / input history | Palette, agent rail |
| Enter | Execute / submit / confirm | Palette, forms |
| Double-click | Rename column header / note title | Kanban, Notes |

---

## 12. Mobile (iOS)

### Onboarding

| Screen | Elements | Notes |
|--------|----------|-------|
| Welcome | "Start writing" / "Sync across devices" / "Join a shared project" | Same tier-based design as desktop |
| Sync choice | "Connect a notebook" / "Connect all my notebooks" | Single project vs manifest |
| Manifest input | URL + token → clone manifest repo | Discovers all projects |
| Project list | Select project → clone | Shows name + role badge |
| Single project | URL + branch + token → clone | |
| Cloning | Spinner: "Setting up..." | Async |
| Done | "You're all set" → "Open project" | |
| Error | Error message + "Start over" | |

### Tab Bar

| Tab | View | Notes |
|-----|------|-------|
| Notes | Note list → tap to view detail, back button | |
| Board | Kanban board (same layout as desktop) | |
| Graph | Force-directed SVG graph (Rust-rendered, same as desktop) | |
| Omegon | Agent chat view | |
| Settings | Project name, sync status, path | **Read-only** — no editable settings on mobile yet |

### Share Extension

| Trigger | Behavior | Expected Result |
|---------|----------|-----------------|
| Share from any app | SwiftUI sheet: edit title | |
| Save | Writes `.md` to App Group inbox (`group.io.styrene.flynt`) | |
| Main app polls | Every 5 seconds | `drain_inbox()` moves `.md` files + assets into project, indexes |

### First Project

| Condition | Behavior |
|-----------|----------|
| Fresh install | Project created at `Documents/Flynt/` |
| No notes (reindex = 0) | Welcome.md auto-created with getting-started content |

---

## 13. File Formats

| File | Format | Location | Notes |
|------|--------|----------|-------|
| Project config | TOML | `.flynt/config.toml` | Survives TestFlight upgrades (inside project) |
| Operator settings | JSON | `.flynt/operator-settings.json` | Daemon config, persona, vox |
| Omegon profile | JSON | `.omegon/profile.json` | Model, thinking level |
| Notes | Markdown + TOML frontmatter (`+++`) | `*.md` anywhere in project | |
| Tasks | Markdown + TOML frontmatter (`kind = "task"`) | Project subdirectories | |
| Drawings | JSON (Excalidraw scene) | `drawings/*.excalidraw` | `.md` wrapper for indexing, `.svg` sidecar for embedding |
| D2 diagrams | D2 source language | `diagrams/*.d2` | `.md` wrapper, `.svg` sidecar |
| Delegations | Markdown | `ai/delegations/*.md` | Hidden from sidebar, searchable |
| Memory facts | Markdown | `ai/memory/**/*.md` | Hidden from sidebar |
| Communications | Markdown | `references/comms/**/*.md` | Hidden from sidebar |
| Project manifest | TOML | `projects.toml` in manifest repo | |
| Local manifest sidecar | TOML | `projects.local.toml` (gitignored) | Device-specific clone paths |
| Launcher profile | JSON | `~/Library/Application Support/flynt/launcher-profile.json` | Known projects, recent, wizard state |
| Auth tokens | JSON | `~/.config/omegon/auth.json` | 0600 permissions, atomic write + lock file |
| Identity | Binary (argon2id + ChaCha20Poly1305) | `~/.config/styrene/identity.key` | 97 bytes, STID magic header |
| SQLite index | SQLite WAL | `.flynt-local/flynt/flynt-index.db` | Ephemeral — rebuilt from project files on reindex |

---

## 14. Sync Behaviors

| Backend | Mechanism | Conflict handling |
|---------|-----------|-------------------|
| **None** | Local only | N/A |
| **iCloud** | macOS filesystem sync | iCloud creates "conflicted copy" files — **Flynt does not detect or resolve these.** Operator must manually reconcile. |
| **Git** | Auto-commit + push/pull on configurable interval (min 30s) | Git merge conflicts produce markers → resolution banner in note view |
| **Google Drive / Dropbox / OneDrive** | Provider's desktop client handles filesystem sync | Provider-specific conflict handling — Flynt treats the folder as local |

### Project Snapshots (Git only)

| Action | Trigger | Result |
|--------|---------|--------|
| Create Snapshot | `Cmd+P → Create Snapshot` | Auto-commits + tags HEAD as `snapshot-YYYYMMDD-HHMMSS` + pushes tags |
| **Non-Git project** | Same command | **Silently no-ops. No error, no feedback.** |

### Project Migration

| Transition | What happens |
|-----------|-------------|
| None → iCloud | Copies all files (excluding `.flynt-local/`, `.git/`) to iCloud Drive. Updates config. Switches runtime. Old copy remains. |
| None → Git | Stays in place. Inits git repo + adds remote. Creates `.gitignore`. |
| iCloud → Git | Stays in iCloud location. Adds git repo on top. |
| Any → None | Copies to `~/Documents/<name>/`. Old location not deleted. |

**Known limitation:** Migration is synchronous in the save handler. No progress indicator. Large projects may freeze the UI for several seconds.

---

## 15. Visualization Pipeline

| Source | Trigger | Renderer | Output | Timeout |
|--------|---------|----------|--------|---------|
| `.excalidraw` | File watcher (create/modify) | Webview Excalidraw bundle (`renderSceneToSvg`) | `.svg` sidecar | None (JS async) |
| `.d2` | File watcher (create/modify) | `d2` CLI with configured theme + layout | `.svg` sidecar | 30 seconds |
| Graph | On-demand (view opened) | Pure Rust force-directed layout | Inline SVG | None |

**D2 CLI not found:** Logs debug message, no user-visible error. Placeholder shown in note embed.

**Concurrent Excalidraw exports:** Serialized via JS promise queue — safe for git pulls that modify multiple `.excalidraw` files simultaneously.

**D2 PATH enrichment:** The render pipeline prepends `/opt/homebrew/bin`, `/usr/local/bin`, Nix paths, `~/.local/bin` to PATH before invoking `d2`, handling GUI apps that inherit a stripped environment.

### Agent-Created Visuals (MCP Tools)

| Tool | Input | Output | Guard |
|------|-------|--------|-------|
| `create_drawing` | Name + optional scene JSON | `drawings/<name>.excalidraw` + `drawings/<name>.md` | Refuses if file exists |
| `drawing_create_spec` | Name + semantic `DrawingSpec` | `.md` wrapper + `.excalidraw` scene + `.drawing.json` sidecar | Refuses if any target exists |
| `drawing_get_spec` | `.excalidraw` path | Semantic sidecar, when present | Returns `null` spec for hand-authored drawings |
| `drawing_render_spec` | `.excalidraw` path + full spec | Replaces scene and sidecar from semantic spec | Requires existing drawing path |
| `drawing_patch_spec` | `.excalidraw` path + component/connection upserts/removes | Re-renders scene from patched semantic spec | Requires existing `.drawing.json` sidecar |
| `drawing_validate_spec` | Semantic spec | Validation warnings | No writes |
| `create_d2_diagram` | Name + D2 source + optional directory | `<dir>/<name>.d2` + `<dir>/<name>.md` | Refuses if file exists |

---

## 16. Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `FLYNT_PROJECT` | Override project root directory (legacy aliases: `FLYNT_VAULT`, `CODEX_VAULT`) | `~/Documents/Flynt` |
| `OMEGON_BIN` | Override Omegon binary path | Channel-resolved from `~/.omegon/versions/`; the agent setup panel can also persist an explicit project-level binary path. |
| `OMEGON_HOME` | Override Omegon home directory | Derived from project config |
| `FLYNT_LAUNCHER_PROFILE` | Override launcher profile path | `~/Library/Application Support/flynt/launcher-profile.json` |
| `OMEGON_AUTH_JSON` | Override auth.json path | `~/.config/omegon/auth.json` |
| `FLYNT_LOCAL_STATE` | Override local state root | `~/.local/share/flynt/` |

---

## 17. Known Gaps

| Area | Gap | Severity |
|------|-----|----------|
| Cloud project creation | No UI error feedback on failure | Low — error logged |
| iCloud conflicts | "Conflicted copy" files not detected | Medium — manual reconciliation needed |
| Project migration | Synchronous, blocks UI, no progress | Medium — large projects freeze |
| Column rename | No empty-name validation | Low — cosmetic |
| Create Snapshot | Silent no-op on non-Git projects | Low — confusing but harmless |
| Mobile settings | Read-only, no editing | Medium — must use desktop for config |
| Delegation files | Accumulate without cleanup | Low — searchable, hidden from sidebar |
| Sync status | Only for Git backend | Low — cloud providers handle their own |
