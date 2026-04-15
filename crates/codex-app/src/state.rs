use codex_core::models::BoardId;
use codex_store::vault::Vault;
use std::sync::Arc;

/// Top-level application state passed as Dioxus context.
#[derive(Clone)]
pub struct AppState {
    pub vault: Arc<Vault>,
    pub active_view: ActiveView,
    pub selected_doc: Option<String>,
    pub selected_board: Option<BoardId>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ActiveView {
    Notes,
    Kanban,
    Search,
    Settings,
}

impl Default for ActiveView {
    fn default() -> Self { Self::Notes }
}
