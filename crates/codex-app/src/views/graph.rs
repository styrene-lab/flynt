use crate::{bootstrap::AppContext, state::{Route, TabState}};
use codex_core::{
    graph::{build_graph_payload, GraphEdgeKind, GraphNodeKind, GraphPayload},
    models::DocumentId,
    store::VaultStore,
};
use dioxus::prelude::*;
use std::str::FromStr;

#[component]
pub fn GraphView() -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();
    let mut selected_group = use_signal(|| Option::<String>::None);
    let mut selected_kind = use_signal(|| Option::<GraphNodeKind>::None);

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

    // Derive filtered JSON from graph + filters
    let graph_json = use_memo(move || {
        let binding = graph.read();
        let Some(Some(payload)) = &*binding else {
            return String::new();
        };
        let (nodes, edges) = filter_graph(payload, &selected_kind.read(), &selected_group.read());
        graph_to_json(&nodes, &edges)
    });
    use_effect(move || {
        let json = graph_json.read().clone();
        if json.is_empty() {
            return;
        }
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

    rsx! {
        div { class: "view-graph",
            match &*graph.read() {
                None => rsx! {
                    div { class: "graph-chrome",
                        p { class: "placeholder", "Loading knowledge graph…" }
                    }
                },
                Some(Some(payload)) => {
                    let (filtered_nodes, filtered_edges) =
                        filter_graph(payload, &selected_kind.read(), &selected_group.read());

                    rsx! {
                        div { class: "graph-chrome",
                            div { class: "graph-stats",
                                "{filtered_nodes.len()} nodes · {filtered_edges.len()} links"
                            }
                            div { class: "graph-filters",
                                button {
                                    class: if selected_kind.read().is_none() { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                    onclick: move |_| *selected_kind.write() = None,
                                    "All"
                                }
                                for kind in [GraphNodeKind::Document, GraphNodeKind::Communication, GraphNodeKind::MemoryFact, GraphNodeKind::Task, GraphNodeKind::Board] {
                                    button {
                                        class: if selected_kind.read().as_ref() == Some(&kind) { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                        onclick: move |_| *selected_kind.write() = Some(kind.clone()),
                                        "{format_node_kind(&kind)}"
                                    }
                                }
                            }
                            if !payload.groups.is_empty() {
                                div { class: "graph-filters",
                                    button {
                                        class: if selected_group.read().is_none() { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                        onclick: move |_| *selected_group.write() = None,
                                        "All groups"
                                    }
                                    for group in &payload.groups {
                                        button {
                                            class: if selected_group.read().as_ref() == Some(group) { "btn btn-primary btn-xs" } else { "btn btn-ghost btn-xs" },
                                            onclick: {
                                                let group = group.clone();
                                                move |_| *selected_group.write() = Some(group.clone())
                                            },
                                            "{group}"
                                        }
                                    }
                                }
                            }
                        }
                        div {
                            id: "graph-canvas",
                            class: "graph-canvas",
                        }
                        div {
                            id: "graph-tooltip",
                            class: "graph-tooltip",
                        }
                    }
                },
                Some(None) => rsx! {
                    div { class: "graph-chrome",
                        p { class: "placeholder", "Graph unavailable." }
                    }
                },
            }
        }
    }
}

fn filter_graph<'a>(
    payload: &'a GraphPayload,
    kind_filter: &Option<GraphNodeKind>,
    group_filter: &Option<String>,
) -> (Vec<&'a codex_core::graph::GraphNode>, Vec<&'a codex_core::graph::GraphEdge>) {
    let nodes: Vec<_> = payload.nodes.iter().filter(|n| {
        kind_filter.as_ref().map(|k| &n.kind == k).unwrap_or(true)
            && group_filter.as_ref().map(|g| &n.group == g).unwrap_or(true)
    }).collect();
    let ids: std::collections::HashSet<_> = nodes.iter().map(|n| &n.id).collect();
    let edges: Vec<_> = payload.edges.iter().filter(|e| {
        ids.contains(&e.source) && ids.contains(&e.target)
    }).collect();
    (nodes, edges)
}

fn graph_to_json(
    nodes: &[&codex_core::graph::GraphNode],
    edges: &[&codex_core::graph::GraphEdge],
) -> String {
    let nodes_json: Vec<String> = nodes.iter().map(|n| {
        format!(
            r#"{{"id":"{}","kind":"{}","title":"{}","group":"{}"}}"#,
            escape_json(&n.id),
            format_node_kind(&n.kind),
            escape_json(&n.title),
            escape_json(&n.group),
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
    format!(
        r#"{{"nodes":[{}],"edges":[{}]}}"#,
        nodes_json.join(","),
        edges_json.join(","),
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

const GRAPH_JS: &str = r#"
function codexGraph(data) {
  if (window._codexGraph) cancelAnimationFrame(window._codexGraph.raf);

  var c = document.getElementById('graph-canvas');
  if (!c || !data.nodes.length) return;
  c.innerHTML = '';

  var W = c.clientWidth || 800, H = c.clientHeight || 600;

  // Colors per kind
  var C = {
    document:      '#2ab4c8',
    task:          '#c86418',
    board:         '#1ab878',
    memory:        '#8860d0',
    communication: '#1a8898'
  };

  // Build node objects
  var nMap = {};
  var nodes = data.nodes.map(function(n) {
    var o = {
      id: n.id, kind: n.kind, title: n.title, group: n.group,
      x: W/2 + (Math.random()-0.5)*W*0.6,
      y: H/2 + (Math.random()-0.5)*H*0.6,
      vx: 0, vy: 0, deg: 0, r: 5
    };
    nMap[o.id] = o;
    return o;
  });

  var edges = data.edges.filter(function(e){ return nMap[e.source] && nMap[e.target]; });
  edges.forEach(function(e){ nMap[e.source].deg++; nMap[e.target].deg++; });
  nodes.forEach(function(n){ n.r = 4 + Math.min(n.deg, 12) * 1.2; });

  // Physics
  var REP = 1200, SPR = 0.006, SLEN = 100, CGRAV = 0.008, DAMP = 0.82;
  var alpha = 1.0;

  function tick() {
    var i, j, a, b, dx, dy, d, f, fx, fy;
    for (i = 0; i < nodes.length; i++) {
      for (j = i+1; j < nodes.length; j++) {
        a = nodes[i]; b = nodes[j];
        dx = b.x - a.x; dy = b.y - a.y;
        d = Math.sqrt(dx*dx + dy*dy) || 1;
        f = REP / (d * d);
        fx = dx/d * f; fy = dy/d * f;
        a.vx -= fx; a.vy -= fy;
        b.vx += fx; b.vy += fy;
      }
    }
    for (i = 0; i < edges.length; i++) {
      a = nMap[edges[i].source]; b = nMap[edges[i].target];
      if (!a || !b) continue;
      dx = b.x - a.x; dy = b.y - a.y;
      d = Math.sqrt(dx*dx + dy*dy) || 1;
      f = (d - SLEN) * SPR;
      fx = dx/d * f; fy = dy/d * f;
      a.vx += fx; a.vy += fy;
      b.vx -= fx; b.vy -= fy;
    }
    for (i = 0; i < nodes.length; i++) {
      nodes[i].vx += (W/2 - nodes[i].x) * CGRAV;
      nodes[i].vy += (H/2 - nodes[i].y) * CGRAV;
    }
    for (i = 0; i < nodes.length; i++) {
      a = nodes[i];
      a.vx *= DAMP; a.vy *= DAMP;
      a.x += a.vx * alpha;
      a.y += a.vy * alpha;
    }
    alpha *= 0.997;
  }

  // SVG
  var NS = 'http://www.w3.org/2000/svg';
  var svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('width', '100%');
  svg.setAttribute('height', '100%');
  svg.style.display = 'block';
  c.appendChild(svg);

  var g = document.createElementNS(NS, 'g');
  svg.appendChild(g);

  // Edges
  var eEls = edges.map(function(e) {
    var l = document.createElementNS(NS, 'line');
    l.setAttribute('stroke', 'var(--graph-edge)');
    l.setAttribute('stroke-width', '1');
    l.setAttribute('stroke-opacity', '0.5');
    g.appendChild(l);
    return { el: l, s: e.source, t: e.target };
  });

  // Nodes
  var nEls = nodes.map(function(n) {
    var gr = document.createElementNS(NS, 'g');
    gr.style.cursor = n.kind === 'document' ? 'pointer' : 'default';

    var ci = document.createElementNS(NS, 'circle');
    ci.setAttribute('r', n.r);
    ci.setAttribute('fill', C[n.kind] || '#2ab4c8');
    ci.setAttribute('fill-opacity', '0.85');
    ci.setAttribute('stroke', C[n.kind] || '#2ab4c8');
    ci.setAttribute('stroke-width', '1.5');
    ci.setAttribute('stroke-opacity', '0.3');
    gr.appendChild(ci);

    if (n.deg >= 2 || nodes.length < 40) {
      var tx = document.createElementNS(NS, 'text');
      var label = n.title.length > 22 ? n.title.slice(0,20) + '\u2026' : n.title;
      tx.textContent = label;
      tx.setAttribute('dx', n.r + 4);
      tx.setAttribute('dy', 4);
      tx.setAttribute('fill', 'var(--graph-label)');
      tx.setAttribute('font-size', '10');
      tx.setAttribute('font-family', '-apple-system, BlinkMacSystemFont, sans-serif');
      tx.setAttribute('pointer-events', 'none');
      gr.appendChild(tx);
    }

    gr.addEventListener('click', function(evt) {
      evt.stopPropagation();
      if (n.kind === 'document') {
        dioxus.send(n.id);
      }
    });

    gr.addEventListener('mouseenter', function() {
      ci.setAttribute('fill-opacity', '1');
      ci.setAttribute('stroke-opacity', '1');
      ci.setAttribute('r', n.r * 1.4);
      eEls.forEach(function(ee) {
        if (ee.s === n.id || ee.t === n.id) {
          ee.el.setAttribute('stroke', 'var(--graph-edge-active)');
          ee.el.setAttribute('stroke-opacity', '1');
          ee.el.setAttribute('stroke-width', '2');
        }
      });
      var tip = document.getElementById('graph-tooltip');
      if (tip) { tip.textContent = n.title + ' \xb7 ' + n.kind; tip.style.opacity = '1'; }
    });
    gr.addEventListener('mouseleave', function() {
      ci.setAttribute('fill-opacity', '0.85');
      ci.setAttribute('stroke-opacity', '0.3');
      ci.setAttribute('r', n.r);
      eEls.forEach(function(ee) {
        ee.el.setAttribute('stroke', 'var(--graph-edge)');
        ee.el.setAttribute('stroke-opacity', '0.5');
        ee.el.setAttribute('stroke-width', '1');
      });
      var tip = document.getElementById('graph-tooltip');
      if (tip) tip.style.opacity = '0';
    });

    g.appendChild(gr);
    return { el: gr, n: n, ci: ci };
  });

  // Zoom & pan
  var zm = 1, px = 0, py = 0;
  function upd() { g.setAttribute('transform', 'translate('+px+','+py+') scale('+zm+')'); }

  svg.addEventListener('wheel', function(e) {
    e.preventDefault();
    var rect = svg.getBoundingClientRect();
    var mx = e.clientX - rect.left, my = e.clientY - rect.top;
    var f = e.deltaY > 0 ? 0.9 : 1.1;
    var nz = Math.max(0.1, Math.min(8, zm * f));
    px = mx - (mx - px) * nz / zm;
    py = my - (my - py) * nz / zm;
    zm = nz;
    upd();
  }, { passive: false });

  var panning = false, psx = 0, psy = 0;
  svg.addEventListener('mousedown', function(e) {
    if (e.target === svg) { panning = true; psx = e.clientX - px; psy = e.clientY - py; }
  });

  // Node drag
  var dragging = null;
  nEls.forEach(function(ne) {
    ne.el.addEventListener('mousedown', function(e) {
      e.stopPropagation();
      dragging = ne.n;
    });
  });

  svg.addEventListener('mousemove', function(e) {
    if (panning) { px = e.clientX - psx; py = e.clientY - psy; upd(); }
    if (dragging) {
      var rect = svg.getBoundingClientRect();
      dragging.x = (e.clientX - rect.left - px) / zm;
      dragging.y = (e.clientY - rect.top - py) / zm;
      dragging.vx = 0; dragging.vy = 0;
      alpha = Math.max(alpha, 0.3);
    }
  });
  svg.addEventListener('mouseup', function() { panning = false; dragging = null; });
  svg.addEventListener('mouseleave', function() { panning = false; dragging = null; });

  // Render loop
  function render() {
    if (alpha > 0.001) tick();
    for (var i = 0; i < eEls.length; i++) {
      var s = nMap[eEls[i].s], t = nMap[eEls[i].t];
      if (s && t) {
        eEls[i].el.setAttribute('x1', s.x);
        eEls[i].el.setAttribute('y1', s.y);
        eEls[i].el.setAttribute('x2', t.x);
        eEls[i].el.setAttribute('y2', t.y);
      }
    }
    for (var j = 0; j < nEls.length; j++) {
      nEls[j].el.setAttribute('transform', 'translate('+nEls[j].n.x+','+nEls[j].n.y+')');
    }
    window._codexGraph.raf = requestAnimationFrame(render);
  }

  window._codexGraph = { raf: 0 };
  render();
}
"#;
