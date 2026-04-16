use crate::models::{
    Board, BoardId, Document, DocumentId, DocumentMeta, SearchResult, Task, TaskId,
};
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct DocumentMetadataFilter {
    pub field: String,
    pub value: String,
}
/// Filter for task queries.
#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub board_id: Option<BoardId>,
    pub column: Option<String>,
    pub tags: Vec<String>,
}

/// The storage abstraction. Implementations live in `codex-store`.
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

    // ── Tasks ─────────────────────────────────────────────────────────────────
    fn get_task(&self, id: &TaskId) -> Result<Option<Task>>;
    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>>;
    fn save_task(&self, task: &Task) -> Result<()>;
    fn delete_task(&self, id: &TaskId) -> Result<()>;

    // ── Boards ────────────────────────────────────────────────────────────────
    fn get_board(&self, id: &BoardId) -> Result<Option<Board>>;
    fn list_boards(&self) -> Result<Vec<Board>>;
    fn save_board(&self, board: &Board) -> Result<()>;
}
