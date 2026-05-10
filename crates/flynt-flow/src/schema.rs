//! Flow graph schema.
//!
//! Designed to be friendly to both react-flow's wire format (the webview
//! editor) and to agent tool calls. Position is `(f32, f32)` to match
//! react-flow's `{ x, y }` and to keep diffs stable when the operator
//! drags nodes (an integer grid would also work, but float matches the
//! editor's native space).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

// ── Whole-flow ──────────────────────────────────────────────────────────────

/// A node-flow graph: nodes + edges + lightweight metadata.
///
/// `Flow` is the JSON body inside a `.flow` file. The frontmatter wrapper
/// (id, kind="flow", title, schema_version) lives separately and is
/// produced by `io::serialize_flow`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Flow {
    /// Display metadata. Kept out of the frontmatter `[data]` table so the
    /// JSON body stays self-describing for agent tool calls that round-trip
    /// just the body.
    #[serde(default)]
    pub meta: FlowMeta,
    /// `default` so a minimal agent-authored body like `{"meta": {...}}`
    /// or even `{}` parses cleanly. Empty graphs are valid (just-created
    /// files, scaffolded skeletons).
    #[serde(default)]
    pub nodes: Vec<FlowNode>,
    #[serde(default)]
    pub edges: Vec<FlowEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FlowMeta {
    /// Operator-facing title. Mirrored into frontmatter `[data].title` so
    /// the sidebar/indexer pick it up; the JSON body keeps a copy so the
    /// agent tool surface doesn't need to read frontmatter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Free-form description shown above the canvas. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ── Nodes ───────────────────────────────────────────────────────────────────

/// One node in the flow graph.
///
/// `data` is schema-flexible per `kind` — concrete shapes are documented
/// alongside each `NodeKind`. Keeping it as `serde_json::Value` lets the
/// agent invent fields without a recompile, and the editor renders unknown
/// fields generically. Type-safety can come later via per-kind sub-schemas
/// once a second consumer (e.g. omegon workflow export) shows the friction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowNode {
    pub id: Uuid,
    pub kind: NodeKind,
    /// Editor position in flow space. react-flow consumes `{ x, y }`
    /// natively; the bridge layer flattens this tuple for JS.
    pub position: (f32, f32),
    /// Per-kind payload. See `NodeKind` docs for shape per variant.
    #[serde(default)]
    pub data: serde_json::Value,
    /// Named connection points. Order is preserved so the editor renders
    /// them deterministically.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sockets: Vec<Socket>,
}

/// Coarse-grained node taxonomy.
///
/// Kept narrow on purpose. `Custom(String)` is the escape hatch for new
/// kinds the agent invents — they parse, render with a default node body,
/// and a future PR can promote them to first-class variants if they earn
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A unit of work in a workflow. Typical `data`: `{ name, description }`.
    Step,
    /// Graph entry — the workflow's input. Typical `data`: `{ schema }`.
    Input,
    /// Graph exit. Typical `data`: `{ schema }`.
    Output,
    /// Conditional split. Typical `data`: `{ condition }`.
    Branch,
    /// Agent invocation — Omegon skill call. Typical `data`:
    /// `{ skill, prompt_template?, model?, max_turns? }`.
    AgentCall,
    /// Free-form annotation; never executes. Typical `data`: `{ body }`.
    Note,
    /// Operator- or agent-defined kind. Renders generically until promoted.
    #[serde(untagged)]
    Custom(String),
}

// ── Sockets ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Socket {
    /// Local name within the node — must be unique per node.
    pub name: String,
    pub direction: SocketDir,
    /// Documentation-only type tag in v1. Future versions may enforce
    /// matching `ty` between connected sockets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocketDir {
    Input,
    Output,
}

// ── Edges ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowEdge {
    pub id: Uuid,
    pub source: FlowEndpoint,
    pub target: FlowEndpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowEndpoint {
    pub node: Uuid,
    /// Name of the socket on `node`. Empty string is allowed and means
    /// "the node's default socket" — used by lightweight kinds like Note
    /// that don't bother declaring sockets.
    #[serde(default)]
    pub socket: String,
}

// ── Convenience ─────────────────────────────────────────────────────────────

impl Flow {
    pub fn new() -> Self { Self::default() }

