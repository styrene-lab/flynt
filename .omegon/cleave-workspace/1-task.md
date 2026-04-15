---
task_id: 1
label: track-b-ui-shell
siblings: [0:track-a-launch, 2:track-c-git-sync]
---

# Task 1: track-b-ui-shell

## Root Directive

> Implement three parallel tracks for Codex: (A) Vault launch integration — Dioxus.toml, full macOS main.rs mirroring auspex, Vault::open reindex + FSEvents watcher feeding a tokio broadcast channel for signal invalidation; (B) UI shell — three-column layout sidebar/main/agent-rail, route enum, Notes/Kanban/Graph/Settings stub views wired to routing; (C) Git sync backend — git2-backed SyncBackend impl: auto-commit debounced 30s, pull, conflict detection stored in DB conflicts table.

## Mission

Implement the three-column UI shell for codex-app. Tasks: (1) Define Route enum in crates/codex-app/src/state.rs: Notes, Kanban, Graph, Settings. (2) Rewrite crates/codex-app/src/app.rs: top-level App component renders a three-column layout — left sidebar (220px fixed), center main-content (flex-grow), right agent-rail (320px, hidden by default, toggled by show_agent signal). Use Dioxus signals for active_route: Signal<Route> and show_agent: Signal<bool>. (3) Create crates/codex-app/src/components/sidebar.rs: file tree section listing document metas, boards section listing board names, nav icons for Notes/Kanban/Graph/Settings. Clicking items sets active_route signal. (4) Create crates/codex-app/src/components/agent_rail.rs: right panel stub with header 'Omegon' and a textarea for chat input placeholder. Toggled by Cmd+Shift+A (keyboard shortcut via onkeydown on document). (5) Create crates/codex-app/src/components/toolbar.rs: top bar with app name, sync status indicator (Idle/Syncing/Conflict), and agent toggle button. (6) Flesh out crates/codex-app/src/views/notes.rs, kanban.rs, and add graph.rs and settings.rs stubs. Each view gets a distinct placeholder with correct CSS class. (7) Update crates/codex-app/src/lib.rs to export all new modules.

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

**Depends on:** track-a-launch

## Siblings

- **track-a-launch**: Implement vault launch integration for codex-app. Tasks: (1) Create Dioxus.toml at workspace root with bundle identifier com.black-meridian.codex, macOS category productivity. (2) Rewrite crates/codex-app/src/main.rs to match auspex pattern: muda menu (Codex menu with Settings + Quit, Edit menu with standard items), tao macOS window extensions (titlebar_transparent=false, fullsize_content_view=false), window size 1440x900 min 900x600, embed assets/main.css via include_str! in custom_head, LaunchBuilder::desktop().with_cfg(...).with_context(bootstrap).launch(app::App). (3) Create crates/codex-app/src/bootstrap.rs: reads CODEX_VAULT env var or defaults to ~/Documents/Codex, calls Vault::open, runs reindex, spawns a tokio task that loops on VaultWatcher::rx and sends on a tokio::sync::broadcast::Sender<VaultEvent> (re-exported from codex-store). (4) Add broadcast channel re-export to codex-store/src/lib.rs. (5) Create assets/main.css with minimal dark theme stub matching Alpharius palette (#06080e background, #c4d8e4 foreground, #2ab4c8 accent). (6) Cargo.toml for codex-app: add dioxus desktop feature flag.
- **track-c-git-sync**: Implement the git2-backed SyncBackend for codex-store. Tasks: (1) Add git2 dependency to crates/codex-store/Cargo.toml. (2) Create crates/codex-store/src/sync/mod.rs re-exporting git module. (3) Create crates/codex-store/src/sync/git.rs implementing the SyncBackend trait from codex-core: struct GitSync { vault_root: PathBuf, remote: String, branch: String }. Implement: status() checks for uncommitted changes and unpushed commits using git2::Repository::open; pull() does fetch + merge (fast-forward only, conflict detection on non-FF); push() does push to remote with stored credentials (ssh-agent or credential helper via git2); sync() calls pull then push; auto_commit(message) stages all changes (git add -A equivalent via git2 index) and commits with author Codex <codex@local> and provided message. (4) Add a conflicts table to the SQLite SCHEMA in sqlite.rs: id TEXT PK, path TEXT, ours TEXT, theirs TEXT, base TEXT, detected_at TEXT. (5) Implement conflict detection: when merge is not fast-forward, detect conflicted files via git2 index entries with CONFLICTED flag, insert rows into conflicts table, return SyncResult with conflicts vec. (6) Add get_conflicts and resolve_conflict methods to SqliteStore (resolve deletes the row). (7) Update codex-store/src/lib.rs to pub mod sync.

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
