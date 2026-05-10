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
    pub nodes: Vec<FlowNode>,
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

    /// Lightweight integrity check used by the I/O layer. Returns the list
    /// of edges whose endpoints reference nodes that don't exist, plus the
    /// list of duplicate node ids. This is best-effort — we never refuse
    /// to load a Flow because of these (the editor is the right place to
    /// surface bad references); the caller decides what to do with the
    /// report.
    pub fn validate(&self) -> ValidationReport {
        let known: std::collections::HashSet<&Uuid> = self.nodes.iter().map(|n| &n.id).collect();
        let mut dangling = Vec::new();
        for edge in &self.edges {
            if !known.contains(&edge.source.node) || !known.contains(&edge.target.node) {
                dangling.push(edge.id);
            }
        }
        let mut counts: BTreeMap<Uuid, usize> = BTreeMap::new();
        for n in &self.nodes {
            *counts.entry(n.id).or_default() += 1;
        }
        let duplicate_node_ids = counts.into_iter().filter(|(_, c)| *c > 1).map(|(id, _)| id).collect();
        ValidationReport { dangling_edges: dangling, duplicate_node_ids }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationReport {
    pub dangling_edges: Vec<Uuid>,
    pub duplicate_node_ids: Vec<Uuid>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.dangling_edges.is_empty() && self.duplicate_node_ids.is_empty()
    }
}
