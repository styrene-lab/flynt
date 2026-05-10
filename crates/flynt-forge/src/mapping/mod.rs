//! Task ↔ Issue field mapping — provider-agnostic.
//!
//! The flow editor's lesson applied here: one schema, one trait, three
//! mappers behind it. Push/pull code never touches provider-specific
//! types past the `TaskFieldMapper` boundary.
//!
//! ## What gets mapped
//!
//! Eight flynt task fields, four push directions. The defaults are
//! sensible; operators override via `<project>/.flynt/forge-mapping.toml`.
//!
//! | flynt field    | push behavior (default)                              |
//! |----------------|------------------------------------------------------|
//! | `title`        | → issue title (string)                               |
//! | `description`  | → issue body (markdown)                              |
//! | `status`       | → issue state (open/closed) + optional `status:*` label |
//! | `priority`     | → label `priority:<level>` (configurable prefix)     |
//! | `tags`         | → labels (passthrough; non-prefixed labels preserved)|
//! | `due_date`     | → milestone (configurable: milestone / label / skip) |
//! | `column`       | not pushed (organizational; flynt-local)             |
//! | `engagement`   | not pushed (project-level; flynt-local)              |
//! | `design_node`  | not pushed (parent reference; flynt-local)           |
//! | `decay`        | not pushed (lifecycle-only; flynt-local)             |
//! | `position`     | not pushed (kanban-only; flynt-local)                |
//!
//! ## The labels-as-fields contract
//!
//! flynt encodes some structured fields (priority, optionally status)
//! as prefixed labels. The contract:
//!
//! - **On push**: replace ONLY labels matching our managed prefixes.
//!   Non-matching labels (operator-managed: `good-first-issue`,
//!   `infra`, etc.) round-trip verbatim.
//! - **On pull**: read prefixed labels back into typed fields. Unprefixed
//!   labels land in `tags`.
//!
//! `MappingConfig::owned_label_prefixes` exposes the prefix set so the
//! push code knows which labels to rewrite.

pub mod github;

pub use github::GitHubMapper;

use anyhow::Result;
use flynt_models::task::{Task, TaskPatch};
use styrene_forge::{CreateIssue, ForgeIssue, UpdateIssue};

/// Maps flynt `Task` ↔ provider `ForgeIssue` shapes. One impl per
/// `ForgeKind`. Stateless: config-in, payload-out.
pub trait TaskFieldMapper: Send + Sync {
    /// Build the create payload for a task that's never been pushed.
    /// `cfg` controls which fields are included and how they're encoded.
    fn task_to_issue_create(&self, task: &Task, cfg: &MappingConfig) -> CreateIssue;

    /// Build the update payload for a task that already has an
    /// upstream issue. Only includes fields that differ from `current`
    /// — minimizes API surface and keeps the operator's other-tool
    /// edits intact.
    fn task_to_issue_update(
        &self,
        task: &Task,
        current: &ForgeIssue,
        cfg: &MappingConfig,
    ) -> UpdateIssue;

    /// Translate an upstream issue back into a `TaskPatch` — used by
    /// the pull direction. Prefixed labels become typed fields; the
    /// rest land in `tags`.
    fn issue_to_task_patch(&self, issue: &ForgeIssue, cfg: &MappingConfig) -> TaskPatch;
}

// ── Config ──────────────────────────────────────────────────────────────────

/// Per-project sync configuration. Loaded from
/// `<project>/.flynt/forge-mapping.toml` — defaults are baked into
/// code (see `Default::default`) so the file is optional. Operators
/// only need to write the file to override.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MappingConfig {
    #[serde(default)]
    pub sync_fields: SyncFields,
    #[serde(default)]
    pub status_to_state: StatusStateMap,
    #[serde(default)]
    pub status_labels: StatusLabelMap,
    #[serde(default)]
    pub priority: PriorityMapping,
    #[serde(default)]
    pub due_date: DueDateMapping,
}

impl Default for MappingConfig {
    fn default() -> Self {
        Self {
            sync_fields: SyncFields::default(),
            status_to_state: StatusStateMap::default(),
            status_labels: StatusLabelMap::default(),
            priority: PriorityMapping::default(),
            due_date: DueDateMapping::default(),
        }
    }
}

