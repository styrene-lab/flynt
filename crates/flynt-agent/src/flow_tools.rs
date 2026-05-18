//! Agent tools for `.flow` files.
//!
//! Three flat tools mirror the document-tool surface:
//!
//! - `flow_create({ path, title?, nodes?, edges? })` — write a fresh
//!   `.flow` file. Idempotent on path: refuses to overwrite an existing
//!   file (the agent should call `flow_patch` instead).
//! - `flow_get(path) → { id, schema_version, meta, nodes, edges }` —
//!   read a `.flow` file as JSON. Includes the doc id from frontmatter
//!   so the agent can correlate with indexer entries.
//! - `flow_patch(path, { add_nodes?, remove_nodes?, add_edges?,
//!   remove_edges?, move_nodes?, set_meta? })` — mutate in place. Each
//!   field is optional; missing means "no change for that operation."
//!   Skip complex transactions for v1: load → mutate → save.
//!
//! All paths are interpreted relative to the project root, matching
//! `forge_tools` and `execute_canvas_create`. Path traversal is rejected
//! with `invalid_params`.
//!
//! ## Concurrency
//!
//! Last-writer-wins. A `flow_patch` does load → mutate → save with no
//! file lock; if two agents (or an agent and the desktop editor)
//! write the same `.flow` file in the same window, one set of changes
//! is silently overwritten. With one operator and one agent acting
//! through them, this is acceptable — the agent typically waits for a
//! tool result before issuing the next call. Multi-agent or
//! agent-while-editor scenarios need a future revision (file locking
//! or a CRDT layer).
//!
//! ## Why this surface and not "render arbitrary structured data"
//!
//! The pitch was a node editor for "architecture, workflows, diagrams,
//! anything structured." The narrower wedge — "agent draws an
//! architecture, operator edits, agent reads back" — is what justifies
//! shipping the editor at all. These three tools are the minimum
//! surface that supports that loop.

use flynt_flow::{Flow, FlowEdge, FlowEndpoint, FlowMeta, FlowNode, NodeKind, Socket, SocketDir};
use flynt_store::project::Project;
use omegon_extension::{Error as ExtError, Result as ExtResult};
use serde_json::{Value, json};
use std::path::PathBuf;
use uuid::Uuid;

// ── Tool definitions (advertised to omegon) ─────────────────────────────────

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "flow_create",
            "label": "Create Flow",
            "description": "Write a fresh .flow file. The .flow format is a node-flow graph (nodes + edges) — use it to sketch architectures, workflows, or any structured graph the operator should be able to drag around. Refuses to overwrite an existing file; call flow_patch to mutate.",
            "parameters": {
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string", "description": "Project-relative path. Must end in .flow." },
                    "title": { "type": "string", "description": "Display title (also written into frontmatter so the indexer surfaces it)." },
                    "description": { "type": "string" },
                    "nodes": {
                        "type": "array",
                        "items": flow_node_schema(),
                        "description": "Optional initial nodes."
                    },
                    "edges": {
                        "type": "array",
                        "items": flow_edge_schema(),
                        "description": "Optional initial edges."
                    }
                }
            }
        }),
        json!({
            "name": "flow_get",
            "label": "Get Flow",
            "description": "Read a .flow file. Returns { id, schema_version, meta, nodes, edges } so the agent can inspect what the operator drew before patching.",
            "parameters": {
                "type": "object",
                "required": ["path"],
                "properties": { "path": { "type": "string" } }
            }
        }),
        json!({
            "name": "flow_patch",
            "label": "Patch Flow",
            "description": "Mutate a .flow file in place. Each operation is optional; supply only what you want to change. Operations apply in order: remove_nodes (cascades to edges touching them), remove_edges, add_nodes, add_edges, move_nodes, set_meta. Returns the post-mutation { id, nodes, edges, validation } where validation is the Flow::validate report.",
            "parameters": {
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" },
                    "add_nodes": { "type": "array", "items": flow_node_schema() },
                    "remove_nodes": {
                        "type": "array",
                        "items": { "type": "string", "description": "Node UUID" }
                    },
                    "add_edges": { "type": "array", "items": flow_edge_schema() },
                    "remove_edges": {
                        "type": "array",
                        "items": { "type": "string", "description": "Edge UUID" }
                    },
                    "move_nodes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["id", "position"],
                            "properties": {
                                "id": { "type": "string" },
                                "position": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }
                            }
                        }
                    },
                    "set_meta": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "description": { "type": "string" }
                        }
                    }
                }
            }
        }),
    ]
}

