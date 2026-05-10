//! Task model and related types.
//!
//! These are the canonical types for flynt kanban tasks. They are defined here
//! (not in flynt-core) so they can be consumed without heavy dependencies.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

// ── Newtype IDs ─────────────────────────────────────────────────────────────

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

// ── Priority ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    #[default]
    Medium,
    Low,
    High,
    Critical,
}

// ── Status ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Todo,
    InProgress,
    Done,
    Archived,
}

// ── Decay ───────────────────────────────────────────────────────────────────

/// Controls how quickly a task loses relevance without interaction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecayRate {
    /// No decay — stays fully relevant until manually resolved.
    None,
    /// Slow decay — ~14 day half-life.
    Slow,
    /// Natural decay — ~7 day half-life. Default.
    Natural,
    /// Fast decay — ~3 day half-life.
    Fast,
    /// Custom half-life in days.
    Custom(f64),
}

impl Default for DecayRate {
    fn default() -> Self { Self::Natural }
}

impl DecayRate {
    /// Half-life in days. Returns None for non-decaying tasks.
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

// ── ExecutionSpec ───────────────────────────────────────────────────────────
//
// Sentry execution parameters attached to a task. Mirrors the wire shape of
// `omegon::sentry::types::TaskSpec` (minus `prompt`, which is `task.description`)
// so the planned `FlyntTaskBoard` adapter is a thin pass-through and not a
// translation layer. See flynt/design/sentry-integration.md priority 3.
//
// On disk this lives under `[data.execution]` in the task file's TOML
// frontmatter. In storage it round-trips as a JSON blob in a single SQLite
// column.

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExecutionSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Extra env vars to inject into the spawned task process. Empty map is
    /// serialized as absent so blank tasks don't pollute the on-disk form.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl ExecutionSpec {
    /// True when no field is meaningful — used to skip serializing an empty
    /// execution block to disk.
    pub fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.skill.is_none()
            && self.max_turns.is_none()
            && self.timeout_secs.is_none()
            && self.token_budget.is_none()
            && self.cwd.is_none()
            && self.env.is_empty()
    }
}

// ── TaskPatch ───────────────────────────────────────────────────────────────
//
// Partial-update payload for `ProjectStore::update_task`. Only the `Some(_)`
// fields are applied; `None` means "leave unchanged." This is the contract
// the sentry integration relies on (see flynt/design/sentry-integration.md
// — claim/release/complete need to mutate status + column without
// clobbering description/tags/etc).
//
// `due_date: Option<Option<NaiveDate>>` is the standard sentinel: `None`
// means leave-unchanged, `Some(None)` means clear, `Some(Some(d))` sets.
// Same shape applies to `design_node_id`.

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Some(None) clears the due date; Some(Some(d)) sets it; None leaves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<Option<NaiveDate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_refs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_refs: Option<Vec<DocumentId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decay: Option<DecayRate>,
    /// Some(None) clears; Some(Some(uuid)) sets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_node_id: Option<Option<Uuid>>,
    /// Some(None) clears; Some(Some(name)) sets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openspec_change: Option<Option<String>>,
    /// Some(None) clears the engagement link; Some(Some(id)) sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engagement_id: Option<Option<crate::engagement::EngagementId>>,
    /// Some(None) clears the execution block; Some(Some(spec)) replaces it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<Option<ExecutionSpec>>,
}

impl TaskPatch {
    /// True when no field is set — caller can short-circuit and skip the
    /// roundtrip to storage.
    pub fn is_empty(&self) -> bool {
        self.column.is_none()
            && self.title.is_none()
            && self.description.is_none()
            && self.priority.is_none()
            && self.status.is_none()
            && self.tags.is_none()
            && self.due_date.is_none()
            && self.external_refs.is_none()
            && self.document_refs.is_none()
            && self.position.is_none()
            && self.decay.is_none()
            && self.design_node_id.is_none()
            && self.openspec_change.is_none()
            && self.engagement_id.is_none()
            && self.execution.is_none()
    }
}

// ── Task ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub board_id: BoardId,
    pub column: String,
    pub title: String,
    /// Markdown body.
    pub description: String,
    pub priority: Priority,
    pub status: TaskStatus,
    pub tags: Vec<String>,
    /// Documents linked to this task.
    pub document_refs: Vec<DocumentId>,
    /// External references — URLs to forge issues, PRs, etc.
    #[serde(default)]
    pub external_refs: Vec<String>,
    pub due_date: Option<NaiveDate>,
    /// Ordering position within column.
    pub position: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub decay: DecayRate,
    #[serde(default)]
    pub last_touched_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_node_id: Option<Uuid>,
    /// OpenSpec change name this task implements/verifies. Set by the agent
    /// or operator when the task is part of a spec-driven flow; consumed
    /// by sentry's lifecycle hooks to advance the change's stage on
    /// completion. None for tasks unrelated to OpenSpec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openspec_change: Option<String>,
    /// Engagement scope this task belongs to. Populated when the task is
    /// part of a multi-repo engagement (see flynt_models::engagement);
    /// used by the kanban filter pill and by the agent to resolve which
    /// repo binding owns forge issues linked from `external_refs`. None
    /// for un-scoped / personal tasks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engagement_id: Option<crate::engagement::EngagementId>,
    /// Sentry execution parameters. None for human-only tasks; populated
    /// when the task is intended for autonomous execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionSpec>,
}