    /// Find a node by id. O(n); flows are expected to be small (<200 nodes).
    pub fn node(&self, id: &Uuid) -> Option<&FlowNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    /// Lightweight integrity check used by the editor and agent tools.
    /// Best-effort — we never refuse to load a Flow because of these
    /// findings (the editor is the right place to surface bad references);
    /// the caller decides what to do with the report.
    ///
    /// Edge sockets are checked against declared sockets only when the
    /// referenced node *has* declared sockets. Nodes that omit sockets
    /// entirely (typical for `Note`) accept any socket name on edges
    /// terminating at them, including the empty string.
    pub fn validate(&self) -> ValidationReport {
        // Index nodes by id for O(1) lookups during edge validation.
        let nodes_by_id: BTreeMap<Uuid, &FlowNode> =
            self.nodes.iter().map(|n| (n.id, n)).collect();

        // Duplicate node ids — detected via the index size mismatch.
        let mut duplicate_node_ids = Vec::new();
        if nodes_by_id.len() != self.nodes.len() {
            let mut counts: BTreeMap<Uuid, usize> = BTreeMap::new();
            for n in &self.nodes {
                *counts.entry(n.id).or_default() += 1;
            }
            duplicate_node_ids = counts.into_iter()
                .filter(|(_, c)| *c > 1)
                .map(|(id, _)| id)
                .collect();
        }

        // Duplicate socket names within a node. Reported as (node_id,
        // socket_name) pairs so the editor can highlight the offending
        // node without the operator having to scan a list.
        let mut duplicate_socket_names = Vec::new();
        for n in &self.nodes {
            let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
            for s in &n.sockets {
                *seen.entry(s.name.as_str()).or_default() += 1;
            }
            for (name, count) in seen {
                if count > 1 {
                    duplicate_socket_names.push((n.id, name.to_string()));
                }
            }
        }

        let mut dangling_edges = Vec::new();
        let mut edges_with_unknown_sockets = Vec::new();
        let mut duplicate_edge_ids = Vec::new();
        let mut edge_id_seen: BTreeMap<Uuid, usize> = BTreeMap::new();

        for edge in &self.edges {
            *edge_id_seen.entry(edge.id).or_default() += 1;

            // Endpoint reaches a non-existent node → fully dangling.
            let source_node = nodes_by_id.get(&edge.source.node);
            let target_node = nodes_by_id.get(&edge.target.node);
            if source_node.is_none() || target_node.is_none() {
                dangling_edges.push(edge.id);
                continue;
            }

            // Socket-name check: only enforced when the referenced node
            // declares sockets at all. Nodes with no declared sockets
            // (typical for Note) match any socket name including "".
            let unknown_source = source_node
                .map(|n| !n.sockets.is_empty()
                    && !n.sockets.iter().any(|s| s.name == edge.source.socket))
                .unwrap_or(false);
            let unknown_target = target_node
                .map(|n| !n.sockets.is_empty()
                    && !n.sockets.iter().any(|s| s.name == edge.target.socket))
                .unwrap_or(false);
            if unknown_source || unknown_target {
                edges_with_unknown_sockets.push(edge.id);
            }
        }

        for (id, count) in edge_id_seen {
            if count > 1 { duplicate_edge_ids.push(id); }
        }

        ValidationReport {
            dangling_edges,
            duplicate_node_ids,
            duplicate_edge_ids,
            duplicate_socket_names,
            edges_with_unknown_sockets,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationReport {
    /// Edges whose source or target points at a non-existent node id.
    pub dangling_edges: Vec<Uuid>,
    /// Node ids that appear more than once in `Flow::nodes`.
    pub duplicate_node_ids: Vec<Uuid>,
    /// Edge ids that appear more than once in `Flow::edges`.
    pub duplicate_edge_ids: Vec<Uuid>,
    /// `(node_id, socket_name)` pairs where a node declared the same
    /// socket name twice. The editor should rename or remove one.
    pub duplicate_socket_names: Vec<(Uuid, String)>,
    /// Edges whose source or target socket name is not declared on the
    /// referenced node. Nodes that declare no sockets are exempt — the
    /// edge accepts any socket name (including ""). Important for agent
    /// tool calls that may invent a socket name without first patching
    /// the node.
    pub edges_with_unknown_sockets: Vec<Uuid>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.dangling_edges.is_empty()
            && self.duplicate_node_ids.is_empty()
            && self.duplicate_edge_ids.is_empty()
            && self.duplicate_socket_names.is_empty()
            && self.edges_with_unknown_sockets.is_empty()
    }
}
