use async_trait::async_trait;
use codex_core::{
    graph::{build_graph_payload, format_kind},
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
                    "label": "Search Documents",
                    "description": "Full-text search across all vault documents.",
                    "parameters": {
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
                    "label": "List Documents",
                    "description": "List all vault documents (metadata only: id, path, title, tags, updated_at).",
                    "parameters": { "type": "object", "properties": {} }
                },
                {
                    "name": "find_document_by_slug",
                    "label": "Find Document",
                    "description": "Find a document by title or filename slug.",
                    "parameters": {
                        "type": "object",
                        "properties": { "slug": { "type": "string" } },
                        "required": ["slug"]
                    }
                },
                {
                    "name": "get_document",
                    "label": "Get Document",
                    "description": "Retrieve full markdown content and metadata for a document by path.",
                    "parameters": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                },
                {
                    "name": "create_document",
                    "label": "Create Document",
                    "description": "Create or overwrite a markdown document in the vault.",
                    "parameters": {
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
                    "label": "Get Backlinks",
                    "description": "List documents that link to the specified document path.",
                    "parameters": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                },
                {
                    "name": "store_memory_fact",
                    "label": "Store Memory",
                    "description": "Store a durable Omegon memory fact as a canonical markdown knowledge artifact.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "topic": { "type": "string" },
                            "title": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["topic", "title", "content"]
                    }
                },
                {
                    "name": "store_agent_communication",
                    "label": "Store Communication",
                    "description": "Store an internal Omegon or Scribe communication as a canonical markdown reference document.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "channel": { "type": "string" },
                            "title": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["channel", "title", "content"]
                    }
                },
                {
                    "name": "list_tasks",
                    "label": "List Tasks",
                    "description": "List kanban tasks, optionally filtered by board or column.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "board_id": { "type": "string" },
                            "column": { "type": "string" }
                        }
                    }
                },
                {
                    "name": "get_task",
                    "label": "Get Task",
                    "description": "Get a kanban task by id.",
                    "parameters": {
                        "type": "object",
                        "properties": { "id": { "type": "string" } },
                        "required": ["id"]
                    }
                },
                {
                    "name": "create_task",
                    "label": "Create Task",
                    "description": "Create a kanban task in a board column.",
                    "parameters": {
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
                    "label": "List Boards",
                    "description": "List all kanban boards with their columns.",
                    "parameters": { "type": "object", "properties": {} }
                },
                {
                    "name": "get_board",
                    "label": "Get Board",
                    "description": "Get a kanban board by id.",
                    "parameters": {
                        "type": "object",
                        "properties": { "id": { "type": "string" } },
                        "required": ["id"]
                    }
                },
                {
                    "name": "create_board",
                    "label": "Create Board",
                    "description": "Create a default sprint board.",
                    "parameters": {
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"]
                    }
                },
                {
                    "name": "get_graph",
                    "label": "Get Graph",
                    "description": "Get the full knowledge graph — all nodes (documents, tasks, boards, repos, links) and their relationships (wikilinks, task membership, semantic links). Use to understand vault structure and connections.",
                    "parameters": { "type": "object", "properties": {} }
                },
                {
                    "name": "get_graph_filtered",
                    "label": "Get Filtered Graph",
                    "description": "Get a filtered view of the knowledge graph. Filter by node kind, group (folder), tag, title search, or minimum connection count.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "description": "Node kind: document, task, board, repo, link, memory, communication" },
                            "group": { "type": "string", "description": "Group (top-level folder name)" },
                            "tag": { "type": "string", "description": "Only nodes with this tag" },
                            "search": { "type": "string", "description": "Title substring (case-insensitive)" },
                            "min_degree": { "type": "integer", "description": "Minimum connection count" }
                        }
                    }
                },
                {
                    "name": "get_node_neighbors",
                    "label": "Get Node Neighbors",
                    "description": "Get a node and its immediate neighbors in the knowledge graph — all directly connected nodes and the edges between them.",
                    "parameters": {
                        "type": "object",
                        "properties": { "node_id": { "type": "string" } },
                        "required": ["node_id"]
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

            "execute_store_memory_fact" => {
                let topic = params["topic"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'topic'"))?;
                let title = params["title"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'title'"))?;
                let content = params["content"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'content'"))?;
                let path = self
                    .vault
                    .store_memory_fact(topic, title, content)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({ "path": path }))
            }

            "execute_store_agent_communication" => {
                let channel = params["channel"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'channel'"))?;
                let title = params["title"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'title'"))?;
                let content = params["content"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'content'"))?;
                let path = self
                    .vault
                    .store_agent_communication(channel, title, content)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({ "path": path }))
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

            "execute_get_graph" => {
                let payload = build_graph_payload(&*self.vault.store)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(&payload).unwrap_or(json!({})))
            }

            "execute_get_graph_filtered" => {
                let payload = build_graph_payload(&*self.vault.store)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let kind_filter = params["kind"].as_str();
                let group_filter = params["group"].as_str();
                let tag_filter = params["tag"].as_str();
                let search = params["search"].as_str().unwrap_or("");
                let min_degree = params["min_degree"].as_u64().unwrap_or(0) as u32;

                let mut degree: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
                for edge in &payload.edges {
                    *degree.entry(&edge.source).or_default() += 1;
                    *degree.entry(&edge.target).or_default() += 1;
                }

                let search_lower = search.to_lowercase();
                let nodes: Vec<_> = payload.nodes.iter().filter(|n| {
                    if let Some(k) = kind_filter {
                        if format_kind(&n.kind) != k { return false; }
                    }
                    if let Some(g) = group_filter {
                        if n.group != g { return false; }
                    }
                    if let Some(t) = tag_filter {
                        if !n.tags.contains(&t.to_string()) { return false; }
                    }
                    if !search_lower.is_empty() && !n.title.to_lowercase().contains(&search_lower) {
                        return false;
                    }
                    if min_degree > 0 {
                        if degree.get(n.id.as_str()).copied().unwrap_or(0) < min_degree { return false; }
                    }
                    true
                }).collect();

                let ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
                let edges: Vec<_> = payload.edges.iter()
                    .filter(|e| ids.contains(e.source.as_str()) && ids.contains(e.target.as_str()))
                    .collect();

                Ok(json!({
                    "nodes": nodes,
                    "edges": edges,
                    "groups": payload.groups,
                    "all_tags": payload.all_tags,
                    "total_nodes": payload.nodes.len(),
                    "filtered_nodes": nodes.len(),
                }))
            }

            "execute_get_node_neighbors" => {
                let node_id = params["node_id"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'node_id'"))?;

                let payload = build_graph_payload(&*self.vault.store)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let connected_edges: Vec<_> = payload.edges.iter()
                    .filter(|e| e.source == node_id || e.target == node_id)
                    .collect();

                let mut neighbor_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
                neighbor_ids.insert(node_id);
                for edge in &connected_edges {
                    neighbor_ids.insert(&edge.source);
                    neighbor_ids.insert(&edge.target);
                }

                let neighbor_nodes: Vec<_> = payload.nodes.iter()
                    .filter(|n| neighbor_ids.contains(n.id.as_str()))
                    .collect();

                Ok(json!({
                    "center": node_id,
                    "nodes": neighbor_nodes,
                    "edges": connected_edges,
                }))
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
        assert!(names.contains(&"store_memory_fact".to_string()));
        assert!(names.contains(&"store_agent_communication".to_string()));
        assert!(names.contains(&"get_task".to_string()));
        assert!(names.contains(&"create_task".to_string()));
        assert!(names.contains(&"get_board".to_string()));
        assert!(names.contains(&"create_board".to_string()));
    }

    #[tokio::test]
    async fn create_board_and_task_are_exposed_end_to_end() {
        let (_tmp, ext) = test_extension();

        let memory = ext
            .handle_rpc(
                "execute_store_memory_fact",
                json!({
                    "topic": "storage",
                    "title": "Canonical vs Local",
                    "content": "Supports [[Sprint 1]]."
                }),
            )
            .await
            .unwrap();
        assert!(memory["path"].as_str().unwrap().contains("ai/memory/storage"));

        let comm = ext
            .handle_rpc(
                "execute_store_agent_communication",
                json!({
                    "channel": "scribe",
                    "title": "Standup Recall",
                    "content": "See [[Sprint 1]]."
                }),
            )
            .await
            .unwrap();
        assert!(comm["path"].as_str().unwrap().contains("references/comms/scribe"));

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
            .handle_rpc("execute_list_tasks", json!({ "column": "Backlog", "board_id": board_id }))
            .await
            .unwrap();
        assert_eq!(tasks.as_array().unwrap().len(), 1);
    }
}
