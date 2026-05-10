//! Node-flow graphs for Flynt.
//!
//! A `Flow` is a typed graph of nodes connected by edges via named sockets.
//! Used by the desktop app's flow editor (operator drag-drop) and by
//! flynt-agent tools (agents render architecture/workflows as flows).
//!
//! ## File format
//!
//! Stored as `.flow` files: a TOML frontmatter wrapper around a JSON body,
//! mirroring the wrapper used for tasks and Excalidraw drawings:
//!
//! ```text
//! +++
//! id = "uuid"
//! kind = "flow"
//! [data]
//! title = "Auth Subsystem"
//! schema_version = 1
//! +++
//! { "nodes": [...], "edges": [...] }
//! ```
//!
//! The frontmatter is parseable through Flynt's existing pipeline (sidebar,
//! search, indexer get it for free); the JSON body is the editor payload.
//!
//! ## Schema design
//!
//! See `schema.rs`. Key choices documented there:
//! - Sockets are untyped strings in v1 (`ty: Option<String>` is doc-only).
//! - `data: serde_json::Value` per node — schema-flexible per `kind`.
//! - No subgraphs / groups in v1 — flat graphs cover ~80% of operator value.

pub mod io;
pub mod schema;

pub use schema::{
    Flow, FlowEdge, FlowEndpoint, FlowMeta, FlowNode, NodeKind, Socket, SocketDir,
    ValidationReport,
};
pub use io::{load_flow, parse_flow, save_flow, serialize_flow, FlowDocument, SCHEMA_VERSION};
