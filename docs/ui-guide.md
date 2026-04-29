# Codyx UI Guide

> Interface map for QA, user documentation, and feature verification.
> Version: 0.6.2

---

## 1. First Launch (Welcome Screen)

**Route:** `Welcome` | **Trigger:** No vault configured, or `Cmd+P → Welcome`

### Primary Actions

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| **Start writing** | Click | Creates vault at `~/Documents/Codyx/`, writes Welcome.md, switches to Notes view |
| **Start writing** (vault exists) | Click | Label changes to "Open your notebook", switches to existing vault |
| **Sync across devices** | Click | Expands sync options panel below |
| **Join a shared vault** | Click | Opens clone dialog modal |

### Sync Options (expanded)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| **Cloud storage grid** | Auto-detected | Shows only installed providers (iCloud/Google Drive/Dropbox/OneDrive) |
| Cloud provider button | Click | Creates vault in provider's sync folder, switches to Notes. If vault exists, opens it. |
| **More options ▾** | Click | Expands git hosting section (auto-expanded if no cloud providers detected) |
| **Connect a notebook** | Click | Opens clone dialog |
| **Codeberg** button | Click | Opens `codeberg.org/repo/create` in browser |
| **GitHub** button | Click | Opens `github.com/new` in browser |

### Clone Dialog (modal)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Notebook URL | Text input | Accepts any git URL (HTTPS or SSH) |
| Branch | Text input, default "main" | Sets the branch to clone |
| Access token | Password input, optional | Used for HTTPS auth if provided |
| Clone button | Click | Async clone with spinner ("Cloning..."), switches to Notes on success |
| Cancel button | Click | Closes dialog |
| Overlay click | Click | Closes dialog |
| Error | Clone failure | Error message shown inline in dialog |

### Advanced Options (collapsed)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| **Open an existing folder** | Click | File picker → opens folder as vault (no frontmatter writing) |
| **Clone from Git** | Click | Opens clone dialog |
| **Import markdown files** | Click | File picker → copies files into vault |

---

## 2. Notes View

**Route:** `Notes` | **Hotkey:** Sidebar "Notes" button

### Note List (Sidebar)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Note item | Click | Opens note in main pane, adds to tab bar |
| Note item | Right-click | Context menu: Rename, Delete |
| Folder group | Click header | Expand/collapse folder |
| `+` button | Click | Opens inline new-note input |
| New note input | Enter | Creates `<name>.md`, opens in tab |
| New note input | Escape | Cancels |
| **Filtered out:** | | `ai/delegations/`, `ai/memory/`, `references/comms/` not shown |

### Note Editor

| Mode | Description |
|------|-------------|
| **Live** (default) | Rendered markdown with CodeMirror inline editing. Click text to edit, changes auto-save. |
| **Source** | Raw markdown editor with syntax highlighting. Full frontmatter visible. |

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Mode toggle (top bar) | Click | Switches between Live and Source |
| Title (top bar) | Double-click | Inline rename |
| Auto-save | 2s debounce | Saves to disk, re-renders |
| `Cmd+S` | Keydown | Immediate save |
| `![[file.excalidraw]]` | Rendered | Inline SVG from sidecar, clickable to open editor |
| `![[diagram.d2]]` | Rendered | Inline SVG from D2 render, placeholder if d2 CLI unavailable |
| `![[image.png]]` | Rendered | Inline image via `vault://` protocol |
| `[[wikilink]]` | Click | Navigates to linked note |
| Conflict markers | Detected | Yellow banner: "Keep mine" / "Keep theirs" / "Edit manually" |

### Conflict Resolution Banner

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Keep mine | Click | Resolves all conflicts with local changes, saves, re-renders |
| Keep theirs | Click | Resolves all conflicts with remote changes, saves, re-renders |
| Edit manually | Click | Switches to Source mode for manual editing |

---

## 3. Excalidraw Drawing

