//! Task model and related types.
//!
//! These are the canonical types for codex kanban tasks. They are defined here
//! (not in codex-core) so they can be consumed without heavy dependencies.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
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
}

impl Task {
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
