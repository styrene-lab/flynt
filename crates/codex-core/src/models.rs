use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

// ── Newtype IDs ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoardId(pub Uuid);

impl DocumentId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}
impl TaskId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}
impl BoardId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}

impl Default for DocumentId { fn default() -> Self { Self::new() } }
impl Default for TaskId { fn default() -> Self { Self::new() } }
impl Default for BoardId { fn default() -> Self { Self::new() } }

// ── Document ─────────────────────────────────────────────────────────────────

/// A note or wiki page stored as a markdown file.
/// The file on disk is the source of truth; this is the parsed representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    /// Path relative to vault root
    pub path: PathBuf,
    pub title: String,
    /// Raw markdown content (includes frontmatter stripped away)
    pub content: String,
    pub frontmatter: Frontmatter,
    pub outgoing_links: Vec<WikiLink>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Lightweight index record — used for listing without loading full content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub id: DocumentId,
    pub path: PathBuf,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at: DateTime<Utc>,
}

/// YAML/TOML frontmatter parsed from the top of a document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Frontmatter {
    /// Stable document identity — written on first index, survives DB wipe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<uuid::Uuid>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// A `[[wikilink]]` extracted from document content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiLink {
    /// Raw target as written, e.g. `"some-note"` or `"some-note#heading"`
    pub target: String,
    /// Optional display text after the pipe: `[[target|display]]`
    pub display: Option<String>,
    /// Heading anchor, split from target at `#`
    pub anchor: Option<String>,
}

// ── Kanban Task ───────────────────────────────────────────────────────────────

/// A task stored as a markdown file with TOML frontmatter under `.codex/tasks/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub board_id: BoardId,
    /// Name of the column this task lives in
    pub column: String,
    pub title: String,
    /// Markdown body — rendered in the task detail pane
    pub description: String,
    pub priority: Priority,
    pub status: TaskStatus,
    pub tags: Vec<String>,
    /// Documents linked to this task
    pub document_refs: Vec<DocumentId>,
    pub due_date: Option<NaiveDate>,
    /// Ordering position within column (ascending)
    pub position: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    #[default]
    Medium,
    Low,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Todo,
    InProgress,
    Done,
    Archived,
}

// ── Kanban Board ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    pub id: BoardId,
    pub name: String,
    pub columns: Vec<Column>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub wip_limit: Option<u32>,
}

impl Board {
    pub fn default_sprint(name: impl Into<String>) -> Self {
        Self {
            id: BoardId::new(),
            name: name.into(),
            columns: vec![
                Column { name: "Backlog".into(), wip_limit: None },
                Column { name: "In Progress".into(), wip_limit: Some(3) },
                Column { name: "Review".into(), wip_limit: None },
                Column { name: "Done".into(), wip_limit: None },
            ],
            created_at: Utc::now(),
        }
    }
}

// ── Search ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub document_id: DocumentId,
    pub path: PathBuf,
    pub title: String,
    pub excerpt: String,
    pub score: f32,
}

// ── Vault config ──────────────────────────────────────────────────────────────

/// Persisted configuration stored in `<vault_root>/.codex/config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VaultConfig {
    pub vault_name: String,
    pub sync: SyncConfig,
    #[serde(default)]
    pub appearance: AppearanceConfig,
}

/// Appearance settings — theme name and prose font scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceConfig {
    #[serde(default = "AppearanceConfig::default_theme")]
    pub theme: String,
    #[serde(default)]
    pub font_size: FontSizePreset,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self { theme: Self::default_theme(), font_size: FontSizePreset::default() }
    }
}

impl AppearanceConfig {
    fn default_theme() -> String { "alpharius".into() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FontSizePreset {
    Small,
    #[default]
    Medium,
    Large,
    XLarge,
}

impl FontSizePreset {
    pub fn label(self) -> &'static str {
        match self { Self::Small => "S", Self::Medium => "M", Self::Large => "L", Self::XLarge => "XL" }
    }
    pub fn css_class(self) -> &'static str {
        match self { Self::Small => "font-sm", Self::Medium => "font-md", Self::Large => "font-lg", Self::XLarge => "font-xl" }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum SyncConfig {
    #[default]
    None,
    /// iCloud: vault_root must already be inside iCloud Drive; no extra config needed.
    ICloud,
    Git {
        remote: String,
        branch: String,
        /// Auto-commit debounce in seconds (0 = manual only)
        auto_commit_seconds: u64,
    },
    S3 {
        bucket: String,
        prefix: String,
        region: String,
        endpoint: Option<String>,
    },
}
