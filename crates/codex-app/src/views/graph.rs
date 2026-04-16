use crate::bootstrap::AppContext;
use codex_core::graph::{build_graph_payload, GraphEdgeKind, GraphNodeKind, GraphPayload};
use dioxus::prelude::*;

#[component]
pub fn GraphView() -> Element {
    let ctx = use_context::<AppContext>();
    let mut selected_group = use_signal(|| Option::<String>::None);
    let mut selected_kind = use_signal(|| Option::<GraphNodeKind>::None);

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
                Some(Some(payload)) => {
                    let filtered_nodes = payload.nodes.iter().filter(|node| {
                        selected_group.read().as_ref().map(|group| &node.group == group).unwrap_or(true)
                            && selected_kind.read().as_ref().map(|kind| &node.kind == kind).unwrap_or(true)
                    }).collect::<Vec<_>>();
                    let filtered_ids = filtered_nodes.iter().map(|node| node.id.clone()).collect::<std::collections::HashSet<_>>();
                    let filtered_edges = payload.edges.iter().filter(|edge| {
                        filtered_ids.contains(&edge.source) || filtered_ids.contains(&edge.target)
                    }).collect::<Vec<_>>();

                    rsx! {
                        p {
                            class: "placeholder",
                            "{filtered_nodes.len()} nodes • {filtered_edges.len()} links"
                        }
                        div { class: "graph-filters",
                            button {
                                class: if selected_kind.read().is_none() { "btn btn-primary" } else { "btn btn-ghost" },
                                onclick: move |_| *selected_kind.write() = None,
                                "All kinds"
                            }
                            for kind in [GraphNodeKind::Document, GraphNodeKind::Task, GraphNodeKind::Board] {
                                button {
                                    class: if selected_kind.read().as_ref() == Some(&kind) { "btn btn-primary" } else { "btn btn-ghost" },
                                    onclick: move |_| *selected_kind.write() = Some(kind.clone()),
                                    "{format_node_kind(&kind)}"
                                }
                            }
                        }
                        div { class: "graph-filters",
                            button {
                                class: if selected_group.read().is_none() { "btn btn-primary" } else { "btn btn-ghost" },
                                onclick: move |_| *selected_group.write() = None,
                                "All groups"
                            }
                            for group in &payload.groups {
                                button {
                                    class: if selected_group.read().as_ref() == Some(group) { "btn btn-primary" } else { "btn btn-ghost" },
                                    onclick: {
                                        let group = group.clone();
                                        move |_| *selected_group.write() = Some(group.clone())
                                    },
                                    "{group}"
                                }
                            }
                        }
                        GraphSummary { payload: payload.clone(), filtered_node_ids: filtered_ids.into_iter().collect() }
                        div { class: "graph-list",
                            for node in filtered_nodes {
                                div { class: "graph-node-row",
                                    strong { "{node.title}" }
                                    span { class: "muted", " {format_node_kind(&node.kind)} • {node.group}" }
                                }
                            }
                        }
                        div { class: "graph-list",
                            for edge in filtered_edges {
                                div { class: "graph-node-row muted",
                                    "{edge.source} → {edge.target} • {format_edge_kind(&edge.kind)}"
                                }
                            }
                        }
                    }
                },
                Some(None) => rsx! { p { class: "placeholder", "Graph unavailable." } },
            }
        }
    }
}

#[component]
fn GraphSummary(payload: GraphPayload, filtered_node_ids: Vec<String>) -> Element {
    let filtered = filtered_node_ids.into_iter().collect::<std::collections::HashSet<_>>();
    let document_count = payload.nodes.iter().filter(|node| filtered.contains(&node.id) && node.kind == GraphNodeKind::Document).count();
    let task_count = payload.nodes.iter().filter(|node| filtered.contains(&node.id) && node.kind == GraphNodeKind::Task).count();
    let board_count = payload.nodes.iter().filter(|node| filtered.contains(&node.id) && node.kind == GraphNodeKind::Board).count();

    rsx! {
        div { class: "graph-list",
            div { class: "graph-node-row", strong { "Documents" } span { class: "muted", " {document_count}" } }
            div { class: "graph-node-row", strong { "Tasks" } span { class: "muted", " {task_count}" } }
            div { class: "graph-node-row", strong { "Boards" } span { class: "muted", " {board_count}" } }
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
