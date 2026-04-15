use async_trait::async_trait;
use codex_core::store::{TaskFilter, VaultStore};
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
    fn name(&self)    -> &str { "codex" }
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
                            "path":    { "type": "string" },
                            "title":   { "type": "string" },
                            "content": { "type": "string" },
                            "tags":    { "type": "array", "items": { "type": "string" } }
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
                    "description": "List kanban tasks, optionally filtered by column.",
                    "input_schema": {
                        "type": "object",
                        "properties": { "column": { "type": "string" } }
                    }
                },
                {
                    "name": "list_boards",
                    "description": "List all kanban boards with their columns.",
                    "input_schema": { "type": "object", "properties": {} }
                }
            ])),

            // ── Tool execution ────────────────────────────────────────────────
            "execute_search_documents" => {
                let query = params["query"].as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'query'"))?
                    .to_string();
                let limit = params["limit"].as_u64().unwrap_or(20) as usize;
                let results = self.vault.store.search_documents(&query)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                let results: Vec<_> = results.into_iter().take(limit).collect();
                Ok(serde_json::to_value(results).unwrap_or(json!([])))
            }

            "execute_list_documents" => {
                let docs = self.vault.store.list_documents()
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(docs).unwrap_or(json!([])))
            }

            "execute_get_document" => {
                let path = params["path"].as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'path'"))?;
                let doc = self.vault.store.get_document_by_path(std::path::Path::new(path))
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                match doc {
                    Some(d) => Ok(serde_json::to_value(d).unwrap_or(json!({}))),
                    None    => Err(omegon_extension::Error::internal_error(format!("not found: {path}"))),
                }
            }

            "execute_create_document" => {
                let path    = params["path"].as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'path'"))?;
                let title   = params["title"].as_str().unwrap_or("Untitled");
                let content = params["content"].as_str().unwrap_or("");
                let tags: Vec<&str> = params["tags"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let tags_toml = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
                let full = format!("+++\ntitle = \"{title}\"\ntags = {tags_toml}\n+++\n\n# {title}\n\n{content}");
                let rel  = std::path::Path::new(path);
                self.vault.save_document_content(rel, &full)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({ "created": path }))
            }

            "execute_get_backlinks" => {
                let path = params["path"].as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'path'"))?;
                let doc = self.vault.store.get_document_by_path(std::path::Path::new(path))
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?
                    .ok_or_else(|| omegon_extension::Error::internal_error(format!("not found: {path}")))?;
                let links = self.vault.store.get_backlinks(&doc.id)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(links).unwrap_or(json!([])))
            }

            "execute_list_tasks" => {
                let column = params["column"].as_str().map(str::to_string);
                let tasks  = self.vault.store.list_tasks(&TaskFilter { column, ..Default::default() })
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(tasks).unwrap_or(json!([])))
            }

            "execute_list_boards" => {
                let boards = self.vault.store.list_boards()
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(boards).unwrap_or(json!([])))
            }

            _ => Err(omegon_extension::Error::method_not_found(method)),
        }
    }
}
