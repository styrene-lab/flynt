//! Integration tests for `.flow` file I/O and schema validation.
//!
//! Golden-file tests pin the on-disk format. A regression here means the
//! editor or agent will lose data on the next read/write cycle, so these
//! are load-bearing.

use flynt_flow::{
    load_flow, parse_flow, save_flow, serialize_flow, Flow, FlowEdge, FlowEndpoint, FlowMeta,
    FlowNode, NodeKind, Socket, SocketDir, SCHEMA_VERSION,
};
use uuid::Uuid;

const GOLDEN_AUTH: &str = include_str!("golden/auth-subsystem.flow");
const GOLDEN_ID: &str = "550e8400-e29b-41d4-a716-446655440000";

// ── Parse + roundtrip ───────────────────────────────────────────────────────

#[test]
fn parses_golden_auth_subsystem() {
    let doc = parse_flow(GOLDEN_AUTH).expect("golden parses");
    assert_eq!(doc.id, Uuid::parse_str(GOLDEN_ID).unwrap());
    assert_eq!(doc.schema_version, SCHEMA_VERSION);
    assert!(doc.schema_matches());

    let flow = &doc.flow;
    assert_eq!(flow.meta.title.as_deref(), Some("Auth Subsystem"));
    assert_eq!(flow.nodes.len(), 5);
    assert_eq!(flow.edges.len(), 3);

    // Spot-check a typed node — agent_call carries skill metadata in `data`.
    let agent_node = flow
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::AgentCall))
        .expect("agent_call present");
    assert_eq!(
        agent_node.data.get("skill").and_then(|v| v.as_str()),
        Some("auth.verify_password")
    );
    assert_eq!(agent_node.sockets.len(), 3);

    // A note node has no sockets — proves the optional sockets array
    // round-trips empty.
    let note = flow
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Note))
        .expect("note present");
    assert!(note.sockets.is_empty());

    assert!(flow.validate().is_clean(), "golden validates clean");
}

#[test]
fn roundtrip_preserves_structure() {
    let doc = parse_flow(GOLDEN_AUTH).expect("golden parses");
    let raw = serialize_flow(&doc.flow, doc.id);
    let reloaded = parse_flow(&raw).expect("re-parse");

    assert_eq!(reloaded.id, doc.id);
    assert_eq!(reloaded.schema_version, doc.schema_version);
    assert_eq!(reloaded.flow.nodes, doc.flow.nodes);
    assert_eq!(reloaded.flow.edges, doc.flow.edges);
    assert_eq!(reloaded.flow.meta, doc.flow.meta);
}

#[test]
fn save_and_load_disk_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("subdir/test.flow");

    let id = Uuid::new_v4();
    let flow = make_minimal_flow();
    save_flow(&path, &flow, Some(id)).expect("save creates parents + writes file");
    assert!(path.exists());

    let doc = load_flow(&path).expect("load");
    assert_eq!(doc.id, id);
    assert_eq!(doc.flow.nodes.len(), flow.nodes.len());
}

// ── Failure modes ───────────────────────────────────────────────────────────

#[test]
fn rejects_non_flow_kind() {
    let raw = "+++\nid = \"550e8400-e29b-41d4-a716-446655440000\"\nkind = \"task\"\n+++\n{}\n";
    let err = parse_flow(raw).unwrap_err().to_string();
    assert!(err.contains("kind"), "{err}");
}

#[test]
fn rejects_missing_frontmatter() {
    let raw = "{}";
    let err = parse_flow(raw).unwrap_err().to_string();
    assert!(err.contains("frontmatter"), "{err}");
}

#[test]
fn rejects_unclosed_frontmatter() {
    let raw = "+++\nid = \"550e8400-e29b-41d4-a716-446655440000\"\nkind = \"flow\"\n";
    let err = parse_flow(raw).unwrap_err().to_string();
    assert!(err.contains("not closed") || err.contains("closing"), "{err}");
}

#[test]
fn empty_body_parses_as_empty_flow() {
    let raw =
        "+++\nid = \"550e8400-e29b-41d4-a716-446655440000\"\nkind = \"flow\"\n\n[data]\ntitle = \"Empty\"\nschema_version = 1\n+++\n";
    let doc = parse_flow(raw).expect("empty body is valid");
    assert!(doc.flow.nodes.is_empty());
    assert!(doc.flow.edges.is_empty());
}

#[test]
fn minimal_body_without_nodes_or_edges_parses() {
    // Adversarial review caught this: agents will absolutely send
    // partial bodies during early integration. A `meta`-only body must
    // not panic — `Flow.nodes` and `Flow.edges` carry `#[serde(default)]`.
    let raw = r#"+++
id = "550e8400-e29b-41d4-a716-446655440000"
kind = "flow"

[data]
title = "Skeleton"
schema_version = 1
+++
{ "meta": { "title": "Skeleton", "description": "WIP" } }
"#;
    let doc = parse_flow(raw).expect("minimal body parses");
    assert!(doc.flow.nodes.is_empty());
    assert!(doc.flow.edges.is_empty());
    assert_eq!(doc.flow.meta.title.as_deref(), Some("Skeleton"));
}

