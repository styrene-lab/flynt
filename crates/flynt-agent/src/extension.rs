use async_trait::async_trait;
use flynt_core::{
    graph::{build_graph_payload, format_kind},
    models::{Board, Task},
    store::{TaskFilter, VaultStore},
};
use flynt_store::vault::Vault;
use omegon_extension::Extension;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct FlyntExtension {
    vault: Arc<Vault>,
}

impl FlyntExtension {
    pub fn new(vault: Arc<Vault>) -> Self {
        Self { vault }
    }
}

#[async_trait]
impl Extension for FlyntExtension {
    fn name(&self) -> &str { "flynt" }
    fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }

    async fn handle_rpc(&self, method: &str, params: Value) -> omegon_extension::Result<Value> {
        match method {
            // ── v2 handshake ────────────────────────────────────────────────
            "initialize" => {
                let tools = self.handle_rpc("get_tools", json!({})).await?;
                Ok(json!({
                    "protocol_version": 2,
                    "extension_info": {
                        "name": self.name(),
                        "version": self.version(),
                        "sdk_version": "0.16.0"
                    },
                    "capabilities": {
                        "tools": true, "widgets": false, "mind": true,
                        "vox": false, "resources": false, "prompts": false,
                        "sampling": false, "elicitation": false, "streaming": false
                    },
                    "tools": tools
                }))
            }

            // ── v2 tool execution (tools/call + execute_tool) ───────────────
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                self.handle_rpc(&format!("execute_{name}"), args).await
            }
            "execute_tool" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("args").cloned().unwrap_or(json!({}));
                self.handle_rpc(&format!("execute_{name}"), args).await
            }

            // ── Discovery ────────────────────────────────────────────────────
            "get_tools" | "tools/list" => Ok(json!([
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
                    "name": "convert_to_design_node",
                    "label": "Convert to Design Node",
                    "description": "Convert an existing document into a design node with structured frontmatter. Preserves existing content as the Overview section and adds an Open Questions section.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path of the existing document to convert" },
                            "status": { "type": "string", "default": "seed", "description": "Lifecycle status: seed, exploring, resolved, decided, implementing, implemented, blocked, deferred, archived" },
                            "issue_type": { "type": "string", "description": "Optional issue type (e.g. epic, feature, task, bug)" },
                            "priority": { "type": "integer", "description": "Priority (1=low, 2=medium, 3=high, 4=critical)" },
                            "parent": { "type": "string", "description": "Optional parent design node UUID" }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "list_design_nodes",
                    "label": "List Design Nodes",
                    "description": "List all design nodes in the vault, optionally filtered by lifecycle status.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "status": { "type": "string", "description": "Filter by status: seed, exploring, resolved, decided, implementing, implemented, blocked, deferred, archived" }
                        }
                    }
                },
                {
                    "name": "create_drawing",
                    "label": "Create Drawing",
                    "description": "Create an Excalidraw drawing with optional scene elements. Returns the wrapper document path. The desktop app auto-exports SVG for inline rendering.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Drawing name (used for filename)" },
                            "scene": { "type": "string", "description": "Optional Excalidraw scene JSON. If omitted, creates an empty dark-themed canvas." }
                        },
                        "required": ["name"]
                    }
                },
                {
                    "name": "create_d2_diagram",
                    "label": "Create D2 Diagram",
                    "description": "Create a D2 diagram file with source code. The desktop app auto-renders to SVG via the d2 CLI. Use ![[name.d2]] to embed inline in documents.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Diagram name (used for filename)" },
                            "source": { "type": "string", "description": "D2 diagram source code" },
                            "directory": { "type": "string", "default": "diagrams", "description": "Directory within vault (default: diagrams)" }
                        },
                        "required": ["name", "source"]
                    }
                },
                {
                    "name": "delete_board",
                    "label": "Delete Board",
                    "description": "Delete a kanban board and all its tasks.",
                    "parameters": {
                        "type": "object",
                        "properties": { "id": { "type": "string" } },
                        "required": ["id"]
                    }
                },
                {
                    "name": "get_workspace_leases",
                    "label": "Get Workspace Leases",
                    "description": "List workspace leases — machine checkouts of this vault. Shows federation key, machine id, heartbeat, role, mutability, and staleness. Useful for showing workspace sync status in the Omegon sidebar.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "federation_key": { "type": "string", "description": "Filter by federation key" },
                            "include_archived": { "type": "boolean", "default": false, "description": "Include archived (decommissioned) leases" },
                            "staleness_threshold_secs": { "type": "integer", "default": 300, "description": "Seconds after which a lease is considered stale" }
                        }
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
                            "kind": { "type": "string", "description": "Node kind: document, task, board, repo, link, memory, communication, design_node" },
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
                },
                {
                    "name": "git_login",
                    "label": "Git Login",
                    "description": "Check git credential status for a provider or save a personal access token. Use this when a user wants to authenticate with GitHub, Codeberg, or GitLab for git sync. Without a token, returns the current credential status. With a token, saves it securely for all future git operations.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "provider": {
                                "type": "string",
                                "description": "Provider ID: github, forgejo (for Codeberg), or gitlab"
                            },
                            "token": {
                                "type": "string",
                                "description": "Personal access token to save. Omit to just check current status."
                            }
                        },
                        "required": ["provider"]
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
                            .map(flynt_core::models::BoardId)
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
                let id = flynt_core::models::TaskId(
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
                let board_id = flynt_core::models::BoardId(
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
                let id = flynt_core::models::BoardId(
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

            "execute_convert_to_design_node" => {
                let path = params["path"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'path'"))?;
                let status = params["status"].as_str().unwrap_or("seed");
                let issue_type = params["issue_type"].as_str();
                let priority = params["priority"].as_i64();
                let parent = params["parent"].as_str();

                // Read the existing document
                let doc = self
                    .vault
                    .store
                    .get_document_by_path(std::path::Path::new(path))
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?
                    .ok_or_else(|| omegon_extension::Error::internal_error(format!("not found: {path}")))?;

                // Guard: refuse to overwrite an existing design node
                if let Some(ref entity) = doc.entity {
                    if entity.kind == flynt_core::datum::EntityKind::DesignNode {
                        return Err(omegon_extension::Error::internal_error(
                            "Document is already a design node. Use design_tree_update to modify it.".to_string(),
                        ));
                    }
                }

                let existing_content = doc.content.clone();
                let title = doc.title.clone();
                let doc_id = doc.frontmatter.id.unwrap_or_else(uuid::Uuid::new_v4);

                // Build [data] table entries
                let mut data_lines = Vec::new();
                data_lines.push(format!("title = \"{}\"", title.replace('"', "\\\"")));
                data_lines.push(format!("status = \"{}\"", status));
                if let Some(it) = issue_type {
                    data_lines.push(format!("issue_type = \"{}\"", it));
                }
                if let Some(p) = priority {
                    data_lines.push(format!("priority = {}", p));
                }
                if let Some(par) = parent {
                    data_lines.push(format!("parent = \"{}\"", par));
                }
                data_lines.push("dependencies = []".into());
                data_lines.push("open_questions = []".into());

                let full = format!(
                    "+++\nid = \"{}\"\nkind = \"design_node\"\n\n[data]\n{}\n+++\n\n## Overview\n\n{}\n\n## Open Questions\n",
                    doc_id,
                    data_lines.join("\n"),
                    existing_content.trim(),
                );

                let rel = std::path::Path::new(path);
                self.vault
                    .save_document_content(rel, &full)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({ "converted": path, "id": doc_id.to_string(), "status": status }))
            }

            "execute_list_design_nodes" => {
                let status_filter = params["status"].as_str();
                let nodes = self
                    .vault
                    .store
                    .list_entities_by_kind(&flynt_core::datum::EntityKind::DesignNode)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let mut results: Vec<Value> = Vec::new();
                for meta in nodes {
                    // Load full document for entity fields
                    let doc = self
                        .vault
                        .store
                        .get_document(&meta.id)
                        .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                    let (node_status, node_parent, node_priority, node_issue_type, open_questions_count, deps_count) =
                        if let Some(ref d) = doc {
                            if let Some(ref entity) = d.entity {
                                let s = entity.get_text("status").unwrap_or("seed").to_string();
                                let p = entity.get_text("parent").map(String::from);
                                let pr = entity.get_int("priority");
                                let it = entity.get_text("issue_type").map(String::from);
                                let oq = entity.get_text_list("open_questions").len();
                                let dc = entity.get_text_list("dependencies").len();
                                (s, p, pr, it, oq, dc)
                            } else {
                                ("seed".into(), None, None, None, 0, 0)
                            }
                        } else {
                            ("seed".into(), None, None, None, 0, 0)
                        };

                    // Apply status filter if provided
                    if let Some(sf) = status_filter {
                        if node_status != sf {
                            continue;
                        }
                    }

                    results.push(json!({
                        "id": meta.id.0.to_string(),
                        "title": meta.title,
                        "status": node_status,
                        "parent": node_parent,
                        "priority": node_priority,
                        "issue_type": node_issue_type,
                        "open_questions_count": open_questions_count,
                        "dependencies_count": deps_count,
                    }));
                }
                Ok(json!(results))
            }

            "execute_create_drawing" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'name'"))?;
                let scene = params["scene"].as_str();

                // Create drawings directory and files
                let drawings_dir = self.vault.root.join("drawings");
                std::fs::create_dir_all(&drawings_dir)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                // Write .excalidraw scene file (refuse to overwrite)
                let excalidraw_file = format!("{name}.excalidraw");
                let excalidraw_abs = drawings_dir.join(&excalidraw_file);
                if excalidraw_abs.exists() {
                    return Err(omegon_extension::Error::internal_error(
                        format!("Drawing already exists: drawings/{excalidraw_file}. Use a different name."),
                    ));
                }
                let scene_content = scene.unwrap_or(
                    r#"{"type":"excalidraw","version":2,"elements":[],"appState":{"viewBackgroundColor":"transparent","theme":"dark"}}"#
                );
                std::fs::write(&excalidraw_abs, scene_content)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                // Write .md wrapper for indexing
                let md_rel = format!("drawings/{name}.md");
                let md_content = format!(
                    "+++\ntitle = \"{}\"\ntags = [\"drawing\"]\n+++\n\n![[{excalidraw_file}]]\n",
                    name.replace('"', "\\\"")
                );
                let rel = std::path::Path::new(&md_rel);
                self.vault
                    .save_document_content(rel, &md_content)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                Ok(json!({
                    "created": md_rel,
                    "excalidraw_file": format!("drawings/{excalidraw_file}"),
                    "has_scene": scene.is_some(),
                }))
            }

            "execute_create_d2_diagram" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'name'"))?;
                let source = params["source"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'source'"))?;
                let directory = params["directory"].as_str().unwrap_or("diagrams");

                // Create directory and write .d2 file
                let dir = self.vault.root.join(directory);
                std::fs::create_dir_all(&dir)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let d2_file = format!("{name}.d2");
                let d2_abs = dir.join(&d2_file);
                if d2_abs.exists() {
                    return Err(omegon_extension::Error::internal_error(
                        format!("Diagram already exists: {directory}/{d2_file}. Use a different name."),
                    ));
                }
                std::fs::write(&d2_abs, source)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                // Write .md wrapper for indexing and embedding
                let md_rel = format!("{directory}/{name}.md");
                let md_content = format!(
                    "+++\ntitle = \"{}\"\ntags = [\"diagram\", \"d2\"]\n+++\n\n![[{d2_file}]]\n",
                    name.replace('"', "\\\"")
                );
                let rel = std::path::Path::new(&md_rel);
                self.vault
                    .save_document_content(rel, &md_content)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                Ok(json!({
                    "created": md_rel,
                    "d2_file": format!("{directory}/{d2_file}"),
                }))
            }

            "execute_delete_board" => {
                let id = params["id"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'id'"))?;
                let id = flynt_core::models::BoardId(
                    uuid::Uuid::parse_str(id)
                        .map_err(|_| omegon_extension::Error::invalid_params("invalid 'id'"))?,
                );
                self.vault
                    .store
                    .delete_board(&id)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({ "deleted": true }))
            }

            "execute_get_workspace_leases" => {
                let federation_key_filter = params["federation_key"].as_str();
                let include_archived = params["include_archived"].as_bool().unwrap_or(false);
                let staleness_secs = params["staleness_threshold_secs"].as_i64().unwrap_or(300);

                let leases = self
                    .vault
                    .store
                    .list_entities_by_kind(&flynt_core::datum::EntityKind::WorkspaceLease)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let mut results: Vec<Value> = Vec::new();
                for meta in leases {
                    let doc = self
                        .vault
                        .store
                        .get_document(&meta.id)
                        .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                    let view = doc.as_ref()
                        .and_then(|d| d.entity.as_ref())
                        .and_then(flynt_core::datum::WorkspaceLeaseView::from_entity);

                    let view = match view {
                        Some(v) => v,
                        None => continue,
                    };

                    if !include_archived && view.archived() {
                        continue;
                    }
                    if let Some(fk) = federation_key_filter {
                        if view.federation_key() != fk {
                            continue;
                        }
                    }

                    let stale = view.is_stale(staleness_secs);
                    results.push(json!({
                        "id": meta.id.0.to_string(),
                        "federation_key": view.federation_key(),
                        "machine_id": view.machine_id(),
                        "last_heartbeat": view.last_heartbeat(),
                        "role": view.role(),
                        "mutability": view.mutability(),
                        "label": view.label(),
                        "archived": view.archived(),
                        "stale": stale,
                    }));
                }
                Ok(json!(results))
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

                let mut ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

                // Design node filter: also include direct dependency targets
                // so the graph shows what design nodes depend on.
                if kind_filter == Some("design_node") {
                    let dep_targets: Vec<&str> = payload.edges.iter()
                        .filter(|e| {
                            ids.contains(e.source.as_str())
                                && (e.kind == flynt_core::graph::GraphEdgeKind::Dependency
                                    || e.kind == flynt_core::graph::GraphEdgeKind::ParentChild)
                        })
                        .map(|e| e.target.as_str())
                        .collect();
                    // Also include parent sources for ParentChild edges
                    let parent_sources: Vec<&str> = payload.edges.iter()
                        .filter(|e| {
                            ids.contains(e.target.as_str())
                                && e.kind == flynt_core::graph::GraphEdgeKind::ParentChild
                        })
                        .map(|e| e.source.as_str())
                        .collect();
                    for t in dep_targets {
                        ids.insert(t);
                    }
                    for s in parent_sources {
                        ids.insert(s);
                    }
                }

                // Re-collect nodes including any added dependency/parent targets
                let nodes: Vec<_> = payload.nodes.iter()
                    .filter(|n| ids.contains(n.id.as_str()))
                    .collect();

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

            "execute_git_login" => {
                let provider_id = params["provider"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'provider'"))?;

                let provider = flynt_core::providers::PROVIDERS
                    .iter()
                    .find(|p| p.id == provider_id)
                    .ok_or_else(|| {
                        omegon_extension::Error::invalid_params(format!(
                            "Unknown provider: {provider_id}. Use: github, forgejo, gitlab"
                        ))
                    })?;

                // If a token was provided, save it
                if let Some(token) = params.get("token").and_then(|v| v.as_str()) {
                    if !token.trim().is_empty() {
                        flynt_core::providers::save_api_key(provider_id, token.trim())
                            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                        return Ok(json!({
                            "status": "saved",
                            "provider": provider_id,
                            "label": provider.label,
                            "message": format!(
                                "Token saved for {}. All future git operations to {} will use this token automatically.",
                                provider.label, provider_id
                            ),
                        }));
                    }
                }

                // Otherwise report current credential status
                let status = flynt_core::providers::probe_provider(provider);
                let (status_str, authenticated) = match &status {
                    flynt_core::providers::CredentialStatus::Authenticated { source } => {
                        (format!("authenticated ({source})"), true)
                    }
                    flynt_core::providers::CredentialStatus::Expired => {
                        ("expired".to_string(), false)
                    }
                    flynt_core::providers::CredentialStatus::Missing => {
                        ("not configured".to_string(), false)
                    }
                };

                Ok(json!({
                    "provider": provider_id,
                    "label": provider.label,
                    "status": status_str,
                    "authenticated": authenticated,
                }))
            }

            _ => Err(omegon_extension::Error::method_not_found(method)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FlyntExtension;
    use flynt_store::vault::Vault;
    use omegon_extension::Extension;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_extension() -> (TempDir, FlyntExtension) {
        let tmp = TempDir::new().unwrap();
        let vault = Arc::new(Vault::open(tmp.path()).unwrap());
        (tmp, FlyntExtension::new(vault))
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