impl MappingConfig {
    /// The label prefixes flynt manages. On push we rewrite labels
    /// matching these; everything else is preserved as the operator's
    /// own labels.
    pub fn owned_label_prefixes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.priority.label_prefix.is_empty() {
            out.push(self.priority.label_prefix.clone());
        }
        // status labels are full strings (e.g. "status:in-progress"),
        // not a prefix — derive the prefix from the first one.
        if let Some(first) = self.status_labels.values().next() {
            if let Some(colon) = first.find(':') {
                out.push(first[..=colon].to_string());
            }
        }
        // Due-date label is flynt-managed when the strategy is Label.
        // Without this, a stale `due:2026-01-01` would survive across
        // a push that moved due_date to a different date — both would
        // accumulate on the issue.
        if matches!(self.due_date.strategy, DueDateStrategy::Label) {
            out.push("due:".to_string());
        }
        out
    }

    /// Whether a label is one flynt owns (and may safely overwrite).
    pub fn owns_label(&self, label: &str) -> bool {
        self.owned_label_prefixes()
            .iter()
            .any(|p| label.starts_with(p))
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SyncFields {
    #[serde(default = "default_true")]
    pub title: bool,
    #[serde(default = "default_true")]
    pub description: bool,
    #[serde(default = "default_true")]
    pub status: bool,
    #[serde(default = "default_true")]
    pub priority: bool,
    #[serde(default = "default_true")]
    pub tags: bool,
    #[serde(default = "default_true")]
    pub due_date: bool,
}

impl Default for SyncFields {
    fn default() -> Self {
        Self {
            title: true,
            description: true,
            status: true,
            priority: true,
            tags: true,
            due_date: true,
        }
    }
}

fn default_true() -> bool { true }

/// Mapping from flynt's 5-state status to provider's binary state.
///
/// Forge providers (GitHub/GitLab/Forgejo issues) have an open/closed
/// binary state. flynt's status is richer (todo/in_progress/review/done/
/// archived). This struct says which statuses are "open" upstream vs
/// "closed" upstream. The richer detail rides along in `status_labels`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StatusStateMap {
    #[serde(default = "open_state")] pub todo: ProviderState,
    #[serde(default = "open_state")] pub in_progress: ProviderState,
    #[serde(default = "open_state")] pub review: ProviderState,
    #[serde(default = "closed_state")] pub done: ProviderState,
    #[serde(default = "closed_state")] pub archived: ProviderState,
}

impl Default for StatusStateMap {
    fn default() -> Self {
        Self {
            todo: ProviderState::Open,
            in_progress: ProviderState::Open,
            review: ProviderState::Open,
            done: ProviderState::Closed,
            archived: ProviderState::Closed,
        }
    }
}

impl StatusStateMap {
    pub fn for_status(&self, status: &str) -> ProviderState {
        match status {
            "todo" => self.todo,
            "in_progress" => self.in_progress,
            "review" => self.review,
            "done" => self.done,
            "archived" => self.archived,
            _ => self.todo,
        }
    }
}

fn open_state() -> ProviderState { ProviderState::Open }
fn closed_state() -> ProviderState { ProviderState::Closed }

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderState {
    Open,
    Closed,
}

impl From<ProviderState> for styrene_forge::IssueState {
    fn from(p: ProviderState) -> Self {
        match p {
            ProviderState::Open => styrene_forge::IssueState::Open,
            ProviderState::Closed => styrene_forge::IssueState::Closed,
        }
    }
}

/// Additional labels to attach per status — used when the provider's
/// open/closed binary loses the in_progress vs review vs todo distinction.
/// Default attaches `status:in-progress` and `status:review` since those
/// three statuses all collapse to `Open` upstream.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StatusLabelMap {
    #[serde(default = "default_in_progress_label")]
    pub in_progress: Option<String>,
    #[serde(default = "default_review_label")]
    pub review: Option<String>,
    #[serde(default)]
    pub todo: Option<String>,
    #[serde(default)]
    pub done: Option<String>,
    #[serde(default)]
    pub archived: Option<String>,
}

