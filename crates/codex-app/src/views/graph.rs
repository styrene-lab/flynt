use crate::{bootstrap::AppContext, state::{Route, TabState}};
use codex_core::{
    graph::{build_graph_payload, GraphEdgeKind, GraphNodeKind, GraphPayload},
    models::DocumentId,
    store::VaultStore,
};
use dioxus::prelude::*;
use std::str::FromStr;

// ── Full filter + display state ─────────────────────────────────────────────

#[derive(Clone, PartialEq)]
struct GraphSettings {
    // Filters
    kind: Option<GraphNodeKind>,
    group: Option<String>,
    tag: Option<String>,
    search: String,
    min_degree: u32,
    show_wikilinks: bool,
    show_task_links: bool,
    show_semantic: bool,
    show_orphans: bool,
    // Display
    show_arrows: bool,
    node_size: f32,      // 1.0 = default
    link_thickness: f32, // 1.0 = default
    text_fade: f32,      // 0.0–1.0, higher = labels visible at further zoom-out
    // Physics
    center_force: f32,
    repel_force: f32,
    link_force: f32,
    link_distance: f32,
    // Local graph
    local_mode: bool,
    local_depth: u32,
    // Highlight
    highlight_ids: Vec<String>,
}

impl Default for GraphSettings {
    fn default() -> Self {
        Self {
            kind: None,
            group: None,
            tag: None,
            search: String::new(),
            min_degree: 0,
            show_wikilinks: true,
            show_task_links: true,
            show_semantic: true,
            show_orphans: true,
            show_arrows: false,
            node_size: 1.0,
            link_thickness: 1.0,
            text_fade: 0.5,
            center_force: 0.5,
            repel_force: 0.5,
            link_force: 0.5,
            link_distance: 0.5,
            local_mode: false,
            local_depth: 2,
            highlight_ids: vec![],
        }
    }
}

