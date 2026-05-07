use flynt_core::models::DocumentId;
pub use flynt_core::models::FontSizePreset;

/// Active theme name — context-provided so any component can read or swap it.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeName(pub String);

/// Bump counter to trigger inline rename in NotesView.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RenameTrigger(pub u64);

#[derive(Clone, PartialEq, Debug, Default)]
pub enum Route {
    Welcome,
    #[default]
    Notes,
    Search,
    Kanban,
    Graph,
    Settings,
}

/// Settings view tab — determines which panel is visible.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Vault,
    Omegon,
    Advanced,
}

impl SettingsTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Vault => "Vault",
            Self::Omegon => "Omegon",
            Self::Advanced => "Advanced",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::General, Self::Vault, Self::Omegon, Self::Advanced]
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum SyncStatus {
    #[default]
    Idle,
    Syncing,
    Conflict(usize),
}

/// Open tabs — the core of multi-document editing.
/// Stores (id, title) pairs so the tab bar never hits the store.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TabState {
    pub tabs:   Vec<(DocumentId, String)>,
    pub active: usize,
}

impl TabState {
    pub fn open(&mut self, id: DocumentId, title: String) {
        if let Some(idx) = self.tabs.iter().position(|(t, _)| t == &id) {
            self.active = idx;
        } else {
            self.tabs.push((id, title));
            self.active = self.tabs.len() - 1;
        }
    }

    pub fn close(&mut self, idx: usize) {
        if idx >= self.tabs.len() { return; }
        self.tabs.remove(idx);
        if !self.tabs.is_empty() {
            self.active = self.active.min(self.tabs.len() - 1);
        }
    }

    pub fn active_id(&self) -> Option<&DocumentId> {
        self.tabs.get(self.active).map(|(id, _)| id)
    }

    pub fn active_title(&self) -> Option<&str> {
        self.tabs.get(self.active).map(|(_, t)| t.as_str())
    }
}
