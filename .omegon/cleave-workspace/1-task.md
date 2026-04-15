---
task_id: 1
label: track-b-ui-shell
siblings: [0:track-a-launch, 2:track-c-git-sync]
---

# Task 1: track-b-ui-shell

## Root Directive

> Implement three parallel tracks for Codex: (A) Vault launch integration — Dioxus.toml, full macOS main.rs mirroring auspex, Vault::open reindex + FSEvents watcher feeding a tokio broadcast channel for signal invalidation; (B) UI shell — three-column layout sidebar/main/agent-rail, route enum, Notes/Kanban/Graph/Settings stub views wired to routing; (C) Git sync backend — git2-backed SyncBackend impl: auto-commit debounced 30s, pull, conflict detection stored in DB conflicts table.

## Mission

Implement the three-column UI shell for codex-app. IMPORTANT: Do not touch main.rs or bootstrap.rs — those are owned by track-a-launch. Work only on the files in scope. Tasks: (1) Rewrite crates/codex-app/src/state.rs: define Route enum (Notes, Kanban, Graph, Settings) deriving Clone+PartialEq+Debug+Default(Notes). Define SyncStatus enum (Idle, Syncing, Conflict(usize)). Keep AppContext import as 'use crate::bootstrap::AppContext' but use a placeholder type alias if bootstrap is not yet complete — the important thing is the state compiles. (2) Rewrite crates/codex-app/src/app.rs: App component uses use_signal for active_route: Signal<Route> and show_agent: Signal<bool>. Renders div.codex-shell containing: Toolbar component at top, div.codex-body containing Sidebar on left (w-220), div.main-content in center, and conditionally AgentRail on right when show_agent is true. Match on active_route to render NotesView / KanbanView / GraphView / SettingsView in the center pane. (3) Write crates/codex-app/src/components/sidebar.rs: Sidebar component renders nav.sidebar with two sections: section.sidebar-notes (heading Notes, placeholder list) and section.sidebar-boards (heading Boards, placeholder list). Bottom icons row for route navigation using emoji or text labels. (4) Write crates/codex-app/src/components/toolbar.rs: Toolbar renders div.toolbar with span.app-title Codex, div.sync-status showing Idle/Syncing/Conflict text, button.agent-toggle text Agent toggling show_agent signal passed as prop. (5) Write crates/codex-app/src/components/agent_rail.rs: AgentRail renders div.agent-rail with header Omegon, div.agent-messages placeholder, div.agent-input with textarea and send button. (6) Write crates/codex-app/src/views/notes.rs full stub, views/kanban.rs full stub, views/graph.rs full stub, views/settings.rs full stub — each a #[component] returning a named div with class and a placeholder heading. (7) Update crates/codex-app/src/components/mod.rs and views/mod.rs to export all modules. Update lib.rs.

## Scope

- `crates/codex-app/src/app.rs`
- `crates/codex-app/src/state.rs`
- `crates/codex-app/src/lib.rs`
- `crates/codex-app/src/components/mod.rs`
- `crates/codex-app/src/components/sidebar.rs`
- `crates/codex-app/src/components/toolbar.rs`
- `crates/codex-app/src/components/agent_rail.rs`
- `crates/codex-app/src/views/mod.rs`
- `crates/codex-app/src/views/notes.rs`
- `crates/codex-app/src/views/kanban.rs`
- `crates/codex-app/src/views/graph.rs`
- `crates/codex-app/src/views/settings.rs`

**Depends on:** none (independent)

## Siblings