#[test]
fn fully_empty_object_body_parses() {
    let raw = r#"+++
id = "550e8400-e29b-41d4-a716-446655440000"
kind = "flow"

[data]
schema_version = 1
+++
{}
"#;
    let doc = parse_flow(raw).expect("`{}` body parses as default Flow");
    assert!(doc.flow.nodes.is_empty());
    assert!(doc.flow.edges.is_empty());
    assert_eq!(doc.flow.meta, FlowMeta::default());
}

// ── Schema versioning ──────────────────────────────────────────────────────

#[test]
fn future_schema_version_parses_but_signals_mismatch() {
    let raw = r#"+++
id = "550e8400-e29b-41d4-a716-446655440000"
kind = "flow"

[data]
title = "From the future"
schema_version = 99
+++
{ "nodes": [], "edges": [] }
"#;
    let doc = parse_flow(raw).expect("future-version files still parse (be liberal)");
    assert_eq!(doc.schema_version, 99);
    assert!(!doc.schema_matches(), "editor should refuse to save back");
}

#[test]
fn missing_schema_version_defaults_to_current() {
    // Older `.flow` files (or hand-edited fixtures) that omit the version
    // — assume current rather than fail-on-load.
    let raw = r#"+++
id = "550e8400-e29b-41d4-a716-446655440000"
kind = "flow"

[data]
title = "T"
+++
{ "nodes": [], "edges": [] }
"#;
    let doc = parse_flow(raw).unwrap();
    assert_eq!(doc.schema_version, SCHEMA_VERSION);
}

// ── Validation ──────────────────────────────────────────────────────────────

#[test]
fn validate_flags_dangling_edges() {
    let mut flow = make_minimal_flow();
    let ghost_node = Uuid::new_v4();
    flow.edges.push(FlowEdge {
        id: Uuid::new_v4(),
        source: FlowEndpoint { node: ghost_node, socket: "out".into() },
        target: FlowEndpoint { node: flow.nodes[0].id, socket: "in".into() },
    });
    let report = flow.validate();
    assert_eq!(report.dangling_edges.len(), 1);
    assert!(!report.is_clean());
}

#[test]
fn validate_flags_duplicate_node_ids() {
    let mut flow = make_minimal_flow();
    let dup = flow.nodes[0].clone();
    flow.nodes.push(dup);
    let report = flow.validate();
    assert_eq!(report.duplicate_node_ids.len(), 1);
}

#[test]
fn validate_flags_duplicate_edge_ids() {
    // Adversarial review: agent might patch in an edge with an existing
    // id by accident. The editor should warn, not silently overwrite.
    let mut flow = make_minimal_flow();
    let dup = flow.edges[0].clone();
    flow.edges.push(dup);
    let report = flow.validate();
    assert_eq!(report.duplicate_edge_ids.len(), 1);
    assert!(!report.is_clean());
}

#[test]
fn validate_flags_unknown_socket_names_on_typed_nodes() {
    // Edge points at a socket the target node doesn't declare. This is
    // the most common agent-tool misuse: the agent invents a socket name
    // without first patching the node's sockets list. Editor needs to
    // surface this so the operator can fix.
    let mut flow = make_minimal_flow();
    let bad_target = flow.nodes[1].id;
    flow.edges[0].target = FlowEndpoint {
        node: bad_target,
        socket: "nonexistent".into(),
    };
    let report = flow.validate();
    assert_eq!(report.edges_with_unknown_sockets.len(), 1);
    assert!(report.dangling_edges.is_empty(), "node exists, only the socket is wrong");
}

#[test]
fn validate_skips_socket_check_when_node_declares_no_sockets() {
    // Note nodes typically have no sockets. Edges terminating at them
    // (or originating from them) should NOT be flagged for socket-name
    // mismatch — the design choice is "no declared sockets means open
    // socket policy."
    let note_id = Uuid::new_v4();
    let other_id = Uuid::new_v4();
    let flow = Flow {
        meta: FlowMeta::default(),
        nodes: vec![
            FlowNode {
                id: note_id,
                kind: NodeKind::Note,
                position: (0.0, 0.0),
                data: serde_json::json!({ "body": "annotation" }),
                sockets: vec![],
            },
            FlowNode {
                id: other_id,
                kind: NodeKind::Step,
                position: (200.0, 0.0),
                data: serde_json::json!({}),
                sockets: vec![Socket {
                    name: "out".into(),
                    direction: SocketDir::Output,
                    ty: None,
                }],
            },
        ],
        edges: vec![FlowEdge {
            id: Uuid::new_v4(),
            source: FlowEndpoint { node: other_id, socket: "out".into() },
            target: FlowEndpoint { node: note_id, socket: "anything".into() },
        }],
    };
    let report = flow.validate();
    assert!(
        report.edges_with_unknown_sockets.is_empty(),
        "edges to socketless nodes are open by design"
    );
}