fn flow_node_schema() -> Value {
    json!({
        "type": "object",
        "required": ["id", "kind", "position"],
        "properties": {
            "id": { "type": "string", "description": "UUID. Generate one if creating a new node." },
            "kind": { "type": "string", "description": "step | input | output | branch | agent_call | note | <custom>" },
            "position": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
            "data": { "type": "object", "description": "Per-kind payload. e.g. { title, name, skill }." },
            "sockets": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["name", "direction"],
                    "properties": {
                        "name": { "type": "string" },
                        "direction": { "type": "string", "enum": ["input", "output"] },
                        "ty": { "type": "string", "description": "Optional type hint, doc-only in v1." }
                    }
                }
            }
        }
    })
}

fn flow_edge_schema() -> Value {
    json!({
        "type": "object",
        "required": ["id", "source", "target"],
        "properties": {
            "id": { "type": "string", "description": "UUID." },
            "source": {
                "type": "object",
                "required": ["node"],
                "properties": {
                    "node": { "type": "string" },
                    "socket": { "type": "string", "default": "" }
                }
            },
            "target": {
                "type": "object",
                "required": ["node"],
                "properties": {
                    "node": { "type": "string" },
                    "socket": { "type": "string", "default": "" }
                }
            }
        }
    })
}

// ── Tool implementations ────────────────────────────────────────────────────

