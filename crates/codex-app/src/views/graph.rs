use crate::bootstrap::AppContext;
use codex_core::graph::{build_graph_payload, GraphEdgeKind, GraphNodeKind};
use dioxus::prelude::*;

#[component]
pub fn GraphView() -> Element {
    let ctx = use_context::<AppContext>();

    let graph = use_resource(move || {
        let ctx = ctx.clone();
        async move {
            tokio::task::spawn_blocking(move || build_graph_payload(&*ctx.vault.store))
                .await
                .ok()
                .and_then(Result::ok)
        }
    });

    rsx! {
        div { class: "view-graph",
            h2 { class: "view-heading", "Graph" }
            match &*graph.read() {
                None => rsx! { p { class: "placeholder", "Loading knowledge graph…" } },
                Some(Some(payload)) => rsx! {
                    p {
                        class: "placeholder",
                        "{payload.nodes.len()} nodes • {payload.edges.len()} links"
                    }
                    div { class: "graph-list",
                        for node in &payload.nodes {
                            div { class: "graph-node-row",
                                strong { "{node.title}" }
                                span { class: "muted", " {format_node_kind(&node.kind)} • {node.group}" }
                            }
                        }
                    }
                    div { class: "graph-list",
                        for edge in &payload.edges {
                            div { class: "graph-node-row muted",
                                "{edge.source} → {edge.target} • {format_edge_kind(&edge.kind)}"
                            }
                        }
                    }
                },
                Some(None) => rsx! { p { class: "placeholder", "Graph unavailable." } },
            }
        }
    }
}

fn format_node_kind(kind: &GraphNodeKind) -> &'static str {
    match kind {
        GraphNodeKind::Document => "document",
        GraphNodeKind::Task => "task",
        GraphNodeKind::Board => "board",
        GraphNodeKind::MemoryFact => "memory",
    }
}

fn format_edge_kind(kind: &GraphEdgeKind) -> &'static str {
    match kind {
        GraphEdgeKind::Wikilink => "wikilink",
        GraphEdgeKind::TaskMembership => "task-membership",
        GraphEdgeKind::SemanticSupport => "semantic-support",
    }
}
