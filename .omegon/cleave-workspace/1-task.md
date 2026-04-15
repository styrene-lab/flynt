---
task_id: 1
label: track-b-notes-editor
siblings: [0:track-a-agent-binary, 2:track-c-toolbar-search-sidebar-boards]
---

# Task 1: track-b-notes-editor

## Root Directive

> Implement the remaining structural layers to make Codex UI stable: agent binary, notes write mode, toolbar search, sidebar boards + live refresh, mcp.json bootstrap.

## Mission

Add a proper edit/preview mode to the notes view. In crates/codex-store/src/vault.rs, add a pub fn save_document_content(rel_path: &Path, content: &str) -> Result<()> method that writes the file to disk (preserving existing frontmatter UUID) and calls self.index_file on it. In crates/codex-app/src/views/notes.rs, add an edit mode toggle: when a document is selected, show an Edit button in the top-right of the notes pane. In edit mode, show a full-height textarea with the raw markdown content (not the rendered HTML). Show a Save button and a Preview button to toggle back. On Save, call vault.save_document_content via spawn_blocking and refresh the rendered preview. Also add a 'New Note' button to the sidebar (crates/codex-app/src/components/sidebar.rs) that creates a new .md file in the vault root and opens it for editing. The sidebar's document list must also subscribe to vault_events (the broadcast::Sender in AppContext) using use_resource with a refresh counter that increments when any VaultChangeEvent arrives. All new code must compile with cargo check.

## Scope

- `crates/codex-store/src/vault.rs`
- `crates/codex-app/src/views/notes.rs`
- `crates/codex-app/src/components/sidebar.rs`
- `crates/codex-app/assets/styles/markdown.css`

**Depends on:** none (independent)

## Siblings

- **track-a-agent-binary**: Wire the codex-agent into a real executable. Add [[bin]] to crates/codex-agent/Cargo.toml with a main.rs that uses clap to accept --vault <path> and calls run_mcp_server. Add a tracing_subscriber setup. Also update crates/codex-app/src/bootstrap.rs to write an mcp.json file at launch time pointing to the agent binary location so Omegon can discover it. The mcp.json should be written to the vault root's .codex/ directory as mcp.json with a 'codex' server entry using the command transport. Also add the 'clap' crate with derive feature to codex-agent's Cargo.toml. The agent binary must compile cleanly with cargo check.
- **track-c-toolbar-search-sidebar-boards**: Improve the toolbar and sidebar. In crates/codex-app/src/components/toolbar.rs: add a centered search input that searches the vault using vault.store.search_documents. The search results should be displayed as an overlay dropdown list (div absolutely positioned below the input) showing document title + excerpt, clicking a result sets the selected_doc signal and navigates to Notes view. Pass selected_doc: Signal<Option<DocumentId>> and active_route: Signal<Route> as props to Toolbar. Thread these props through from app.rs. In crates/codex-app/src/components/sidebar.rs: add a 'Boards' section below the Notes section that lists all boards from vault.store.list_boards(), clicking a board sets the active_route to Route::Kanban. Also subscribe to vault_events broadcast to auto-refresh the document list: use tokio::sync::broadcast and a use_resource with a refresh counter. In crates/codex-app/assets/styles/components.css: add toolbar search input styles (.toolbar-search, .search-overlay, .search-result-item). Update crates/codex-app/src/app.rs to pass the new props to Toolbar. All code must compile with cargo check.

## Dependency Versions

Use these exact versions — do not rely on training data for API shapes:

```toml
# crates/codex-store/Cargo.toml
[dependencies]
codex-core = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
rusqlite = { workspace = true }
notify = { workspace = true }
tracing = { workspace = true }
git2 = { version = "0.20", default-features = false }
walkdir = "2"

```

```toml
# crates/codex-app/Cargo.toml
[dependencies]
codex-core  = { workspace = true }
codex-store = { workspace = true }
codex-agent = { workspace = true }
serde       = { workspace = true }
serde_json  = { workspace = true }
anyhow      = { workspace = true }
thiserror   = { workspace = true }
tokio       = { workspace = true }
tracing     = { workspace = true }
tracing-subscriber = { workspace = true }
chrono     = { workspace = true }
comrak      = { workspace = true }
dirs        = "5"
dioxus      = { workspace = true, features = ["desktop"] }

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
