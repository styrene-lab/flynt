use codex_core::store::{TaskFilter, VaultStore};
use codex_store::vault::Vault;
use rmcp::{ServerHandler, handler::server::wrapper::Parameters, schemars, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

pub struct CodexToolHandler {
    vault: Arc<Vault>,
}

impl CodexToolHandler {
    pub fn new(vault: Arc<Vault>) -> Self {
        Self { vault }
    }
}

// ── Tool input schemas ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchInput {
    /// Full-text search query
    pub query: String,
    /// Maximum number of results (default 20)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PathInput {
    /// Path relative to vault root, e.g. "projects/codex.md"
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateDocumentInput {
    pub path: String,
    pub title: String,
    pub content: String,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTasksInput {
    pub column: Option<String>,
}

// ── Handler implementation ────────────────────────────────────────────────────

#[tool_router]
impl CodexToolHandler {
    /// Search documents in the vault using full-text search
    #[tool(description = "Full-text search across all vault documents. Returns matching documents with excerpts.")]
    async fn search_documents(&self, Parameters(input): Parameters<SearchInput>) -> String {
        let limit = input.limit.unwrap_or(20);
        match self.vault.store.search_documents(&input.query) {
            Ok(results) => {
                let results: Vec<_> = results.into_iter().take(limit).collect();
                serde_json::to_string_pretty(&results).unwrap_or_else(|e| e.to_string())
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    /// Get the full content of a document by path
    #[tool(description = "Retrieve full markdown content and metadata for a document by its vault-relative path.")]
    async fn get_document(&self, Parameters(input): Parameters<PathInput>) -> String {
        let path = std::path::Path::new(&input.path);
        match self.vault.store.get_document_by_path(path) {
            Ok(Some(doc)) => serde_json::to_string_pretty(&doc).unwrap_or_else(|e| e.to_string()),
            Ok(None) => format!("Document not found: {}", input.path),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// List all documents in the vault (metadata only)
    #[tool(description = "List all documents in the vault. Returns id, path, title, tags, and updated_at.")]
    async fn list_documents(&self) -> String {
        match self.vault.store.list_documents() {
            Ok(docs) => serde_json::to_string_pretty(&docs).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// Create or update a document in the vault
    #[tool(description = "Create or update a markdown document. Writes the file to disk and indexes it.")]
    async fn create_document(&self, Parameters(input): Parameters<CreateDocumentInput>) -> String {
        use std::fs;
        let abs_path = self.vault.root.join(&input.path);
        let tags_str = serde_json::to_string(&input.tags.unwrap_or_default())
            .unwrap_or_else(|_| "[]".to_string());
        let content =
            format!("+++\ntags = {tags_str}\n+++\n\n# {}\n\n{}", input.title, input.content);

        if let Some(parent) = abs_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return format!("Error creating directories: {e}");
            }
        }
        if let Err(e) = fs::write(&abs_path, &content) {
            return format!("Error writing file: {e}");
        }
        match self.vault.index_file(&abs_path) {
            Ok(_) => format!("Document created at {}", input.path),
            Err(e) => format!("File written but indexing failed: {e}"),
        }
    }

    /// Get backlinks for a document (other docs linking to it)
    #[tool(description = "List all documents that link to the specified document path.")]
    async fn get_backlinks(&self, Parameters(input): Parameters<PathInput>) -> String {
        let path = std::path::Path::new(&input.path);
        let doc = match self.vault.store.get_document_by_path(path) {
            Ok(Some(d)) => d,
            Ok(None) => return format!("Document not found: {}", input.path),
            Err(e) => return format!("Error: {e}"),
        };
        match self.vault.store.get_backlinks(&doc.id) {
            Ok(links) => serde_json::to_string_pretty(&links).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// List tasks, optionally filtered by column
    #[tool(description = "List kanban tasks. Optionally filter by column name.")]
    async fn list_tasks(&self, Parameters(input): Parameters<ListTasksInput>) -> String {
        let filter = TaskFilter { board_id: None, column: input.column, tags: vec![] };
        match self.vault.store.list_tasks(&filter) {
            Ok(tasks) => serde_json::to_string_pretty(&tasks).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// List all kanban boards
    #[tool(description = "List all kanban boards with their columns.")]
    async fn list_boards(&self) -> String {
        match self.vault.store.list_boards() {
            Ok(boards) => serde_json::to_string_pretty(&boards).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error: {e}"),
        }
    }
}

#[tool_handler]
impl ServerHandler for CodexToolHandler {}
