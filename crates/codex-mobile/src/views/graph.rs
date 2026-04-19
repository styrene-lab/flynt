use codex_core::graph::{build_graph_payload, render_graph_svg, LayoutConfig};
use codex_core::store::VaultStore;
use dioxus::prelude::*;
use crate::bootstrap::MobileRuntime;

#[component]
pub fn GraphView() -> Element {
    let rt = use_context::<Signal<MobileRuntime>>();

    let graph_svg = use_memo(move || {
        let graph = build_graph_payload(&*rt.read().vault.store).ok()?;
        if graph.nodes.is_empty() {
            return None;
        }
        let config = LayoutConfig {
            width: 380.0,
            height: 600.0,
            node_radius: 4.0,
            ..Default::default()
        };
        Some(render_graph_svg(&graph, &config))
    });

    match &*graph_svg.read() {
        Some(svg) => rsx! {
            div { class: "graph-mobile",
                div { class: "graph-canvas-mobile", dangerous_inner_html: "{svg}" }
            }
        },
        None => rsx! {
            div { class: "graph-mobile-empty",
                p { class: "muted", "No graph data — vault may be empty." }
            }
        },
    }
}
