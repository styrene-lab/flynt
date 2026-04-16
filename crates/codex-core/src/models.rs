use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
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

// ── Metadata ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataProtection {
    PlaintextIndexed,
    EncryptedOpaque,
}

impl Default for MetadataProtection {
    fn default() -> Self { Self::PlaintextIndexed }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetadataValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    StringList(Vec<String>),
}

impl Default for MetadataValue {
    fn default() -> Self { Self::Null }
}

pub type MetadataMap = BTreeMap<String, MetadataValue>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MetadataField {
    #[serde(default)]
    pub value: MetadataValue,
    #[serde(default)]
    pub protection: MetadataProtection,
}

pub type MetadataFieldMap = BTreeMap<String, MetadataField>;

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
    #[serde(default)]
    pub metadata: MetadataFieldMap,
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
    #[serde(default, flatten)]
    pub metadata: MetadataMap,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub id: BoardId,
    pub name: String,
    pub columns: Vec<Column>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

impl Task {
    pub fn new(
        board_id: BoardId,
        column:   impl Into<String>,
        title:    impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id:            TaskId::new(),
            board_id,
            column:        column.into(),
            title:         title.into(),
            description:   String::new(),
            priority:      Priority::Medium,
            status:        TaskStatus::Todo,
            tags:          Vec::new(),
            document_refs: Vec::new(),
            due_date:      None,
            position:      u32::MAX, // store sorts by position asc; MAX = append
            created_at:    now,
            updated_at:    now,
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
    #[serde(default)]
    pub local_runtime: LocalRuntimeConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LocalRuntimeConfig {
    #[serde(default)]
    pub local_state_root: Option<PathBuf>,
    #[serde(default)]
    pub codex_index_db_path: Option<PathBuf>,
    #[serde(default)]
    pub omegon_runtime_root: Option<PathBuf>,
    #[serde(default)]
    pub omegon_mind_db_path: Option<PathBuf>,
    #[serde(default)]
    pub styrene_identity_profile: Option<String>,
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

// ── Omegon profile + Codex operator settings ────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OmegonProfile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_model: Option<OmegonProfileModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub avoid_providers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_floor_pin: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub downgrade_overrides: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmegonProfileModel {
    pub provider: String,
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOperatorSettings {
    pub active_persona: String,
    pub enabled_skills: Vec<String>,
    pub preferred_extensions: Vec<String>,
    pub rail_extension: String,
    pub vox: VoxSettings,
}

impl Default for CodexOperatorSettings {
    fn default() -> Self {
        Self {
            active_persona: "off".into(),
            enabled_skills: Vec::new(),
            preferred_extensions: vec!["vox".into()],
            rail_extension: "vox".into(),
            vox: VoxSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoxSettings {
    pub enabled: bool,
    pub tts_enabled: bool,
    pub voice: String,
}

impl Default for VoxSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            tts_enabled: false,
            voice: "default".into(),
        }
    }
}
