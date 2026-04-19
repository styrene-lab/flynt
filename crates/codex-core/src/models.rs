use crate::datum::{Entity, EntityKind};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}};
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
    /// When this document has a `kind` in its frontmatter, the parsed Entity.
    /// Populated during indexing from the `[data]` table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<Entity>,
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
    /// Entity kind when this document is a typed entity (project, task, etc.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_kind: Option<EntityKind>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicationTarget {
    pub repo: String,
    pub branch: String,
    pub site_dir: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PublicationRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_path_prefix: Option<String>,
    #[serde(default)]
    pub visibility: PublicationVisibility,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PublicationPolicy {
    #[serde(default)]
    pub default_visibility: PublicationVisibility,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<PublicationRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PublicationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default)]
    pub visibility: PublicationVisibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<PublicationTarget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PublicationVisibility {
    #[default]
    Private,
    Public,
    Unlisted,
}

/// YAML/TOML frontmatter parsed from the top of a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Frontmatter {
    /// Stable document identity — written on first index, survives DB wipe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<uuid::Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Entity kind discriminator. When present, this document is a typed entity
    /// (e.g. "project", "task", "contact"). When absent, it's a plain document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Typed entity fields. Populated from the `[data]` table in frontmatter.
    /// Only meaningful when `kind` is set. Stored as a generic TOML table so
    /// fields are schema-flexible by default and validated against Pkl schemas
    /// when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<toml::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub publication: PublicationConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub imported_reference: bool,
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
    /// Decay rate — controls how quickly relevance fades without interaction.
    #[serde(default)]
    pub decay: DecayRate,
    /// Last time a human or agent interacted with this task (view, edit, mention).
    /// Resets the decay clock. Defaults to updated_at if never set.
    #[serde(default)]
    pub last_touched_at: Option<DateTime<Utc>>,
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

// ── Task Decay ───────────────────────────────────────────────────────────────

/// Controls how quickly a task loses relevance without interaction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecayRate {
    /// No decay — task stays fully relevant until manually resolved.
    /// Use for tracked project work, sprint items.
    None,
    /// Slow decay — ~14 day half-life. Longer-term personal goals.
    Slow,
    /// Natural decay — ~7 day half-life. Default for personal tasks.
    Natural,
    /// Fast decay — ~3 day half-life. Ephemeral reminders, quick errands.
    Fast,
    /// Custom half-life in days.
    Custom(f64),
}

impl Default for DecayRate {
    fn default() -> Self {
        Self::Natural
    }
}

impl DecayRate {
    /// Half-life in days. Returns None for non-decaying tasks.
    /// Custom values are clamped to a minimum of 0.1 days (~2.4 hours).
    pub fn half_life_days(&self) -> Option<f64> {
        match self {
            Self::None => Option::None,
            Self::Slow => Some(14.0),
            Self::Natural => Some(7.0),
            Self::Fast => Some(3.0),
            Self::Custom(d) => Some(d.max(0.1)),
        }
    }
}

impl Task {
    /// Compute the current relevance score (0.0–1.0).
    ///
    /// - 1.0 = just touched, fully relevant
    /// - 0.0 = completely decayed
    /// - Tasks with `DecayRate::None` always return 1.0
    /// - Done/Archived tasks always return 0.0
    pub fn relevance(&self) -> f64 {
        if matches!(self.status, TaskStatus::Done | TaskStatus::Archived) {
            return 0.0;
        }
        let half_life = match self.decay.half_life_days() {
            Some(hl) => hl,
            Option::None => return 1.0, // no decay
        };

        let anchor = self.last_touched_at.unwrap_or(self.updated_at);
        let elapsed_days = (Utc::now() - anchor).num_seconds() as f64 / 86400.0;
        if elapsed_days <= 0.0 {
            return 1.0;
        }

        // Exponential decay: relevance = 2^(-t/half_life)
        let lambda = (2.0_f64).ln() / half_life;
        (-lambda * elapsed_days).exp()
    }

    /// Whether the task has decayed below the visibility threshold (0.3).
    pub fn is_fading(&self) -> bool {
        self.relevance() < 0.3
    }

    /// Whether the task should be auto-archived (relevance < 0.1).
    pub fn should_auto_archive(&self) -> bool {
        self.relevance() < 0.1
    }

    /// Touch the task — resets the decay clock.
    pub fn touch(&mut self) {
        self.last_touched_at = Some(Utc::now());
    }
}

// ── Notifications (git-synced, serverless push) ──────────────────────────────

