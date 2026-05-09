use crate::{
    datum::EntityKind,
    models::{
        Board, BoardId, Document, DocumentId, DocumentMeta, SearchResult, Task, TaskId,
    },
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct DocumentMetadataFilter {
    pub field: String,
    pub value: String,
}
/// Filter for task queries.
///
/// All fields combine with AND semantics. Tags require ALL listed tags to
/// be present on the task (intersection, not union). Sentry's
/// `list_actionable()` calls into the `FlyntTaskBoard` adapter with
/// column/status/tag filters to discover ready work — see
/// flynt/design/sentry-integration.md priority 4.
#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub board_id: Option<BoardId>,
    pub column: Option<String>,
    pub tags: Vec<String>,
    pub status: Option<crate::models::TaskStatus>,
}

/// The storage abstraction. Implementations live in `flynt-store`.
/// Both the UI and the MCP agent go through this trait.
pub trait VaultStore: Send + Sync {
    // ── Documents ─────────────────────────────────────────────────────────────
    fn get_document(&self, id: &DocumentId) -> Result<Option<Document>>;
    fn get_document_by_path(&self, path: &Path) -> Result<Option<Document>>;
    /// Find a document whose title or filename slug loosely matches `slug`.
    fn find_document_by_slug(&self, slug: &str) -> Result<Option<DocumentMeta>>;
    fn list_documents(&self) -> Result<Vec<DocumentMeta>>;
    fn list_documents_by_metadata(&self, filter: &DocumentMetadataFilter) -> Result<Vec<DocumentMeta>>;
    fn save_document(&self, doc: &Document) -> Result<()>;
    fn delete_document(&self, id: &DocumentId) -> Result<()>;
    fn search_documents(&self, query: &str) -> Result<Vec<SearchResult>>;
    fn get_backlinks(&self, id: &DocumentId) -> Result<Vec<DocumentMeta>>;

    // ── Entities ─────────────────────────────────────────────────────────────
    /// List documents that are typed entities of a given kind.
    fn list_entities_by_kind(&self, kind: &EntityKind) -> Result<Vec<DocumentMeta>>;

    // ── Tasks ─────────────────────────────────────────────────────────────────
    fn get_task(&self, id: &TaskId) -> Result<Option<Task>>;
    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>>;
    fn save_task(&self, task: &Task) -> Result<()>;
    fn delete_task(&self, id: &TaskId) -> Result<()>;
    /// Apply a partial update to an existing task. Only the `Some(_)` fields
    /// in `patch` are modified; `None` means leave-unchanged. `updated_at`
    /// is bumped automatically. Returns Ok(false) if no task with that id
    /// exists, Ok(true) on a successful update. Empty patches are a no-op
    /// and return Ok(true) without writing.
    ///
    /// Foundational for the sentry integration's claim/release/complete
    /// semantics — see flynt/design/sentry-integration.md.
    fn update_task(&self, id: &TaskId, patch: &flynt_models::TaskPatch) -> Result<bool>;

    // ── Boards ────────────────────────────────────────────────────────────────
    fn get_board(&self, id: &BoardId) -> Result<Option<Board>>;
    fn list_boards(&self) -> Result<Vec<Board>>;
    fn save_board(&self, board: &Board) -> Result<()>;
    fn delete_board(&self, id: &BoardId) -> Result<()>;

    // ── Project dirty tracking ───────────────────────────────────────────────
    /// List tasks for a project that have been modified since last commit.
    fn list_dirty_tasks(&self, project_id: &Uuid) -> Result<Vec<Task>>;
    /// List documents under a project that have been modified since last commit.
    fn list_dirty_documents(&self, project_id: &Uuid) -> Result<Vec<Document>>;
    /// Mark entities as committed at the given timestamp.
    fn mark_committed(
        &self,
        task_ids: &[TaskId],
        doc_ids: &[DocumentId],
        at: DateTime<Utc>,
    ) -> Result<()>;
    /// Record a soft-delete for a project entity (task/board) so the file
    /// can be removed from disk on next flush.
    fn record_project_deletion(
        &self,
        entity_id: &Uuid,
        entity_kind: &str,
        project_id: &Uuid,
    ) -> Result<()>;
    /// List pending (uncommitted) deletions for a project.
    fn list_pending_deletions(&self, project_id: &Uuid) -> Result<Vec<(Uuid, String)>>;
    /// Mark deletions as committed so they can be cleaned up.
    fn mark_deletions_committed(&self, entity_ids: &[Uuid]) -> Result<()>;
}
