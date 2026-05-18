use async_trait::async_trait;
use flynt_core::{
    graph::{build_graph_payload, format_kind},
    models::{Board, Task},
    store::{ProjectStore, TaskFilter},
};
use flynt_store::project::Project;
use omegon_extension::Extension;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::forge_tools::{self, SecretBag};
use crate::{drawing_tools, flow_tools};

pub struct FlyntExtension {
    project: Arc<Project>,
    /// In-process secret bag — populated by `bootstrap_secrets` (omegon
    /// push) and seeded from the `FLYNT_GITHUB_TOKEN` env var on
    /// construction. Forge clients resolve tokens through this bag, so
    /// the same bag is reachable from every tool handler that needs
    /// authenticated access.
    secrets: SecretBag,
}

impl FlyntExtension {
    pub fn new(project: Arc<Project>) -> Self {
        let secrets = SecretBag::new();
        secrets.seed_from_env();
        Self { project, secrets }
    }
}

#[async_trait]
impl Extension for FlyntExtension {
    fn name(&self) -> &str {
        "flynt"
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

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
            "get_tools" | "tools/list" => {
                let mut tools = json!([
                {
                    "name": "search_documents",
                    "label": "Search Documents",
                    "description": "Full-text search across all project documents.",
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
                    "description": "List all project documents (metadata only: id, path, title, tags, updated_at).",
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
                    "description": "Retrieve full markdown content and metadata for a document. Pass either `path` (relative-to-project, e.g. \"Identity.md\") OR `id` (UUID from get_ui_state / list_documents). At least one must be provided.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Relative path inside the project." },
                            "id":   { "type": "string", "description": "Document UUID. Either path or id is required." }
                        }
                    }
                },
                {
                    "name": "create_document",
                    "label": "Create Document",
                    "description": "Create or overwrite a markdown document in the project.",
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
                    "name": "move_document",
                    "label": "Move Document",
                    "description": "Move a plain markdown note to a new project-relative `.md` path and update Flynt's index. Use this when reorganizing notes into better folders. Do not use it for Excalidraw drawing wrappers, design canvas wrappers, or non-markdown files.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "from_path": { "type": "string", "description": "Existing project-relative markdown path." },
                            "to_path": { "type": "string", "description": "Destination project-relative markdown path. Must end in .md." }
                        },
                        "required": ["from_path", "to_path"]
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
                    "description": "List kanban tasks. Filters AND together. `tags` requires ALL listed tags to be present on the task (intersection, not union). `status` accepts the canonical lowercase form (`todo`, `in_progress`, `done`, `archived`). Used by sentry's list_actionable() to discover ready work — column/status/tag filters together pick out tasks in a 'Scheduled' column with status=todo and tag=sentry, for example.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "board_id": { "type": "string" },
                            "column": { "type": "string" },
                            "tags": { "type": "array", "items": { "type": "string" } },
                            "status": { "type": "string", "enum": ["todo", "in_progress", "done", "archived"] }
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
                    "name": "update_task",
                    "label": "Update Task",
                    "description": "Apply a partial update to a task. Only the fields you supply are changed; everything else is preserved. Use this for status/column moves (e.g., 'to_do' → 'in_progress'), priority bumps, tag edits, and other field-level mutations without rewriting the whole task. Returns { updated: bool, task_id }. `updated: false` means no task with that id existed. This is the foundation of the sentry integration's claim/release/complete cycle — see flynt/design/sentry-integration.md.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Task UUID." },
                            "column": { "type": "string" },
                            "title": { "type": "string" },
                            "description": { "type": "string" },
                            "priority": { "type": "string", "description": "low | medium | high | urgent" },
                            "status": { "type": "string", "description": "todo | in_progress | done | blocked | cancelled" },
                            "tags": { "type": "array", "items": { "type": "string" } },
                            "external_refs": { "type": "array", "items": { "type": "string" } },
                            "position": { "type": "integer", "minimum": 0 },
                            "design_node_id": { "type": "string", "description": "UUID of an associated design tree node, or empty string to clear." },
                            "openspec_change": { "type": "string", "description": "OpenSpec change name (used by sentry's lifecycle hooks). Empty string clears." },
                            "execution": {
                                "description": "Sentry execution parameters. Pass an object to set; pass null to clear. Mirrors omegon::sentry::types::TaskSpec (minus `prompt`, which is the task description).",
                                "type": ["object", "null"],
                                "properties": {
                                    "model": { "type": "string" },
                                    "skill": { "type": "string" },
                                    "max_turns": { "type": "integer", "minimum": 1 },
                                    "timeout_secs": { "type": "integer", "minimum": 1 },
                                    "token_budget": { "type": "integer", "minimum": 1 },
                                    "cwd": { "type": "string" },
                                    "env": { "type": "object", "additionalProperties": { "type": "string" } }
                                }
                            }
                        },
                        "required": ["id"]
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
                    "description": "List all design nodes in the project, optionally filtered by lifecycle status.",
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
                    "description": "Create a Flynt Excalidraw drawing. This ALWAYS writes `drawings/<name>.excalidraw` plus an openable/indexable wrapper `drawings/<name>.md`; do not put Excalidraw drawings under `diagrams/` and do not create Excalidraw wrappers with create_document. Returns { wrapper_path, drawing_path }. The user opens the drawing by selecting the wrapper path in Flynt. Use drawing_active/drawing_get/drawing_set_scene to inspect or edit an opened drawing.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Drawing name (used for filename)" },
                            "scene": { "type": "string", "description": "Optional Excalidraw scene JSON string. If omitted, creates an empty dark-themed drawing." }
                        },
                        "required": ["name"]
                    }
                },
                {
                    "name": "drawing_active",
                    "label": "Drawing: Active",
                    "description": "Resolve the Excalidraw drawing the user is currently viewing in Flynt. Reads the UI-state mirror and returns { wrapper_path, drawing_path } when the active document is a drawing wrapper. Use this before editing 'the open drawing'. Returns null if no drawing is active.",
                    "parameters": { "type": "object", "properties": {} }
                },
                {
                    "name": "drawing_get",
                    "label": "Drawing: Get",
                    "description": "Read an Excalidraw scene JSON file. Pass `path` relative to project root, usually from create_drawing or drawing_active, e.g. `drawings/Architecture.excalidraw`.",
                    "parameters": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                },
                {
                    "name": "drawing_set_scene",
                    "label": "Drawing: Set Scene",
                    "description": "Replace an existing Excalidraw scene JSON file. Use drawing_active first for the currently open drawing. Accepts `scene` as either a JSON object or a JSON string. This edits `.excalidraw` scene data, not Flynt design `.canvas` cells.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "scene": { "description": "Excalidraw scene JSON object or JSON string" }
                        },
                        "required": ["path", "scene"]
                    }
                },
                {
                    "name": "create_d2_diagram",
                    "label": "Create D2 Diagram",
                    "description": "Create a D2 diagram source file. This is for text-authored D2 diagrams and defaults to `diagrams/`; it is not Excalidraw. Use create_drawing for freeform Excalidraw sketches, canvas_create/canvas_set_cells for Flynt design canvases, and flow_create/flow_patch for node-flow diagrams.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Diagram name (used for filename)" },
                            "source": { "type": "string", "description": "D2 diagram source code" },
                            "directory": { "type": "string", "default": "diagrams", "description": "Directory within project (default: diagrams)" }
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
                    "description": "List workspace leases — machine checkouts of this project. Shows federation key, machine id, heartbeat, role, mutability, and staleness. Useful for showing workspace sync status in the Omegon sidebar.",
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
                    "description": "Get the full knowledge graph — all nodes (documents, tasks, boards, repos, links) and their relationships (wikilinks, task membership, semantic links). Use to understand project structure and connections.",
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
                },
                {
                    "name": "flynt_surface_guide",
                    "label": "Flynt Surface Guide",
                    "description": "Return the operational map for Flynt's document surfaces and tool families. Call this when choosing between notes, drawings, D2 diagrams, design canvases, and flow graphs, or when the user asks what is open/current in Flynt.",
                    "parameters": { "type": "object", "properties": {} }
                },
                {
                    "name": "get_ui_state",
                    "label": "Get UI State",
                    "description": "Return what the user is currently looking at in Flynt: the active document (if any), other open document tabs, and the current view (notes/kanban/graph/settings/search/welcome). Call this BEFORE asking the user clarifying questions about 'what they have open' or 'what they're working on' — Flynt mirrors this state to disk on every tab/view change so the answer is always current. Returns {active_document, open_documents, current_view, project_root, updated_at}. The active_document.path can be passed straight to get_document.",
                    "parameters": { "type": "object", "properties": {} }
                },
                {
                    "name": "canvas_get",
                    "label": "Canvas: Get",
                    "description": "Read a design canvas file (.canvas JSON) and return its parsed shape: { version, theme, grid: {cols, rows, gap}, cells: [{ id, x, y, w, h, html, css, js? }] }. Pass `path` relative to project root, e.g. 'canvases/Hero.canvas'. Use canvas_active first to discover which canvas the user has open.",
                    "parameters": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                },
                {
                    "name": "canvas_set_cells",
                    "label": "Canvas: Set Cells",
                    "description": "Patch a canvas file. `cells` upserts by id (matching id replaces, new id appends). `delete_ids` removes cells. `grid` and `theme` are optional and only applied when present. Use this for incremental edits — never rewrite the whole document if you can target specific cells. Each cell must specify x, y, w, h in grid coordinates (0-indexed) plus html and css; js is optional. Response includes `lint_warnings`: an array of advisory strings flagging Flynt-canvas-specific issues (cells lacking h-full will show theme-bg below content; Tailwind arbitrary-value classes that the curated subset can't resolve). Lint never blocks the write — review warnings and fix in your next turn.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "cells": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string" },
                                        "x": { "type": "integer", "minimum": 0 },
                                        "y": { "type": "integer", "minimum": 0 },
                                        "w": { "type": "integer", "minimum": 1 },
                                        "h": { "type": "integer", "minimum": 1 },
                                        "html": { "type": "string" },
                                        "css": { "type": "string" },
                                        "js": { "type": "string" }
                                    },
                                    "required": ["id", "x", "y", "w", "h", "html", "css"]
                                }
                            },
                            "delete_ids": { "type": "array", "items": { "type": "string" } },
                            "grid": {
                                "type": "object",
                                "properties": {
                                    "cols": { "type": "integer", "minimum": 1 },
                                    "rows": { "type": "integer", "minimum": 1 },
                                    "gap": { "type": "integer", "minimum": 0 }
                                }
                            },
                            "theme": { "type": "string" }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "canvas_apply_theme",
                    "label": "Canvas: Apply Theme",
                    "description": "Set the canvas's theme. Theme tokens (--background, --primary, etc.) inject into every cell's iframe and are picked up by Tailwind utility classes. Use canvas_list_primitives to discover available themes (presets ship with the install). Unknown themes fall back to 'default' at render time, but persist as-is so an upcoming preset can take effect later.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "theme": { "type": "string" }
                        },
                        "required": ["path", "theme"]
                    }
                },
                {
                    "name": "canvas_list_primitives",
                    "label": "Canvas: List Primitives",
                    "description": "Return everything you need to design well on a canvas: (1) `primitives` — shadcn-styled HTML snippets (Button, Card, Input, Badge, Alert, Avatar, Separator, etc.) each with a `usage_notes` field calling out CSS-discipline gotchas (especially: wrap cell-outermost elements in `h-full` so cell body bg doesn't show through), (2) `themes` — every available theme preset with its full CSS-variable map under `vars` (so you know what `bg-card`/`text-foreground`/etc. actually resolve to before designing), and (3) `cell_authoring_guidance` — a short array of rules to follow when composing cells (theme/visual-language matching, sizing discipline, the Tailwind subset's lack of arbitrary-value classes). Read all three before writing cell HTML. Use canvas_apply_theme to switch themes.",
                    "parameters": { "type": "object", "properties": {} }
                },
                {
                    "name": "canvas_create",
                    "label": "Canvas: Create",
                    "description": "Create a new design canvas in the user's project. Writes a `.canvas` data file at `canvases/<name>.canvas` plus a sibling `.md` wrapper that makes it indexable and openable as a tab. Returns { wrapper_path, canvas_path } you can pass to canvas_set_cells immediately. Refuses to overwrite an existing canvas — pick a different name. Use this when the user asks to design something fresh; use canvas_set_cells on the existing canvas when they want to edit what's already open (call canvas_active first to find out).",
                    "parameters": {
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"]
                    }
                },
                {
                    "name": "canvas_active",
                    "label": "Canvas: Active",
                    "description": "Resolve the canvas the user is currently viewing. Reads the ui-state mirror, checks whether the active document is a canvas wrapper (.md whose body is exactly `![[X.canvas]]`), and returns the resolved .canvas path you can pass to canvas_get. Returns null if no canvas is active. Cheaper than running get_ui_state + parsing the body yourself.",
                    "parameters": { "type": "object", "properties": {} }
                }
                ]);
                // Append forge / engagement tools (Phase 3 — scribe absorption).
                if let Some(arr) = tools.as_array_mut() {
                    arr.extend(forge_tools::tool_definitions());
                    arr.extend(drawing_tools::tool_definitions());
                    // Append flow tools (Phase 4 — node-flow editor).
                    arr.extend(flow_tools::tool_definitions());
                }
                Ok(tools)
            }

            // ── Tool execution ────────────────────────────────────────────────
            "execute_search_documents" => {
                let query = params["query"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'query'"))?
                    .to_string();
                let limit = params["limit"].as_u64().unwrap_or(20) as usize;
                let results = self
                    .project
                    .store
                    .search_documents(&query)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                let results: Vec<_> = results.into_iter().take(limit).collect();
                Ok(serde_json::to_value(results).unwrap_or(json!([])))
            }

            "execute_list_documents" => {
                let docs = self
                    .project
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
                    .project
                    .store
                    .find_document_by_slug(slug)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(doc).unwrap_or(json!(null)))
            }

            "execute_get_document" => {
                // Accept either path or id — the agent often reaches for the id
                // returned by get_ui_state, and rejecting it forces a wasteful
                // retry on the model's part.
                let path_arg = params.get("path").and_then(|v| v.as_str());
                let id_arg = params.get("id").and_then(|v| v.as_str());
                let doc = match (path_arg, id_arg) {
                    (Some(p), _) => self
                        .project
                        .store
                        .get_document_by_path(std::path::Path::new(p))
                        .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?,
                    (None, Some(id_str)) => {
                        let uuid = uuid::Uuid::parse_str(id_str).map_err(|e| {
                            omegon_extension::Error::invalid_params(format!(
                                "id is not a UUID: {e}"
                            ))
                        })?;
                        let did = flynt_core::models::DocumentId(uuid);
                        self.project
                            .store
                            .get_document(&did)
                            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?
                    }
                    (None, None) => {
                        return Err(omegon_extension::Error::invalid_params(
                            "must provide either 'path' or 'id'",
                        ));
                    }
                };
                match doc {
                    Some(d) => Ok(serde_json::to_value(d).unwrap_or(json!({}))),
                    None => {
                        let key = path_arg.or(id_arg).unwrap_or("?");
                        Err(omegon_extension::Error::internal_error(format!(
                            "not found: {key}"
                        )))
                    }
                }
            }

            "execute_flynt_surface_guide" => Ok(flynt_surface_guide()),

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
                if contains_excalidraw_embed(content) || tags.iter().any(|tag| *tag == "drawing") {
                    return Err(omegon_extension::Error::invalid_params(
                        "Excalidraw drawings must be created with create_drawing so Flynt writes the canonical drawings/<name>.excalidraw + drawings/<name>.md pair. Do not create drawing wrappers with create_document.",
                    ));
                }
                let tags_toml = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
                let full = format!(
                    "+++\ntitle = \"{title}\"\ntags = {tags_toml}\n+++\n\n# {title}\n\n{content}"
                );
                let rel = std::path::Path::new(path);
                self.project
                    .save_document_content(rel, &full)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({ "created": path }))
            }

            "execute_move_document" => {
                let from_path = params["from_path"].as_str().ok_or_else(|| {
                    omegon_extension::Error::invalid_params("missing 'from_path'")
                })?;
                let to_path = params["to_path"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'to_path'"))?;
                let from = std::path::Path::new(from_path);
                let to = std::path::Path::new(to_path);
                self.project
                    .move_document_file(from, to)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({
                    "moved": true,
                    "from_path": from_path,
                    "to_path": to_path,
                }))
            }

            "execute_get_backlinks" => {
                let path = params["path"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'path'"))?;
                let doc = self
                    .project
                    .store
                    .get_document_by_path(std::path::Path::new(path))
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?
                    .ok_or_else(|| {
                        omegon_extension::Error::internal_error(format!("not found: {path}"))
                    })?;
                let links = self
                    .project
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
                    .project
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
                    .project
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
                            .map_err(|_| {
                                omegon_extension::Error::invalid_params("invalid 'board_id'")
                            })
                    })
                    .transpose()?;
                let column = params["column"].as_str().map(str::to_string);
                let tags: Vec<String> = params
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let status = params
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| {
                        serde_json::from_value::<flynt_core::models::TaskStatus>(json!(s)).map_err(
                            |e| omegon_extension::Error::invalid_params(format!("status: {e}")),
                        )
                    })
                    .transpose()?;
                let tasks = self
                    .project
                    .store
                    .list_tasks(&TaskFilter {
                        board_id,
                        column,
                        tags,
                        status,
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
                    .project
                    .store
                    .get_task(&id)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(task).unwrap_or(json!(null)))
            }

            "execute_create_task" => {
                let board_id = params["board_id"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'board_id'"))?;
                let board_id =
                    flynt_core::models::BoardId(uuid::Uuid::parse_str(board_id).map_err(|_| {
                        omegon_extension::Error::invalid_params("invalid 'board_id'")
                    })?);
                let column = params["column"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'column'"))?;
                let title = params["title"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'title'"))?;
                let task = Task::new(board_id, column, title);
                // Route through project.persist_task so the new task lands
                // as a .md file alongside the sqlite row.
                self.project
                    .persist_task(&task)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(&task).unwrap_or(json!({})))
            }

            "execute_update_task" => {
                let id_str = params["id"]
                    .as_str()
                    .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'id'"))?;
                let task_id =
                    flynt_core::models::TaskId(uuid::Uuid::parse_str(id_str).map_err(|_| {
                        omegon_extension::Error::invalid_params("invalid 'id' (not a UUID)")
                    })?);

                let mut patch = flynt_core::models::TaskPatch::default();
                if let Some(v) = params.get("column").and_then(|v| v.as_str()) {
                    patch.column = Some(v.to_string());
                }
                if let Some(v) = params.get("title").and_then(|v| v.as_str()) {
                    patch.title = Some(v.to_string());
                }
                if let Some(v) = params.get("description").and_then(|v| v.as_str()) {
                    patch.description = Some(v.to_string());
                }
                if let Some(v) = params.get("priority").and_then(|v| v.as_str()) {
                    let pr: flynt_core::models::Priority = serde_json::from_value(json!(v))
                        .map_err(|e| {
                            omegon_extension::Error::invalid_params(format!("priority: {e}"))
                        })?;
                    patch.priority = Some(pr);
                }
                if let Some(v) = params.get("status").and_then(|v| v.as_str()) {
                    let st: flynt_core::models::TaskStatus = serde_json::from_value(json!(v))
                        .map_err(|e| {
                            omegon_extension::Error::invalid_params(format!("status: {e}"))
                        })?;
                    patch.status = Some(st);
                }
                if let Some(arr) = params.get("tags").and_then(|v| v.as_array()) {
                    patch.tags = Some(
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                    );
                }
                if let Some(arr) = params.get("external_refs").and_then(|v| v.as_array()) {
                    patch.external_refs = Some(
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                    );
                }
                if let Some(v) = params.get("position").and_then(|v| v.as_u64()) {
                    patch.position = Some(v as u32);
                }
                if let Some(v) = params.get("design_node_id").and_then(|v| v.as_str()) {
                    // Empty string clears, non-empty parses as UUID.
                    patch.design_node_id = if v.is_empty() {
                        Some(None)
                    } else {
                        let uuid = uuid::Uuid::parse_str(v).map_err(|e| {
                            omegon_extension::Error::invalid_params(format!("design_node_id: {e}"))
                        })?;
                        Some(Some(uuid))
                    };
                }
                if let Some(v) = params.get("openspec_change").and_then(|v| v.as_str()) {
                    // Empty string clears; non-empty sets.
                    patch.openspec_change = if v.is_empty() {
                        Some(None)
                    } else {
                        Some(Some(v.to_string()))
                    };
                }
                if let Some(v) = params.get("execution") {
                    // Explicit null clears; object sets; missing field
                    // (filtered above by .get) leaves unchanged.
                    if v.is_null() {
                        patch.execution = Some(None);
                    } else {
                        let parsed: flynt_core::models::ExecutionSpec =
                            serde_json::from_value(v.clone()).map_err(|e| {
                                omegon_extension::Error::invalid_params(format!("execution: {e}"))
                            })?;
                        patch.execution = Some(Some(parsed));
                    }
                }

                let updated = self
                    .project
                    .update_any_task(&task_id, &patch)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({ "updated": updated, "task_id": id_str }))
            }

            "execute_list_boards" => {
                let boards = self
                    .project
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
                    .project
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
                self.project
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
                    .project
                    .store
                    .get_document_by_path(std::path::Path::new(path))
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?
                    .ok_or_else(|| {
                        omegon_extension::Error::internal_error(format!("not found: {path}"))
                    })?;

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
                self.project
                    .save_document_content(rel, &full)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(json!({ "converted": path, "id": doc_id.to_string(), "status": status }))
            }

            "execute_list_design_nodes" => {
                let status_filter = params["status"].as_str();
                let nodes = self
                    .project
                    .store
                    .list_entities_by_kind(&flynt_core::datum::EntityKind::DesignNode)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let mut results: Vec<Value> = Vec::new();
                for meta in nodes {
                    // Load full document for entity fields
                    let doc = self
                        .project
                        .store
                        .get_document(&meta.id)
                        .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                    let (
                        node_status,
                        node_parent,
                        node_priority,
                        node_issue_type,
                        open_questions_count,
                        deps_count,
                    ) = if let Some(ref d) = doc {
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
                validate_file_stem(name)?;
                let scene = params["scene"].as_str();
                if let Some(scene) = scene {
                    let _: Value = serde_json::from_str(scene).map_err(|e| {
                        omegon_extension::Error::invalid_params(format!(
                            "scene must be valid Excalidraw JSON: {e}"
                        ))
                    })?;
                }

                // Create drawings directory and files
                let drawings_dir = self.project.root.join("drawings");
                std::fs::create_dir_all(&drawings_dir)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                // Write .excalidraw scene file (refuse to overwrite)
                let excalidraw_file = format!("{name}.excalidraw");
                let excalidraw_abs = drawings_dir.join(&excalidraw_file);
                if excalidraw_abs.exists() {
                    return Err(omegon_extension::Error::internal_error(format!(
                        "Drawing already exists: drawings/{excalidraw_file}. Use a different name."
                    )));
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
                self.project
                    .save_document_content(rel, &md_content)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                Ok(json!({
                    "created": md_rel.clone(),
                    "wrapper_path": md_rel,
                    "drawing_path": format!("drawings/{excalidraw_file}"),
                    "excalidraw_file": format!("drawings/{excalidraw_file}"),
                    "has_scene": scene.is_some(),
                }))
            }

            "execute_drawing_active" => self.execute_drawing_active(),
            "execute_drawing_get" => self.execute_drawing_get(params),
            "execute_drawing_set_scene" => self.execute_drawing_set_scene(params),
            "execute_drawing_create_spec" => {
                drawing_tools::drawing_create_spec(&self.project, params)
            }
            "execute_drawing_get_spec" => drawing_tools::drawing_get_spec(&self.project, params),
            "execute_drawing_render_spec" => {
                drawing_tools::drawing_render_spec(&self.project, params)
            }
            "execute_drawing_patch_spec" => {
                drawing_tools::drawing_patch_spec(&self.project, params)
            }
            "execute_drawing_validate_spec" => {
                drawing_tools::drawing_validate_spec(&self.project, params)
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
                let dir = self.project.root.join(directory);
                std::fs::create_dir_all(&dir)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let d2_file = format!("{name}.d2");
                let d2_abs = dir.join(&d2_file);
                if d2_abs.exists() {
                    return Err(omegon_extension::Error::internal_error(format!(
                        "Diagram already exists: {directory}/{d2_file}. Use a different name."
                    )));
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
                self.project
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
                self.project
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
                    .project
                    .store
                    .list_entities_by_kind(&flynt_core::datum::EntityKind::WorkspaceLease)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let mut results: Vec<Value> = Vec::new();
                for meta in leases {
                    let doc = self
                        .project
                        .store
                        .get_document(&meta.id)
                        .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                    let view = doc
                        .as_ref()
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
                let payload = build_graph_payload(&*self.project.store)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
                Ok(serde_json::to_value(&payload).unwrap_or(json!({})))
            }

            "execute_get_graph_filtered" => {
                let payload = build_graph_payload(&*self.project.store)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let kind_filter = params["kind"].as_str();
                let group_filter = params["group"].as_str();
                let tag_filter = params["tag"].as_str();
                let search = params["search"].as_str().unwrap_or("");
                let min_degree = params["min_degree"].as_u64().unwrap_or(0) as u32;

                let mut degree: std::collections::HashMap<&str, u32> =
                    std::collections::HashMap::new();
                for edge in &payload.edges {
                    *degree.entry(&edge.source).or_default() += 1;
                    *degree.entry(&edge.target).or_default() += 1;
                }

                let search_lower = search.to_lowercase();
                let nodes: Vec<_> = payload
                    .nodes
                    .iter()
                    .filter(|n| {
                        if let Some(k) = kind_filter {
                            if format_kind(&n.kind) != k {
                                return false;
                            }
                        }
                        if let Some(g) = group_filter {
                            if n.group != g {
                                return false;
                            }
                        }
                        if let Some(t) = tag_filter {
                            if !n.tags.contains(&t.to_string()) {
                                return false;
                            }
                        }
                        if !search_lower.is_empty()
                            && !n.title.to_lowercase().contains(&search_lower)
                        {
                            return false;
                        }
                        if min_degree > 0 {
                            if degree.get(n.id.as_str()).copied().unwrap_or(0) < min_degree {
                                return false;
                            }
                        }
                        true
                    })
                    .collect();

                let mut ids: std::collections::HashSet<&str> =
                    nodes.iter().map(|n| n.id.as_str()).collect();

                // Design node filter: also include direct dependency targets
                // so the graph shows what design nodes depend on.
                if kind_filter == Some("design_node") {
                    let dep_targets: Vec<&str> = payload
                        .edges
                        .iter()
                        .filter(|e| {
                            ids.contains(e.source.as_str())
                                && (e.kind == flynt_core::graph::GraphEdgeKind::Dependency
                                    || e.kind == flynt_core::graph::GraphEdgeKind::ParentChild)
                        })
                        .map(|e| e.target.as_str())
                        .collect();
                    // Also include parent sources for ParentChild edges
                    let parent_sources: Vec<&str> = payload
                        .edges
                        .iter()
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
                let nodes: Vec<_> = payload
                    .nodes
                    .iter()
                    .filter(|n| ids.contains(n.id.as_str()))
                    .collect();

                let edges: Vec<_> = payload
                    .edges
                    .iter()
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

                let payload = build_graph_payload(&*self.project.store)
                    .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

                let connected_edges: Vec<_> = payload
                    .edges
                    .iter()
                    .filter(|e| e.source == node_id || e.target == node_id)
                    .collect();

                let mut neighbor_ids: std::collections::HashSet<&str> =
                    std::collections::HashSet::new();
                neighbor_ids.insert(node_id);
                for edge in &connected_edges {
                    neighbor_ids.insert(&edge.source);
                    neighbor_ids.insert(&edge.target);
                }

                let neighbor_nodes: Vec<_> = payload
                    .nodes
                    .iter()
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

            "execute_get_ui_state" => {
                // Read the live snapshot flynt-app maintains. Returning an empty
                // shape (instead of an error) when the file is absent matters:
                // the agent can still answer "no file open" without surfacing
                // a confusing tool error to the user.
                let path = self
                    .project
                    .root
                    .join(".flynt-local")
                    .join("flynt")
                    .join("ui-state.json");
                match std::fs::read_to_string(&path) {
                    Ok(body) => serde_json::from_str::<Value>(&body).map_err(|e| {
                        omegon_extension::Error::internal_error(format!(
                            "ui-state.json malformed: {e}"
                        ))
                    }),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({
                        "active_document": null,
                        "open_documents": [],
                        "current_view": null,
                        "project_root": self.project.root.to_string_lossy(),
                        "updated_at": null,
                        "note": "ui-state.json not yet written — flynt-app may not be running, or no view has rendered yet"
                    })),
                    Err(e) => Err(omegon_extension::Error::internal_error(e.to_string())),
                }
            }

            "execute_canvas_get" => self.execute_canvas_get(params),
            "execute_canvas_set_cells" => self.execute_canvas_set_cells(params),
            "execute_canvas_apply_theme" => self.execute_canvas_apply_theme(params),
            "execute_canvas_list_primitives" => self.execute_canvas_list_primitives(),
            "execute_canvas_active" => self.execute_canvas_active(),
            "execute_canvas_create" => self.execute_canvas_create(params),

            // ── Forge / engagement tools (Phase 3) ────────────────────────
            // The omegon-pushed secret hand-off. Falls through to env fallback
            // (FLYNT_GITHUB_TOKEN) seeded at extension construction, so this
            // RPC is optional from omegon's side and the ACP/Zed launch path
            // works without it.
            "bootstrap_secrets" => forge_tools::bootstrap_secrets(&self.secrets, params),

            "execute_engagement_create" => forge_tools::engagement_create(&self.project, params),
            "execute_engagement_update" => forge_tools::engagement_update(&self.project, params),
            "execute_engagement_list" => forge_tools::engagement_list(&self.project, params),
            "execute_engagement_status" => forge_tools::engagement_status(&self.project, params),
            "execute_forge_status" => {
                forge_tools::forge_status(&self.project, &self.secrets, params)
            }
            "execute_forge_list_issues" => {
                forge_tools::forge_list_issues(&self.project, &self.secrets, params).await
            }
            "execute_forge_sync_issues" => {
                forge_tools::forge_sync_issues(&self.project, &self.secrets, params).await
            }
            "execute_forge_create_issue" => {
                forge_tools::forge_create_issue(&self.project, &self.secrets, params).await
            }
            "execute_log_work" => forge_tools::log_work(&self.project, params),
            "execute_timeline" => forge_tools::timeline(&self.project, params),

            // ── Flow tools (Phase 4 — node-flow editor) ───────────────────
            "execute_flow_create" => flow_tools::flow_create(&self.project, params),
            "execute_flow_get" => flow_tools::flow_get(&self.project, params),
            "execute_flow_patch" => flow_tools::flow_patch(&self.project, params),

            _ => Err(omegon_extension::Error::method_not_found(method)),
        }
    }
}

fn flynt_surface_guide() -> Value {
    json!({
        "identity": "You are operating inside Flynt, a local-first project workspace. Use get_ui_state before assuming what the operator has open.",
        "surfaces": [
            {
                "kind": "note",
                "paths": ["*.md"],
                "tools": ["get_document", "create_document"],
                "use_for": "ordinary markdown notes, research, project docs"
            },
            {
                "kind": "drawing",
                "paths": ["drawings/<name>.md", "drawings/<name>.excalidraw"],
                "tools": ["create_drawing", "drawing_active", "drawing_get", "drawing_set_scene", "drawing_create_spec", "drawing_get_spec", "drawing_render_spec", "drawing_patch_spec", "drawing_validate_spec"],
                "use_for": "freeform Excalidraw sketches",
                "rules": [
                    "Excalidraw drawings live under drawings/, not diagrams/.",
                    "The openable sidebar/tab entry is the drawings/<name>.md wrapper; Flynt renders the sibling .excalidraw file visually from that wrapper.",
                    "Prefer drawing_create_spec / drawing_patch_spec for agent-authored architecture diagrams instead of generating raw Excalidraw JSON.",
                    "Do not create Excalidraw wrapper markdown with create_document.",
                    "Do not tell the operator to switch to a separate drawing view; have them open/select the wrapper entry.",
                    "Use drawing_active before editing the drawing the operator has open."
                ]
            },
            {
                "kind": "d2_diagram",
                "paths": ["diagrams/<name>.d2"],
                "tools": ["create_d2_diagram"],
                "use_for": "text-authored D2 diagrams"
            },
            {
                "kind": "design_canvas",
                "paths": ["canvases/<name>.md", "canvases/<name>.canvas"],
                "tools": ["canvas_create", "canvas_active", "canvas_get", "canvas_set_cells", "canvas_apply_theme", "canvas_list_primitives"],
                "use_for": "Flynt design canvases made of grid-positioned HTML/CSS cells",
                "rules": [
                    "This is not Excalidraw.",
                    "Read canvas_list_primitives before authoring polished cells.",
                    "Use canvas_active before editing the canvas the operator has open."
                ]
            },
            {
                "kind": "flow_graph",
                "paths": ["*.flow"],
                "tools": ["flow_create", "flow_get", "flow_patch"],
                "use_for": "node-flow architecture or workflow graphs"
            }
        ]
    })
}

fn validate_file_stem(name: &str) -> omegon_extension::Result<()> {
    if name.trim().is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(omegon_extension::Error::invalid_params(
            "name must not be empty or contain path separators",
        ));
    }
    Ok(())
}

fn contains_excalidraw_embed(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("![[") && trimmed.ends_with(".excalidraw]]")
    })
}

fn excalidraw_embed_path(content: &str) -> Option<String> {
    let body = if let Some(rest) = content.strip_prefix("+++\n") {
        if let Some(end) = rest.find("\n+++") {
            rest[end + 4..].trim()
        } else {
            content.trim()
        }
    } else {
        content.trim()
    };

    let lines: Vec<&str> = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() == 1 {
        let line = lines[0].trim();
        if line.starts_with("![[") && line.ends_with(".excalidraw]]") {
            return Some(line[3..line.len() - 2].to_string());
        }
    }
    None
}

fn drawing_path_arg(params: &Value) -> omegon_extension::Result<&str> {
    params
        .get("path")
        .and_then(|v| v.as_str())
        .or_else(|| params.get("drawing_path").and_then(|v| v.as_str()))
        .ok_or_else(|| {
            omegon_extension::Error::invalid_params("missing 'path' (or 'drawing_path')")
        })
}

// ── Canvas tool implementations ───────────────────────────────────────────────
//
// Pulled out as inherent methods (rather than inline match arms) so each tool
// is independently unit-testable and the dispatch table above stays scannable.
// All paths are interpreted relative to the project root, matching get_document
// ergonomics. None of these tools panic on malformed input — they cross the
// ACP boundary, where a panic would kill the worker thread.
impl FlyntExtension {
    fn resolve_drawing_path(
        &self,
        path_arg: &str,
    ) -> Result<std::path::PathBuf, omegon_extension::Error> {
        let rel = std::path::Path::new(path_arg);
        if rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(omegon_extension::Error::invalid_params(
                "path must not contain '..'",
            ));
        }
        if rel.extension().and_then(|ext| ext.to_str()) != Some("excalidraw") {
            return Err(omegon_extension::Error::invalid_params(
                "path must point to a .excalidraw file",
            ));
        }
        Ok(self.project.root.join(rel))
    }

    fn execute_drawing_get(&self, params: Value) -> omegon_extension::Result<Value> {
        let path = drawing_path_arg(&params)?;
        let abs = self.resolve_drawing_path(path)?;
        let body = std::fs::read_to_string(&abs)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        let scene: Value = serde_json::from_str(&body).map_err(|e| {
            omegon_extension::Error::internal_error(format!("parse drawing json: {e}"))
        })?;
        Ok(json!({
            "path": path,
            "scene": scene,
        }))
    }

    fn execute_drawing_set_scene(&self, params: Value) -> omegon_extension::Result<Value> {
        let path = drawing_path_arg(&params)?;
        let scene_value = params
            .get("scene")
            .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'scene'"))?;
        let scene = coerce_to_value(scene_value.clone(), "scene")?;
        let serialized = serde_json::to_string_pretty(&scene)
            .map_err(|e| omegon_extension::Error::invalid_params(format!("scene: {e}")))?;
        let abs = self.resolve_drawing_path(path)?;
        if !abs.exists() {
            return Err(omegon_extension::Error::internal_error(format!(
                "drawing not found: {path}"
            )));
        }
        std::fs::write(&abs, serialized)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        Ok(json!({
            "path": path,
            "updated": true,
        }))
    }

    fn execute_drawing_active(&self) -> omegon_extension::Result<Value> {
        let ui_path = self
            .project
            .root
            .join(".flynt-local")
            .join("flynt")
            .join("ui-state.json");
        let ui: Value = match std::fs::read_to_string(&ui_path) {
            Ok(body) => serde_json::from_str(&body).unwrap_or(Value::Null),
            Err(_) => return Ok(Value::Null),
        };

        let active = ui.get("active_document");
        let active_path = active.and_then(|d| d.get("path")).and_then(|v| v.as_str());
        let Some(md_path) = active_path else {
            return Ok(Value::Null);
        };

        let typed_drawing = active
            .and_then(|d| d.get("document_type"))
            .and_then(|v| v.as_str())
            == Some("drawing");

        let drawing_file = if typed_drawing {
            std::path::Path::new(md_path)
                .file_stem()
                .map(|s| format!("{}.excalidraw", s.to_string_lossy()))
        } else {
            let md_abs = self.project.root.join(md_path);
            let md_body = match std::fs::read_to_string(&md_abs) {
                Ok(s) => s,
                Err(_) => return Ok(Value::Null),
            };
            excalidraw_embed_path(&md_body)
        };
        let Some(drawing_file) = drawing_file else {
            return Ok(Value::Null);
        };

        let doc_dir = std::path::Path::new(md_path)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        let drawing_rel = doc_dir.join(&drawing_file);
        Ok(json!({
            "wrapper_path": md_path,
            "drawing_path": drawing_rel.to_string_lossy(),
        }))
    }

    fn resolve_canvas_path(
        &self,
        path_arg: &str,
    ) -> Result<std::path::PathBuf, omegon_extension::Error> {
        // Refuse paths that escape the project root (path traversal). The agent
        // shouldn't be writing outside the project, ever.
        let rel = std::path::Path::new(path_arg);
        if rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(omegon_extension::Error::invalid_params(
                "path must not contain '..'",
            ));
        }
        Ok(self.project.root.join(rel))
    }

    fn execute_canvas_get(&self, params: Value) -> omegon_extension::Result<Value> {
        let path = canvas_path_arg(&params)?;
        let abs = self.resolve_canvas_path(path)?;
        let canvas = flynt_core::canvas::Canvas::load(&abs)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        serde_json::to_value(&canvas)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))
    }

    fn execute_canvas_set_cells(&self, params: Value) -> omegon_extension::Result<Value> {
        let path = canvas_path_arg(&params)?;
        let abs = self.resolve_canvas_path(path)?;

        // Load existing or start fresh. Phase 5 lets the agent create cells in
        // a freshly-touched file by writing a default canvas first; that keeps
        // the tool useful even when the user hasn't created the canvas via UI.
        let mut canvas = match flynt_core::canvas::Canvas::load(&abs) {
            Ok(c) => c,
            Err(_) if !abs.exists() => flynt_core::canvas::Canvas::default(),
            Err(e) => return Err(omegon_extension::Error::internal_error(e.to_string())),
        };

        if let Some(theme) = params.get("theme").and_then(|v| v.as_str()) {
            canvas.theme = theme.to_string();
        }
        if let Some(grid_val) = params.get("grid") {
            let grid_val = coerce_to_value(grid_val.clone(), "grid")?;
            let grid: flynt_core::canvas::Grid = serde_json::from_value(grid_val)
                .map_err(|e| omegon_extension::Error::invalid_params(format!("grid: {e}")))?;
            canvas.grid = grid;
        }

        // delete_ids accepts either an array or a JSON-stringified array (some
        // LLMs emit nested args as strings). Reject any other shape so silent
        // no-ops don't masquerade as success.
        let mut deleted = Vec::new();
        if let Some(ids_val) = params.get("delete_ids") {
            let ids_val = coerce_to_value(ids_val.clone(), "delete_ids")?;
            let ids = ids_val.as_array().ok_or_else(|| {
                omegon_extension::Error::invalid_params("delete_ids: expected array")
            })?;
            for id in ids {
                if let Some(id_str) = id.as_str() {
                    if canvas.remove_cell(id_str) {
                        deleted.push(id_str.to_string());
                    }
                }
            }
        }

        // cells accepts either an array or a JSON-stringified array. Same
        // rationale as delete_ids — silently ignoring a stringified payload
        // looked like success while doing nothing, which is exactly the kind
        // of failure mode a tool should never let through.
        let mut upserted = Vec::new();
        let mut lint_warnings: Vec<String> = Vec::new();
        if let Some(cells_val) = params.get("cells") {
            let cells_val = coerce_to_value(cells_val.clone(), "cells")?;
            let cells = cells_val
                .as_array()
                .ok_or_else(|| omegon_extension::Error::invalid_params("cells: expected array"))?;
            for c in cells {
                let cell: flynt_core::canvas::Cell = serde_json::from_value(c.clone())
                    .map_err(|e| omegon_extension::Error::invalid_params(format!("cell: {e}")))?;
                lint_warnings.extend(lint_cell(&cell));
                let id = cell.id.clone();
                let replaced = canvas.upsert_cell(cell);
                upserted.push(json!({ "id": id, "replaced": replaced }));
            }
        }

        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        }
        canvas
            .save(&abs)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

        Ok(json!({
            "path": path,
            "cell_count": canvas.cells.len(),
            "upserted": upserted,
            "deleted": deleted,
            // Lint warnings are advisory — the cells were written. Agent
            // should review and follow up with corrections in the next turn
            // if any are present. Empty array when everything looks clean.
            "lint_warnings": lint_warnings,
        }))
    }

    fn execute_canvas_apply_theme(&self, params: Value) -> omegon_extension::Result<Value> {
        let path = canvas_path_arg(&params)?;
        let theme = params["theme"]
            .as_str()
            .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'theme'"))?;
        let abs = self.resolve_canvas_path(path)?;
        let mut canvas = flynt_core::canvas::Canvas::load(&abs)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        let previous = canvas.theme.clone();
        canvas.theme = theme.to_string();
        canvas
            .save(&abs)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        Ok(json!({ "path": path, "theme": theme, "previous_theme": previous }))
    }

    fn execute_canvas_list_primitives(&self) -> omegon_extension::Result<Value> {
        // Read from the project-side copy that flynt-app's canvas_assets bootstrap
        // writes on launch. If the bootstrap hasn't run yet (agent started before
        // app), return an empty primitives list rather than erroring.
        let dir = self
            .project
            .root
            .join(".flynt-local")
            .join("flynt")
            .join("assets");
        let primitives_doc = read_json_or_default(
            &dir.join("shadcn-primitives.json"),
            json!({ "version": 1, "primitives": [] }),
        );
        let presets = read_json_or_default(&dir.join("tweakcn-presets.json"), json!({}));

        // Theme summaries now include the full `vars` map. The agent uses these
        // to design with concrete colors (avoids guessing what `bg-card` is at
        // render time) and to recommend a theme that matches the user's intent
        // before designing rather than mid-flight.
        let themes: Vec<Value> = presets
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(id, v)| {
                        json!({
                            "id": id,
                            "name": v.get("name").cloned().unwrap_or(json!(id)),
                            "description": v.get("description").cloned().unwrap_or(Value::Null),
                            "vars": v.get("vars").cloned().unwrap_or(Value::Null),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(json!({
            "primitives": primitives_doc.get("primitives").cloned().unwrap_or(json!([])),
            // Cell-authoring guidance (h-full discipline, theme/visual-language
            // matching, Tailwind subset constraints). Lives in the JSON file so
            // it's editable by the design skill author without recompiling.
            "cell_authoring_guidance": primitives_doc
                .get("cell_authoring_guidance")
                .cloned()
                .unwrap_or(json!([])),
            "themes": themes,
        }))
    }

    fn execute_canvas_create(&self, params: Value) -> omegon_extension::Result<Value> {
        let name = params["name"]
            .as_str()
            .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'name'"))?;
        // Refuse names that would escape the canvases/ directory or contain
        // path separators — same posture as resolve_canvas_path's traversal
        // guard but applied to the name component before it's joined.
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(omegon_extension::Error::invalid_params(
                "name must not be empty or contain path separators",
            ));
        }
        let md_rel = flynt_core::canvas::create_canvas(&self.project.root, name)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        self.project
            .index_file(&self.project.root.join(&md_rel))
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        let stem = std::path::Path::new(&md_rel)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| name.to_string());
        Ok(json!({
            "wrapper_path": md_rel.to_string_lossy(),
            "canvas_path": format!("canvases/{stem}.canvas"),
        }))
    }

    fn execute_canvas_active(&self) -> omegon_extension::Result<Value> {
        let ui_path = self
            .project
            .root
            .join(".flynt-local")
            .join("flynt")
            .join("ui-state.json");
        let ui: Value = match std::fs::read_to_string(&ui_path) {
            Ok(body) => serde_json::from_str(&body).unwrap_or(Value::Null),
            Err(_) => return Ok(Value::Null),
        };

        let active = ui.get("active_document");
        let active_path = active.and_then(|d| d.get("path")).and_then(|v| v.as_str());
        let Some(md_path) = active_path else {
            return Ok(Value::Null);
        };

        // Fast path: if flynt-app classified this doc as "canvas" in the
        // mirror, trust it and skip the body parse. Falls back to parsing
        // the wrapper if the field is missing (older flynt-app, or a
        // foreign tool wrote ui-state.json).
        let typed_canvas = active
            .and_then(|d| d.get("document_type"))
            .and_then(|v| v.as_str())
            == Some("canvas");

        let canvas_file = if typed_canvas {
            // The wrapper md and the .canvas data file share a stem, so we can
            // skip the body read entirely.
            std::path::Path::new(md_path)
                .file_stem()
                .map(|s| format!("{}.canvas", s.to_string_lossy()))
        } else {
            let md_abs = self.project.root.join(md_path);
            let md_body = match std::fs::read_to_string(&md_abs) {
                Ok(s) => s,
                Err(_) => return Ok(Value::Null),
            };
            canvas_embed_path(&md_body)
        };
        let Some(canvas_file) = canvas_file else {
            return Ok(Value::Null);
        };

        let doc_dir = std::path::Path::new(md_path)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        let canvas_rel = doc_dir.join(&canvas_file);
        Ok(json!({
            "wrapper_path": md_path,
            "canvas_path": canvas_rel.to_string_lossy(),
        }))
    }
}