**Trigger:** Open a `.excalidraw` wrapper document, or `Cmd+Shift+D`

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Canvas | Draw | Full Excalidraw editor, dark theme |
| Auto-save | 2s interval | Saves `.excalidraw` JSON to disk |
| SVG auto-export | After save | Writes `.svg` sidecar for inline embedding |
| Export SVG button | Click | Exports SVG, opens in Finder |
| Export PNG button | Click | Exports 2x PNG via canvas, opens in Finder |
| Tab bar | Hidden | Reclaimed for canvas space during drawing |
| Navigate away | | Tab bar restored, React root unmounted |

---

## 4. Kanban Board

**Route:** `Kanban` | **Hotkey:** Sidebar "Kanban" button

### Board Tabs

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Board tab | Click | Switches to that board |
| `+ New board` | Click | Inline name input, Enter creates board |
| Delete button (×) | Click | "Delete this board and its tasks? This cannot be undone." Confirm/Cancel |

### Columns

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Column header | Double-click | Inline rename, Enter saves, Escape cancels |
| Column × button | Hover to show, click | Removes column (only if empty) |
| `+ Add column` | Click | Inline name input at end of board |
| WIP counter | Display | Shows `count/limit`, turns orange when over |

### Task Cards

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Card | Drag | Drag between columns, drop zone highlights |
| Card `+` button | Hover to show, click | Expands detail editor |
| Detail editor | Fields | Title, Description, Priority (dropdown), Due date (date picker) |
| Save | Click | Persists changes |
| Archive | Click | Moves to archived status, hidden from column |
| `+ Add task` | Click per column | Inline title input, Enter creates task |

---

## 5. Graph View

**Route:** `Graph` | **Hotkey:** Sidebar "Graph" button

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Graph canvas | Display | Force-directed SVG layout of all vault nodes |
| Node | Hover | Shows title label |
| Filter buttons | Click | Filter by kind: Document, Task, Board, Repo, etc. |
| Node colors | | Document (teal), Task (amber), Board (blue), Repo (violet), etc. |
| Design nodes | | Status icon overlay (seed ◌, implementing ⚙, implemented ✓) |

---

## 6. Command Palette

### Command Mode (`Cmd+P`)

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Input | Type | Fuzzy-searches commands + note titles |
| Arrow keys | Navigate | Highlight moves through results |
| Enter | Select | Executes command or opens note |
| Escape | Close | Closes palette |
| Categories | Display | Navigate, Create, Template, Tag, Open, Action |

### Key Commands

| Command | Category | Action |
|---------|----------|--------|
| Notes / Board / Graph / Settings / Welcome | Navigate | Switch route |
| New Note | Create | Timestamped note |
| New Drawing | Create | Excalidraw + wrapper |
| Insert Drawing Here | Create | Embeds `![[drawing.excalidraw]]` at cursor |
| Today's Note | Create | Daily note from template |
| New from: `<template>` | Template | Creates note from template |
| Create Snapshot | Action | Auto-commits + git tags HEAD |
| Sync Now | Action | Manual git commit + push/pull |
| `<note title>` | Open | Opens note in tab |

### Agent Mode (`Cmd+K`)

**Only available when agent is connected (agent panel has been opened).**

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Agent tab | Hidden | Not shown until ACP session exists |
| Input | Type | Natural language delegation prompt |
| Enter | Submit | Fire-and-forget: submits to shared ACP session, shows green "Delegated" for 400ms, closes |
| Context injection | Automatic | Active note title + current view prepended to prompt |
| Audit trail | Automatic | Delegation saved to `ai/delegations/<timestamp>.md` |

---

## 7. Settings

**Route:** `Settings` | **Hotkey:** Sidebar "Settings" button

### Basic (always visible)

