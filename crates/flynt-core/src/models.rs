use crate::datum::{Entity, EntityKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}};
use uuid::Uuid;

// ── Re-export task types from flynt-models ──────────────────────────────────
// These are the canonical definitions. flynt-core re-exports for backward compat.
pub use flynt_models::task::{
    BoardId, DecayRate, DocumentId, ExecutionSpec, Priority, Task, TaskId, TaskPatch, TaskStatus,
};
pub use flynt_models::engagement::{
    Engagement, EngagementId, EngagementStatus, Partnership, PartnershipId, RepoBinding,
};

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
    /// Path relative to project root
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
    /// Whether this note's body is encrypted at rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed: Option<bool>,
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

// Task, Priority, TaskStatus, DecayRate, TaskId, BoardId, DocumentId
// are defined in flynt-models and re-exported above.

// ── Notifications (git-synced, serverless push) ──────────────────────────────

/// A notification record synced via git between devices.
/// Written to `.flynt/notifications/pending/<id>.json`.
/// After delivery, moved to `.flynt/notifications/delivered/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
    /// Which project originated this notification.
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
    /// Default columns for a new board.
    ///
    /// `Backlog → Scheduled → Running → Done` mirrors sentry's task lifecycle
    /// (queued → claimed → executing → terminal) so an autonomous flow lands
    /// in the right column without renaming. `Failed` is a terminal sibling
    /// of `Done` for non-recoverable runs. Hand-tracked tasks are unaffected
    /// — operators can still rename or reshape any column.
    pub fn default_sprint(name: impl Into<String>) -> Self {
        Self {
            id: BoardId::new(),
            name: name.into(),
            columns: vec![
                Column { name: "Backlog".into(), wip_limit: None },
                Column { name: "Scheduled".into(), wip_limit: None },
                Column { name: "Running".into(), wip_limit: Some(3) },
                Column { name: "Done".into(), wip_limit: None },
                Column { name: "Failed".into(), wip_limit: None },
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

// ── Search ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub document_id: DocumentId,
    pub path: PathBuf,
    pub title: String,
    pub excerpt: String,
    pub score: f32,
}

// ── Project config ──────────────────────────────────────────────────────────────

/// Persisted configuration stored in `<vault_root>/.flynt/config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct VaultConfig {
    #[serde(default)]
    pub vault_name: String,
    #[serde(default)]
    pub sync: SyncConfig,
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub local_runtime: LocalRuntimeConfig,
    #[serde(default)]
    pub publication: PublicationPolicy,
    #[serde(default)]
    pub security: crate::seal::SealConfig,
    #[serde(default)]
    pub indexing: IndexingConfig,
    #[serde(default)]
    pub visualization: VisualizationConfig,
}

/// Controls how the indexer interacts with source files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexingConfig {
    /// When true (default), the indexer writes a stable UUID into files missing
    /// frontmatter IDs. When false, IDs are tracked in the database only and
    /// source files are never modified — ideal for existing repos and shared codebases.
    #[serde(default = "IndexingConfig::default_write_frontmatter")]
    pub write_frontmatter: bool,

    /// Opt-in managed paths. Files under a scope can override the project-wide
    /// `write_frontmatter` default and be auto-assigned an entity `kind`.
    /// When empty, all files follow the project-wide setting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<IndexScope>,
}

/// A scoped path prefix that opts a subdirectory into (or out of) full
/// document management by Flynt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexScope {
    /// Path prefix relative to project root (e.g. `"design/"`).
    pub prefix: PathBuf,

    /// Auto-assigned entity kind for files without an existing `kind`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Override the project-wide `write_frontmatter` for files under this prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_frontmatter: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTier {
    /// Flynt writes frontmatter, assigns kinds — the file is fully managed.
    Managed,
    /// Indexed into SQLite/FTS for search, but the source file is never modified.
    Discoverable,
}

impl IndexingConfig {
    fn default_write_frontmatter() -> bool { true }

    /// Returns the longest-prefix-matching scope for `rel_path`, if any.
    pub fn scope_for_path(&self, rel_path: &Path) -> Option<&IndexScope> {
        self.scopes
            .iter()
            .filter(|s| !s.prefix.as_os_str().is_empty() && rel_path.starts_with(&s.prefix))
            .max_by_key(|s| s.prefix.as_os_str().len())
    }