/// Structural lint for cell HTML — surfaces Flynt-canvas-specific issues
/// only. Deliberately NOT a general-purpose visual/CSS/a11y linter:
/// the rule of thumb is "does this cell behave correctly inside Flynt's
/// canvas pipeline?", not "is this good design?".
///
/// Currently checks:
///   1. The outermost element fills the cell (has `h-full` or `height:100%`).
///      Otherwise the cell's iframe body shows the theme `--background`
///      below the content as empty space — the most-reported visual bug.
///   2. Tailwind arbitrary-value classes (`bg-[#...]`, `text-[#...]`, etc.).
///      Flynt's Tailwind subset is hand-curated and lacks the JIT compiler
///      that would resolve these — they silently no-op at render time.
///
/// Things this lint will NOT do, by design:
///   - Color/typography/hierarchy quality (skill territory)
///   - Accessibility (contrast, ARIA, semantics)
///   - HTML/CSS quality, redundancy, idiomatic style
///   - Theme-token vs explicit-color preference (sometimes intentional)
///
/// Returns warnings as plain strings prefixed with the cell id. The tool
/// surfaces these as `lint_warnings` in the response — never blocks the
/// write. Agent sees them in-band and can react on the same turn.
fn lint_cell(cell: &flynt_core::canvas::Cell) -> Vec<String> {
    let mut warnings = Vec::new();
    let html = &cell.html;
    let id = &cell.id;

    // (1) Outermost-fills-cell check. Cheap heuristic: look at the first
    //     element-open tag in the HTML and check its class/style attr for
    //     h-full or height:100%. If neither is present and there's no
    //     wrapping element with explicit height, warn.
    if !outermost_fills_cell(html) {
        warnings.push(format!(
            "cell '{id}': outermost element lacks h-full (or height:100%) — \
             empty space will show the canvas theme --background below your \
             content. Add h-full to the outer element or wrap in <div class=\"h-full\">."
        ));
    }

    // (2) Tailwind arbitrary-value classes. Match the `[…]` syntax that
    //     follows known utility prefixes. Cheap substring scan; false
    //     positives possible if the same pattern appears in attribute
    //     text content, but it's a warning, not a blocker.
    let arbitrary_prefixes = [
        "bg-[",
        "text-[",
        "border-[",
        "ring-[",
        "shadow-[",
        "p-[",
        "px-[",
        "py-[",
        "pt-[",
        "pb-[",
        "pl-[",
        "pr-[",
        "m-[",
        "mx-[",
        "my-[",
        "mt-[",
        "mb-[",
        "ml-[",
        "mr-[",
        "w-[",
        "h-[",
        "min-w-[",
        "min-h-[",
        "max-w-[",
        "max-h-[",
        "gap-[",
        "rounded-[",
    ];
    for prefix in &arbitrary_prefixes {
        if html.contains(prefix) {
            warnings.push(format!(
                "cell '{id}': uses Tailwind arbitrary-value class '{prefix}…]' \
                 — Flynt's Tailwind subset can't resolve these (no JIT compiler). \
                 Use a theme token like bg-primary, or put the custom rule in cell.css."
            ));
            break; // one warning per cell is enough; agent will scan all of them
        }
    }

    warnings
}