pub fn flow_create(project: &Project, params: Value) -> ExtResult<Value> {
    let rel_path = parse_flow_path(&params)?;
    let abs = project.root.join(&rel_path);

    if abs.exists() {
        return Err(ExtError::invalid_params(format!(
            "{} already exists — use flow_patch to mutate",
            rel_path.display()
        )));
    }

    let mut flow = Flow::default();
    flow.meta = FlowMeta {
        title: params
            .get("title")
            .and_then(|v| v.as_str())
            .map(String::from),
        description: params
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    if let Some(arr) = params.get("nodes").and_then(|v| v.as_array()) {
        for n in arr {
            flow.nodes.push(parse_node(n)?);
        }
    }
    if let Some(arr) = params.get("edges").and_then(|v| v.as_array()) {
        for e in arr {
            flow.edges.push(parse_edge(e)?);
        }
    }

    let id = Uuid::new_v4();
    flynt_flow::save_flow(&abs, &flow, Some(id))
        .map_err(|e| ExtError::internal_error(e.to_string()))?;

    // Run validation post-save so an agent that creates a graph with
    // dangling edges learns immediately. Mirrors what flow_patch returns.
    let validation = flow.validate();
    Ok(json!({
        "path": rel_path.to_string_lossy(),
        "id": id.to_string(),
        "node_count": flow.nodes.len(),
        "edge_count": flow.edges.len(),
        "validation": validation_report_json(&validation),
    }))
}

pub fn flow_get(project: &Project, params: Value) -> ExtResult<Value> {
    let rel_path = parse_flow_path(&params)?;
    let abs = project.root.join(&rel_path);

    if !abs.exists() {
        return Err(ExtError::invalid_params(format!(
            "no such file: {}",
            rel_path.display()
        )));
    }
    let doc = flynt_flow::load_flow(&abs).map_err(|e| ExtError::internal_error(e.to_string()))?;

    Ok(json!({
        "id": doc.id.to_string(),
        "schema_version": doc.schema_version,
        "meta": doc.flow.meta,
        "nodes": doc.flow.nodes,
        "edges": doc.flow.edges,
    }))
}

pub fn flow_patch(project: &Project, params: Value) -> ExtResult<Value> {
    let rel_path = parse_flow_path(&params)?;
    let abs = project.root.join(&rel_path);

    if !abs.exists() {
        return Err(ExtError::invalid_params(format!(
            "no such file: {} — call flow_create first",
            rel_path.display()
        )));
    }
    let mut doc =
        flynt_flow::load_flow(&abs).map_err(|e| ExtError::internal_error(e.to_string()))?;

    // Apply operations in a deterministic order. Removes first so an
    // add for the same id isn't accidentally undone; cascade-remove
    // edges touching dropped nodes so the agent doesn't have to track
    // edge cleanup manually.
    if let Some(arr) = params.get("remove_nodes").and_then(|v| v.as_array()) {
        let ids: std::collections::HashSet<Uuid> = arr
            .iter()
            .filter_map(|v| v.as_str())
            .filter_map(|s| Uuid::parse_str(s).ok())
            .collect();
        doc.flow.nodes.retain(|n| !ids.contains(&n.id));
        // Cascade: edges touching a removed node also go.
        doc.flow
            .edges
            .retain(|e| !ids.contains(&e.source.node) && !ids.contains(&e.target.node));
    }

    if let Some(arr) = params.get("remove_edges").and_then(|v| v.as_array()) {
        let ids: std::collections::HashSet<Uuid> = arr
            .iter()
            .filter_map(|v| v.as_str())
            .filter_map(|s| Uuid::parse_str(s).ok())
            .collect();
        doc.flow.edges.retain(|e| !ids.contains(&e.id));
    }

    if let Some(arr) = params.get("add_nodes").and_then(|v| v.as_array()) {
        for n in arr {
            doc.flow.nodes.push(parse_node(n)?);
        }
    }

    if let Some(arr) = params.get("add_edges").and_then(|v| v.as_array()) {
        for e in arr {
            doc.flow.edges.push(parse_edge(e)?);
        }
    }

    if let Some(arr) = params.get("move_nodes").and_then(|v| v.as_array()) {
        for m in arr {
            let id_str = m
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ExtError::invalid_params("move_nodes[].id is required"))?;
            let id = Uuid::parse_str(id_str).map_err(|_| {
                ExtError::invalid_params(format!("move_nodes[].id: {id_str} is not a UUID"))
            })?;
            let pos = parse_position(m.get("position"))?;
            if let Some(n) = doc.flow.nodes.iter_mut().find(|n| n.id == id) {
                n.position = pos;
            } else {
                return Err(ExtError::invalid_params(format!(
                    "move_nodes[]: no node with id {id_str}"
                )));
            }
        }
    }

    if let Some(meta) = params.get("set_meta") {
        if let Some(v) = meta.get("title").and_then(|v| v.as_str()) {
            doc.flow.meta.title = Some(v.to_string());
        }
        if let Some(v) = meta.get("description").and_then(|v| v.as_str()) {
            doc.flow.meta.description = Some(v.to_string());
        }
    }

    flynt_flow::save_flow(&abs, &doc.flow, Some(doc.id))
        .map_err(|e| ExtError::internal_error(e.to_string()))?;

    let validation = doc.flow.validate();
    Ok(json!({
        "id": doc.id.to_string(),
        "node_count": doc.flow.nodes.len(),
        "edge_count": doc.flow.edges.len(),
        "validation": validation_report_json(&validation),
    }))
}