    /// Whether the indexer should write frontmatter into a file at `rel_path`.
    pub fn should_write_frontmatter(&self, rel_path: &Path) -> bool {
        self.scope_for_path(rel_path)
            .and_then(|s| s.write_frontmatter)
            .unwrap_or(self.write_frontmatter)
    }

    pub fn file_tier(&self, rel_path: &Path) -> FileTier {
        if self.should_write_frontmatter(rel_path) {
            FileTier::Managed
        } else {
            FileTier::Discoverable
        }
    }
}

impl Default for IndexingConfig {
    fn default() -> Self {
        Self { write_frontmatter: true, scopes: Vec::new() }
    }
}

/// Visualization and diagram rendering settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualizationConfig {
    /// Auto-export SVG when Excalidraw drawings are saved.
    #[serde(default = "VisualizationConfig::default_true")]
    pub excalidraw_auto_export: bool,

    /// Auto-render D2 diagrams to SVG when .d2 files change.
    #[serde(default = "VisualizationConfig::default_true")]
    pub d2_auto_render: bool,

    /// D2 theme number (200 = dark/Alpharius, 0 = default light).
    #[serde(default = "VisualizationConfig::default_d2_theme")]
    pub d2_theme: u32,

    /// D2 layout engine: elk (default), dagre, tala.
    #[serde(default = "VisualizationConfig::default_d2_layout")]
    pub d2_layout: String,

    /// Custom path to the d2 binary (if not on PATH).
    #[serde(default)]
    pub d2_bin: Option<String>,
}

impl VisualizationConfig {
    fn default_true() -> bool { true }
    fn default_d2_theme() -> u32 { 200 }
    fn default_d2_layout() -> String { "elk".into() }
}