/// Heuristic: does the outermost element of `html` have h-full (or
/// height:100%)? Examines only the first element-open tag's class and
/// style attributes. Scans linearly; doesn't parse HTML.
fn outermost_fills_cell(html: &str) -> bool {
    // Find the first '<' followed by an alphabetic char (skip text/comments).
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            break;
        }
        i += 1;
    }
    if i >= bytes.len() {
        // No element at all — fragment-only or empty. Treat as filling
        // (nothing to warn about; the cell is intentionally bare).
        return true;
    }
    // Find the matching '>' (skip past the entire opening tag).
    let tag_end = match html[i..].find('>') {
        Some(e) => i + e,
        None => return true, // malformed; not our problem to flag here
    };
    let tag = &html[i..=tag_end];

    // h-full in any class attribute (single-quoted or double-quoted)
    if tag.contains("h-full") {
        return true;
    }
    // explicit height:100% in style attr
    if tag.contains("height:100%") || tag.contains("height: 100%") {
        return true;
    }
    // h-screen also fills (within iframe)
    if tag.contains("h-screen") {
        return true;
    }
    false
}

/// Accept the canvas path under either `path` or `canvas_path`. The agent's
/// own tool listing lists `path` as the parameter, but it consistently
/// reaches for `canvas_path` (likely cued by the field name in canvas_active's
/// return shape). Accepting both eliminates a recurring round-trip failure
/// where the first call errors and the agent retries with the right name.
fn canvas_path_arg(params: &Value) -> omegon_extension::Result<&str> {
    params
        .get("path")
        .and_then(|v| v.as_str())
        .or_else(|| params.get("canvas_path").and_then(|v| v.as_str()))
        .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'path' (or 'canvas_path')"))
}

