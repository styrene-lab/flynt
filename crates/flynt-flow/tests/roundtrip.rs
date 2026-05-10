//! Integration tests for `.flow` file I/O.
//!
//! Golden-file tests pin the on-disk format. A regression here means the
//! editor or agent will lose data on the next read/write cycle, so these
//! are load-bearing.

use flynt_flow::{
    load_flow, parse_flow, save_flow, serialize_flow, Flow, FlowEdge, FlowEndpoint, FlowMeta,
    FlowNode, NodeKind, Socket, SocketDir,
};
use uuid::Uuid;

const GOLDEN_AUTH: &str = include_str!("golden/auth-subsystem.flow");
const GOLDEN_ID: &str = "550e8400-e29b-41d4-a716-446655440000";

#[test]
fn parses_golden_auth_subsystem() {
    let (flow, id) = parse_flow(GOLDEN_AUTH).expect("golden parses");
    assert_eq!(id, Uuid::parse_str(GOLDEN_ID).unwrap());
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

    // Edges resolve to real nodes — validation should be clean.
    assert!(flow.validate().is_clean(), "golden validates clean");
}

#[test]
fn roundtrip_preserves_structure() {
    let (flow, id) = parse_flow(GOLDEN_AUTH).expect("golden parses");
    let raw = serialize_flow(&flow, id);
    let (reloaded, reloaded_id) = parse_flow(&raw).expect("re-parse");

    assert_eq!(reloaded_id, id);
    assert_eq!(reloaded.nodes, flow.nodes);
    assert_eq!(reloaded.edges, flow.edges);
    assert_eq!(reloaded.meta, flow.meta);
}

#[test]
fn save_and_load_disk_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("subdir/test.flow");

    let id = Uuid::new_v4();
    let flow = make_minimal_flow();
    save_flow(&path, &flow, Some(id)).expect("save creates parents + writes file");
    assert!(path.exists());

    let (loaded, loaded_id) = load_flow(&path).expect("load");
    assert_eq!(loaded_id, id);
    assert_eq!(loaded.nodes.len(), flow.nodes.len());
}

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
    // Useful for "newly created, never edited" files — the editor mounts,
    // operator drags first node, save fills in nodes.
    let raw =
        "+++\nid = \"550e8400-e29b-41d4-a716-446655440000\"\nkind = \"flow\"\n\n[data]\ntitle = \"Empty\"\nschema_version = 1\n+++\n";
    let (flow, _) = parse_flow(raw).expect("empty body is valid");
    assert!(flow.nodes.is_empty());
    assert!(flow.edges.is_empty());
}

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
fn frontmatter_title_round_trips_via_serialize() {
    let mut flow = make_minimal_flow();
    flow.meta.title = Some("My Architecture".into());
    let raw = serialize_flow(&flow, Uuid::new_v4());
    assert!(raw.contains("title = \"My Architecture\""), "{raw}");
    let (reparsed, _) = parse_flow(&raw).unwrap();
    assert_eq!(reparsed.meta.title.as_deref(), Some("My Architecture"));
}

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
    let (flow, _) = parse_flow(raw).expect("custom kind parses");
    assert!(matches!(&flow.nodes[0].kind, NodeKind::Custom(s) if s == "queue_consumer"));

    let reraw = serialize_flow(&flow, Uuid::new_v4());
    assert!(
        reraw.contains("\"kind\": \"queue_consumer\""),
        "custom kind re-serializes as the original string, got: {reraw}"
    );
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