| Section | Fields |
|---------|--------|
| **Appearance** | Theme (Alpharius), Font size (S/M/L/XL) |
| **Vault** | Name (editable), Location (read-only) |
| **Sync** | Backend dropdown (None/iCloud/Git/S3/Forge), Git: remote + branch + auto-commit interval |

**Sync backend change** triggers vault migration:
- None → iCloud: copies vault to iCloud Drive, switches runtime
- None → Git: inits git repo at current location
- iCloud → Git: adds git to iCloud location

### Advanced (collapsed, "Show advanced settings ▾")

| Section | Fields |
|---------|--------|
| **Visualization** | Excalidraw auto-export toggle, D2 auto-render toggle, D2 theme (number), D2 layout (ELK/Dagre/TALA), D2 binary path |
| **Indexing** | Write frontmatter toggle (disable for shared repos) |
| **Publication** | Default visibility (Private/Public/Unlisted), rules editor |
| **Local Runtime** | State root, Index DB, Omegon runtime root, Omegon mind DB, Styrene Identity profile, Omegon channel (Stable/RC/Nightly), Omegon binary override |
| **Omegon Profile** | Model provider, Model ID, Thinking level (None/Low/Medium/High), Max turns |
| **Identity** | Status (active/none), Create (passphrase + confirm), Unlock (passphrase), SSH auth key (copy), Git signing key (copy), Enable git signing button |
| **Providers** | Status per provider (Anthropic, OpenAI, OpenRouter, Groq, xAI, Mistral, Google, GitHub, Forgejo/Codeberg, GitLab). API key entry or OAuth login. Remove credentials. |
| **Operator** | Active persona (Off/Scribe/Omegon), Rail extension (None/Vox/Codex), Vox enabled, Vox TTS, Vox voice |
| **Agent Daemon** | Status indicator, Enable/Auto-start, Start/Stop/Restart, Model, Posture (Default/Fabricator/Architect/Explorator/Devastator), Persona, Port, Capabilities (8 checkboxes), Vox channels (Signal/Email/Webhook enable + detail) |

### Save Bar

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Save changes | Click | Persists all config to `.codex/config.toml` + operator settings. If sync changed, triggers migration. |
| Export local preview | Click (advanced only) | Exports publication to local directory |
| Success/error message | Inline | Green "ok" or red error text |

---

## 8. Toolbar

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Vault name | Display | Shows current vault name |
| Build hash | Display | Tiny monospace hash for version identification |
| Sync badge | Display | ✓ green (synced), ↻ spinning (syncing), ⚠ amber (conflict with count) |
| Search input | Type | Live search across vault, results grouped by folder |
| Search result | Click | Opens note in tab |
| Agent toggle | Click | Shows/hides agent rail panel |

---

## 9. Sidebar — Vault Switcher

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Current vault | Display | Name + path |
| Other vaults | Click | Switches runtime to that vault |
| Vault × button | Hover to show | "Your notes are not deleted" + Remove/Cancel |
| Uncloned manifest vaults | Click (⤓ icon) | Clones vault to device |
| Add vault | Click | Inline form: name + repo URL, creates in manifest + clones |
| Open folder | Click | File picker → opens folder as vault |
| Error banner | Display | Shows clone/add errors with dismiss button |

---

## 10. Agent Rail (sidebar panel)

**Trigger:** Toolbar agent toggle, or `View > Agent` menu

| Element | Behavior | Expected Result |
|---------|----------|-----------------|
| Status badge | Display | connected (green), connecting (yellow), thinking/tool running (blue pulsing) |
| Chat messages | Display | User prompts + assistant responses (markdown rendered) |
| Tool calls | Display | Kind badge + title + status |
| Slash commands | Type `/` | Popup menu of available commands |
| Config options | Dropdown | Model, thinking level, posture — persisted to operator settings |
| Input | Enter | Sends prompt to agent via ACP |
| Input | Up/Down arrows | Navigate input history |

---