impl Default for VisualizationConfig {
    fn default() -> Self {
        Self {
            excalidraw_auto_export: true,
            d2_auto_render: true,
            d2_theme: 200,
            d2_layout: "elk".into(),
            d2_bin: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LocalRuntimeConfig {
    #[serde(default)]
    pub local_state_root: Option<PathBuf>,
    #[serde(default)]
    pub flynt_index_db_path: Option<PathBuf>,
    #[serde(default)]
    pub omegon_runtime_root: Option<PathBuf>,
    #[serde(default)]
    pub omegon_mind_db_path: Option<PathBuf>,
    #[serde(default)]
    pub styrene_identity_profile: Option<String>,
    /// Omegon serve daemon host:port for mobile agent connections.
    #[serde(default)]
    pub omegon_serve_host: Option<String>,
    /// Omegon release channel: stable, rc, nightly. Determines which binary to use.
    #[serde(default)]
    pub omegon_channel: OmegonChannel,
    /// Explicit path override — bypasses channel resolution entirely.
    #[serde(default)]
    pub omegon_bin_override: Option<String>,
}

/// Omegon release channel — determines which installed version to use.
/// Omegon installs versioned binaries to `~/.omegon/versions/<version>/omegon`.
/// The channel selects the latest matching version from that directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OmegonChannel {
    /// Latest non-prerelease version (no -rc, no -nightly in version string).
    #[default]
    Stable,
    /// Latest release candidate (-rc.N suffix).
    Rc,
    /// Latest nightly build (-nightly.YYYYMMDD suffix).
    Nightly,
    /// Pinned to a specific version string (e.g., "0.17.0-rc.1").
    Pinned(String),
}

impl OmegonChannel {
    pub fn label(&self) -> &str {
        match self {
            Self::Stable => "Stable",
            Self::Rc => "RC",
            Self::Nightly => "Nightly",
            Self::Pinned(v) => v,
        }
    }

    pub fn all_named() -> &'static [Self] {
        &[Self::Stable, Self::Rc, Self::Nightly]
    }
}

/// Resolve the Omegon binary path from config.
///
/// Priority:
///   1. Explicit binary path override
///   2. OMEGON_BIN environment variable
///   3. Channel-matched version in ~/.omegon/versions/
///   4. `omegon` on PATH or well-known locations
pub fn resolve_omegon_binary(config: &LocalRuntimeConfig) -> std::path::PathBuf {
    // 1. Explicit override
    if let Some(ref p) = config.omegon_bin_override {
        let path = std::path::PathBuf::from(p);
        if path.exists() {
            return path;
        }
    }

    // 2. OMEGON_BIN env var
    if let Ok(bin) = std::env::var("OMEGON_BIN") {
        let path = std::path::PathBuf::from(&bin);
        if path.exists() {
            return path;
        }
    }

    // 3. Channel-matched version in ~/.omegon/versions/
    let home = std::env::var("HOME").unwrap_or_default();
    let versions_dir = std::path::PathBuf::from(&home).join(".omegon/versions");
    if versions_dir.is_dir() {
        if let Some(path) = resolve_from_versions_dir(&versions_dir, &config.omegon_channel) {
            return path;
        }
    }

    // 4. Bare `omegon` on PATH or well-known locations
    let candidates = [
        format!("{home}/.local/bin/omegon"),
        format!("{home}/.cargo/bin/omegon"),
        "/usr/local/bin/omegon".into(),
        "/opt/homebrew/bin/omegon".into(),
    ];
    for c in &candidates {
        let path = std::path::PathBuf::from(c);
        if path.exists() {
            return path;
        }
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("omegon");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    std::path::PathBuf::from("omegon")
}

/// Parse a version string into (major, minor, patch, prerelease) for sorting.
/// "v0.17.0-rc.1" → (0, 17, 0, "rc.1"), "0.16.1" → (0, 16, 1, "")
fn parse_version_key(v: &str) -> (u32, u32, u32, String) {
    let bare = v.strip_prefix('v').unwrap_or(v);
    let (version_part, pre) = if let Some(idx) = bare.find('-') {
        (&bare[..idx], bare[idx + 1..].to_string())
    } else {
        (bare, String::new())
    };
    let parts: Vec<u32> = version_part.split('.').filter_map(|s| s.parse().ok()).collect();
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
        pre,
    )
}

/// Sort version strings by semver (newest first).
fn sort_versions_newest_first(versions: &mut [&String]) {
    versions.sort_by(|a, b| {
        let ka = parse_version_key(a);
        let kb = parse_version_key(b);
        kb.cmp(&ka) // descending
    });
}

/// Scan ~/.omegon/versions/ and pick the best match for the channel.
fn resolve_from_versions_dir(
    versions_dir: &std::path::Path,
    channel: &OmegonChannel,
) -> Option<std::path::PathBuf> {
    let entries: Vec<String> = std::fs::read_dir(versions_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().ok().map(|ft| ft.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();

    let best = match channel {
        OmegonChannel::Pinned(version) => {
            // Exact match (with or without v prefix)
            let bare = version.strip_prefix('v').unwrap_or(version);
            entries.iter().find(|e| {
                let e_bare = e.strip_prefix('v').unwrap_or(e);
                e_bare == bare
            }).cloned()
        }
        OmegonChannel::Stable => {
            let mut stable: Vec<&String> = entries.iter()
                .filter(|v| {
                    let bare = v.strip_prefix('v').unwrap_or(v);
                    !bare.contains("-rc.") && !bare.contains("-nightly.")
                })
                .collect();
            sort_versions_newest_first(&mut stable);
            stable.first().cloned().cloned()
        }
        OmegonChannel::Rc => {
            let mut rcs: Vec<&String> = entries.iter()
                .filter(|v| v.contains("-rc."))
                .collect();
            sort_versions_newest_first(&mut rcs);
            rcs.first().cloned().cloned()
                .or_else(|| {
                    let mut stable: Vec<&String> = entries.iter()
                        .filter(|v| !v.contains("-nightly."))
                        .collect();
                    sort_versions_newest_first(&mut stable);
                    stable.first().cloned().cloned()
                })
        }
        OmegonChannel::Nightly => {
            let mut nightlies: Vec<&String> = entries.iter()
                .filter(|v| v.contains("-nightly."))
                .collect();
            sort_versions_newest_first(&mut nightlies);
            nightlies.first().cloned().cloned()
                .or_else(|| {
                    let mut all: Vec<&String> = entries.iter().collect();
                    sort_versions_newest_first(&mut all);
                    all.first().cloned().cloned()
                })
        }
    };

    best.map(|version| versions_dir.join(&version).join("omegon"))
        .filter(|p| p.exists())
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
    /// Forge-backed sync via Scribe — bidirectional issue/task sync with
    /// Forgejo, GitHub, or GitLab.
    Forge {
        /// Scribe forge endpoint identifier.
        forge_id: String,
        /// Org/owner on the forge.
        org: String,
        /// Repo name on the forge.
        repo: String,
        /// Sync issues ↔ flynt tasks.
        #[serde(default)]
        sync_issues: bool,
        /// Auto-commit debounce in seconds (0 = manual only).
        #[serde(default)]
        auto_commit_seconds: u64,
    },
}

// ── Git-backed project config ────────────────────────────────────────────────

/// Describes how a project's data maps to a git repository.
///
/// Each project is backed 1:1 by a git repo. The repo can be the project's own
/// repo (most common) or a separate external repo. In both cases the project
/// data lives at a configurable sub-path within the repo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GitBacking {
    /// Project data lives inside the project's own git repo.
    VaultRepo {
        /// Path relative to project root where this project's data lives
        /// (e.g. ".flynt/projects/my-project").
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
    /// Project is backed by a forge-managed repo (via Scribe sync engine).
    ForgeRepo {
        /// Scribe forge endpoint identifier.
        forge_id: String,
        /// Org/owner on the forge.
        org: String,
        /// Repo name on the forge.
        repo: String,
        /// Local clone path (managed by scribe).
        local_path: PathBuf,
        /// Sub-path within the repo where project data lives.
        sub_path: PathBuf,
    },
}

impl GitBacking {
    /// The sub-path within the repo where project data lives.
    pub fn sub_path(&self) -> &Path {
        match self {
            Self::VaultRepo { sub_path } => sub_path,
            Self::ExternalRepo { sub_path, .. } => sub_path,
            Self::ForgeRepo { sub_path, .. } => sub_path,
        }
    }

    /// Whether this backing uses the project's own repo.
    pub fn is_vault_repo(&self) -> bool {
        matches!(self, Self::VaultRepo { .. })
    }

    /// Whether this backing is managed by a forge via Scribe.
    pub fn is_forge_repo(&self) -> bool {
        matches!(self, Self::ForgeRepo { .. })
    }

    /// Resolve the absolute repo root directory.
    ///
    /// For `VaultRepo`, returns `vault_root`.
    /// For `ExternalRepo` and `ForgeRepo`, returns their own root path.
    pub fn repo_root(&self, vault_root: &Path) -> PathBuf {
        match self {
            Self::VaultRepo { .. } => vault_root.to_path_buf(),
            Self::ExternalRepo { repo_root, .. } => repo_root.clone(),
            Self::ForgeRepo { local_path, .. } => local_path.clone(),
        }
    }

    /// Resolve the absolute path to the data directory (repo_root + sub_path).
    pub fn data_root(&self, vault_root: &Path) -> PathBuf {
        self.repo_root(vault_root).join(self.sub_path())
    }
}

/// Configuration for project-level atomic commits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProjectCommitConfig {
    /// Auto-commit debounce in seconds. 0 = manual only.
    #[serde(default)]
    pub auto_commit_seconds: u64,
    /// Commit message prefix (e.g. "[flynt:my-project]").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_prefix: Option<String>,
}

// ── Omegon profile + Flynt operator settings ────────────────────────────────

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
pub struct FlyntOperatorSettings {
    pub active_persona: String,
    pub enabled_skills: Vec<String>,
    pub preferred_extensions: Vec<String>,
    pub rail_extension: String,
    /// Agent profile ID to apply when starting an ACP session.
    /// None = default agent (no --agent flag passed to omegon acp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub vox: VoxSettings,
    /// Persisted ACP config — model, thinking level, posture, etc.
    /// Keys are config option IDs, values are the selected value IDs.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub acp_config: std::collections::HashMap<String, String>,
    /// Per-project agent daemon configuration — model, posture, vox channels.
    #[serde(default)]
    pub agent_daemon: crate::daemon::AgentDaemonConfig,
    /// Design canvas settings — default theme, grid, asset bootstrap state.
    /// Phase 1+2 ship the field with defaults; Phase 4 fills it in.
    #[serde(default)]
    pub canvas: crate::canvas::CanvasSettings,
}

impl Default for FlyntOperatorSettings {
    fn default() -> Self {
        Self {
            active_persona: "off".into(),
            enabled_skills: Vec::new(),
            preferred_extensions: vec!["vox".into()],
            rail_extension: "vox".into(),
            agent_id: None,
            vox: VoxSettings::default(),
            acp_config: std::collections::HashMap::new(),
            agent_daemon: crate::daemon::AgentDaemonConfig::default(),
            canvas: crate::canvas::CanvasSettings::default(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    // ── DecayRate ────────────────────────────────────────────────────

    #[test]
    fn decay_rate_none_has_no_half_life() {
        assert_eq!(DecayRate::None.half_life_days(), Option::None);
    }

    #[test]
    fn decay_rate_variants_have_correct_half_lives() {
        assert_eq!(DecayRate::Slow.half_life_days(), Some(14.0));
        assert_eq!(DecayRate::Natural.half_life_days(), Some(7.0));
        assert_eq!(DecayRate::Fast.half_life_days(), Some(3.0));
    }

    #[test]
    fn decay_rate_custom_clamps_to_minimum() {
        assert_eq!(DecayRate::Custom(0.05).half_life_days(), Some(0.1));
        assert_eq!(DecayRate::Custom(0.0).half_life_days(), Some(0.1));
        assert_eq!(DecayRate::Custom(-5.0).half_life_days(), Some(0.1));
        assert_eq!(DecayRate::Custom(30.0).half_life_days(), Some(30.0));
    }

    #[test]
    fn decay_rate_default_is_natural() {
        assert_eq!(DecayRate::default(), DecayRate::Natural);
    }

    // ── Task relevance ──────────────────────────────────────────────

    fn make_task(decay: DecayRate, status: TaskStatus, touched_ago: Duration) -> Task {
        let anchor = Utc::now() - touched_ago;
        Task {
            id: TaskId(uuid::Uuid::new_v4()),
            board_id: BoardId(uuid::Uuid::new_v4()),
            column: "Backlog".into(),
            title: "Test".into(),
            description: String::new(),
            priority: Priority::Medium,
            status,
            tags: vec![],
            document_refs: vec![],
            external_refs: vec![],
            due_date: None,
            position: 0,
            created_at: anchor,
            updated_at: anchor,
            decay,
            last_touched_at: Some(anchor),
            design_node_id: None,
            openspec_change: None,
            engagement_id: None,
            execution: None,
        }
    }

    #[test]
    fn relevance_fresh_task_is_one() {
        let task = make_task(DecayRate::Natural, TaskStatus::Todo, Duration::zero());
        assert!((task.relevance() - 1.0).abs() < 0.01);
    }

    #[test]
    fn relevance_no_decay_always_one() {
        let task = make_task(DecayRate::None, TaskStatus::Todo, Duration::days(365));
        assert_eq!(task.relevance(), 1.0);
    }

    #[test]
    fn relevance_done_always_zero() {
        let task = make_task(DecayRate::Natural, TaskStatus::Done, Duration::zero());
        assert_eq!(task.relevance(), 0.0);
    }

    #[test]
    fn relevance_archived_always_zero() {
        let task = make_task(DecayRate::Natural, TaskStatus::Archived, Duration::zero());
        assert_eq!(task.relevance(), 0.0);
    }

    #[test]
    fn relevance_decays_over_time() {
        let task = make_task(DecayRate::Natural, TaskStatus::Todo, Duration::days(7));
        let r = task.relevance();
        // After exactly one half-life (7 days), should be ~0.5
        assert!(r > 0.4 && r < 0.6, "expected ~0.5, got {r}");
    }

    #[test]
    fn relevance_fast_decay_drops_quickly() {
        let task = make_task(DecayRate::Fast, TaskStatus::Todo, Duration::days(6));
        let r = task.relevance();
        // After 2 half-lives (3 days * 2 = 6), should be ~0.25
        assert!(r > 0.2 && r < 0.3, "expected ~0.25, got {r}");
    }

    #[test]
    fn is_fading_below_threshold() {
        // 21 days with 7-day half-life = 3 half-lives = 0.125 relevance
        let task = make_task(DecayRate::Natural, TaskStatus::Todo, Duration::days(21));
        assert!(task.is_fading());
    }

    #[test]
    fn is_fading_above_threshold() {
        let task = make_task(DecayRate::Natural, TaskStatus::Todo, Duration::days(1));
        assert!(!task.is_fading());
    }

    #[test]
    fn should_auto_archive_very_old() {
        // 28 days with 7-day half-life = 4 half-lives = 0.0625 relevance
        let task = make_task(DecayRate::Natural, TaskStatus::Todo, Duration::days(28));
        assert!(task.should_auto_archive());
    }

    #[test]
    fn should_not_auto_archive_recent() {
        let task = make_task(DecayRate::Natural, TaskStatus::Todo, Duration::days(7));
        assert!(!task.should_auto_archive());
    }

    #[test]
    fn touch_resets_decay_clock() {
        let mut task = make_task(DecayRate::Natural, TaskStatus::Todo, Duration::days(30));
        assert!(task.relevance() < 0.1);
        task.touch();
        assert!(task.relevance() > 0.99);
    }

    // ── Notification ────────────────────────────────────────────────

    #[test]
    fn notification_new_has_correct_fields() {
        let n = Notification::new(NotificationKind::DueDate, "Due", "Task is due", "my-project");
        assert_eq!(n.kind, NotificationKind::DueDate);
        assert_eq!(n.title, "Due");
        assert_eq!(n.body, "Task is due");
        assert_eq!(n.source_vault, "my-project");
        assert!(n.task_id.is_none());
        assert!(n.delivered_at.is_none());
    }

    #[test]
    fn notification_for_task_sets_task_id() {
        let tid = TaskId(uuid::Uuid::new_v4());
        let n = Notification::new(NotificationKind::Decay, "Fading", "...", "project")
            .for_task(tid.clone());
        assert_eq!(n.task_id, Some(tid));
    }

    // ── Board ───────────────────────────────────────────────────────

    #[test]
    fn board_default_sprint_has_lifecycle_columns() {
        let board = Board::default_sprint("Sprint 1");
        assert_eq!(board.name, "Sprint 1");
        assert_eq!(board.columns.len(), 5);
        assert_eq!(board.columns[0].name, "Backlog");
        assert_eq!(board.columns[1].name, "Scheduled");
        assert_eq!(board.columns[2].name, "Running");
        assert_eq!(board.columns[3].name, "Done");
        assert_eq!(board.columns[4].name, "Failed");
        assert!(board.project_id.is_none());
    }

    #[test]
    fn board_for_project_sets_project_id() {
        let pid = uuid::Uuid::new_v4();
        let board = Board::for_project("Project Board", pid);
        assert_eq!(board.project_id, Some(pid));
    }

    // ── Task constructors ───────────────────────────────────────────

    #[test]
    fn task_new_defaults() {
        let bid = BoardId(uuid::Uuid::new_v4());
        let task = Task::new(bid.clone(), "Backlog", "Do the thing");
        assert_eq!(task.board_id, bid);
        assert_eq!(task.column, "Backlog");
        assert_eq!(task.title, "Do the thing");
        assert_eq!(task.status, TaskStatus::Todo);
        assert_eq!(task.priority, Priority::Medium);
        assert_eq!(task.decay, DecayRate::Natural);
    }

    #[test]
    fn task_new_tracked_no_decay() {
        let bid = BoardId(uuid::Uuid::new_v4());
        let task = Task::new_tracked(bid, "Backlog", "Tracked");
        assert_eq!(task.decay, DecayRate::None);
    }

    // ── FontSizePreset ──────────────────────────────────────────────

    #[test]
    fn font_size_labels() {
        assert_eq!(FontSizePreset::Small.label(), "S");
        assert_eq!(FontSizePreset::Medium.label(), "M");
        assert_eq!(FontSizePreset::Large.label(), "L");
        assert_eq!(FontSizePreset::XLarge.label(), "XL");
    }

    #[test]
    fn font_size_css_classes() {
        assert_eq!(FontSizePreset::Small.css_class(), "font-sm");
        assert_eq!(FontSizePreset::Medium.css_class(), "font-md");
        assert_eq!(FontSizePreset::Large.css_class(), "font-lg");
        assert_eq!(FontSizePreset::XLarge.css_class(), "font-xl");
    }

    // ── Version resolver tests ──────────────────────────────────────────

    #[test]
    fn parse_version_key_basic() {
        assert_eq!(super::parse_version_key("v0.17.0"), (0, 17, 0, "".into()));
        assert_eq!(super::parse_version_key("0.16.1"), (0, 16, 1, "".into()));
        assert_eq!(super::parse_version_key("v0.17.0-rc.1"), (0, 17, 0, "rc.1".into()));
        assert_eq!(super::parse_version_key("v1.2.3-nightly.20260425"), (1, 2, 3, "nightly.20260425".into()));
    }

    #[test]
    fn semver_sort_orders_correctly() {
        let mut versions = vec![
            &"v0.9.0".to_string(),
            &"v0.17.0".to_string(),
            &"v0.16.1".to_string(),
            &"v1.0.0".to_string(),
            &"v0.17.0-rc.1".to_string(),
        ];
        // Borrow checker: need owned strings
        let owned: Vec<String> = vec!["v0.9.0".into(), "v0.17.0".into(), "v0.16.1".into(), "v1.0.0".into(), "v0.17.0-rc.1".into()];
        let mut refs: Vec<&String> = owned.iter().collect();
        super::sort_versions_newest_first(&mut refs);
        let names: Vec<&str> = refs.iter().map(|s| s.as_str()).collect();
        // v1.0.0 > v0.17.0-rc.1 > v0.17.0 > v0.16.1 > v0.9.0
        assert_eq!(names[0], "v1.0.0");
        assert_eq!(names[1], "v0.17.0-rc.1"); // rc sorts after release because "rc.1" > ""
        assert_eq!(names[2], "v0.17.0");
        assert_eq!(names[3], "v0.16.1");
        assert_eq!(names[4], "v0.9.0");
    }

    #[test]
    fn resolve_from_versions_dir_stable() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("v0.16.1")).unwrap();
        std::fs::create_dir_all(tmp.path().join("v0.17.0-rc.1")).unwrap();
        std::fs::write(tmp.path().join("v0.16.1/omegon"), "bin").unwrap();
        std::fs::write(tmp.path().join("v0.17.0-rc.1/omegon"), "bin").unwrap();

        let result = super::resolve_from_versions_dir(tmp.path(), &OmegonChannel::Stable);
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("v0.16.1/omegon"));
    }

    #[test]
    fn resolve_from_versions_dir_rc() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("v0.16.1")).unwrap();
        std::fs::create_dir_all(tmp.path().join("v0.17.0-rc.1")).unwrap();
        std::fs::write(tmp.path().join("v0.16.1/omegon"), "bin").unwrap();
        std::fs::write(tmp.path().join("v0.17.0-rc.1/omegon"), "bin").unwrap();

        let result = super::resolve_from_versions_dir(tmp.path(), &OmegonChannel::Rc);
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("v0.17.0-rc.1/omegon"));
    }

    #[test]
    fn resolve_from_versions_dir_pinned() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("v0.16.1")).unwrap();
        std::fs::create_dir_all(tmp.path().join("v0.17.0-rc.1")).unwrap();
        std::fs::write(tmp.path().join("v0.16.1/omegon"), "bin").unwrap();
        std::fs::write(tmp.path().join("v0.17.0-rc.1/omegon"), "bin").unwrap();

        let result = super::resolve_from_versions_dir(tmp.path(), &OmegonChannel::Pinned("0.16.1".into()));
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("v0.16.1/omegon"));

        let missing = super::resolve_from_versions_dir(tmp.path(), &OmegonChannel::Pinned("0.15.0".into()));
        assert!(missing.is_none());
    }

    #[test]
    fn resolve_from_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = super::resolve_from_versions_dir(tmp.path(), &OmegonChannel::Stable);
        assert!(result.is_none());
    }

    #[test]
    fn visualization_config_defaults() {
        let config = VisualizationConfig::default();
        assert!(config.excalidraw_auto_export);
        assert!(config.d2_auto_render);
        assert_eq!(config.d2_theme, 200);
        assert_eq!(config.d2_layout, "elk");
        assert!(config.d2_bin.is_none());
    }

    #[test]
    fn visualization_config_serde_roundtrip() {
        let config = VisualizationConfig {
            excalidraw_auto_export: false,
            d2_auto_render: true,
            d2_theme: 0,
            d2_layout: "dagre".into(),
            d2_bin: Some("/usr/local/bin/d2".into()),
        };
        let toml = toml::to_string(&config).unwrap();
        let parsed: VisualizationConfig = toml::from_str(&toml).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn omegon_channel_serde_roundtrip() {
        for channel in [OmegonChannel::Stable, OmegonChannel::Rc, OmegonChannel::Nightly] {
            let json = serde_json::to_string(&channel).unwrap();
            let parsed: OmegonChannel = serde_json::from_str(&json).unwrap();
            assert_eq!(channel, parsed);
        }
        let pinned = OmegonChannel::Pinned("0.17.0-rc.1".into());
        let json = serde_json::to_string(&pinned).unwrap();
        let parsed: OmegonChannel = serde_json::from_str(&json).unwrap();
        assert_eq!(pinned, parsed);
    }

    // ── IndexingConfig scoping ─────────────────────────────────────

    #[test]
    fn no_scopes_uses_vault_wide_default() {
        let cfg = IndexingConfig { write_frontmatter: true, scopes: vec![] };
        assert!(cfg.should_write_frontmatter(Path::new("README.md")));
        assert_eq!(cfg.file_tier(Path::new("README.md")), FileTier::Managed);

        let cfg = IndexingConfig { write_frontmatter: false, scopes: vec![] };
        assert!(!cfg.should_write_frontmatter(Path::new("README.md")));
        assert_eq!(cfg.file_tier(Path::new("README.md")), FileTier::Discoverable);
    }

    #[test]
    fn scope_overrides_vault_default() {
        let cfg = IndexingConfig {
            write_frontmatter: false,
            scopes: vec![IndexScope {
                prefix: PathBuf::from("design/"),
                kind: Some("design_node".into()),
                write_frontmatter: Some(true),
            }],
        };
        assert!(cfg.should_write_frontmatter(Path::new("design/omega.md")));
        assert!(!cfg.should_write_frontmatter(Path::new("README.md")));
        assert!(!cfg.should_write_frontmatter(Path::new("core/README.md")));
    }

    #[test]
    fn longest_prefix_wins() {
        let cfg = IndexingConfig {
            write_frontmatter: false,
            scopes: vec![
                IndexScope {
                    prefix: PathBuf::from("docs/"),
                    kind: Some("document".into()),
                    write_frontmatter: Some(true),
                },
                IndexScope {
                    prefix: PathBuf::from("docs/internal/"),
                    kind: None,
                    write_frontmatter: Some(false),
                },
            ],
        };
        assert!(cfg.should_write_frontmatter(Path::new("docs/guide.md")));
        assert!(!cfg.should_write_frontmatter(Path::new("docs/internal/notes.md")));
    }

    #[test]
    fn scope_without_write_override_falls_through() {
        let cfg = IndexingConfig {
            write_frontmatter: true,
            scopes: vec![IndexScope {
                prefix: PathBuf::from("design/"),
                kind: Some("design_node".into()),
                write_frontmatter: None,
            }],
        };
        assert!(cfg.should_write_frontmatter(Path::new("design/omega.md")));

        let scope = cfg.scope_for_path(Path::new("design/omega.md"));
        assert!(scope.is_some());
        assert_eq!(scope.unwrap().kind.as_deref(), Some("design_node"));
    }

    #[test]
    fn scope_for_path_returns_none_outside_scopes() {
        let cfg = IndexingConfig {
            write_frontmatter: false,
            scopes: vec![IndexScope {
                prefix: PathBuf::from("design/"),
                kind: Some("design_node".into()),
                write_frontmatter: Some(true),
            }],
        };
        assert!(cfg.scope_for_path(Path::new("README.md")).is_none());
        assert!(cfg.scope_for_path(Path::new("core/src/main.rs")).is_none());
    }

    #[test]
    fn indexing_config_serde_roundtrip_with_scopes() {
        let cfg = IndexingConfig {
            write_frontmatter: false,
            scopes: vec![
                IndexScope {
                    prefix: PathBuf::from("design/"),
                    kind: Some("design_node".into()),
                    write_frontmatter: Some(true),
                },
                IndexScope {
                    prefix: PathBuf::from("docs/"),
                    kind: Some("document".into()),
                    write_frontmatter: None,
                },
            ],
        };
        let toml_str = toml::to_string(&cfg).unwrap();
        let parsed: IndexingConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn indexing_config_serde_backwards_compat() {
        let old_toml = "write_frontmatter = true\n";
        let parsed: IndexingConfig = toml::from_str(old_toml).unwrap();
        assert!(parsed.write_frontmatter);
        assert!(parsed.scopes.is_empty());
    }
}
