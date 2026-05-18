use flynt_core::models::DocumentId;
pub use flynt_core::models::FontSizePreset;

/// Active theme name — context-provided so any component can read or swap it.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeName(pub String);

/// Bump counter to trigger inline rename in NotesView.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RenameTrigger(pub u64);

/// One-shot command bus for the active note context inspector. The
/// command palette bumps `version`; NotesView consumes the newest value
/// without requiring the palette to know about note-view internals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteInspectorTarget {
    Toggle,
    Links,
    Outline,
    Properties,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteInspectorCommand {
    pub version: u64,
    pub target: NoteInspectorTarget,
}

impl Default for NoteInspectorCommand {
    fn default() -> Self {
        Self {
            version: 0,
            target: NoteInspectorTarget::Toggle,
        }
    }
}

/// One-shot command bus for active note history/recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NoteHistoryCommand {
    pub version: u64,
}

/// Whether the settings modal is currently open. Settings used to be
/// a top-level Route, but that meant entering settings replaced the
/// whole main content area (including the project sidebar and tab
/// bar) — operators couldn't glance at a setting and return to their
/// work. Modal overlay keeps the underlying view visible behind a
/// backdrop and avoids sidebar+rail+content all competing for the
/// same narrow width on split-screen displays.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SettingsOpen(pub bool);

#[derive(Clone, PartialEq, Debug, Default)]
pub enum Route {
    Welcome,
    #[default]
    Notes,
    Search,
    Kanban,
    Graph,
}

/// Top-level settings category. Each category may render as a single
/// page (its content lives at the category level) or expand into
/// child pages that each render their own pane.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SettingsCategory {
    General,
    Project,
    Omegon,
    Advanced,
}

impl SettingsCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Project => "Project",
            Self::Omegon => "Omegon",
            Self::Advanced => "Advanced",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::General, Self::Project, Self::Omegon, Self::Advanced]
    }
}

/// A single settings page — a leaf in the sidebar tree. Pages know
/// which category they belong to and what label to show in the rail.
///
/// Only Omegon expands into sub-pages today; the other categories
/// each have a single page. As General / Project / Advanced bloat
/// past one screen, split them the same way (add variants here and
/// route them in the settings view).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum SettingsPage {
    // General — split into focused sub-pages. Previously one mega-page
    // mixed appearance, sync, identity, and providers — splitting them
    // matches how operators reason about each (visual vs. data vs.
    // credentials).
    #[default]
    GeneralAppearance,
    GeneralSync,
    GeneralIdentity,
    GeneralUpdates,
    // Project — single page (name + location + indexing + visualization + publication)
    Project,
    // Omegon — expanded into sub-pages
    OmegonProfile,
    OmegonProviders,
    OmegonExtensions,
    OmegonArmory,
    OmegonSkills,
    OmegonDaemon,
    OmegonRuntime,
    // Advanced — single page (local paths + raw config editor)
    Advanced,
}

impl SettingsPage {
    pub fn label(self) -> &'static str {
        match self {
            Self::GeneralAppearance => "Appearance",
            Self::GeneralSync => "Sync",
            Self::GeneralIdentity => "Identity",
            Self::GeneralUpdates => "Updates",
            Self::Project => "Project",
            Self::OmegonProfile => "Profile",
            Self::OmegonProviders => "Providers",
            Self::OmegonExtensions => "Extensions",
            Self::OmegonArmory => "Armory",
            Self::OmegonSkills => "Skills",
            Self::OmegonDaemon => "Daemon",
            Self::OmegonRuntime => "Runtime",
            Self::Advanced => "Advanced",
        }
    }

    pub fn category(self) -> SettingsCategory {
        match self {
            Self::GeneralAppearance
            | Self::GeneralSync
            | Self::GeneralIdentity
            | Self::GeneralUpdates => SettingsCategory::General,
            Self::Project => SettingsCategory::Project,
            Self::OmegonProfile
            | Self::OmegonProviders
            | Self::OmegonExtensions
            | Self::OmegonArmory
            | Self::OmegonSkills
            | Self::OmegonDaemon
            | Self::OmegonRuntime => SettingsCategory::Omegon,
            Self::Advanced => SettingsCategory::Advanced,
        }
    }

    /// Pages in display order, grouped by category. Used to build the
    /// sidebar tree.
    pub fn all() -> &'static [Self] {
        &[
            Self::GeneralAppearance,
            Self::GeneralSync,
            Self::GeneralIdentity,
            Self::GeneralUpdates,
            Self::Project,
            Self::OmegonProfile,
            Self::OmegonProviders,
            Self::OmegonExtensions,
            Self::OmegonArmory,
            Self::OmegonSkills,
            Self::OmegonDaemon,
            Self::OmegonRuntime,
            Self::Advanced,
        ]
    }

    /// Pages within a given category, in display order.
    pub fn in_category(cat: SettingsCategory) -> Vec<Self> {
        Self::all()
            .iter()
            .filter(|p| p.category() == cat)
            .copied()
            .collect()
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum SyncStatus {
    #[default]
    Idle,
    Syncing,
    Conflict(usize),
}

#[derive(Clone, PartialEq, Debug)]
pub enum SyncRunOutcome {
    Success,
    Error(String),
    Conflict(Vec<String>),
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct SyncActivityState {
    pub current_phase: Option<String>,
    pub last_started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_outcome: Option<SyncRunOutcome>,
    pub successful_runs: u64,
    pub failed_runs: u64,
}

/// Open tabs — the core of multi-document editing.
/// Stores (id, title) pairs so the tab bar never hits the store.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TabState {
    pub tabs: Vec<(DocumentId, String)>,
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
        if idx >= self.tabs.len() {
            return;
        }
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
