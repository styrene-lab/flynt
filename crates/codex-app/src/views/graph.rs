use crate::bootstrap::AppContext;
use codex_core::store::VaultStore;
use dioxus::prelude::*;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
struct GraphNode {
    id: String,
    title: String,
    group: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct GraphEdge {
    source: String,
    target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct GraphPayload {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

#[component]
pub fn GraphView() -> Element {
    let ctx = use_context::<AppContext>();

    let graph = use_resource(move || {
        let ctx = ctx.clone();
        async move {
            tokio::task::spawn_blocking(move || build_graph_payload(&ctx)).await.ok().flatten()
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
                                span { class: "muted", " {node.group}" }
                            }
                        }
                    }
                },
                Some(None) => rsx! { p { class: "placeholder", "Graph unavailable." } },
            }
        }
    }
}

fn build_graph_payload(ctx: &AppContext) -> Option<GraphPayload> {
    let docs = ctx.vault.store.list_documents().ok()?;
    let mut nodes = Vec::with_capacity(docs.len());
    let mut edges = Vec::new();

    for meta in docs {
        let id = meta.id.0.to_string();
        nodes.push(GraphNode {
            id: id.clone(),
            title: meta.title.clone(),
            group: top_level_group(&meta.path),
        });

        if let Ok(Some(doc)) = ctx.vault.store.get_document(&meta.id) {
            for link in doc.outgoing_links {
                if let Ok(Some(target)) = ctx.vault.store.find_document_by_slug(&link.target) {
                    edges.push(GraphEdge {
                        source: id.clone(),
                        target: target.id.0.to_string(),
                    });
                }
            }
        }
    }

    Some(GraphPayload { nodes, edges })
}

fn top_level_group(path: &std::path::Path) -> String {
    path.components()
        .next()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".into())
}
