//! UI state mirror — writes the panel/tab/view state the embedded Omegon
//! agent needs to answer "what am I looking at?".
//!
//! Shape on disk: `<vault>/.flynt-local/flynt/ui-state.json`. The flynt-agent
//! extension reads this file via its `get_ui_state` tool. Atomic write via
//! tempfile + rename so the agent never sees a half-written document.

use flynt_core::store::VaultStore;
use flynt_store::vault::Vault;
use serde::Serialize;
use std::path::Path;

use crate::state::{Route, TabState};

#[derive(Debug, Serialize)]
struct DocumentRef {
    id: String,
    title: String,
    /// Path relative to vault root. Pass directly to `get_document`.
    path: String,
}

#[derive(Debug, Serialize)]
struct UiStateSnapshot<'a> {
    /// The document the user is currently looking at, if any.
    active_document: Option<DocumentRef>,
    /// All open document tabs in tab-bar order.
    open_documents: Vec<DocumentRef>,
    /// Which top-level view is shown (notes, kanban, graph, settings, search, welcome).
    current_view: &'static str,
    /// Absolute path of the vault root the user is in.
    vault_root: &'a str,
    /// ISO-8601 UTC timestamp of this snapshot.
    updated_at: String,
}

fn route_label(route: &Route) -> &'static str {
    match route {
        Route::Welcome => "welcome",
        Route::Notes => "notes",
        Route::Search => "search",
        Route::Kanban => "kanban",
        Route::Graph => "graph",
        Route::Settings => "settings",
    }
}

fn resolve_doc_ref(vault: &Vault, id: &flynt_core::models::DocumentId, title: &str) -> Option<DocumentRef> {
    let doc = vault.store.get_document(id).ok().flatten()?;
    Some(DocumentRef {
        id: id.0.to_string(),
        title: title.to_string(),
        path: doc.path.to_string_lossy().to_string(),
    })
}

/// Write the UI state snapshot to `<vault>/.flynt-local/flynt/ui-state.json`.
/// Errors are logged but never propagated — UI state is best-effort and must
/// not block the editor on a slow disk.
pub fn write_snapshot(vault: &Vault, tabs: &TabState, route: &Route) {
    let active_document = tabs
        .tabs
        .get(tabs.active)
        .and_then(|(id, title)| resolve_doc_ref(vault, id, title));

    let open_documents: Vec<DocumentRef> = tabs
        .tabs
        .iter()
        .filter_map(|(id, title)| resolve_doc_ref(vault, id, title))
        .collect();

    let vault_root_buf = vault.root.to_string_lossy();
    let snapshot = UiStateSnapshot {
        active_document,
        open_documents,
        current_view: route_label(route),
        vault_root: vault_root_buf.as_ref(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Err(e) = write_atomic(&vault.root, &snapshot) {
        tracing::warn!("ui-state write failed: {e}");
    }
}

fn write_atomic(vault_root: &Path, snapshot: &UiStateSnapshot<'_>) -> std::io::Result<()> {
    let dir = vault_root.join(".flynt-local").join("flynt");
    std::fs::create_dir_all(&dir)?;
    let final_path = dir.join("ui-state.json");
    let tmp_path = dir.join("ui-state.json.tmp");
    let body = serde_json::to_vec_pretty(snapshot)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&tmp_path, &body)?;
    std::fs::rename(&tmp_path, &final_path)?;
    Ok(())
}
