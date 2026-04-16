use async_trait::async_trait;
use codex_core::{
    models::{Board, Task},
    store::{TaskFilter, VaultStore},
};
use codex_store::vault::Vault;
use omegon_extension::Extension;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct CodexExtension {
    vault: Arc<Vault>,
}

impl CodexExtension {
    pub fn new(vault: Arc<Vault>) -> Self {
        Self { vault }
    }
}

#[async_trait]
impl Extension for CodexExtension {
    fn name(&self) -> &str { "codex" }
    fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }

    async fn handle_rpc(&self, method: &str, params: Value) -> omegon_extension::Result<Value> {
        match method {
            // ── Discovery ────────────────────────────────────────────────────
            "get_tools" => Ok(json!([
                {
                    "name": "search_documents",
                    "description": "Full-text search across all vault documents.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "limit": { "type": "integer", "default": 20 }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "list_documents",
                    "description": "List all vault documents (metadata only: id, path, title, tags, updated_at).",
                    "input_schema": { "type": "object", "properties": {} }
                },
                {
                    "name": "find_document_by_slug",
                    "description": "Find a document by title or filename slug.",
                    "input_schema": {
                        "type": "object",
                        "properties": { "slug": { "type": "string" } },
                        "required": ["slug"]
                    }
                },
                {
                    "name": "get_document",
                    "description": "Retrieve full markdown content and metadata for a document by path.",
                    "input_schema": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                },
                {
                    "name": "create_document",
                    "description": "Create or overwrite a markdown document in the vault.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "title": { "type": "string" },
                            "content": { "type": "string" },
                            "tags": { "type": "array", "items": { "type": "string" } }
                        },
                        "required": ["path", "title", "content"]
                    }
                },
                {
                    "name": "get_backlinks",
                    "description": "List documents that link to the specified document path.",
                    "input_schema": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                },
                {
                    "name": "list_tasks",
                    "description": "List kanban tasks, optionally filtered by board or column.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "board_id": { "type": "string" },
                            "column": { "type": "string" }
                        }
                    }
                },
                {
                    "name": "get_task",
                    "description": "Get a kanban task by id.",
                    "input_schema": {
                        "type": "object",
                        "properties": { "id": { "type": "string" } },
                        "required": ["id"]
                    }
                },
                {
                    "name": "create_task",
                    "description": "Create a kanban task in a board column.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "board_id": { "type": "string" },
                            "column": { "type": "string" },
                            "title": { "type": "string" }
                        },
                        "required": ["board_id", "column", "title"]
                    }
                },
                {
                    "name": "list_boards",
                    "description": "List all kanban boards with their columns.",
                    "input_schema": { "type": "object", "properties": {} }
                },
                {
                    "name": "get_board",
                    "description": "Get a kanban board by id.",
                    "input_schema": {
                        "type": "object",
                        "properties": { "id": { "type": "string" } },
                        "required": ["id"]
                    }
                },
                {
                    "name": "create_board",
                    "description": "Create a default sprint board.",
                    "input_schema": {
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"]
                    }
                }
            ])),

            // ── Tool execution ────────────────────────────────────────────────
            "execute_search_documents" => {
                let query = params["query"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'query'"))?
                    .to_string();
                let limit = params["limit"].as_u64().unwrap_or(20) as usize;
                let results = self
                    .vault
                    .store
                    .search_documents(&query)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                let results: Vec<_> = results.into_iter().take(limit).collect();
                Ok(serde_json::to_value(results).unwrap_or(json!([])))
            }

            "execute_list_documents" => {
                let docs = self
                    .vault
                    .store
                    .list_documents()
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(docs).unwrap_or(json!([])))
            }

            "execute_find_document_by_slug" => {
                let slug = params["slug"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'slug'"))?;
                let doc = self
                    .vault
                    .store
                    .find_document_by_slug(slug)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(doc).unwrap_or(json!(null)))
            }

            "execute_get_document" => {
                let path = params["path"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'path'"))?;
                let doc = self
                    .vault
                    .store
                    .get_document_by_path(std::path::Path::new(path))
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                match doc {
                    Some(d) => Ok(serde_json::to_value(d).unwrap_or(json!({}))),
                    None => Err(omegon_extension::Error::internal_error(format!("not found: {path}"))),
                }
            }

            "execute_create_document" => {
                let path = params["path"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'path'"))?;
                let title = params["title"].as_str().unwrap_or("Untitled");
                let content = params["content"].as_str().unwrap_or("");
                let tags: Vec<&str> = params["tags"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let tags_toml = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
                let full = format!(
                    "+++\ntitle = \"{title}\"\ntags = {tags_toml}\n+++\n\n# {title}\n\n{content}"
                );
                let rel = std::path::Path::new(path);
                self.vault
                    .save_document_content(rel, &full)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({ "created": path }))
            }

            "execute_get_backlinks" => {
                let path = params["path"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'path'"))?;
                let doc = self
                    .vault
                    .store
                    .get_document_by_path(std::path::Path::new(path))
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?
                    .ok_or_else(|| omegon_extension::Error::internal_error(format!("not found: {path}")))?;
                let links = self
                    .vault
                    .store
                    .get_backlinks(&doc.id)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(links).unwrap_or(json!([])))
            }

            "execute_list_tasks" => {
                let board_id = params["board_id"]
                    .as_str()
                    .map(|raw| {
                        uuid::Uuid::parse_str(raw)
                            .map(codex_core::models::BoardId)
                            .map_err(|_| omegon_extension::Error::invalid_params("invalid 'board_id'"))
                    })
                    .transpose()?;
                let column = params["column"].as_str().map(str::to_string);
                let tasks = self
                    .vault
                    .store
                    .list_tasks(&TaskFilter {
                        board_id,
                        column,
                        ..Default::default()
                    })
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(tasks).unwrap_or(json!([])))
            }

            "execute_get_task" => {
                let id = params["id"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'id'"))?;
                let id = codex_core::models::TaskId(
                    uuid::Uuid::parse_str(id)
                        .map_err(|_| omegon_extension::Error::invalid_params("invalid 'id'"))?,
                );
                let task = self
                    .vault
                    .store
                    .get_task(&id)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(task).unwrap_or(json!(null)))
            }

            "execute_create_task" => {
                let board_id = params["board_id"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'board_id'"))?;
                let board_id = codex_core::models::BoardId(
                    uuid::Uuid::parse_str(board_id)
                        .map_err(|_| omegon_extension::Error::invalid_params("invalid 'board_id'"))?,
                );
                let column = params["column"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'column'"))?;
                let title = params["title"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'title'"))?;
                let task = Task::new(board_id, column, title);
                self.vault
                    .store
                    .save_task(&task)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(&task).unwrap_or(json!({})))
            }

            "execute_list_boards" => {
                let boards = self
                    .vault
                    .store
                    .list_boards()
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(boards).unwrap_or(json!([])))
            }

            "execute_get_board" => {
                let id = params["id"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'id'"))?;
                let id = codex_core::models::BoardId(
                    uuid::Uuid::parse_str(id)
                        .map_err(|_| omegon_extension::Error::invalid_params("invalid 'id'"))?,
                );
                let board = self
                    .vault
                    .store
                    .get_board(&id)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(board).unwrap_or(json!(null)))
            }

            "execute_create_board" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'name'"))?;
                let board = Board::default_sprint(name);
                self.vault
                    .store
                    .save_board(&board)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(&board).unwrap_or(json!({})))
            }

            _ => Err(omegon_extension::Error::method_not_found(method)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CodexExtension;
    use codex_store::vault::Vault;
    use omegon_extension::Extension;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_extension() -> (TempDir, CodexExtension) {
        let tmp = TempDir::new().unwrap();
        let vault = Arc::new(Vault::open(tmp.path()).unwrap());
        (tmp, CodexExtension::new(vault))
    }

    #[tokio::test]
    async fn get_tools_includes_kanban_and_lookup_surfaces() {
        let (_tmp, ext) = test_extension();
        let tools = ext.handle_rpc("get_tools", json!({})).await.unwrap();
        let names: Vec<String> = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect();

        assert!(names.contains(&"find_document_by_slug".to_string()));
        assert!(names.contains(&"get_task".to_string()));
        assert!(names.contains(&"create_task".to_string()));
        assert!(names.contains(&"get_board".to_string()));
        assert!(names.contains(&"create_board".to_string()));
    }

    #[tokio::test]
    async fn create_board_and_task_are_exposed_end_to_end() {
        let (_tmp, ext) = test_extension();

        let board = ext
            .handle_rpc("execute_create_board", json!({ "name": "Sprint 1" }))
            .await
            .unwrap();
        let board_id = board["id"].as_str().unwrap().to_string();

        let task = ext
            .handle_rpc(
                "execute_create_task",
                json!({
                    "board_id": board_id,
                    "column": "Backlog",
                    "title": "Wire extension surface"
                }),
            )
            .await
            .unwrap();

        assert_eq!(task["title"], "Wire extension surface");

        let tasks = ext
            .handle_rpc("execute_list_tasks", json!({ "column": "Backlog" }))
            .await
            .unwrap();
        assert_eq!(tasks.as_array().unwrap().len(), 1);
    }
}
