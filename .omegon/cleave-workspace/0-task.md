---
task_id: 0
label: tabs
siblings: [1:search]
---

# Task 0: tabs

## Root Directive

> Implement multi-tab editor support and full-page search view for Codex

## Mission

Implement multi-tab editor support. In crates/codex-app/src/state.rs: add a TabState struct with fields open_tabs: Vec<DocumentId> and active: usize, derive Clone+PartialEq. Add impl TabState with open(id: DocumentId) method that appends if not present and sets active index, and close(idx: usize) that removes the tab and adjusts active. In crates/codex-app/src/app.rs: replace selected_doc: Signal<Option<DocumentId>> with let tab_state = use_context_provider(|| TabState::default()); keep active_route signal. Pass tab_state as context — any component calls use_context::<Signal<TabState>>() to open docs. In crates/codex-app/src/components/ create tab_bar.rs: TabBar component reads tab_state context, renders a horizontal strip of tabs (each showing doc title from the vault store, a close button). Active tab is highlighted. Clicking a tab sets active index. Closing removes it. Add TabBar to mod.rs exports. In crates/codex-app/src/app.rs: render TabBar above the main content match block. In crates/codex-app/src/views/notes.rs: change NotesView to not take selected_doc prop — instead read active tab from use_context::<Signal<TabState>>(). Update crates/codex-app/src/components/sidebar.rs DocItem to use tab_state context to open docs instead of writing to selected_doc. Add CSS for tab bar in a new crates/codex-app/assets/styles/tabs.css (tabs strip, active/hover/close button styles using CSS vars). Add the stylesheet to app.rs. All code must compile with cargo check.

## Scope

- `crates/codex-app/src/state.rs`
- `crates/codex-app/src/app.rs`
- `crates/codex-app/src/components/tab_bar.rs`
- `crates/codex-app/src/components/mod.rs`
- `crates/codex-app/src/views/notes.rs`
- `crates/codex-app/src/components/sidebar.rs`
- `crates/codex-app/assets/styles/tabs.css`

**Depends on:** none (independent)

## Siblings

- **search**: Implement full-page search view with rich results. In crates/codex-app/src/state.rs: add Route::Search to the Route enum (no data — the query lives in a signal). In crates/codex-app/src/views/ create search.rs: SearchView component. It takes a search_query: Signal<String> prop. Uses use_resource to call vault.store.search_documents on query changes (reactive dep). Results are grouped by path folder prefix using BTreeMap. Render each group as a collapsible section showing folder name + match count. Each result shows: doc title (bold), path (muted), excerpt text with the query term wrapped in <mark> tags for highlighting (use string replace on excerpt, case-insensitive). Clicking a result opens the doc — write to tab_state context (from track 'tabs'). If tab_state is not yet available (this track runs in parallel), just write to a selected_doc Signal<Option<DocumentId>> prop as fallback. In crates/codex-app/src/components/toolbar.rs: add onkeydown to the search input that on Key::Enter sets active_route to Route::Search. Pass active_route as prop (it already is). In crates/codex-app/src/views/mod.rs: export SearchView. In crates/codex-app/src/app.rs: add Route::Search => rsx!{ SearchView { search_query } } match arm, where search_query is a signal threaded from the toolbar. Add CSS for search results in crates/codex-app/assets/styles/search.css (result cards, highlighted mark tag with yellow/teal background, folder group headers). Add stylesheet to app.rs. All code must compile with cargo check.

## Dependency Versions

Use these exact versions — do not rely on training data for API shapes:

```toml
# crates/codex-app/Cargo.toml
[dependencies]
codex-core  = { workspace = true }
codex-store = { workspace = true }
serde       = { workspace = true }
serde_json  = { workspace = true }
anyhow      = { workspace = true }
thiserror   = { workspace = true }
tokio       = { workspace = true }
tracing     = { workspace = true }
tracing-subscriber = { workspace = true }
chrono     = { workspace = true }
comrak      = { workspace = true }
syntect     = { workspace = true }
once_cell   = { workspace = true }
wry         = "0.53"
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