impl Default for StatusLabelMap {
    fn default() -> Self {
        Self {
            in_progress: Some("status:in-progress".into()),
            review: Some("status:review".into()),
            todo: None,
            done: None,
            archived: None,
        }
    }
}

impl StatusLabelMap {
    /// Iterate the configured (status, label) pairs.
    pub fn values(&self) -> impl Iterator<Item = &String> {
        [&self.in_progress, &self.review, &self.todo, &self.done, &self.archived]
            .into_iter()
            .filter_map(|o| o.as_ref())
    }

    pub fn for_status(&self, status: &str) -> Option<&String> {
        match status {
            "todo" => self.todo.as_ref(),
            "in_progress" => self.in_progress.as_ref(),
            "review" => self.review.as_ref(),
            "done" => self.done.as_ref(),
            "archived" => self.archived.as_ref(),
            _ => None,
        }
    }

    /// Reverse-lookup: given a label, return the status it represents.
    pub fn status_for_label(&self, label: &str) -> Option<&'static str> {
        if self.todo.as_deref() == Some(label) { return Some("todo"); }
        if self.in_progress.as_deref() == Some(label) { return Some("in_progress"); }
        if self.review.as_deref() == Some(label) { return Some("review"); }
        if self.done.as_deref() == Some(label) { return Some("done"); }
        if self.archived.as_deref() == Some(label) { return Some("archived"); }
        None
    }
}

fn default_in_progress_label() -> Option<String> { Some("status:in-progress".into()) }
fn default_review_label() -> Option<String> { Some("status:review".into()) }

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PriorityMapping {
    /// Label prefix. `"priority:"` produces `priority:low` / `priority:high`
    /// etc. An empty string means "don't sync priority" — sync_fields.priority
    /// is the cleaner toggle but this works for partial overrides.
    #[serde(default = "default_priority_prefix")]
    pub label_prefix: String,
}

impl Default for PriorityMapping {
    fn default() -> Self {
        Self { label_prefix: "priority:".into() }
    }
}

fn default_priority_prefix() -> String { "priority:".into() }

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DueDateMapping {
    /// Strategy: `"milestone"` (create/find a milestone with the date
    /// as its name), `"label"` (attach `due:YYYY-MM-DD`), or `"skip"`
    /// (don't push due_date upstream).
    #[serde(default = "default_due_strategy")]
    pub strategy: DueDateStrategy,
}