#[test]
fn validate_flags_duplicate_socket_names_within_a_node() {
    let mut flow = make_minimal_flow();
    flow.nodes[1].sockets.push(Socket {
        name: "in".into(),
        direction: SocketDir::Input,
        ty: None,
    });
    let report = flow.validate();
    assert_eq!(report.duplicate_socket_names.len(), 1);
    let (node_id, name) = &report.duplicate_socket_names[0];
    assert_eq!(*node_id, flow.nodes[1].id);
    assert_eq!(name, "in");
}

// ── Frontmatter quirks ─────────────────────────────────────────────────────

#[test]
fn frontmatter_title_round_trips_via_serialize() {
    let mut flow = make_minimal_flow();
    flow.meta.title = Some("My Architecture".into());
    let raw = serialize_flow(&flow, Uuid::new_v4());
    assert!(raw.contains("title = \"My Architecture\""), "{raw}");
    let doc = parse_flow(&raw).unwrap();
    assert_eq!(doc.flow.meta.title.as_deref(), Some("My Architecture"));
}

#[test]
fn frontmatter_title_with_newline_round_trips() {
    // Adversarial review caught: agents will write multiline titles.
    // The default `toml_quote` in task_file.rs only escapes \\ and ",
    // which would have produced invalid TOML here. flynt-flow uses a
    // proper basic-string escape that handles \n, \r, \t, and the rest
    // of the C0 control range.
    let mut flow = make_minimal_flow();
    flow.meta.title = Some("Line 1\nLine 2\twith tab".into());
    let raw = serialize_flow(&flow, Uuid::new_v4());
    let doc = parse_flow(&raw).expect("escaped title parses back");
    assert_eq!(doc.flow.meta.title.as_deref(), Some("Line 1\nLine 2\twith tab"));
}

#[test]
fn frontmatter_title_with_quotes_and_backslashes_round_trips() {
    let mut flow = make_minimal_flow();
    flow.meta.title = Some(r#"path\to "thing""#.into());
    let raw = serialize_flow(&flow, Uuid::new_v4());
    let doc = parse_flow(&raw).unwrap();
    assert_eq!(doc.flow.meta.title.as_deref(), Some(r#"path\to "thing""#));
}

// ── Custom kinds ───────────────────────────────────────────────────────────

#[test]
fn custom_node_kind_round_trips() {
    // Agent invents a kind we don't have a first-class variant for. Should
    // parse as Custom("…") and re-serialize untagged so the editor sees
    // the same string.
    let raw = r#"+++
id = "550e8400-e29b-41d4-a716-446655440000"
kind = "flow"

[data]
title = "T"
schema_version = 1
+++
{
  "nodes": [
    {
      "id": "11111111-1111-1111-1111-111111111111",
      "kind": "queue_consumer",
      "position": [0.0, 0.0],
      "data": { "topic": "events" }
    }
  ],
  "edges": []
}
"#;
    let doc = parse_flow(raw).expect("custom kind parses");
    assert!(matches!(&doc.flow.nodes[0].kind, NodeKind::Custom(s) if s == "queue_consumer"));

    let reraw = serialize_flow(&doc.flow, Uuid::new_v4());
    assert!(
        reraw.contains("\"kind\": \"queue_consumer\""),
        "custom kind re-serializes as the original string, got: {reraw}"
    );
}

#[test]
fn known_kind_string_does_not_become_custom_on_reload() {
    // Sanity check the untagged precedence: if an agent emits "step", it
    // must hit the first-class Step variant, not Custom("step"). A
    // mistake here would silently demote first-class kinds to Custom on
    // every save/load cycle.
    let mut flow = make_minimal_flow();
    flow.nodes[1].kind = NodeKind::Step;
    let raw = serialize_flow(&flow, Uuid::new_v4());
    let doc = parse_flow(&raw).unwrap();
    assert!(matches!(doc.flow.nodes[1].kind, NodeKind::Step));
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn make_minimal_flow() -> Flow {
    let n1 = FlowNode {
        id: Uuid::new_v4(),
        kind: NodeKind::Input,
        position: (0.0, 0.0),
        data: serde_json::json!({}),
        sockets: vec![Socket { name: "out".into(), direction: SocketDir::Output, ty: None }],
    };
    let n2 = FlowNode {
        id: Uuid::new_v4(),
        kind: NodeKind::Step,
        position: (200.0, 0.0),
        data: serde_json::json!({ "name": "transform" }),
        sockets: vec![
            Socket { name: "in".into(), direction: SocketDir::Input, ty: None },
            Socket { name: "out".into(), direction: SocketDir::Output, ty: None },
        ],
    };
    let edge = FlowEdge {
        id: Uuid::new_v4(),
        source: FlowEndpoint { node: n1.id, socket: "out".into() },
        target: FlowEndpoint { node: n2.id, socket: "in".into() },
    };
    Flow {
        meta: FlowMeta { title: Some("Minimal".into()), description: None },
        nodes: vec![n1, n2],
        edges: vec![edge],
    }
}