impl Task {
    /// True when the task carries any signal that omegon's sentry would
    /// pick it up: a cron / webhook trigger in `external_refs`, or an
    /// `execution` block with at least one configured field. Predicate-
    /// driven detection (no separate flag) means cards self-correct as
    /// fields populate. Used by Flynt's UI to surface sentry-aware chips
    /// only when there's something to show.
    pub fn is_sentry_managed(&self) -> bool {
        self.execution.as_ref().map(|e| !e.is_empty()).unwrap_or(false)
            || self.external_refs.iter().any(|r| {
                r.starts_with("cron:") || r.starts_with("webhook:")
            })
    }

    /// Extract the cron expression from external_refs if any (just the
    /// expression part, no `cron:` prefix). Returns the first match;
    /// multiple cron entries on one task is unusual but allowed.
    pub fn cron_trigger(&self) -> Option<&str> {
        self.external_refs
            .iter()
            .find_map(|r| r.strip_prefix("cron:"))
    }

    /// Extract the webhook name from external_refs if any.
    pub fn webhook_trigger(&self) -> Option<&str> {
        self.external_refs
            .iter()
            .find_map(|r| r.strip_prefix("webhook:"))
    }

    pub fn new(
        board_id: BoardId,
        column: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: TaskId::new(),
            board_id,
            column: column.into(),
            title: title.into(),
            description: String::new(),
            priority: Priority::Medium,
            status: TaskStatus::Todo,
            tags: Vec::new(),
            document_refs: Vec::new(),
            external_refs: Vec::new(),
            due_date: None,
            position: u32::MAX,
            created_at: now,
            updated_at: now,
            decay: DecayRate::default(),
            last_touched_at: None,
            design_node_id: None,
            openspec_change: None,
            engagement_id: None,
            execution: None,
        }
    }

    /// Create a non-decaying task.
    pub fn new_tracked(
        board_id: BoardId,
        column: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        let mut task = Self::new(board_id, column, title);
        task.decay = DecayRate::None;
        task
    }

    /// Compute the current relevance score (0.0–1.0).
    pub fn relevance(&self) -> f64 {
        if matches!(self.status, TaskStatus::Done | TaskStatus::Archived) {
            return 0.0;
        }
        let half_life = match self.decay.half_life_days() {
            Some(hl) => hl,
            Option::None => return 1.0,
        };
        let anchor = self.last_touched_at.unwrap_or(self.updated_at);
        let elapsed_days = (Utc::now() - anchor).num_seconds() as f64 / 86400.0;
        if elapsed_days <= 0.0 {
            return 1.0;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> Task {
        Task::new(BoardId::new(), "Backlog", "T")
    }

    #[test]
    fn fresh_task_is_not_sentry_managed() {
        assert!(!task().is_sentry_managed());
    }

    #[test]
    fn cron_external_ref_marks_sentry_managed() {
        let mut t = task();
        t.external_refs = vec!["cron:0 */4 * * *".into()];
        assert!(t.is_sentry_managed());
        assert_eq!(t.cron_trigger(), Some("0 */4 * * *"));
    }

    #[test]
    fn webhook_external_ref_marks_sentry_managed() {
        let mut t = task();
        t.external_refs = vec!["webhook:gh-pr".into()];
        assert!(t.is_sentry_managed());
        assert_eq!(t.webhook_trigger(), Some("gh-pr"));
    }

    #[test]
    fn execution_block_marks_sentry_managed() {
        let mut t = task();
        t.execution = Some(ExecutionSpec {
            model: Some("anthropic:claude".into()),
            ..Default::default()
        });
        assert!(t.is_sentry_managed());
    }

    #[test]
    fn empty_execution_block_does_not_mark_sentry_managed() {
        // is_empty() check on ExecutionSpec — a Some(default) shouldn't
        // count, only a meaningfully populated one.
        let mut t = task();
        t.execution = Some(ExecutionSpec::default());
        assert!(!t.is_sentry_managed());
    }

    #[test]
    fn unrelated_external_refs_do_not_mark_sentry_managed() {
        // GitHub URLs and similar are external_refs but not sentry triggers.
        let mut t = task();
        t.external_refs = vec!["https://github.com/org/repo/issues/42".into()];
        assert!(!t.is_sentry_managed());
        assert!(t.cron_trigger().is_none());
        assert!(t.webhook_trigger().is_none());
    }
}