impl Default for DueDateMapping {
    fn default() -> Self {
        Self { strategy: DueDateStrategy::Milestone }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DueDateStrategy {
    Milestone,
    Label,
    Skip,
}

fn default_due_strategy() -> DueDateStrategy { DueDateStrategy::Milestone }

// ── Loading ─────────────────────────────────────────────────────────────────

/// Path to the per-project mapping config.
pub fn config_path(project_root: &std::path::Path) -> std::path::PathBuf {
    project_root.join(".flynt").join("forge-mapping.toml")
}

/// Load the mapping config from disk; falls back to `Default` when the
/// file is missing or unreadable. Errors only on malformed TOML — we
/// want a missing-file project to "just work" with sensible defaults.
pub fn load(project_root: &std::path::Path) -> Result<MappingConfig> {
    let path = config_path(project_root);
    if !path.exists() {
        return Ok(MappingConfig::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    let cfg: MappingConfig = toml::from_str(&raw)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips_through_toml() {
        let cfg = MappingConfig::default();
        let raw = toml::to_string_pretty(&cfg).unwrap();
        let parsed: MappingConfig = toml::from_str(&raw).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn defaults_sync_all_fields() {
        let cfg = MappingConfig::default();
        assert!(cfg.sync_fields.title);
        assert!(cfg.sync_fields.status);
        assert!(cfg.sync_fields.priority);
        assert!(cfg.sync_fields.tags);
    }

    #[test]
    fn default_status_state_maps_done_to_closed() {
        let cfg = MappingConfig::default();
        assert_eq!(cfg.status_to_state.for_status("todo"), ProviderState::Open);
        assert_eq!(cfg.status_to_state.for_status("in_progress"), ProviderState::Open);
        assert_eq!(cfg.status_to_state.for_status("done"), ProviderState::Closed);
        assert_eq!(cfg.status_to_state.for_status("archived"), ProviderState::Closed);
    }

    #[test]
    fn owned_label_prefixes_include_priority_and_status() {
        let cfg = MappingConfig::default();
        let prefixes = cfg.owned_label_prefixes();
        assert!(prefixes.iter().any(|p| p == "priority:"),
                "priority prefix present: {prefixes:?}");
        assert!(prefixes.iter().any(|p| p == "status:"),
                "status prefix present: {prefixes:?}");
    }

    #[test]
    fn owns_label_distinguishes_flynt_managed_from_operator_managed() {
        let cfg = MappingConfig::default();
        assert!(cfg.owns_label("priority:high"));
        assert!(cfg.owns_label("status:in-progress"));
        assert!(!cfg.owns_label("good-first-issue"));
        assert!(!cfg.owns_label("infra"));
        assert!(!cfg.owns_label("bug"));
    }

    #[test]
    fn due_label_prefix_in_owned_when_strategy_is_label() {
        // Without this, a push that changes due_date would leave the
        // old `due:OLD-DATE` label hanging on the issue alongside the
        // new one — both labels would accumulate.
        let mut cfg = MappingConfig::default();
        cfg.due_date.strategy = DueDateStrategy::Label;
        assert!(cfg.owned_label_prefixes().iter().any(|p| p == "due:"));
        assert!(cfg.owns_label("due:2026-01-01"));
        assert!(cfg.owns_label("due:2026-06-15"));
    }

    #[test]
    fn due_label_prefix_not_owned_for_milestone_strategy() {
        // When the strategy is Milestone, an operator-added `due:*`
        // label is not flynt-managed — it's just a regular label the
        // operator chose to use.
        let cfg = MappingConfig::default(); // Milestone is default
        assert!(!cfg.owns_label("due:2026-01-01"));
        assert!(!cfg.owned_label_prefixes().iter().any(|p| p == "due:"));
    }

    #[test]
    fn empty_priority_prefix_yields_no_priority_in_owned() {
        // Operator opts out of priority syncing via empty prefix — labels
        // they manually add starting with random strings aren't suddenly
        // flynt-managed.
        let mut cfg = MappingConfig::default();
        cfg.priority.label_prefix = String::new();
        let prefixes = cfg.owned_label_prefixes();
        assert!(!prefixes.iter().any(|p| p == "priority:"));
    }

    #[test]
    fn status_label_reverse_lookup() {
        let cfg = MappingConfig::default();
        assert_eq!(cfg.status_labels.status_for_label("status:in-progress"), Some("in_progress"));
        assert_eq!(cfg.status_labels.status_for_label("status:review"), Some("review"));
        assert_eq!(cfg.status_labels.status_for_label("priority:high"), None);
        assert_eq!(cfg.status_labels.status_for_label("unrelated"), None);
    }

    #[test]
    fn load_falls_back_to_default_when_file_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = load(tmp.path()).unwrap();
        assert_eq!(cfg, MappingConfig::default());
    }

    #[test]
    fn load_reads_overrides_from_toml() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".flynt")).unwrap();
        std::fs::write(
            tmp.path().join(".flynt/forge-mapping.toml"),
            r#"
[priority]
label_prefix = "prio:"

[due_date]
strategy = "skip"

[sync_fields]
priority = false
"#,
        ).unwrap();

        let cfg = load(tmp.path()).unwrap();
        assert_eq!(cfg.priority.label_prefix, "prio:");
        assert_eq!(cfg.due_date.strategy, DueDateStrategy::Skip);
        assert!(!cfg.sync_fields.priority);
        // Unspecified fields keep defaults.
        assert!(cfg.sync_fields.title);
    }

    #[test]
    fn load_errors_on_malformed_toml() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".flynt")).unwrap();
        std::fs::write(
            tmp.path().join(".flynt/forge-mapping.toml"),
            "this isn't valid TOML at all =====",
        ).unwrap();
        assert!(load(tmp.path()).is_err());
    }
}