## 11. Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd+P` | Command palette (command mode) |
| `Cmd+K` | Command palette (agent mode, if connected) |
| `Cmd+N` | New note |
| `Cmd+Shift+D` | New drawing |
| `Cmd+S` | Save current note |
| `Cmd+W` | Close current tab |

---

## 12. Mobile (iOS)

### Onboarding

| Screen | Elements |
|--------|----------|
| Welcome | "Start writing" / "Sync across devices" / "Join a shared vault" |
| Sync choice | "Connect a notebook" / "Connect all my notebooks" |
| Manifest input | URL + token → discovers vaults |
| Vault list | Select vault from manifest → clone |
| Single vault | URL + branch + token → clone |
| Cloning | Spinner |
| Done | "Open vault" |

### Tab Bar

| Tab | View |
|-----|------|
| Notes | Note list + detail view |
| Board | Kanban board |
| Graph | Force-directed graph (JS-rendered) |
| Omegon | Agent chat |
| Settings | Vault name, sync status, path (read-only) |

### Share Extension

| Trigger | Behavior |
|---------|----------|
| Share from any app | Opens share sheet with title field |
| Save | Writes `.md` to App Group inbox |
| Main app | Drains inbox every 5 seconds, indexes new notes |

---

## 13. File Formats

| File | Format | Location |
|------|--------|----------|
| Vault config | TOML | `.codex/config.toml` |
| Operator settings | JSON | `.codex/operator-settings.json` |
| Omegon profile | JSON | `.omegon/profile.json` |
| Notes | Markdown + TOML frontmatter (`+++`) | `*.md` anywhere in vault |
| Tasks | Markdown + TOML frontmatter (`kind = "task"`) | Project subdirectories |
| Drawings | JSON | `drawings/*.excalidraw` + `drawings/*.md` wrapper |
| D2 diagrams | D2 source | `diagrams/*.d2` + `diagrams/*.md` wrapper |
| Vault manifest | TOML | `vaults.toml` in manifest repo |
| Local manifest sidecar | TOML | `vaults.local.toml` (gitignored) |
| Launcher profile | JSON | `~/Library/Application Support/codex/launcher-profile.json` |
| Auth tokens | JSON | `~/.config/omegon/auth.json` (0600 permissions) |
| Identity | Binary (argon2id + ChaCha20Poly1305) | `~/.styrene/identity.key` |
| SQLite index | SQLite WAL | `.codex-local/codex/codex-index.db` |

---

## 14. Sync Behaviors

| Backend | Mechanism | Conflict handling |
|---------|-----------|-------------------|
| None | Local only | N/A |
| iCloud | Filesystem (macOS handles sync) | iCloud conflict copies |
| Git | Auto-commit + push/pull on interval | Merge conflict markers → resolution banner |
| Google Drive / Dropbox / OneDrive | Filesystem (provider client handles sync) | Provider's conflict handling |

### Vault Snapshots (Git only)

| Action | Trigger | Result |
|--------|---------|--------|
| Create Snapshot | `Cmd+P → Create Snapshot` | Auto-commits all changes, creates `snapshot-YYYYMMDD-HHMMSS` tag, pushes tag |

---

## 15. Visualization Pipeline

| Source | Render trigger | Output | Inline embed |
|--------|---------------|--------|--------------|
| `.excalidraw` | File change detected by watcher | SVG via webview Excalidraw bundle | `![[name.excalidraw]]` |
| `.d2` | File change detected by watcher | SVG via `d2` CLI (30s timeout) | `![[name.d2]]` |
| Graph | On-demand | Pure Rust force-directed SVG | Graph view |

### Agent-Created Visuals

| MCP Tool | Input | Output |
|----------|-------|--------|
| `create_drawing` | Name + optional scene JSON | `.excalidraw` + `.md` wrapper (refuses overwrite) |
| `create_d2_diagram` | Name + D2 source | `.d2` + `.md` wrapper (refuses overwrite) |