fn validation_report_json(v: &flynt_flow::ValidationReport) -> Value {
    json!({
        "is_clean": v.is_clean(),
        "dangling_edges": v.dangling_edges.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
        "duplicate_node_ids": v.duplicate_node_ids.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
        "duplicate_edge_ids": v.duplicate_edge_ids.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
        "edges_with_unknown_sockets": v.edges_with_unknown_sockets.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
        "duplicate_socket_names": v.duplicate_socket_names.iter().map(|(n, s)| json!({ "node": n.to_string(), "socket": s })).collect::<Vec<_>>(),
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Extract + validate the `path` param. Enforces:
/// - Path is project-relative (no leading `/`)
/// - No `..` traversal components
/// - Ends in `.flow`
fn parse_flow_path(params: &Value) -> ExtResult<PathBuf> {
    let raw = params
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("path is required"))?;
    let p = PathBuf::from(raw);

    if p.is_absolute() {
        return Err(ExtError::invalid_params("path must be project-relative"));
    }
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(ExtError::invalid_params("path must not contain `..`"));
    }
    if p.extension().map(|e| e != "flow").unwrap_or(true) {
        return Err(ExtError::invalid_params("path must end in .flow"));
    }
    Ok(p)
}

fn parse_position(v: Option<&Value>) -> ExtResult<(f32, f32)> {
    let arr = v
        .and_then(|v| v.as_array())
        .ok_or_else(|| ExtError::invalid_params("position must be a [x, y] array"))?;
    if arr.len() != 2 {
        return Err(ExtError::invalid_params(
            "position must have exactly 2 elements",
        ));
    }
    let x = arr[0]
        .as_f64()
        .ok_or_else(|| ExtError::invalid_params("position[0] must be a number"))?
        as f32;
    let y = arr[1]
        .as_f64()
        .ok_or_else(|| ExtError::invalid_params("position[1] must be a number"))?
        as f32;
    if !x.is_finite() || !y.is_finite() {
        return Err(ExtError::invalid_params(
            "position coordinates must be finite",
        ));
    }
    Ok((x, y))
}

fn parse_node(v: &Value) -> ExtResult<FlowNode> {
    let id_str = v
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("node.id is required"))?;
    let id = Uuid::parse_str(id_str)
        .map_err(|_| ExtError::invalid_params(format!("node.id: {id_str} is not a UUID")))?;
    let kind_str = v
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("node.kind is required"))?;
    let kind = NodeKind::from_wire_str(kind_str);
    let position = parse_position(v.get("position"))?;
    let data = v.get("data").cloned().unwrap_or(json!({}));
    let sockets = v
        .get("sockets")
        .and_then(|s| s.as_array())
        .map(|arr| arr.iter().map(parse_socket).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();

    Ok(FlowNode {
        id,
        kind,
        position,
        data,
        sockets,
    })
}

fn parse_socket(v: &Value) -> ExtResult<Socket> {
    let name = v
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("socket.name is required"))?
        .to_string();
    let dir_str = v
        .get("direction")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("socket.direction is required"))?;
    let direction = match dir_str {
        "input" => SocketDir::Input,
        "output" => SocketDir::Output,
        other => {
            return Err(ExtError::invalid_params(format!(
                "socket.direction must be 'input' or 'output', got '{other}'"
            )));
        }
    };
    let ty = v.get("ty").and_then(|v| v.as_str()).map(String::from);
    Ok(Socket {
        name,
        direction,
        ty,
    })
}

fn parse_edge(v: &Value) -> ExtResult<FlowEdge> {
    let id_str = v
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("edge.id is required"))?;
    let id = Uuid::parse_str(id_str)
        .map_err(|_| ExtError::invalid_params(format!("edge.id: {id_str} is not a UUID")))?;
    let source = parse_endpoint(v.get("source"), "source")?;
    let target = parse_endpoint(v.get("target"), "target")?;
    Ok(FlowEdge { id, source, target })
}