- **track-a-launch**: Implement vault launch integration for codex-app. Tasks: (1) Write Dioxus.toml at workspace root with bundle identifier com.black-meridian.codex, macOS category public.app-category.productivity, publisher Black Meridian. (2) Rewrite crates/codex-app/src/main.rs to match auspex pattern exactly: muda menu (Codex menu with Settings... Cmd+, and Quit, Edit menu with undo/redo/cut/copy/paste/select-all predefined items), tao macOS window extensions with_titlebar_transparent(false) with_fullsize_content_view(false), window size LogicalSize 1440x900 min 900x600, embed assets/main.css via include_str! in custom_head string, LaunchBuilder::desktop().with_cfg(Config::new().with_menu().with_window().with_custom_head().with_on_window()).with_context(bootstrap).launch(app::App). (3) Write crates/codex-app/src/bootstrap.rs: pub struct AppContext { pub vault: Arc<Vault>, pub vault_events: broadcast::Sender<VaultChangeEvent> }. Function bootstrap_from_env() reads CODEX_VAULT env var or defaults to ~/Documents/Codex, creates dir if missing, calls Vault::open, runs vault.reindex() logging results, creates broadcast::channel(256), spawns tokio::task that loops on VaultWatcher::new(&vault.root) receiving events and forwarding on sender, returns AppContext. (4) Add VaultChangeEvent enum to codex-store/src/watcher.rs: variants FileModified(PathBuf), FileCreated(PathBuf), FileDeleted(PathBuf). (5) Write assets/main.css: CSS variables and base styles using Alpharius palette. --bg: #06080e, --card-bg: #0e1622, --fg: #c4d8e4, --accent: #2ab4c8, --border: #1a3448. Style body, .codex-root, .sidebar, .main-content, .agent-rail, .toolbar with these vars. Dark scrollbars. Monospace font stack. (6) Update crates/codex-app/Cargo.toml to add dioxus with desktop feature, tokio with full features. (7) Update crates/codex-app/src/lib.rs to pub mod bootstrap.
- **track-c-git-sync**: Implement the git2-backed SyncBackend for codex-store. Do not touch any codex-app files. Tasks: (1) Add git2 = { version = '0.20', default-features = false } to crates/codex-store/Cargo.toml. (2) Write crates/codex-store/src/sync/mod.rs: pub mod git; pub use git::GitSync;. (3) Write crates/codex-store/src/sync/git.rs: use git2::Repository; pub struct GitSync { pub vault_root: PathBuf, pub remote: String, pub branch: String }. Implement codex_core::sync::SyncBackend: name() returns 'git'. status() opens repo, checks if HEAD is ahead of remote branch (unpushed commits) and if working tree is dirty (uncommitted changes) — returns SyncStatus::Idle if clean and up to date. pull() does: open repo, find remote, fetch with empty refspecs (fetch all), find remote branch ref, do merge analysis, if FastForward checkout the new tree and set HEAD, if Normal (non-FF) detect conflicted index entries (entries with CONFLICTED stage), return SyncResult with conflict paths. push() does: open repo, find remote, push current branch refspec, return SyncResult. sync() calls pull() then if no conflicts calls push(). auto_commit(message: &str) does: open repo, get index, add_all ['.'] callback, write tree, create commit with signature 'Codex <codex@local>', parent is current HEAD if exists. (4) Add conflicts table to SCHEMA in crates/codex-store/src/sqlite.rs: CREATE TABLE IF NOT EXISTS conflicts (id TEXT PRIMARY KEY, path TEXT NOT NULL, ours TEXT NOT NULL, theirs TEXT NOT NULL, base TEXT NOT NULL DEFAULT '', detected_at TEXT NOT NULL). (5) Add get_conflicts() -> Result<Vec<ConflictRecord>> and resolve_conflict(id: &str) -> Result<()> methods to SqliteStore. Add pub struct ConflictRecord { pub id: String, pub path: String, pub ours: String, pub theirs: String, pub detected_at: DateTime<Utc> } in a new file crates/codex-store/src/conflicts.rs. (6) Update crates/codex-store/src/lib.rs: pub mod sync; pub mod conflicts;

## Dependency Versions

Use these exact versions — do not rely on training data for API shapes:

```toml
# crates/codex-app/Cargo.toml
[dependencies]
codex-core = { workspace = true }
codex-store = { workspace = true }
codex-agent = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
dioxus = { workspace = true }

```



## Project Guardrails

Before reporting success, run these deterministic checks and fix any failures:

1. **clippy**: `cargo clippy -- -D warnings`

Include command output in the Verification section. If any check fails, fix the errors before completing your task.

## Testing Requirements

### Test Convention

Write tests as #[test] functions in the same file or a tests submodule


## Contract

1. Only work on files within your scope
2. Follow the Testing Requirements section above
3. If the task is too complex, set status to NEEDS_DECOMPOSITION

## Finalization (REQUIRED before completion)

You MUST complete these steps before finishing:

1. Run all guardrail checks listed above and fix failures
2. Commit your in-scope work with a clean git state when you are done
3. Commit with a clear message: `git commit -m "feat(<label>): <summary>"`
4. Verify clean state: `git status` should show nothing to commit

Do NOT edit `.cleave-prompt.md` or any task/result metadata files. Those are orchestrator-owned and may be ignored by git.
Return your completion summary in your normal final response instead of modifying the prompt file.

> ⚠️ Uncommitted work will be lost. The orchestrator merges from your branch's commits.

## Result

**Status:** PENDING

**Summary:**

**Artifacts:**

**Decisions Made:**

**Assumptions:**