#[component]
pub fn GraphView() -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();
    let mut settings = use_signal(GraphSettings::default);
    let mut panel_open = use_signal(|| true);

    let ctx_res = ctx.clone();
    let ctx_click = ctx.clone();

    let graph = use_resource(move || {
        let vault = ctx_res.vault();
        async move {
            tokio::task::spawn_blocking(move || build_graph_payload(&*vault.store))
                .await
                .ok()
                .and_then(Result::ok)
        }
    });

    // Derive filtered JSON
    let graph_json = use_memo(move || {
        let binding = graph.read();
        let Some(Some(payload)) = &*binding else {
            return String::new();
        };
        let s = settings.read();

        // Local graph: if active, center on currently active tab's document
        let local_center = if s.local_mode {
            tab_state.read().active_id().map(|id| id.0.to_string())
        } else {
            None
        };

        let (nodes, edges) = filter_graph(payload, &s, local_center.as_deref());
        graph_to_json(&nodes, &edges, &s)
    });

    use_effect(move || {
        let json = graph_json.read().clone();
        if json.is_empty() { return; }
        let js = format!("{GRAPH_JS}\ncodexGraph({json});");
        let mut eval = document::eval(&js);
        let click_ctx = ctx_click.clone();

        spawn(async move {
            loop {
                match eval.recv::<String>().await {
                    Ok(node_id) => {
                        if let Ok(uuid) = uuid::Uuid::from_str(&node_id) {
                            let doc_id = DocumentId(uuid);
                            let vault = click_ctx.vault();
                            let result: Result<Option<codex_core::models::Document>, _> =
                                tokio::task::spawn_blocking(move || {
                                    vault.store.get_document(&doc_id)
                                }).await.unwrap_or(Ok(None));
                            if let Ok(Some(doc)) = result {
                                tab_state.write().open(doc.id, doc.title);
                                *active_route.write() = Route::Notes;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    });

    let is_panel_open = *panel_open.read();
    rsx! {
        div { class: if is_panel_open { "view-graph panel-open" } else { "view-graph" },
            match &*graph.read() {
                None => rsx! {
                    div { class: "graph-topbar",
                        div { class: "graph-stats", "Loading knowledge graph..." }
                    }
                },
                Some(Some(payload)) => {
                    let s = settings.read().clone();
                    let local_center = if s.local_mode {
                        tab_state.read().active_id().map(|id| id.0.to_string())
                    } else { None };
                    let (filtered_nodes, filtered_edges) = filter_graph(payload, &s, local_center.as_deref());
                    let is_open = *panel_open.read();

                    rsx! {
                        // ── Floating top bar ─────────────────────────────
                        div { class: "graph-topbar",
                            div { class: "graph-stats",
                                "{filtered_nodes.len()} nodes · {filtered_edges.len()} links"
                            }
                            input {
                                r#type: "text",
                                placeholder: "Search nodes...",
                                value: "{s.search}",
                                class: "graph-search-input",
                                oninput: move |e| settings.write().search = e.value(),
                            }
                            button {
                                class: if is_open { "graph-settings-toggle active" } else { "graph-settings-toggle" },
                                onclick: move |_| { let v = *panel_open.read(); *panel_open.write() = !v; },
                                if is_open { "Close" } else { "Settings" }
                            }
                        }

                        // ── Graph canvas ─────────────────────────────────
                        div { id: "graph-canvas", class: "graph-canvas" }

                        // ── Settings panel (right drawer) ────────────────
                        if is_open {
                            div { class: "graph-panel",
                                div { class: "panel-header",
                                    span { class: "panel-header-title", "Graph Settings" }
                                    button {
                                        class: "panel-close",
                                        onclick: move |_| *panel_open.write() = false,
                                        "×"
                                    }
                                }
                                // Filters
                                div { class: "panel-section",
                                    div { class: "panel-heading", "Filters" }
                                    div { class: "panel-row",
                                        button {
                                            class: if s.kind.is_none() { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                            onclick: move |_| settings.write().kind = None,
                                            "All"
                                        }
                                        for kind in [GraphNodeKind::Document, GraphNodeKind::Repo, GraphNodeKind::Link, GraphNodeKind::Task, GraphNodeKind::Board, GraphNodeKind::Communication, GraphNodeKind::MemoryFact] {
                                            button {
                                                class: if s.kind.as_ref() == Some(&kind) { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                                onclick: {
                                                    let kind = kind.clone();
                                                    move |_| settings.write().kind = Some(kind.clone())
                                                },
                                                "{format_node_kind(&kind)}"
                                            }
                                        }
                                    }
                                    if payload.groups.len() > 1 {
                                        div { class: "panel-row",
                                            button {
                                                class: if s.group.is_none() { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                                onclick: move |_| settings.write().group = None,
                                                "All groups"
                                            }
                                            for group in &payload.groups {
                                                button {
                                                    class: if s.group.as_ref() == Some(group) { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                                    onclick: {
                                                        let group = group.clone();
                                                        move |_| settings.write().group = Some(group.clone())
                                                    },
                                                    "{group}"
                                                }
                                            }
                                        }
                                    }
                                    if !payload.all_tags.is_empty() {
                                        div { class: "panel-row",
                                            button {
                                                class: if s.tag.is_none() { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                                onclick: move |_| settings.write().tag = None,
                                                "all tags"
                                            }
                                            for tag in &payload.all_tags {
                                                button {
                                                    class: if s.tag.as_ref() == Some(tag) { "btn btn-primary btn-xs tag-chip" } else { "btn btn-ghost btn-xs tag-chip" },
                                                    onclick: {
                                                        let tag = tag.clone();
                                                        move |_| settings.write().tag = Some(tag.clone())
                                                    },
                                                    "#{tag}"
                                                }
                                            }
                                        }
                                    }
                                    div { class: "panel-toggle",
                                        span { class: "filter-label", title: "Show notes that have no links to other notes", "Unlinked notes" }
                                        button {
                                            class: if s.show_orphans { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                            onclick: move |_| { let v = settings.read().show_orphans; settings.write().show_orphans = !v; },
                                            if s.show_orphans { "on" } else { "off" }
                                        }
                                    }
                                    div { class: "panel-toggle",
                                        span { class: "filter-label", title: "Show only notes near the currently selected note", "Focus on selection" }
                                        button {
                                            class: if s.local_mode { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                            onclick: move |_| { let v = settings.read().local_mode; settings.write().local_mode = !v; },
                                            if s.local_mode { "on" } else { "off" }
                                        }
                                    }
                                    if s.local_mode {
                                        div { class: "panel-slider",
                                            span { class: "filter-label", title: "How many hops away from the selected note to show", "Neighborhood" }
                                            input { r#type: "range", min: "1", max: "6", step: "1",
                                                value: "{s.local_depth}",
                                                oninput: move |e| { settings.write().local_depth = e.value().parse().unwrap_or(2); },
                                            }
                                            span { class: "slider-value", "{s.local_depth}" }
                                        }
                                    }
                                }

                                // Edges
                                div { class: "panel-section",
                                    div { class: "panel-heading", "Edges" }
                                    div { class: "panel-toggle",
                                        span { class: "filter-label", "Wikilinks" }
                                        button {
                                            class: if s.show_wikilinks { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                            onclick: move |_| { let v = settings.read().show_wikilinks; settings.write().show_wikilinks = !v; },
                                            if s.show_wikilinks { "on" } else { "off" }
                                        }
                                    }
                                    div { class: "panel-toggle",
                                        span { class: "filter-label", "Task links" }
                                        button {
                                            class: if s.show_task_links { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                            onclick: move |_| { let v = settings.read().show_task_links; settings.write().show_task_links = !v; },
                                            if s.show_task_links { "on" } else { "off" }
                                        }
                                    }
                                    div { class: "panel-toggle",
                                        span { class: "filter-label", "Semantic" }
                                        button {
                                            class: if s.show_semantic { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                            onclick: move |_| { let v = settings.read().show_semantic; settings.write().show_semantic = !v; },
                                            if s.show_semantic { "on" } else { "off" }
                                        }
                                    }
                                    div { class: "panel-toggle",
                                        span { class: "filter-label", "Arrows" }
                                        button {
                                            class: if s.show_arrows { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                            onclick: move |_| { let v = settings.read().show_arrows; settings.write().show_arrows = !v; },
                                            if s.show_arrows { "on" } else { "off" }
                                        }
                                    }
                                }

                                // Display
                                div { class: "panel-section",
                                    div { class: "panel-heading", "Display" }
                                    div { class: "panel-slider",
                                        span { class: "filter-label", "Node size" }
                                        input { r#type: "range", min: "0.3", max: "3.0", step: "0.1",
                                            value: "{s.node_size}",
                                            oninput: move |e| { settings.write().node_size = e.value().parse().unwrap_or(1.0); },
                                        }
                                    }
                                    div { class: "panel-slider",
                                        span { class: "filter-label", "Link width" }
                                        input { r#type: "range", min: "0.2", max: "4.0", step: "0.1",
                                            value: "{s.link_thickness}",
                                            oninput: move |e| { settings.write().link_thickness = e.value().parse().unwrap_or(1.0); },
                                        }
                                    }
                                    div { class: "panel-slider",
                                        span { class: "filter-label", "Labels" }
                                        input { r#type: "range", min: "0", max: "1.0", step: "0.05",
                                            value: "{s.text_fade}",
                                            oninput: move |e| { settings.write().text_fade = e.value().parse().unwrap_or(0.5); },
                                        }
                                    }
                                    div { class: "panel-slider",
                                        span { class: "filter-label", title: "Hide notes with fewer than this many connections", "Min connections" }
                                        input { r#type: "range", min: "0", max: "10", step: "1",
                                            value: "{s.min_degree}",
                                            oninput: move |e| { settings.write().min_degree = e.value().parse().unwrap_or(0); },
                                        }
                                        span { class: "slider-value", "{s.min_degree}" }
                                    }
                                }

                                // Layout
                                div { class: "panel-section",
                                    div { class: "panel-heading", "Layout" }
                                    div { class: "panel-slider",
                                        span { class: "filter-label", title: "How strongly nodes are pulled toward the center", "Gravity" }
                                        input { r#type: "range", min: "0", max: "1.0", step: "0.05",
                                            value: "{s.center_force}",
                                            oninput: move |e| { settings.write().center_force = e.value().parse().unwrap_or(0.5); },
                                        }
                                    }
                                    div { class: "panel-slider",
                                        span { class: "filter-label", title: "How strongly unlinked nodes push apart", "Spacing" }
                                        input { r#type: "range", min: "0", max: "1.0", step: "0.05",
                                            value: "{s.repel_force}",
                                            oninput: move |e| { settings.write().repel_force = e.value().parse().unwrap_or(0.5); },
                                        }
                                    }
                                    div { class: "panel-slider",
                                        span { class: "filter-label", title: "How strongly linked notes pull toward each other", "Link strength" }
                                        input { r#type: "range", min: "0", max: "1.0", step: "0.05",
                                            value: "{s.link_force}",
                                            oninput: move |e| { settings.write().link_force = e.value().parse().unwrap_or(0.5); },
                                        }
                                    }
                                    div { class: "panel-slider",
                                        span { class: "filter-label", title: "Preferred distance between linked notes", "Link gap" }
                                        input { r#type: "range", min: "0", max: "1.0", step: "0.05",
                                            value: "{s.link_distance}",
                                            oninput: move |e| { settings.write().link_distance = e.value().parse().unwrap_or(0.5); },
                                        }
                                    }
                                }

                                button {
                                    class: "btn btn-ghost btn-xs",
                                    style: "align-self: flex-start;",
                                    onclick: move |_| *settings.write() = GraphSettings::default(),
                                    "Restore defaults"
                                }
                            }
                        }

                        // ── Legend ────────────────────────────────────────
                        div {
                            class: "graph-legend",
                            div { class: "graph-legend-item",
                                div { class: "graph-legend-dot", style: "background: #2ab4c8" } "Document"
                            }
                            div { class: "graph-legend-item",
                                div { class: "graph-legend-dot", style: "background: #e0a030" } "Repo"
                            }
                            div { class: "graph-legend-item",
                                div { class: "graph-legend-dot", style: "background: #c06090" } "Link"
                            }
                            div { class: "graph-legend-item",
                                div { class: "graph-legend-dot", style: "background: #c86418" } "Task"
                            }
                            div { class: "graph-legend-item",
                                div { class: "graph-legend-dot", style: "background: #1ab878" } "Board"
                            }
                            div { class: "graph-legend-item",
                                div { class: "graph-legend-dot", style: "background: #8860d0" } "Memory"
                            }
                            div { class: "graph-legend-item",
                                div { class: "graph-legend-dot", style: "background: #1a8898" } "Comms"
                            }
                        }
                        div { id: "graph-tooltip", class: "graph-tooltip" }
                    }
                },
                Some(None) => rsx! {
                    div { class: "graph-topbar",
                        div { class: "graph-stats", "Graph unavailable." }
                    }
                },
            }
        }
    }
}

// ── Filtering ───────────────────────────────────────────────────────────────

fn filter_graph<'a>(
    payload: &'a GraphPayload,
    s: &GraphSettings,
    local_center: Option<&str>,
) -> (Vec<&'a codex_core::graph::GraphNode>, Vec<&'a codex_core::graph::GraphEdge>) {
    // Compute degree
    let mut degree: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for edge in &payload.edges {
        *degree.entry(&edge.source).or_default() += 1;
        *degree.entry(&edge.target).or_default() += 1;
    }

    // Local graph: BFS from center to depth
    let local_ids: Option<std::collections::HashSet<&str>> = local_center.map(|center| {
        let mut visited = std::collections::HashSet::new();
        let mut frontier = vec![center];
        visited.insert(center);
        for _ in 0..s.local_depth {
            let mut next = Vec::new();
            for node_id in &frontier {
                for edge in &payload.edges {
                    let neighbor = if edge.source.as_str() == *node_id {
                        Some(edge.target.as_str())
                    } else if edge.target.as_str() == *node_id {
                        Some(edge.source.as_str())
                    } else {
                        None
                    };
                    if let Some(n) = neighbor {
                        if visited.insert(n) {
                            next.push(n);
                        }
                    }
                }
            }
            frontier = next;
        }
        visited
    });

    let nodes: Vec<_> = payload.nodes.iter().filter(|n| {
        // Local graph restriction
        if let Some(ref ids) = local_ids {
            if !ids.contains(n.id.as_str()) { return false; }
        }
        // Kind filter
        if let Some(ref k) = s.kind {
            if &n.kind != k { return false; }
        }
        // Group filter
        if let Some(ref g) = s.group {
            if &n.group != g { return false; }
        }
        // Tag filter
        if let Some(ref tag) = s.tag {
            if !n.tags.contains(tag) { return false; }
        }
        // Orphan filter
        if !s.show_orphans {
            let deg = degree.get(n.id.as_str()).copied().unwrap_or(0);
            if deg == 0 { return false; }
        }
        // Degree filter
        if s.min_degree > 0 {
            let deg = degree.get(n.id.as_str()).copied().unwrap_or(0);
            if deg < s.min_degree { return false; }
        }
        true
    }).collect();

    let ids: std::collections::HashSet<_> = nodes.iter().map(|n| n.id.as_str()).collect();
    let edges: Vec<_> = payload.edges.iter().filter(|e| {
        if !ids.contains(e.source.as_str()) || !ids.contains(e.target.as_str()) {
            return false;
        }
        match e.kind {
            GraphEdgeKind::Wikilink => s.show_wikilinks,
            GraphEdgeKind::TaskMembership => s.show_task_links,
            GraphEdgeKind::SemanticSupport => s.show_semantic,
        }
    }).collect();

    (nodes, edges)
}

fn graph_to_json(
    nodes: &[&codex_core::graph::GraphNode],
    edges: &[&codex_core::graph::GraphEdge],
    s: &GraphSettings,
) -> String {
    let nodes_json: Vec<String> = nodes.iter().map(|n| {
        let tags_json: Vec<String> = n.tags.iter().map(|t| format!("\"{}\"", escape_json(t))).collect();
        format!(
            r#"{{"id":"{}","kind":"{}","title":"{}","group":"{}","tags":[{}],"priority":{},"status":"{}"}}"#,
            escape_json(&n.id),
            format_node_kind(&n.kind),
            escape_json(&n.title),
            escape_json(&n.group),
            tags_json.join(","),
            n.priority.unwrap_or(0),
            n.status.as_deref().unwrap_or(""),
        )
    }).collect();
    let edges_json: Vec<String> = edges.iter().map(|e| {
        format!(
            r#"{{"source":"{}","target":"{}","kind":"{}"}}"#,
            escape_json(&e.source),
            escape_json(&e.target),
            format_edge_kind(&e.kind),
        )
    }).collect();
    let hl_json: Vec<String> = s.highlight_ids.iter().map(|id| format!("\"{}\"", escape_json(id))).collect();
    // Pass search term for JS-side highlighting
    let search_esc = escape_json(&s.search);
    format!(
        r#"{{"nodes":[{nodes}],"edges":[{edges}],"highlight":[{hl}],"search":"{search}","settings":{{"nodeSize":{ns},"linkThickness":{lt},"textFade":{tf},"arrows":{ar},"centerForce":{cf},"repelForce":{rf},"linkForce":{lf},"linkDistance":{ld}}}}}"#,
        nodes = nodes_json.join(","),
        edges = edges_json.join(","),
        hl = hl_json.join(","),
        search = search_esc,
        ns = s.node_size,
        lt = s.link_thickness,
        tf = s.text_fade,
        ar = if s.show_arrows { "true" } else { "false" },
        cf = s.center_force,
        rf = s.repel_force,
        lf = s.link_force,
        ld = s.link_distance,
    )
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn format_node_kind(kind: &GraphNodeKind) -> &'static str {
    match kind {
        GraphNodeKind::Document => "document",
        GraphNodeKind::Task => "task",
        GraphNodeKind::Board => "board",
        GraphNodeKind::Repo => "repo",
        GraphNodeKind::Link => "link",
        GraphNodeKind::MemoryFact => "memory",
        GraphNodeKind::Communication => "communication",
    }
}

fn format_edge_kind(kind: &GraphEdgeKind) -> &'static str {
    match kind {
        GraphEdgeKind::Wikilink => "wikilink",
        GraphEdgeKind::TaskMembership => "task-membership",
        GraphEdgeKind::SemanticSupport => "semantic-support",
    }
}

// ── Graph JS ────────────────────────────────────────────────────────────────

const GRAPH_JS: &str = r#"
function codexGraph(data) {
  if (window._codexGraph) cancelAnimationFrame(window._codexGraph.raf);
  var c = document.getElementById('graph-canvas');
  if (!c) return;
  c.innerHTML = '';
  if (!data.nodes.length) {
    c.innerHTML = '<div style="display:flex;flex-direction:column;align-items:center;justify-content:center;height:100%;color:var(--muted-foreground);font-size:14px;gap:8px;opacity:0.7"><span style="font-size:32px">&#9675;</span><span>No notes to graph yet.</span><span style="font-size:12px">Create some notes and link them with [[wikilinks]] to see the knowledge graph.</span></div>';
    return;
  }

  var W = c.clientWidth || 800, H = c.clientHeight || 600;
  var S = data.settings || {};
  var NODE_SCALE = S.nodeSize || 1.0;
  var LINK_W = S.linkThickness || 1.0;
  var TEXT_FADE = S.textFade || 0.5;
  var ARROWS = S.arrows || false;
  var searchTerm = (data.search || '').toLowerCase();

  // Physics from settings (0-1 range mapped to useful values)
  var REP = 400 + (S.repelForce || 0.5) * 2400;
  var SPR = 0.001 + (S.linkForce || 0.5) * 0.014;
  var SLEN = 40 + (S.linkDistance || 0.5) * 200;
  var CGRAV = 0.001 + (S.centerForce || 0.5) * 0.02;
  var DAMP = 0.82;
  var alpha = 1.0;

  var C = {
    document:'#2ab4c8', task:'#c86418', board:'#1ab878',
    repo:'#e0a030', link:'#c06090', memory:'#8860d0', communication:'#1a8898'
  };
  var EC = { 'wikilink':'rgba(42,180,200,0.5)', 'task-membership':'rgba(200,100,24,0.5)', 'semantic-support':'rgba(136,96,208,0.5)' };

  var groupHues = {}, hueStep = 0;
  data.nodes.forEach(function(n) {
    if (n.kind === 'document' && n.group && !groupHues[n.group]) {
      groupHues[n.group] = (190 + hueStep * 37) % 360;
      hueStep++;
    }
  });
  function nodeColor(n) {
    if (n.kind !== 'document') return C[n.kind] || '#2ab4c8';
    if (n.group && groupHues[n.group] !== undefined) return 'hsl(' + groupHues[n.group] + ',60%,55%)';
    return C.document;
  }

  var hlSet = {};
  (data.highlight || []).forEach(function(id) { hlSet[id] = true; });

  var nMap = {};
  var nodes = data.nodes.map(function(n) {
    var isSearch = searchTerm && n.title.toLowerCase().indexOf(searchTerm) !== -1;
    var o = {
      id:n.id, kind:n.kind, title:n.title, group:n.group,
      tags:n.tags||[], priority:n.priority||0, status:n.status||'',
      x:W/2+(Math.random()-0.5)*W*0.6, y:H/2+(Math.random()-0.5)*H*0.6,
      vx:0, vy:0, deg:0, r:5,
      hl: !!hlSet[n.id] || isSearch
    };
    nMap[o.id] = o;
    return o;
  });

  var edges = data.edges.filter(function(e){ return nMap[e.source] && nMap[e.target]; });
  edges.forEach(function(e){ nMap[e.source].deg++; nMap[e.target].deg++; });
  nodes.forEach(function(n){ n.r = (4 + Math.min(n.deg, 12) * 1.2) * NODE_SCALE; });

  function tick() {
    var i,j,a,b,dx,dy,d,f,fx,fy;
    for (i=0;i<nodes.length;i++) for (j=i+1;j<nodes.length;j++) {
      a=nodes[i];b=nodes[j]; dx=b.x-a.x;dy=b.y-a.y;d=Math.sqrt(dx*dx+dy*dy)||1;
      f=REP/(d*d); fx=dx/d*f;fy=dy/d*f;
      a.vx-=fx;a.vy-=fy;b.vx+=fx;b.vy+=fy;
    }
    for (i=0;i<edges.length;i++) {
      a=nMap[edges[i].source];b=nMap[edges[i].target];
      if(!a||!b)continue; dx=b.x-a.x;dy=b.y-a.y;d=Math.sqrt(dx*dx+dy*dy)||1;
      f=(d-SLEN)*SPR; fx=dx/d*f;fy=dy/d*f;
      a.vx+=fx;a.vy+=fy;b.vx-=fx;b.vy-=fy;
    }
    for (i=0;i<nodes.length;i++) {
      nodes[i].vx+=(W/2-nodes[i].x)*CGRAV;
      nodes[i].vy+=(H/2-nodes[i].y)*CGRAV;
    }
    for (i=0;i<nodes.length;i++) {
      a=nodes[i];
      if (a._pinned) { a.vx=0; a.vy=0; continue; }
      a.vx*=DAMP;a.vy*=DAMP;
      a.x+=a.vx*alpha; a.y+=a.vy*alpha;
    }
    alpha*=0.997;
  }

  // SVG
  var NS='http://www.w3.org/2000/svg';
  var svg=document.createElementNS(NS,'svg');
  svg.setAttribute('width','100%');svg.setAttribute('height','100%');
  svg.style.display='block'; c.appendChild(svg);

  // Arrow marker defs
  if (ARROWS) {
    var defs=document.createElementNS(NS,'defs');
    var marker=document.createElementNS(NS,'marker');
    marker.setAttribute('id','arrow');marker.setAttribute('viewBox','0 0 10 10');
    marker.setAttribute('refX','10');marker.setAttribute('refY','5');
    marker.setAttribute('markerWidth','6');marker.setAttribute('markerHeight','6');
    marker.setAttribute('orient','auto-start-reverse');
    var path=document.createElementNS(NS,'path');
    path.setAttribute('d','M 0 0 L 10 5 L 0 10 z');
    path.setAttribute('fill','var(--graph-edge)');
    marker.appendChild(path);defs.appendChild(marker);svg.appendChild(defs);
  }

  var g=document.createElementNS(NS,'g');
  svg.appendChild(g);

  var eEls=edges.map(function(e) {
    var l=document.createElementNS(NS,'line');
    l.setAttribute('stroke',EC[e.kind]||'var(--graph-edge)');
    l.setAttribute('stroke-width',LINK_W);
    l.setAttribute('stroke-opacity','0.4');
    if (ARROWS) l.setAttribute('marker-end','url(#arrow)');
    g.appendChild(l);
    return {el:l,s:e.source,t:e.target,kind:e.kind};
  });

  var nEls=nodes.map(function(n) {
    var gr=document.createElementNS(NS,'g');
    gr.style.cursor=n.kind==='document'?'pointer':'default';

    var ci=document.createElementNS(NS,'circle');
    ci.setAttribute('r',n.hl?n.r*1.5:n.r);
    ci.setAttribute('fill',nodeColor(n));
    ci.setAttribute('fill-opacity',n.hl?'1':searchTerm&&!n.hl?'0.25':'0.85');
    ci.setAttribute('stroke',n.hl?'#fff':nodeColor(n));
    ci.setAttribute('stroke-width',n.hl?'2.5':'1.5');
    ci.setAttribute('stroke-opacity',n.hl?'1':'0.3');
    gr.appendChild(ci);

    // Labels: show based on degree and text fade threshold
    var showLabel = n.hl || n.deg >= Math.max(1, Math.round((1-TEXT_FADE)*8)) || nodes.length < 30;
    if (showLabel) {
      var tx=document.createElementNS(NS,'text');
      var label=n.title.length>22?n.title.slice(0,20)+'\u2026':n.title;
      tx.textContent=label;
      tx.setAttribute('dx',(n.hl?n.r*1.5:n.r)+4);
      tx.setAttribute('dy',4);
      tx.setAttribute('fill',n.hl?'#fff':'var(--graph-label)');
      tx.setAttribute('font-size',n.hl?'12':'10');
      tx.setAttribute('font-weight',n.hl?'600':'400');
      tx.setAttribute('font-family','-apple-system,BlinkMacSystemFont,sans-serif');
      tx.setAttribute('pointer-events','none');
      gr.appendChild(tx);
    }

    gr.addEventListener('click',function(evt){
      evt.stopPropagation();
      if(didDrag){didDrag=false;return;}
      if(n.kind==='document') dioxus.send(n.id);
    });

    gr.addEventListener('mouseenter',function(){
      ci.setAttribute('fill-opacity','1');
      ci.setAttribute('stroke-opacity','1');
      ci.setAttribute('r',n.r*1.4);
      eEls.forEach(function(ee){
        if(ee.s===n.id||ee.t===n.id){
          ee.el.setAttribute('stroke',EC[ee.kind]||'var(--graph-edge-active)');
          ee.el.setAttribute('stroke-opacity','1');
          ee.el.setAttribute('stroke-width',LINK_W*2);
        }
      });
      var tip=document.getElementById('graph-tooltip');
      if(tip){
        var parts=[n.title,n.kind];
        if(n.tags&&n.tags.length)parts.push('#'+n.tags.join(' #'));
        if(n.status)parts.push(n.status);
        parts.push(n.deg+' links');
        tip.textContent=parts.join(' \xb7 ');
        tip.style.opacity='1';
      }
    });
    gr.addEventListener('mouseleave',function(){
      ci.setAttribute('fill-opacity',n.hl?'1':searchTerm&&!n.hl?'0.25':'0.85');
      ci.setAttribute('stroke-opacity',n.hl?'1':'0.3');
      ci.setAttribute('r',n.hl?n.r*1.5:n.r);
      eEls.forEach(function(ee){
        ee.el.setAttribute('stroke',EC[ee.kind]||'var(--graph-edge)');
        ee.el.setAttribute('stroke-opacity','0.4');
        ee.el.setAttribute('stroke-width',LINK_W);
      });
      var tip=document.getElementById('graph-tooltip');
      if(tip)tip.style.opacity='0';
    });

    g.appendChild(gr);
    return {el:gr,n:n,ci:ci};
  });

  // Zoom & pan
  var zm=1,px=0,py=0;
  function upd(){g.setAttribute('transform','translate('+px+','+py+') scale('+zm+')');}

  svg.addEventListener('wheel',function(e){
    e.preventDefault();
    var rect=svg.getBoundingClientRect();
    var mx=e.clientX-rect.left,my=e.clientY-rect.top;
    var delta=-e.deltaY;
    if(e.deltaMode===1)delta*=16;
    delta=Math.max(-30,Math.min(30,delta));
    var f=1+delta*0.003;
    var nz=Math.max(0.1,Math.min(8,zm*f));
    px=mx-(mx-px)*nz/zm; py=my-(my-py)*nz/zm;
    zm=nz; upd();
  },{passive:false});

  var panning=false,psx=0,psy=0;
  svg.addEventListener('mousedown',function(e){
    if(e.target===svg){panning=true;psx=e.clientX-px;psy=e.clientY-py;}
  });
  var dragging=null, didDrag=false;
  nEls.forEach(function(ne){
    ne.el.addEventListener('mousedown',function(e){
      e.stopPropagation();
      dragging=ne.n;
      dragging._pinned=true;
      didDrag=false;
    });
  });
  svg.addEventListener('mousemove',function(e){
    if(panning){px=e.clientX-psx;py=e.clientY-psy;upd();}
    if(dragging){
      didDrag=true;
      var rect=svg.getBoundingClientRect();
      dragging.x=(e.clientX-rect.left-px)/zm;
      dragging.y=(e.clientY-rect.top-py)/zm;
      dragging.vx=0;dragging.vy=0;
      alpha=Math.max(alpha,0.3);
    }
  });
  svg.addEventListener('mouseup',function(){
    if(dragging) dragging._pinned=false;
    panning=false;dragging=null;
  });
  svg.addEventListener('mouseleave',function(){
    if(dragging) dragging._pinned=false;
    panning=false;dragging=null;
  });

  function render(){
    if(alpha>0.001)tick();
    for(var i=0;i<eEls.length;i++){
      var s=nMap[eEls[i].s],t=nMap[eEls[i].t];
      if(s&&t){
        // For arrows, shorten the line to end at the node border
        if(ARROWS){
          var dx=t.x-s.x,dy=t.y-s.y,d=Math.sqrt(dx*dx+dy*dy)||1;
          eEls[i].el.setAttribute('x1',s.x+dx/d*s.r);
          eEls[i].el.setAttribute('y1',s.y+dy/d*s.r);
          eEls[i].el.setAttribute('x2',t.x-dx/d*t.r);
          eEls[i].el.setAttribute('y2',t.y-dy/d*t.r);
        } else {
          eEls[i].el.setAttribute('x1',s.x);eEls[i].el.setAttribute('y1',s.y);
          eEls[i].el.setAttribute('x2',t.x);eEls[i].el.setAttribute('y2',t.y);
        }
      }
    }
    for(var j=0;j<nEls.length;j++){
      nEls[j].el.setAttribute('transform','translate('+nEls[j].n.x+','+nEls[j].n.y+')');
    }
    window._codexGraph.raf=requestAnimationFrame(render);
  }
  window._codexGraph={raf:0};
  render();
}
"#;