fn parse_endpoint(v: Option<&Value>, label: &str) -> ExtResult<FlowEndpoint> {
    let obj = v.ok_or_else(|| ExtError::invalid_params(format!("edge.{label} is required")))?;
    let node_str = obj
        .get("node")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params(format!("edge.{label}.node is required")))?;
    let node = Uuid::parse_str(node_str).map_err(|_| {
        ExtError::invalid_params(format!("edge.{label}.node: {node_str} is not a UUID"))
    })?;
    let socket = obj
        .get("socket")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(FlowEndpoint { node, socket })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flynt_store::project::Project;
    use tempfile::TempDir;

    fn make_project() -> (TempDir, Project) {
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        (tmp, project)
    }

    #[test]
    fn create_writes_a_parseable_flow() {
        let (_tmp, project) = make_project();
        let out = flow_create(
            &project,
            json!({
                "path": "diagrams/auth.flow",
                "title": "Auth",
                "nodes": [
                    {
                        "id": "11111111-1111-1111-1111-111111111111",
                        "kind": "input",
                        "position": [0.0, 0.0]
                    }
                ]
            }),
        )
        .unwrap();
        assert_eq!(out["path"], "diagrams/auth.flow");
        assert_eq!(out["node_count"], 1);

        // Round-trip via flow_get to prove the file is parseable.
        let got = flow_get(&project, json!({ "path": "diagrams/auth.flow" })).unwrap();
        assert_eq!(got["meta"]["title"], "Auth");
        assert_eq!(got["nodes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn create_refuses_to_overwrite() {
        let (_tmp, project) = make_project();
        flow_create(&project, json!({ "path": "x.flow" })).unwrap();
        let err = flow_create(&project, json!({ "path": "x.flow" })).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn rejects_path_outside_project() {
        let (_tmp, project) = make_project();
        for bad in [
            "/tmp/escape.flow",
            "../escape.flow",
            "subdir/../../escape.flow",
        ] {
            let err = flow_create(&project, json!({ "path": bad })).unwrap_err();
            assert!(err.to_string().contains("path"), "{bad}: {err}");
        }
    }

    #[test]
    fn rejects_non_flow_extension() {
        let (_tmp, project) = make_project();
        let err = flow_create(&project, json!({ "path": "foo.md" })).unwrap_err();
        assert!(err.to_string().contains(".flow"), "{err}");
    }

    #[test]
    fn patch_remove_node_cascades_to_edges() {
        let (_tmp, project) = make_project();
        let n1 = "11111111-1111-1111-1111-111111111111";
        let n2 = "22222222-2222-2222-2222-222222222222";
        let e1 = "aaaa1111-1111-1111-1111-111111111111";
        flow_create(
            &project,
            json!({
                "path": "x.flow",
                "nodes": [
                    { "id": n1, "kind": "input",  "position": [0.0, 0.0] },
                    { "id": n2, "kind": "output", "position": [200.0, 0.0] }
                ],
                "edges": [
                    { "id": e1, "source": { "node": n1 }, "target": { "node": n2 } }
                ]
            }),
        )
        .unwrap();

        let out = flow_patch(
            &project,
            json!({
                "path": "x.flow",
                "remove_nodes": [n1]
            }),
        )
        .unwrap();
        assert_eq!(out["node_count"], 1, "n1 removed");
        assert_eq!(out["edge_count"], 0, "edge cascaded");
    }

    #[test]
    fn patch_move_nodes_updates_position() {
        let (_tmp, project) = make_project();
        let n1 = "11111111-1111-1111-1111-111111111111";
        flow_create(
            &project,
            json!({
                "path": "x.flow",
                "nodes": [{ "id": n1, "kind": "step", "position": [0.0, 0.0] }]
            }),
        )
        .unwrap();
        flow_patch(
            &project,
            json!({
                "path": "x.flow",
                "move_nodes": [{ "id": n1, "position": [100.5, 200.0] }]
            }),
        )
        .unwrap();
        let got = flow_get(&project, json!({ "path": "x.flow" })).unwrap();
        let pos = got["nodes"][0]["position"].as_array().unwrap();
        assert_eq!(pos[0].as_f64().unwrap(), 100.5);
        assert_eq!(pos[1].as_f64().unwrap(), 200.0);
    }

    #[test]
    fn patch_move_nonexistent_node_errors() {
        let (_tmp, project) = make_project();
        flow_create(&project, json!({ "path": "x.flow" })).unwrap();
        let err = flow_patch(
            &project,
            json!({
                "path": "x.flow",
                "move_nodes": [{
                    "id": "99999999-9999-9999-9999-999999999999",
                    "position": [0.0, 0.0]
                }]
            }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no node"), "{err}");
    }

    #[test]
    fn patch_rejects_malformed_position() {
        // serde_json's `Number::from_f64` returns None for NaN/Infinity, so
        // by the time a non-finite value reaches us via JSON it's already
        // `Value::Null` — caught by the "must be a number" guard. The
        // explicit `is_finite()` check in `parse_position` is defense in
        // depth for callers that construct a `Value` programmatically;
        // this test exercises the JSON-callable surface.
        let (_tmp, project) = make_project();
        let n1 = "11111111-1111-1111-1111-111111111111";
        flow_create(
            &project,
            json!({
                "path": "x.flow",
                "nodes": [{ "id": n1, "kind": "step", "position": [0.0, 0.0] }]
            }),
        )
        .unwrap();
        let err = flow_patch(
            &project,
            json!({
                "path": "x.flow",
                "move_nodes": [{ "id": n1, "position": ["not-a-number", 0.0] }]
            }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("number"), "{err}");
    }

    #[test]
    fn patch_returns_validation_report() {
        let (_tmp, project) = make_project();
        let n1 = "11111111-1111-1111-1111-111111111111";
        let n2 = "22222222-2222-2222-2222-222222222222";
        flow_create(
            &project,
            json!({
                "path": "x.flow",
                "nodes": [
                    { "id": n1, "kind": "input", "position": [0.0, 0.0] },
                    { "id": n2, "kind": "output", "position": [0.0, 0.0] }
                ]
            }),
        )
        .unwrap();
        // Add an edge whose source is a ghost node — Flow::validate flags it.
        let out = flow_patch(
            &project,
            json!({
                "path": "x.flow",
                "add_edges": [{
                    "id": "aaaa1111-1111-1111-1111-111111111111",
                    "source": { "node": "deadbeef-dead-dead-dead-deaddeafbeef" },
                    "target": { "node": n2 }
                }]
            }),
        )
        .unwrap();
        assert_eq!(out["validation"]["is_clean"], false);
        assert_eq!(
            out["validation"]["dangling_edges"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn custom_kind_round_trips() {
        let (_tmp, project) = make_project();
        flow_create(
            &project,
            json!({
                "path": "x.flow",
                "nodes": [{
                    "id": "11111111-1111-1111-1111-111111111111",
                    "kind": "queue_consumer",
                    "position": [0.0, 0.0]
                }]
            }),
        )
        .unwrap();
        let got = flow_get(&project, json!({ "path": "x.flow" })).unwrap();
        assert_eq!(got["nodes"][0]["kind"], "queue_consumer");
    }

    #[test]
    fn full_loop_roundtrip_preserves_complex_data() {
        // End-to-end: agent creates with rich data, patches to mutate
        // structure, reads back. Verifies the JSON `data` payload (which
        // is schema-flexible per kind) survives create → patch → get.
        let (_tmp, project) = make_project();
        let n_input = "11111111-1111-1111-1111-111111111111";
        let n_agent = "22222222-2222-2222-2222-222222222222";
        let n_output = "33333333-3333-3333-3333-333333333333";

        flow_create(
            &project,
            json!({
                "path": "loop.flow",
                "title": "Auth verification",
                "description": "credentials → mint token",
                "nodes": [
                    {
                        "id": n_input,
                        "kind": "input",
                        "position": [0.0, 0.0],
                        "data": { "schema": "Credentials" },
                        "sockets": [{ "name": "out", "direction": "output", "ty": "Credentials" }]
                    },
                    {
                        "id": n_agent,
                        "kind": "agent_call",
                        "position": [240.0, 0.0],
                        "data": {
                            "skill": "auth.verify_password",
                            "model": "anthropic:claude-sonnet-4-6",
                            "max_turns": 1
                        },
                        "sockets": [
                            { "name": "in", "direction": "input", "ty": "Credentials" },
                            { "name": "ok", "direction": "output", "ty": "Token" }
                        ]
                    }
                ]
            }),
        )
        .unwrap();

        // Add an output node + connect agent → output via patch.
        let e1 = "aaaa1111-1111-1111-1111-111111111111";
        let e2 = "bbbb1111-1111-1111-1111-111111111111";
        flow_patch(&project, json!({
            "path": "loop.flow",
            "add_nodes": [{
                "id": n_output,
                "kind": "output",
                "position": [480.0, 0.0],
                "data": { "schema": "Token" },
                "sockets": [{ "name": "in", "direction": "input", "ty": "Token" }]
            }],
            "add_edges": [
                { "id": e1, "source": { "node": n_input, "socket": "out" }, "target": { "node": n_agent, "socket": "in" } },
                { "id": e2, "source": { "node": n_agent, "socket": "ok" }, "target": { "node": n_output, "socket": "in" } }
            ]
        })).unwrap();

        // Read back and verify everything's there.
        let got = flow_get(&project, json!({ "path": "loop.flow" })).unwrap();
        assert_eq!(got["meta"]["title"], "Auth verification");
        assert_eq!(got["meta"]["description"], "credentials → mint token");
        assert_eq!(got["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(got["edges"].as_array().unwrap().len(), 2);

        // Spot-check the rich agent_call payload survived round-trip.
        let agent = got["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["kind"] == "agent_call")
            .unwrap();
        assert_eq!(agent["data"]["skill"], "auth.verify_password");
        assert_eq!(agent["data"]["max_turns"], 1);
        assert_eq!(agent["sockets"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn get_on_missing_file_returns_friendly_error() {
        let (_tmp, project) = make_project();
        let err = flow_get(&project, json!({ "path": "missing.flow" })).unwrap_err();
        assert!(err.to_string().contains("no such file"), "{err}");
    }

    #[test]
    fn patch_on_missing_file_suggests_create() {
        let (_tmp, project) = make_project();
        let err = flow_patch(
            &project,
            json!({
                "path": "missing.flow",
                "add_nodes": []
            }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("flow_create"), "{err}");
    }

    #[test]
    fn create_returns_validation_report() {
        // Symmetric with patch — agent that creates a malformed graph
        // learns immediately, doesn't have to re-call get to find out.
        let (_tmp, project) = make_project();
        let out = flow_create(
            &project,
            json!({
                "path": "bad.flow",
                "edges": [{
                    "id": "aaaa1111-1111-1111-1111-111111111111",
                    "source": { "node": "deadbeef-dead-dead-dead-deaddeafbeef" },
                    "target": { "node": "deadbeef-dead-dead-dead-deaddeafbeef" }
                }]
            }),
        )
        .unwrap();
        assert_eq!(out["validation"]["is_clean"], false);
        assert_eq!(
            out["validation"]["dangling_edges"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn patch_set_meta_updates_title() {
        let (_tmp, project) = make_project();
        flow_create(&project, json!({ "path": "x.flow", "title": "old" })).unwrap();
        flow_patch(
            &project,
            json!({
                "path": "x.flow",
                "set_meta": { "title": "new", "description": "added" }
            }),
        )
        .unwrap();
        let got = flow_get(&project, json!({ "path": "x.flow" })).unwrap();
        assert_eq!(got["meta"]["title"], "new");
        assert_eq!(got["meta"]["description"], "added");
    }
}