/// Accept either a structured JSON value or a stringified JSON value and
/// return the structured form. Some LLM tool-call surfaces emit nested args
/// as JSON-encoded strings (especially Anthropic's, when array/object args
/// are deeply nested); accepting both keeps the tool tolerant without
/// silently dropping a stringified payload.
fn coerce_to_value(v: Value, field: &str) -> omegon_extension::Result<Value> {
    match v {
        Value::String(s) => serde_json::from_str(&s).map_err(|e| {
            omegon_extension::Error::invalid_params(format!(
                "{field}: stringified value did not parse as JSON: {e}"
            ))
        }),
        other => Ok(other),
    }
}

fn read_json_or_default(path: &std::path::Path, fallback: Value) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(fallback)
}

/// Detect a `.md` wrapper whose body is exactly one `![[...canvas]]` embed.
/// Mirrors the same logic in flynt-app::views::canvas — duplicated here so
/// flynt-agent doesn't take a UI-crate dependency.
fn canvas_embed_path(content: &str) -> Option<String> {
    let body = if let Some(rest) = content.strip_prefix("+++\n") {
        if let Some(end) = rest.find("\n+++") {
            rest[end + 4..].trim()
        } else {
            content.trim()
        }
    } else {
        content.trim()
    };
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() == 1 {
        let line = lines[0].trim();
        if line.starts_with("![[") && line.ends_with(".canvas]]") {
            return Some(line[3..line.len() - 2].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::FlyntExtension;
    use flynt_store::project::Project;
    use omegon_extension::Extension;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_extension() -> (TempDir, FlyntExtension) {
        let tmp = TempDir::new().unwrap();
        let project = Arc::new(Project::open(tmp.path()).unwrap());
        (tmp, FlyntExtension::new(project))
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
        assert!(names.contains(&"flynt_surface_guide".to_string()));
        assert!(names.contains(&"move_document".to_string()));
        assert!(names.contains(&"store_memory_fact".to_string()));
        assert!(names.contains(&"store_agent_communication".to_string()));
        assert!(names.contains(&"get_task".to_string()));
        assert!(names.contains(&"create_task".to_string()));
        assert!(names.contains(&"get_board".to_string()));
        assert!(names.contains(&"create_board".to_string()));
        for n in [
            "create_drawing",
            "drawing_active",
            "drawing_get",
            "drawing_set_scene",
            "drawing_create_spec",
            "drawing_get_spec",
            "drawing_render_spec",
            "drawing_patch_spec",
            "drawing_validate_spec",
        ] {
            assert!(names.contains(&n.to_string()), "expected {n} in tools/list");
        }
        // Phase 3 — scribe-absorbed forge / engagement tools.
        for n in [
            "engagement_create",
            "engagement_update",
            "engagement_list",
            "engagement_status",
            "forge_status",
            "forge_list_issues",
            "forge_sync_issues",
            "forge_create_issue",
            "log_work",
            "timeline",
        ] {
            assert!(names.contains(&n.to_string()), "expected {n} in tools/list");
        }
        // Phase 4 — node-flow editor tools.
        for n in ["flow_create", "flow_get", "flow_patch"] {
            assert!(names.contains(&n.to_string()), "expected {n} in tools/list");
        }
        // bootstrap_secrets is intentionally NOT a tool — it's an out-
        // of-band omegon→extension secret-push RPC. Surfacing it as a
        // tool would let an MCP client try to call it via tools/call,
        // which would route to execute_bootstrap_secrets and 404.
        assert!(!names.contains(&"bootstrap_secrets".to_string()));
    }

    #[tokio::test]
    async fn bootstrap_secrets_is_top_level_only_not_a_tool() {
        let (_tmp, ext) = test_extension();
        // Direct top-level call: succeeds.
        let r = ext
            .handle_rpc("bootstrap_secrets", json!({"GITHUB_TOKEN": "x"}))
            .await
            .unwrap();
        assert_eq!(r["acknowledged"], true);

        // Via the execute_ prefix that tools/call would route to:
        // intentionally NOT registered, so it resolves to method_not_found.
        let r = ext.handle_rpc("execute_bootstrap_secrets", json!({})).await;
        assert!(r.is_err(), "execute_bootstrap_secrets must not be wired");
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
        assert!(
            memory["path"]
                .as_str()
                .unwrap()
                .contains("ai/memory/storage")
        );

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
        assert!(
            comm["path"]
                .as_str()
                .unwrap()
                .contains("references/comms/scribe")
        );

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
            .handle_rpc(
                "execute_list_tasks",
                json!({ "column": "Backlog", "board_id": board_id }),
            )
            .await
            .unwrap();
        assert_eq!(tasks.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn surface_guide_distinguishes_drawings_diagrams_and_canvases() {
        let (_tmp, ext) = test_extension();
        let guide = ext
            .handle_rpc("execute_flynt_surface_guide", json!({}))
            .await
            .unwrap();
        let body = guide.to_string();
        assert!(body.contains("drawings/<name>.excalidraw"));
        assert!(body.contains("diagrams/<name>.d2"));
        assert!(body.contains("canvases/<name>.canvas"));
    }

    #[tokio::test]
    async fn create_document_refuses_excalidraw_wrappers() {
        let (_tmp, ext) = test_extension();
        let err = ext
            .handle_rpc(
                "execute_create_document",
                json!({
                    "path": "diagrams/Sketch.md",
                    "title": "Sketch",
                    "content": "![[Sketch.excalidraw]]",
                    "tags": ["drawing"]
                }),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("create_drawing"), "got: {err}");
    }

    #[tokio::test]
    async fn move_document_moves_file_and_updates_index() {
        let (tmp, ext) = test_extension();
        ext.handle_rpc(
            "execute_create_document",
            json!({
                "path": "Inbox/Thing.md",
                "title": "Thing",
                "content": "Body"
            }),
        )
        .await
        .unwrap();

        let out = ext
            .handle_rpc(
                "execute_move_document",
                json!({
                    "from_path": "Inbox/Thing.md",
                    "to_path": "docs/Thing.md"
                }),
            )
            .await
            .unwrap();
        assert_eq!(out["moved"], true);
        assert!(!tmp.path().join("Inbox/Thing.md").exists());
        assert!(tmp.path().join("docs/Thing.md").exists());

        let old = ext
            .handle_rpc("execute_get_document", json!({"path": "Inbox/Thing.md"}))
            .await;
        assert!(old.is_err(), "old indexed path should be removed");
        let new_doc = ext
            .handle_rpc("execute_get_document", json!({"path": "docs/Thing.md"}))
            .await
            .unwrap();
        assert_eq!(new_doc["path"], "docs/Thing.md");
    }

    // ── Canvas tools ────────────────────────────────────────────────────────

    fn write_canvas(tmp: &TempDir, rel: &str, body: &str) {
        let abs = tmp.path().join(rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, body).unwrap();
    }

    fn write_ui_state(tmp: &TempDir, active_path: Option<&str>) {
        let dir = tmp.path().join(".flynt-local").join("flynt");
        std::fs::create_dir_all(&dir).unwrap();
        let body = match active_path {
            Some(p) => json!({
                "active_document": {"id": "x", "title": "x", "path": p},
                "open_documents": [],
                "current_view": "notes",
                "project_root": tmp.path().to_string_lossy(),
                "updated_at": "now"
            }),
            None => json!({
                "active_document": null, "open_documents": [],
                "current_view": "notes", "project_root": tmp.path().to_string_lossy(),
                "updated_at": "now"
            }),
        };
        std::fs::write(
            dir.join("ui-state.json"),
            serde_json::to_string(&body).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn drawing_create_get_set_and_active_round_trip() {
        let (tmp, ext) = test_extension();
        let created = ext
            .handle_rpc("execute_create_drawing", json!({"name": "Sketch"}))
            .await
            .unwrap();
        assert_eq!(created["wrapper_path"], "drawings/Sketch.md");
        assert_eq!(created["drawing_path"], "drawings/Sketch.excalidraw");
        assert!(tmp.path().join("drawings/Sketch.md").exists());
        assert!(tmp.path().join("drawings/Sketch.excalidraw").exists());

        write_ui_state(&tmp, Some("drawings/Sketch.md"));
        let active = ext
            .handle_rpc("execute_drawing_active", json!({}))
            .await
            .unwrap();
        assert_eq!(active["wrapper_path"], "drawings/Sketch.md");
        assert_eq!(active["drawing_path"], "drawings/Sketch.excalidraw");

        let scene = json!({
            "type": "excalidraw",
            "version": 2,
            "elements": [{"id": "box", "type": "rectangle"}],
            "appState": {"theme": "dark"}
        });
        ext.handle_rpc(
            "execute_drawing_set_scene",
            json!({"path": "drawings/Sketch.excalidraw", "scene": scene}),
        )
        .await
        .unwrap();

        let got = ext
            .handle_rpc(
                "execute_drawing_get",
                json!({"path": "drawings/Sketch.excalidraw"}),
            )
            .await
            .unwrap();
        assert_eq!(got["scene"]["elements"][0]["id"], "box");
    }

    #[tokio::test]
    async fn drawing_spec_tools_create_patch_and_get_sidecar() {
        let (tmp, ext) = test_extension();
        let spec = json!({
            "title": "Architecture",
            "components": [
                {"id": "ui", "kind": "actor", "label": "Operator", "rank": 0, "lane": 0},
                {"id": "api", "kind": "service", "label": "Control API", "rank": 1, "lane": 0}
            ],
            "connections": [
                {"id": "ui-api", "from": "ui", "to": "api", "label": "HTTPS"}
            ]
        });
        let created = ext
            .handle_rpc(
                "execute_drawing_create_spec",
                json!({"name": "Spec Sketch", "spec": spec}),
            )
            .await
            .unwrap();
        assert_eq!(created["drawing_path"], "drawings/Spec Sketch.excalidraw");
        assert!(
            tmp.path()
                .join("drawings/Spec Sketch.drawing.json")
                .exists()
        );

        let patched = ext
            .handle_rpc(
                "execute_drawing_patch_spec",
                json!({
                    "path": "drawings/Spec Sketch.excalidraw",
                    "upsert_components": [
                        {"id": "db", "kind": "database", "label": "SQLite", "rank": 2, "lane": 0}
                    ],
                    "upsert_connections": [
                        {"id": "api-db", "from": "api", "to": "db", "label": "read/write"}
                    ]
                }),
            )
            .await
            .unwrap();
        assert_eq!(patched["component_count"], 3);

        let got = ext
            .handle_rpc(
                "execute_drawing_get_spec",
                json!({"path": "drawings/Spec Sketch.excalidraw"}),
            )
            .await
            .unwrap();
        assert_eq!(got["spec"]["components"].as_array().unwrap().len(), 3);

        let scene: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.path().join("drawings/Spec Sketch.excalidraw")).unwrap(),
        )
        .unwrap();
        assert_eq!(scene["source"], "flynt:drawing-spec");
    }

    #[tokio::test]
    async fn create_drawing_rejects_path_separators() {
        let (_tmp, ext) = test_extension();
        let err = ext
            .handle_rpc("execute_create_drawing", json!({"name": "../Sketch"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("name"));
    }

    #[tokio::test]
    async fn canvas_get_returns_parsed_canvas() {
        let (tmp, ext) = test_extension();
        let canvas = flynt_core::canvas::Canvas::default();
        let body = serde_json::to_string(&canvas).unwrap();
        write_canvas(&tmp, "canvases/Hero.canvas", &body);

        let out = ext
            .handle_rpc(
                "execute_canvas_get",
                json!({"path": "canvases/Hero.canvas"}),
            )
            .await
            .unwrap();
        assert_eq!(out["version"], 1);
        assert_eq!(out["theme"], "default");
        assert!(out["cells"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn canvas_get_rejects_missing_path() {
        let (_tmp, ext) = test_extension();
        let err = ext
            .handle_rpc("execute_canvas_get", json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("path"));
    }

    #[tokio::test]
    async fn canvas_get_accepts_canvas_path_alias() {
        // Agent observed reaching for `canvas_path` (the name in
        // canvas_active's return) even though the tool schema uses `path`.
        // Both must work or the agent wastes a round-trip retrying.
        let (tmp, ext) = test_extension();
        let canvas = flynt_core::canvas::Canvas::default();
        write_canvas(&tmp, "x.canvas", &serde_json::to_string(&canvas).unwrap());

        let by_path = ext
            .handle_rpc("execute_canvas_get", json!({"path": "x.canvas"}))
            .await
            .unwrap();
        let by_canvas_path = ext
            .handle_rpc("execute_canvas_get", json!({"canvas_path": "x.canvas"}))
            .await
            .unwrap();
        assert_eq!(by_path, by_canvas_path);
    }

    #[tokio::test]
    async fn canvas_apply_theme_accepts_canvas_path_alias() {
        let (tmp, ext) = test_extension();
        write_canvas(
            &tmp,
            "x.canvas",
            &serde_json::to_string(&flynt_core::canvas::Canvas::default()).unwrap(),
        );

        let out = ext
            .handle_rpc(
                "execute_canvas_apply_theme",
                json!({"canvas_path": "x.canvas", "theme": "amber"}),
            )
            .await
            .unwrap();
        assert_eq!(out["theme"], "amber");
    }

    #[tokio::test]
    async fn canvas_get_rejects_path_traversal() {
        let (_tmp, ext) = test_extension();
        let err = ext
            .handle_rpc("execute_canvas_get", json!({"path": "../etc/passwd"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains(".."));
    }

    #[tokio::test]
    async fn canvas_set_cells_creates_file_when_missing() {
        let (tmp, ext) = test_extension();
        let out = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({
                    "path": "canvases/New.canvas",
                    "cells": [{
                        "id": "a", "x": 0, "y": 0, "w": 4, "h": 2,
                        "html": "<div>x</div>", "css": ""
                    }]
                }),
            )
            .await
            .unwrap();

        assert_eq!(out["cell_count"], 1);
        assert!(tmp.path().join("canvases/New.canvas").exists());

        // Round-trip: canvas_get returns the cell we just wrote.
        let got = ext
            .handle_rpc("execute_canvas_get", json!({"path": "canvases/New.canvas"}))
            .await
            .unwrap();
        assert_eq!(got["cells"][0]["id"], "a");
        assert_eq!(got["cells"][0]["html"], "<div>x</div>");
    }

    #[tokio::test]
    async fn canvas_set_cells_accepts_stringified_cells_array() {
        // Anthropic's tool-call surface sometimes serializes nested array
        // args as a JSON-encoded string. Without coercion we silently dropped
        // the payload and returned success — caller saw "Completed" but the
        // file never changed. Regression test.
        let (tmp, ext) = test_extension();
        let cells_json = serde_json::to_string(&serde_json::json!([
            {"id": "a", "x": 0, "y": 0, "w": 1, "h": 1, "html": "<b>x</b>", "css": ""}
        ]))
        .unwrap();

        let out = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({ "path": "x.canvas", "cells": cells_json }),
            )
            .await
            .unwrap();

        assert_eq!(out["cell_count"], 1);
        assert!(tmp.path().join("x.canvas").exists());

        // Round-trip via canvas_get to confirm the cell actually wrote.
        let got = ext
            .handle_rpc("execute_canvas_get", json!({"path": "x.canvas"}))
            .await
            .unwrap();
        assert_eq!(got["cells"][0]["id"], "a");
        assert_eq!(got["cells"][0]["html"], "<b>x</b>");
    }

    #[tokio::test]
    async fn canvas_set_cells_rejects_invalid_stringified_cells() {
        let (_tmp, ext) = test_extension();
        let err = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({ "path": "x.canvas", "cells": "not-json" }),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cells"));
    }

    #[tokio::test]
    async fn canvas_set_cells_rejects_non_array_non_string_cells() {
        let (_tmp, ext) = test_extension();
        let err = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({ "path": "x.canvas", "cells": 42 }),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cells"));
    }

    #[tokio::test]
    async fn canvas_set_cells_lint_flags_missing_h_full() {
        let (_tmp, ext) = test_extension();
        let out = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({
                    "path": "x.canvas",
                    "cells": [{
                        "id": "needs-fill", "x": 0, "y": 0, "w": 4, "h": 3,
                        "html": "<div class=\"bg-card p-4\">stat</div>", "css": ""
                    }]
                }),
            )
            .await
            .unwrap();
        let warnings = out["lint_warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), 1);
        let w = warnings[0].as_str().unwrap();
        assert!(w.contains("needs-fill"), "{w}");
        assert!(w.contains("h-full"), "{w}");
    }

    #[tokio::test]
    async fn canvas_set_cells_lint_passes_h_full() {
        let (_tmp, ext) = test_extension();
        let out = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({
                    "path": "x.canvas",
                    "cells": [{
                        "id": "fills", "x": 0, "y": 0, "w": 4, "h": 3,
                        "html": "<div class=\"h-full bg-card p-4\">stat</div>", "css": ""
                    }]
                }),
            )
            .await
            .unwrap();
        assert!(out["lint_warnings"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn canvas_set_cells_lint_flags_arbitrary_tailwind() {
        let (_tmp, ext) = test_extension();
        let out = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({
                    "path": "x.canvas",
                    "cells": [{
                        "id": "hot-pink", "x": 0, "y": 0, "w": 4, "h": 3,
                        "html": "<div class=\"h-full bg-[#FF1493] p-4\">x</div>", "css": ""
                    }]
                }),
            )
            .await
            .unwrap();
        let warnings = out["lint_warnings"].as_array().unwrap();
        assert!(!warnings.is_empty());
        let combined = warnings
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(combined.contains("arbitrary"), "{combined}");
        assert!(combined.contains("hot-pink"), "{combined}");
    }

    #[tokio::test]
    async fn canvas_set_cells_lint_height_100_alternate_passes() {
        // Inline `style="height:100%"` should also satisfy the fill check.
        let (_tmp, ext) = test_extension();
        let out = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({
                    "path": "x.canvas",
                    "cells": [{
                        "id": "inline", "x": 0, "y": 0, "w": 4, "h": 3,
                        "html": "<div style=\"height: 100%; background: var(--card)\">x</div>",
                        "css": ""
                    }]
                }),
            )
            .await
            .unwrap();
        assert!(out["lint_warnings"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn canvas_set_cells_lint_empty_html_does_not_warn() {
        // Bare or empty cells aren't flagged — there's nothing to fill.
        let (_tmp, ext) = test_extension();
        let out = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({
                    "path": "x.canvas",
                    "cells": [{
                        "id": "blank", "x": 0, "y": 0, "w": 1, "h": 1,
                        "html": "", "css": ""
                    }]
                }),
            )
            .await
            .unwrap();
        assert!(out["lint_warnings"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn canvas_set_cells_upserts_by_id() {
        let (tmp, ext) = test_extension();
        let mut canvas = flynt_core::canvas::Canvas::default();
        canvas.upsert_cell(flynt_core::canvas::Cell {
            id: "a".into(),
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            html: "old".into(),
            css: "".into(),
            js: None,
        });
        write_canvas(&tmp, "x.canvas", &serde_json::to_string(&canvas).unwrap());

        let out = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({
                    "path": "x.canvas",
                    "cells": [{
                        "id": "a", "x": 0, "y": 0, "w": 1, "h": 1,
                        "html": "new", "css": ""
                    }]
                }),
            )
            .await
            .unwrap();
        assert_eq!(out["cell_count"], 1, "upsert must replace, not append");
        assert_eq!(out["upserted"][0]["replaced"], true);
    }

    #[tokio::test]
    async fn canvas_set_cells_deletes_by_id() {
        let (tmp, ext) = test_extension();
        let mut canvas = flynt_core::canvas::Canvas::default();
        for id in ["a", "b", "c"] {
            canvas.upsert_cell(flynt_core::canvas::Cell {
                id: id.into(),
                x: 0,
                y: 0,
                w: 1,
                h: 1,
                html: "".into(),
                css: "".into(),
                js: None,
            });
        }
        write_canvas(&tmp, "x.canvas", &serde_json::to_string(&canvas).unwrap());

        let out = ext
            .handle_rpc(
                "execute_canvas_set_cells",
                json!({"path": "x.canvas", "delete_ids": ["b", "missing"]}),
            )
            .await
            .unwrap();
        assert_eq!(out["cell_count"], 2);
        let deleted: Vec<&str> = out["deleted"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            deleted,
            vec!["b"],
            "deleted only reports actually-removed ids"
        );
    }

    #[tokio::test]
    async fn canvas_set_cells_updates_grid_and_theme() {
        let (tmp, ext) = test_extension();
        write_canvas(
            &tmp,
            "x.canvas",
            &serde_json::to_string(&flynt_core::canvas::Canvas::default()).unwrap(),
        );

        ext.handle_rpc(
            "execute_canvas_set_cells",
            json!({
                "path": "x.canvas",
                "grid": {"cols": 6, "rows": 4, "gap": 16},
                "theme": "ocean"
            }),
        )
        .await
        .unwrap();

        let got = ext
            .handle_rpc("execute_canvas_get", json!({"path": "x.canvas"}))
            .await
            .unwrap();
        assert_eq!(got["grid"]["cols"], 6);
        assert_eq!(got["grid"]["gap"], 16);
        assert_eq!(got["theme"], "ocean");
    }

    #[tokio::test]
    async fn canvas_apply_theme_returns_previous() {
        let (tmp, ext) = test_extension();
        write_canvas(
            &tmp,
            "x.canvas",
            &serde_json::to_string(&flynt_core::canvas::Canvas::default()).unwrap(),
        );

        let out = ext
            .handle_rpc(
                "execute_canvas_apply_theme",
                json!({"path": "x.canvas", "theme": "amber"}),
            )
            .await
            .unwrap();
        assert_eq!(out["theme"], "amber");
        assert_eq!(out["previous_theme"], "default");
    }

    #[tokio::test]
    async fn canvas_list_primitives_reads_project_assets() {
        let (tmp, ext) = test_extension();
        let dir = tmp.path().join(".flynt-local/flynt/assets");
        std::fs::create_dir_all(&dir).unwrap();
        // Raw-string delimiters need to be longer than any `"#` substring
        // inside — colour values like "#000" force us to use r##"..."## here.
        std::fs::write(
            dir.join("shadcn-primitives.json"),
            r##"{
            "version": 1,
            "cell_authoring_guidance": ["wrap with h-full"],
            "primitives": [{
                "id":"button","name":"Button","category":"input",
                "description":"","usage_notes":"inline",
                "html":"<button/>"
            }]
        }"##,
        )
        .unwrap();
        std::fs::write(
            dir.join("tweakcn-presets.json"),
            r##"{
            "default": {
                "name": "Default", "description": "stub",
                "vars": {"--background": "#000", "--primary": "#fff"}
            }
        }"##,
        )
        .unwrap();

        let out = ext
            .handle_rpc("execute_canvas_list_primitives", json!({}))
            .await
            .unwrap();
        assert_eq!(out["primitives"][0]["id"], "button");
        // Primitives carry their usage_notes through unchanged.
        assert_eq!(out["primitives"][0]["usage_notes"], "inline");
        // Themes now include the full vars map so the agent can see actual
        // color values without guessing what bg-primary resolves to.
        assert_eq!(out["themes"][0]["id"], "default");
        assert_eq!(out["themes"][0]["vars"]["--background"], "#000");
        assert_eq!(out["themes"][0]["vars"]["--primary"], "#fff");
        // Cell-authoring guidance surfaces alongside primitives + themes.
        assert_eq!(out["cell_authoring_guidance"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn canvas_list_primitives_returns_empty_when_no_assets() {
        let (_tmp, ext) = test_extension();
        let out = ext
            .handle_rpc("execute_canvas_list_primitives", json!({}))
            .await
            .unwrap();
        // No bootstrap done — fallback shape, no error.
        assert!(out["primitives"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn canvas_active_returns_null_when_no_ui_state() {
        let (_tmp, ext) = test_extension();
        let out = ext
            .handle_rpc("execute_canvas_active", json!({}))
            .await
            .unwrap();
        assert!(out.is_null());
    }

    #[tokio::test]
    async fn canvas_active_returns_null_for_non_canvas_doc() {
        let (tmp, ext) = test_extension();
        std::fs::create_dir_all(tmp.path().join("notes")).unwrap();
        std::fs::write(tmp.path().join("notes/plain.md"), "Just text.\n").unwrap();
        write_ui_state(&tmp, Some("notes/plain.md"));

        let out = ext
            .handle_rpc("execute_canvas_active", json!({}))
            .await
            .unwrap();
        assert!(out.is_null());
    }

    #[tokio::test]
    async fn canvas_active_resolves_canvas_wrapper() {
        let (tmp, ext) = test_extension();
        std::fs::create_dir_all(tmp.path().join("canvases")).unwrap();
        std::fs::write(
            tmp.path().join("canvases/Hero.md"),
            "+++\ntitle = \"Hero\"\ntags = [\"canvas\"]\n+++\n\n![[Hero.canvas]]\n",
        )
        .unwrap();
        write_ui_state(&tmp, Some("canvases/Hero.md"));

        let out = ext
            .handle_rpc("execute_canvas_active", json!({}))
            .await
            .unwrap();
        assert_eq!(out["wrapper_path"], "canvases/Hero.md");
        assert_eq!(out["canvas_path"], "canvases/Hero.canvas");
    }

    #[tokio::test]
    async fn canvas_tools_appear_in_get_tools() {
        let (_tmp, ext) = test_extension();
        let tools = ext.handle_rpc("get_tools", json!({})).await.unwrap();
        let names: Vec<String> = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect();
        for expected in [
            "canvas_get",
            "canvas_set_cells",
            "canvas_apply_theme",
            "canvas_list_primitives",
            "canvas_active",
            "canvas_create",
        ] {
            assert!(names.contains(&expected.to_string()), "missing: {expected}");
        }
    }

    #[tokio::test]
    async fn canvas_create_writes_pair_and_returns_paths() {
        let (tmp, ext) = test_extension();
        let out = ext
            .handle_rpc("execute_canvas_create", json!({"name": "Hero"}))
            .await
            .unwrap();

        assert_eq!(out["wrapper_path"], "canvases/Hero.md");
        assert_eq!(out["canvas_path"], "canvases/Hero.canvas");
        assert!(tmp.path().join("canvases/Hero.md").exists());
        assert!(tmp.path().join("canvases/Hero.canvas").exists());
    }

    #[tokio::test]
    async fn canvas_create_refuses_path_separators() {
        let (_tmp, ext) = test_extension();
        let err = ext
            .handle_rpc("execute_canvas_create", json!({"name": "../etc/passwd"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("name"));
    }

    #[tokio::test]
    async fn canvas_create_refuses_duplicate() {
        let (_tmp, ext) = test_extension();
        ext.handle_rpc("execute_canvas_create", json!({"name": "Hero"}))
            .await
            .unwrap();
        let err = ext
            .handle_rpc("execute_canvas_create", json!({"name": "Hero"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already exists"), "got: {err}");
    }

    #[tokio::test]
    async fn canvas_create_then_set_cells_round_trip() {
        // After creating a canvas, the agent should be able to immediately
        // populate it via canvas_set_cells using the returned canvas_path.
        let (_tmp, ext) = test_extension();
        let created = ext
            .handle_rpc("execute_canvas_create", json!({"name": "Demo"}))
            .await
            .unwrap();
        let canvas_path = created["canvas_path"].as_str().unwrap();

        ext.handle_rpc(
            "execute_canvas_set_cells",
            json!({
                "path": canvas_path,
                "cells": [{"id": "a", "x": 0, "y": 0, "w": 1, "h": 1, "html": "x", "css": ""}]
            }),
        )
        .await
        .unwrap();

        let got = ext
            .handle_rpc("execute_canvas_get", json!({"path": canvas_path}))
            .await
            .unwrap();
        assert_eq!(got["cells"][0]["id"], "a");
    }

    // ── list_tasks filters ──────────────────────────────────────────────────

    async fn seed_three_tagged_tasks(ext: &FlyntExtension) -> (String, [String; 3]) {
        let board = ext
            .handle_rpc("execute_create_board", json!({"name": "F"}))
            .await
            .unwrap();
        let board_id = board["id"].as_str().unwrap().to_string();
        let mut ids = [String::new(), String::new(), String::new()];
        for (i, (col, tags, status)) in [
            ("Scheduled", vec!["sentry", "recurring"], "todo"),
            ("Scheduled", vec!["sentry"], "todo"),
            ("Backlog", vec!["urgent"], "in_progress"),
        ]
        .iter()
        .enumerate()
        {
            let t = ext
                .handle_rpc(
                    "execute_create_task",
                    json!({"board_id": board_id, "column": col, "title": format!("T{i}")}),
                )
                .await
                .unwrap();
            let id = t["id"].as_str().unwrap().to_string();
            ext.handle_rpc(
                "execute_update_task",
                json!({"id": id, "tags": tags, "status": status, "column": col}),
            )
            .await
            .unwrap();
            ids[i] = id;
        }
        (board_id, ids)
    }

    #[tokio::test]
    async fn list_tasks_filters_by_status() {
        let (_tmp, ext) = test_extension();
        let (_board, _ids) = seed_three_tagged_tasks(&ext).await;

        let todos = ext
            .handle_rpc("execute_list_tasks", json!({"status": "todo"}))
            .await
            .unwrap();
        let in_progress = ext
            .handle_rpc("execute_list_tasks", json!({"status": "in_progress"}))
            .await
            .unwrap();

        assert_eq!(todos.as_array().unwrap().len(), 2);
        assert_eq!(in_progress.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn list_tasks_filters_by_single_tag_uses_intersection() {
        let (_tmp, ext) = test_extension();
        let (_board, _ids) = seed_three_tagged_tasks(&ext).await;

        let sentry_tasks = ext
            .handle_rpc("execute_list_tasks", json!({"tags": ["sentry"]}))
            .await
            .unwrap();
        assert_eq!(sentry_tasks.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn list_tasks_filters_by_multi_tag_requires_all() {
        // Tags AND together — only the recurring + sentry task qualifies.
        let (_tmp, ext) = test_extension();
        let (_board, _ids) = seed_three_tagged_tasks(&ext).await;

        let intersect = ext
            .handle_rpc(
                "execute_list_tasks",
                json!({"tags": ["sentry", "recurring"]}),
            )
            .await
            .unwrap();
        assert_eq!(intersect.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn list_tasks_combines_column_status_and_tags() {
        // The actual sentry list_actionable() shape — column=Scheduled,
        // status=todo, tag=sentry should yield exactly the actionable tasks.
        let (_tmp, ext) = test_extension();
        let (_board, _ids) = seed_three_tagged_tasks(&ext).await;

        let scheduled_sentry_todos = ext
            .handle_rpc(
                "execute_list_tasks",
                json!({"column": "Scheduled", "status": "todo", "tags": ["sentry"]}),
            )
            .await
            .unwrap();
        assert_eq!(scheduled_sentry_todos.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn list_tasks_rejects_unknown_status() {
        let (_tmp, ext) = test_extension();
        let err = ext
            .handle_rpc("execute_list_tasks", json!({"status": "frozen"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("status"));
    }

    // ── update_task ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn update_task_changes_status_and_column_only() {
        // Sentry's claim() flow: change status + column without
        // touching anything else. Verifies the partial-update contract.
        let (_tmp, ext) = test_extension();

        let board = ext
            .handle_rpc("execute_create_board", json!({"name": "Sentry"}))
            .await
            .unwrap();
        let board_id = board["id"].as_str().unwrap();

        let task = ext
            .handle_rpc(
                "execute_create_task",
                json!({"board_id": board_id, "column": "Scheduled", "title": "Run scan"}),
            )
            .await
            .unwrap();
        let task_id = task["id"].as_str().unwrap().to_string();

        let resp = ext
            .handle_rpc(
                "execute_update_task",
                json!({"id": task_id, "status": "in_progress", "column": "Running"}),
            )
            .await
            .unwrap();
        assert_eq!(resp["updated"], true);

        let after = ext
            .handle_rpc("execute_get_task", json!({"id": task_id}))
            .await
            .unwrap();
        assert_eq!(after["column"], "Running");
        assert_eq!(after["status"], "in_progress");
        // Title preserved — that's the partial-update contract.
        assert_eq!(after["title"], "Run scan");
    }

    #[tokio::test]
    async fn update_task_persists_external_refs() {
        // Sentry stores cron expressions and webhook names in external_refs.
        // Round-trip them through the update tool; previously these were
        // hardcoded to Vec::new() in the row deserializer.
        let (_tmp, ext) = test_extension();

        let board = ext
            .handle_rpc("execute_create_board", json!({"name": "B"}))
            .await
            .unwrap();
        let task = ext
            .handle_rpc(
                "execute_create_task",
                json!({"board_id": board["id"], "column": "Backlog", "title": "T"}),
            )
            .await
            .unwrap();
        let task_id = task["id"].as_str().unwrap().to_string();

        ext.handle_rpc(
            "execute_update_task",
            json!({
                "id": task_id,
                "external_refs": ["cron:0 */4 * * *", "webhook:gh-pr"]
            }),
        )
        .await
        .unwrap();

        let after = ext
            .handle_rpc("execute_get_task", json!({"id": task_id}))
            .await
            .unwrap();
        let refs: Vec<String> = after["external_refs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert_eq!(refs, vec!["cron:0 */4 * * *", "webhook:gh-pr"]);
    }

    #[tokio::test]
    async fn update_task_returns_false_for_missing_id() {
        let (_tmp, ext) = test_extension();
        let resp = ext
            .handle_rpc(
                "execute_update_task",
                json!({"id": "00000000-0000-0000-0000-000000000000", "status": "done"}),
            )
            .await
            .unwrap();
        assert_eq!(resp["updated"], false);
    }

    #[tokio::test]
    async fn update_task_rejects_invalid_uuid() {
        let (_tmp, ext) = test_extension();
        let err = ext
            .handle_rpc(
                "execute_update_task",
                json!({"id": "not-a-uuid", "status": "done"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("UUID"));
    }

    #[tokio::test]
    async fn update_task_round_trips_execution_spec() {
        // Sentry's task_spec() depends on execution metadata round-tripping
        // through storage. Mirror omegon's TaskSpec field set exactly so the
        // FlyntTaskBoard adapter is a thin pass-through.
        let (_tmp, ext) = test_extension();

        let board = ext
            .handle_rpc("execute_create_board", json!({"name": "B"}))
            .await
            .unwrap();
        let task = ext
            .handle_rpc(
                "execute_create_task",
                json!({"board_id": board["id"], "column": "Scheduled", "title": "Run scan"}),
            )
            .await
            .unwrap();
        let task_id = task["id"].as_str().unwrap().to_string();

        ext.handle_rpc(
            "execute_update_task",
            json!({
                "id": task_id,
                "execution": {
                    "model": "anthropic:claude-sonnet-4-6",
                    "max_turns": 20,
                    "timeout_secs": 300,
                    "skill": "security",
                    "env": { "SCAN_DEPTH": "deep" }
                },
                "openspec_change": "auth-rewrite"
            }),
        )
        .await
        .unwrap();

        let after = ext
            .handle_rpc("execute_get_task", json!({"id": task_id}))
            .await
            .unwrap();
        assert_eq!(after["execution"]["model"], "anthropic:claude-sonnet-4-6");
        assert_eq!(after["execution"]["max_turns"], 20);
        assert_eq!(after["execution"]["skill"], "security");
        assert_eq!(after["execution"]["env"]["SCAN_DEPTH"], "deep");
        assert_eq!(after["openspec_change"], "auth-rewrite");
    }

    #[tokio::test]
    async fn update_task_clears_execution_on_null() {
        let (_tmp, ext) = test_extension();
        let board = ext
            .handle_rpc("execute_create_board", json!({"name": "B"}))
            .await
            .unwrap();
        let task = ext
            .handle_rpc(
                "execute_create_task",
                json!({"board_id": board["id"], "column": "Scheduled", "title": "T"}),
            )
            .await
            .unwrap();
        let task_id = task["id"].as_str().unwrap().to_string();

        ext.handle_rpc(
            "execute_update_task",
            json!({"id": task_id, "execution": {"model": "x"}}),
        )
        .await
        .unwrap();
        let with_exec = ext
            .handle_rpc("execute_get_task", json!({"id": task_id}))
            .await
            .unwrap();
        assert_eq!(with_exec["execution"]["model"], "x");

        ext.handle_rpc(
            "execute_update_task",
            json!({"id": task_id, "execution": null}),
        )
        .await
        .unwrap();
        let cleared = ext
            .handle_rpc("execute_get_task", json!({"id": task_id}))
            .await
            .unwrap();
        assert!(cleared["execution"].is_null());
    }

    #[tokio::test]
    async fn update_task_clears_openspec_change_on_empty_string() {
        let (_tmp, ext) = test_extension();
        let board = ext
            .handle_rpc("execute_create_board", json!({"name": "B"}))
            .await
            .unwrap();
        let task = ext
            .handle_rpc(
                "execute_create_task",
                json!({"board_id": board["id"], "column": "C", "title": "T"}),
            )
            .await
            .unwrap();
        let task_id = task["id"].as_str().unwrap().to_string();

        ext.handle_rpc(
            "execute_update_task",
            json!({"id": task_id, "openspec_change": "auth-rewrite"}),
        )
        .await
        .unwrap();
        let with = ext
            .handle_rpc("execute_get_task", json!({"id": task_id}))
            .await
            .unwrap();
        assert_eq!(with["openspec_change"], "auth-rewrite");

        ext.handle_rpc(
            "execute_update_task",
            json!({"id": task_id, "openspec_change": ""}),
        )
        .await
        .unwrap();
        let cleared = ext
            .handle_rpc("execute_get_task", json!({"id": task_id}))
            .await
            .unwrap();
        assert!(cleared["openspec_change"].is_null());
    }

    #[tokio::test]
    async fn update_task_clears_design_node_id_on_empty_string() {
        // Empty string is the documented "clear" sentinel.
        let (_tmp, ext) = test_extension();
        let board = ext
            .handle_rpc("execute_create_board", json!({"name": "B"}))
            .await
            .unwrap();
        let task = ext
            .handle_rpc(
                "execute_create_task",
                json!({"board_id": board["id"], "column": "C", "title": "T"}),
            )
            .await
            .unwrap();
        let task_id = task["id"].as_str().unwrap().to_string();
        let node_id = uuid::Uuid::new_v4().to_string();

        ext.handle_rpc(
            "execute_update_task",
            json!({"id": task_id, "design_node_id": node_id}),
        )
        .await
        .unwrap();
        let after = ext
            .handle_rpc("execute_get_task", json!({"id": task_id}))
            .await
            .unwrap();
        assert_eq!(after["design_node_id"], serde_json::Value::String(node_id));

        ext.handle_rpc(
            "execute_update_task",
            json!({"id": task_id, "design_node_id": ""}),
        )
        .await
        .unwrap();
        let after = ext
            .handle_rpc("execute_get_task", json!({"id": task_id}))
            .await
            .unwrap();
        assert!(after["design_node_id"].is_null());
    }
}