/// A notification record synced via git between devices.
/// Written to `.codex/notifications/pending/<id>.json`.
/// After delivery, moved to `.codex/notifications/delivered/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
    /// Which vault originated this notification.
    pub source_vault: String,
    /// Task ID if this notification relates to a task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub created_at: DateTime<Utc>,
    /// Set when delivered on a device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// Task approaching its due date.
    DueDate,
    /// Task is decaying — needs attention or will auto-archive.
    Decay,
    /// Task was auto-archived by the decay system.
    AutoArchived,
    /// Agent-initiated — Omegon wants the user's attention.
    Agent,
    /// Vox communication — inbound message from an agent or system.
    Vox,
}

impl Notification {
    pub fn new(kind: NotificationKind, title: impl Into<String>, body: impl Into<String>, source_vault: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            title: title.into(),
            body: body.into(),
            source_vault: source_vault.into(),
            task_id: None,
            created_at: Utc::now(),
            delivered_at: None,
        }
    }

    pub fn for_task(mut self, task_id: TaskId) -> Self {
        self.task_id = Some(task_id);
        self
    }
}

// ── Kanban Board ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub id: BoardId,
    pub name: String,
    pub columns: Vec<Column>,
    /// When set, tasks on this board belong to a git-backed project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
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
            project_id: None,
            created_at: Utc::now(),
        }
    }

    /// Create a board associated with a git-backed project.
    pub fn for_project(name: impl Into<String>, project_id: Uuid) -> Self {
        let mut board = Self::default_sprint(name);
        board.project_id = Some(project_id);
        board
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
            id:              TaskId::new(),
            board_id,
            column:          column.into(),
            title:           title.into(),
            description:     String::new(),
            priority:        Priority::Medium,
            status:          TaskStatus::Todo,
            tags:            Vec::new(),
            document_refs:   Vec::new(),
            due_date:        None,
            position:        u32::MAX,
            created_at:      now,
            updated_at:      now,
            decay:           DecayRate::default(),
            last_touched_at: None,
        }
    }

    /// Create a non-decaying task (for tracked project work).
    pub fn new_tracked(
        board_id: BoardId,
        column:   impl Into<String>,
        title:    impl Into<String>,
    ) -> Self {
        let mut task = Self::new(board_id, column, title);
        task.decay = DecayRate::None;
        task
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
    #[serde(default)]
    pub publication: PublicationPolicy,
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
    /// Omegon serve daemon host:port for mobile agent connections.
    #[serde(default)]
    pub omegon_serve_host: Option<String>,
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

// ── Git-backed project config ────────────────────────────────────────────────

/// Describes how a project's data maps to a git repository.
///
/// Each project is backed 1:1 by a git repo. The repo can be the vault's own
/// repo (most common) or a separate external repo. In both cases the project
/// data lives at a configurable sub-path within the repo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GitBacking {
    /// Project data lives inside the vault's own git repo.
    VaultRepo {
        /// Path relative to vault root where this project's data lives
        /// (e.g. ".codex/projects/my-project").
        sub_path: PathBuf,
    },
    /// Project data lives in a separate external git repo.
    ExternalRepo {
        /// Absolute path to the repo root on disk.
        repo_root: PathBuf,
        /// Sub-path within the repo where project data lives.
        sub_path: PathBuf,
        /// Remote name (e.g. "origin").
        remote: String,
        /// Branch name.
        branch: String,
    },
}

impl GitBacking {
    /// The sub-path within the repo where project data lives.
    pub fn sub_path(&self) -> &Path {
        match self {
            Self::VaultRepo { sub_path } => sub_path,
            Self::ExternalRepo { sub_path, .. } => sub_path,
        }
    }

    /// Whether this backing uses the vault's own repo.
    pub fn is_vault_repo(&self) -> bool {
        matches!(self, Self::VaultRepo { .. })
    }
}

/// Configuration for project-level atomic commits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProjectCommitConfig {
    /// Auto-commit debounce in seconds. 0 = manual only.
    #[serde(default)]
    pub auto_commit_seconds: u64,
    /// Commit message prefix (e.g. "[codex:my-project]").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_prefix: Option<String>,
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
    /// Persisted ACP config — model, thinking level, posture, etc.
    /// Keys are config option IDs, values are the selected value IDs.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub acp_config: std::collections::HashMap<String, String>,
}

impl Default for CodexOperatorSettings {
    fn default() -> Self {
        Self {
            active_persona: "off".into(),
            enabled_skills: Vec::new(),
            preferred_extensions: vec!["vox".into()],
            rail_extension: "vox".into(),
            vox: VoxSettings::default(),
            acp_config: std::collections::HashMap::new(),
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
