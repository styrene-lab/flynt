use codex_core::graph::{build_graph_payload, GraphEdgeKind, GraphNodeKind, GraphPayload};
use dioxus::prelude::*;
use crate::bootstrap::MobileRuntime;
use std::collections::HashMap;

#[derive(Clone, PartialEq)]
struct GraphSettings {
    kind: Option<GraphNodeKind>,
    show_orphans: bool,
    show_wikilinks: bool,
    show_task_links: bool,
    show_semantic: bool,
    min_degree: u32,
    node_size: f32,
    repel_force: f32,
    link_force: f32,
    link_distance: f32,
    center_force: f32,
}

impl Default for GraphSettings {
    fn default() -> Self {
        Self {
            kind: None, show_orphans: true,
            show_wikilinks: true, show_task_links: true, show_semantic: true,
            min_degree: 0, node_size: 1.0,
            repel_force: 0.5, link_force: 0.5, link_distance: 0.5, center_force: 0.5,
        }
    }
}

fn filter_and_serialize(payload: &GraphPayload, s: &GraphSettings) -> String {
    let mut degree: HashMap<&str, u32> = HashMap::new();
    for e in &payload.edges { *degree.entry(&e.source).or_default() += 1; *degree.entry(&e.target).or_default() += 1; }

    let nodes: Vec<_> = payload.nodes.iter().filter(|n| {
        if let Some(ref k) = s.kind { if &n.kind != k { return false; } }
        if !s.show_orphans && degree.get(n.id.as_str()).copied().unwrap_or(0) == 0 { return false; }
        if s.min_degree > 0 && degree.get(n.id.as_str()).copied().unwrap_or(0) < s.min_degree { return false; }
        true
    }).collect();

    let ids: std::collections::HashSet<_> = nodes.iter().map(|n| n.id.as_str()).collect();
    let edges: Vec<_> = payload.edges.iter().filter(|e| {
        if !ids.contains(e.source.as_str()) || !ids.contains(e.target.as_str()) { return false; }
        match e.kind {
            GraphEdgeKind::Wikilink => s.show_wikilinks,
            GraphEdgeKind::TaskMembership => s.show_task_links,
            GraphEdgeKind::SemanticSupport => s.show_semantic,
        }
    }).collect();

    let nj: Vec<String> = nodes.iter().map(|n| format!(
        r#"{{"id":"{}","kind":"{}","title":"{}","group":"{}"}}"#,
        esc(&n.id), kind_str(&n.kind), esc(&n.title), esc(&n.group)
    )).collect();
    let ej: Vec<String> = edges.iter().map(|e| format!(
        r#"{{"source":"{}","target":"{}","kind":"{}"}}"#, esc(&e.source), esc(&e.target), edge_str(&e.kind)
    )).collect();
    format!(
        r#"{{"nodes":[{}],"edges":[{}],"settings":{{"nodeSize":{},"linkThickness":1.0,"textFade":0.5,"arrows":false,"centerForce":{},"repelForce":{},"linkForce":{},"linkDistance":{}}}}}"#,
        nj.join(","), ej.join(","), s.node_size, s.center_force, s.repel_force, s.link_force, s.link_distance
    )
}

fn esc(s: &str) -> String { s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n") }
fn kind_str(k: &GraphNodeKind) -> &'static str {
    match k { GraphNodeKind::Document=>"document", GraphNodeKind::Task=>"task", GraphNodeKind::Board=>"board",
              GraphNodeKind::Repo=>"repo", GraphNodeKind::Link=>"link", GraphNodeKind::MemoryFact=>"memory", GraphNodeKind::Communication=>"communication" }
}
fn edge_str(k: &GraphEdgeKind) -> &'static str {
    match k { GraphEdgeKind::Wikilink=>"wikilink", GraphEdgeKind::TaskMembership=>"task-membership", GraphEdgeKind::SemanticSupport=>"semantic-support" }
}
fn kind_label(k: &GraphNodeKind) -> &'static str {
    match k { GraphNodeKind::Document=>"Doc", GraphNodeKind::Task=>"Task", GraphNodeKind::Board=>"Board",
              GraphNodeKind::Repo=>"Repo", GraphNodeKind::Link=>"Link", GraphNodeKind::MemoryFact=>"Memory", GraphNodeKind::Communication=>"Comms" }
}

#[component]
pub fn GraphView() -> Element {
    let rt = use_context::<Signal<MobileRuntime>>();
    let mut settings = use_signal(GraphSettings::default);
    let mut panel_open = use_signal(|| false);
    let mut controls_open = use_signal(|| false);

    let payload = use_memo(move || {
        build_graph_payload(&*rt.read().vault.store).ok()
    });

    // Init graph once with full data
    let mut graph_inited = use_signal(|| false);
    use_effect(move || {
        if *graph_inited.read() { return; }
        if let Some(ref p) = *payload.read() {
            if p.nodes.is_empty() { return; }
            let s = settings.read().clone();
            let json = filter_and_serialize(p, &s);
            let js = format!("{GRAPH_JS}\ncodexGraph({json});");
            document::eval(&js);
            *graph_inited.write() = true;
        }
    });

    // Push settings updates to live simulation (no rebuild)
    use_effect(move || {
        let s = settings.read().clone();
        if !*graph_inited.read() { return; }
        if let Some(ref p) = *payload.read() {
            let json = filter_and_serialize(p, &s);
            let js = format!("window._codexGraphUpdate&&window._codexGraphUpdate({json});");
            document::eval(&js);
        }
    });

    if payload.read().as_ref().map(|p| p.nodes.is_empty()).unwrap_or(true) {
        return rsx! {
            div { class: "graph-mobile-empty",
                p { class: "muted", "No graph data." }
            }
        };
    }

    let s = settings.read().clone();

    rsx! {
        div { class: "graph-mobile",
            div { id: "graph-canvas", class: "graph-canvas-mobile" }

            // Floating controls
            div { class: "graph-fab-group",
                if *controls_open.read() {
                    button { class: "graph-fab-btn", onclick: move |_| { document::eval("window._codexGraphZoom&&window._codexGraphZoom(1.5)"); }, "+" }
                    button { class: "graph-fab-btn", onclick: move |_| { document::eval("window._codexGraphZoom&&window._codexGraphZoom(0.67)"); }, "−" }
                    button { class: "graph-fab-btn", onclick: move |_| { document::eval("window._codexGraphReset&&window._codexGraphReset()"); }, "⊙" }
                    button { class: "graph-fab-btn", onclick: move |_| { document::eval("window._codexGraphReheat&&window._codexGraphReheat()"); }, "↻" }
                    button { class: "graph-fab-btn", onclick: move |_| { let v = *panel_open.read(); *panel_open.write() = !v; }, "☰" }
                }
                button {
                    class: if *controls_open.read() { "graph-fab active" } else { "graph-fab" },
                    onclick: move |_| { let v = *controls_open.read(); *controls_open.write() = !v; },
                    if *controls_open.read() { "✕" } else { "⚙" }
                }
            }

            // Settings sheet
            if *panel_open.read() {
                div { class: "graph-sheet-overlay", onclick: move |_| *panel_open.write() = false }
                div { class: "graph-sheet",
                    div { class: "graph-sheet-handle" }

                    // Kind filter
                    div { class: "gs-section",
                        div { class: "gs-heading", "Filter by type" }
                        div { class: "gs-chips",
                            button {
                                class: if s.kind.is_none() { "gs-chip active" } else { "gs-chip" },
                                onclick: move |_| settings.write().kind = None,
                                "All"
                            }
                            for kind in [GraphNodeKind::Document, GraphNodeKind::Task, GraphNodeKind::Board, GraphNodeKind::Repo, GraphNodeKind::MemoryFact, GraphNodeKind::Communication] {
                                {
                                    let label = kind_label(&kind);
                                    rsx! {
                                        button {
                                            class: if s.kind.as_ref() == Some(&kind) { "gs-chip active" } else { "gs-chip" },
                                            onclick: { let k = kind.clone(); move |_| settings.write().kind = Some(k.clone()) },
                                            "{label}"
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Edge toggles
                    div { class: "gs-section",
                        div { class: "gs-heading", "Edges" }
                        div { class: "gs-toggle-row",
                            span { "Wikilinks" }
                            button {
                                class: if s.show_wikilinks { "gs-toggle on" } else { "gs-toggle" },
                                onclick: move |_| { let v = settings.read().show_wikilinks; settings.write().show_wikilinks = !v; },
                                if s.show_wikilinks { "ON" } else { "OFF" }
                            }
                        }
                        div { class: "gs-toggle-row",
                            span { "Task links" }
                            button {
                                class: if s.show_task_links { "gs-toggle on" } else { "gs-toggle" },
                                onclick: move |_| { let v = settings.read().show_task_links; settings.write().show_task_links = !v; },
                                if s.show_task_links { "ON" } else { "OFF" }
                            }
                        }
                        div { class: "gs-toggle-row",
                            span { "Semantic" }
                            button {
                                class: if s.show_semantic { "gs-toggle on" } else { "gs-toggle" },
                                onclick: move |_| { let v = settings.read().show_semantic; settings.write().show_semantic = !v; },
                                if s.show_semantic { "ON" } else { "OFF" }
                            }
                        }
                        div { class: "gs-toggle-row",
                            span { "Orphans" }
                            button {
                                class: if s.show_orphans { "gs-toggle on" } else { "gs-toggle" },
                                onclick: move |_| { let v = settings.read().show_orphans; settings.write().show_orphans = !v; },
                                if s.show_orphans { "ON" } else { "OFF" }
                            }
                        }
                    }

                    // Display
                    div { class: "gs-section",
                        div { class: "gs-heading", "Display" }
                        div { class: "gs-slider-row",
                            span { "Node size" }
                            input { r#type: "range", min: "0.3", max: "3.0", step: "0.1", value: "{s.node_size}",
                                oninput: move |e| { settings.write().node_size = e.value().parse().unwrap_or(1.0); },
                            }
                        }
                        div { class: "gs-slider-row",
                            span { "Min links" }
                            input { r#type: "range", min: "0", max: "10", step: "1", value: "{s.min_degree}",
                                oninput: move |e| { settings.write().min_degree = e.value().parse().unwrap_or(0); },
                            }
                            span { class: "gs-val", "{s.min_degree}" }
                        }
                    }

                    // Physics
                    div { class: "gs-section",
                        div { class: "gs-heading", "Physics" }
                        div { class: "gs-slider-row",
                            span { "Center" }
                            input { r#type: "range", min: "0", max: "1.0", step: "0.05", value: "{s.center_force}",
                                oninput: move |e| { settings.write().center_force = e.value().parse().unwrap_or(0.5); },
                            }
                        }
                        div { class: "gs-slider-row",
                            span { "Repel" }
                            input { r#type: "range", min: "0", max: "1.0", step: "0.05", value: "{s.repel_force}",
                                oninput: move |e| { settings.write().repel_force = e.value().parse().unwrap_or(0.5); },
                            }
                        }
                        div { class: "gs-slider-row",
                            span { "Link pull" }
                            input { r#type: "range", min: "0", max: "1.0", step: "0.05", value: "{s.link_force}",
                                oninput: move |e| { settings.write().link_force = e.value().parse().unwrap_or(0.5); },
                            }
                        }
                        div { class: "gs-slider-row",
                            span { "Link dist" }
                            input { r#type: "range", min: "0", max: "1.0", step: "0.05", value: "{s.link_distance}",
                                oninput: move |e| { settings.write().link_distance = e.value().parse().unwrap_or(0.5); },
                            }
                        }
                    }

                    button {
                        class: "gs-reset",
                        onclick: move |_| *settings.write() = GraphSettings::default(),
                        "Restore defaults"
                    }
                }
            }
        }
    }
}

const GRAPH_JS: &str = r#"
function codexGraph(data) {
  if (window._codexGraph) cancelAnimationFrame(window._codexGraph.raf);
  var c = document.getElementById('graph-canvas');
  if (!c || !data.nodes.length) return;
  c.innerHTML = '';

  var W = c.clientWidth || 380, H = c.clientHeight || 600;
  var S = data.settings || {};
  var NODE_SCALE = S.nodeSize || 1.0;
  var LINK_W = S.linkThickness || 1.0;
  var TEXT_FADE = S.textFade || 0.5;

  var REP = 400 + (S.repelForce || 0.5) * 2400;
  var SPR = 0.001 + (S.linkForce || 0.5) * 0.014;
  var SLEN = 40 + (S.linkDistance || 0.5) * 200;
  var CGRAV = 0.001 + (S.centerForce || 0.5) * 0.02;
  var DAMP = 0.82, alpha = 1.0;

  var C = { document:'#2ab4c8', task:'#c86418', board:'#1ab878', repo:'#e0a030', link:'#c06090', memory:'#8860d0', communication:'#1a8898' };
  var EC = { 'wikilink':'rgba(42,180,200,0.5)', 'task-membership':'rgba(200,100,24,0.5)', 'semantic-support':'rgba(136,96,208,0.5)' };

  var groupHues={},hueStep=0;
  data.nodes.forEach(function(n){ if(n.kind==='document'&&n.group&&!groupHues[n.group]){groupHues[n.group]=(190+hueStep*37)%360;hueStep++;} });
  function nodeColor(n){ if(n.kind!=='document')return C[n.kind]||'#2ab4c8'; if(n.group&&groupHues[n.group]!==undefined)return'hsl('+groupHues[n.group]+',60%,55%)'; return C.document; }

  var nMap={};
  var nodes=data.nodes.map(function(n){ var o={id:n.id,kind:n.kind,title:n.title,group:n.group,x:W/2+(Math.random()-0.5)*W*0.6,y:H/2+(Math.random()-0.5)*H*0.6,vx:0,vy:0,deg:0,r:5}; nMap[o.id]=o; return o; });
  var edges=data.edges.filter(function(e){return nMap[e.source]&&nMap[e.target];});
  edges.forEach(function(e){nMap[e.source].deg++;nMap[e.target].deg++;});
  nodes.forEach(function(n){n.r=(4+Math.min(n.deg,12)*1.2)*NODE_SCALE;});

  function tick(){
    var i,j,a,b,dx,dy,d,f,fx,fy;
    for(i=0;i<nodes.length;i++)for(j=i+1;j<nodes.length;j++){a=nodes[i];b=nodes[j];dx=b.x-a.x;dy=b.y-a.y;d=Math.sqrt(dx*dx+dy*dy)||1;f=REP/(d*d);fx=dx/d*f;fy=dy/d*f;a.vx-=fx;a.vy-=fy;b.vx+=fx;b.vy+=fy;}
    for(i=0;i<edges.length;i++){a=nMap[edges[i].source];b=nMap[edges[i].target];if(!a||!b)continue;dx=b.x-a.x;dy=b.y-a.y;d=Math.sqrt(dx*dx+dy*dy)||1;f=(d-SLEN)*SPR;fx=dx/d*f;fy=dy/d*f;a.vx+=fx;a.vy+=fy;b.vx-=fx;b.vy-=fy;}
    for(i=0;i<nodes.length;i++){nodes[i].vx+=(W/2-nodes[i].x)*CGRAV;nodes[i].vy+=(H/2-nodes[i].y)*CGRAV;}
    for(i=0;i<nodes.length;i++){a=nodes[i];if(a._pinned){a.vx=0;a.vy=0;continue;}a.vx*=DAMP;a.vy*=DAMP;a.x+=a.vx*alpha;a.y+=a.vy*alpha;}
    alpha*=0.997;
  }

  var NS='http://www.w3.org/2000/svg';
  var svg=document.createElementNS(NS,'svg');
  svg.setAttribute('width','100%');svg.setAttribute('height','100%');
  svg.style.display='block';svg.style.touchAction='none';c.appendChild(svg);
  var g=document.createElementNS(NS,'g');svg.appendChild(g);

  var eEls=edges.map(function(e){var l=document.createElementNS(NS,'line');l.setAttribute('stroke',EC[e.kind]||'rgba(45,49,64,0.5)');l.setAttribute('stroke-width',LINK_W);l.setAttribute('stroke-opacity','0.4');g.appendChild(l);return{el:l,s:e.source,t:e.target};});
  var nEls=nodes.map(function(n){
    var gr=document.createElementNS(NS,'g');
    var ci=document.createElementNS(NS,'circle');ci.setAttribute('r',n.r);ci.setAttribute('fill',nodeColor(n));ci.setAttribute('fill-opacity','0.85');ci.setAttribute('stroke',nodeColor(n));ci.setAttribute('stroke-width','1.5');ci.setAttribute('stroke-opacity','0.3');gr.appendChild(ci);
    var showLabel=n.deg>=Math.max(1,Math.round((1-TEXT_FADE)*8))||nodes.length<30;
    if(showLabel){var tx=document.createElementNS(NS,'text');tx.textContent=n.title.length>18?n.title.slice(0,16)+'\u2026':n.title;tx.setAttribute('dx',n.r+3);tx.setAttribute('dy',3);tx.setAttribute('fill','#71717a');tx.setAttribute('font-size','9');tx.setAttribute('font-family','-apple-system,system-ui,sans-serif');tx.setAttribute('pointer-events','none');gr.appendChild(tx);}
    g.appendChild(gr);return{el:gr,n:n,ci:ci};
  });

  var zm=1,px=0,py=0;
  function upd(){g.setAttribute('transform','translate('+px+','+py+') scale('+zm+')');}

  // Pinch zoom
  var lastDist=0;
  svg.addEventListener('touchstart',function(e){if(e.touches.length===2){e.preventDefault();var t=e.touches;lastDist=Math.hypot(t[0].clientX-t[1].clientX,t[0].clientY-t[1].clientY);}},{passive:false});
  svg.addEventListener('touchmove',function(e){if(e.touches.length===2){e.preventDefault();var t=e.touches;var dist=Math.hypot(t[0].clientX-t[1].clientX,t[0].clientY-t[1].clientY);var f=dist/lastDist;var nz=Math.max(0.2,Math.min(6,zm*f));var mx=(t[0].clientX+t[1].clientX)/2;var my=(t[0].clientY+t[1].clientY)/2;var rect=svg.getBoundingClientRect();mx-=rect.left;my-=rect.top;px=mx-(mx-px)*nz/zm;py=my-(my-py)*nz/zm;zm=nz;lastDist=dist;upd();}},{passive:false});
  // Wheel zoom
  svg.addEventListener('wheel',function(e){e.preventDefault();var rect=svg.getBoundingClientRect();var mx=e.clientX-rect.left,my=e.clientY-rect.top;var delta=Math.max(-30,Math.min(30,-e.deltaY));var f=1+delta*0.003;var nz=Math.max(0.2,Math.min(6,zm*f));px=mx-(mx-px)*nz/zm;py=my-(my-py)*nz/zm;zm=nz;upd();},{passive:false});

  // Drag nodes (touch + mouse)
  var dragging=null;
  nEls.forEach(function(ne){
    ne.el.addEventListener('touchstart',function(e){if(e.touches.length!==1)return;e.stopPropagation();dragging=ne.n;dragging._pinned=true;alpha=Math.max(alpha,0.3);});
    ne.el.addEventListener('mousedown',function(e){e.stopPropagation();dragging=ne.n;dragging._pinned=true;alpha=Math.max(alpha,0.3);});
  });
  function dragMove(cx,cy){if(!dragging)return;var rect=svg.getBoundingClientRect();dragging.x=(cx-rect.left-px)/zm;dragging.y=(cy-rect.top-py)/zm;dragging.vx=0;dragging.vy=0;}
  svg.addEventListener('touchmove',function(e){if(dragging&&e.touches.length===1){e.preventDefault();dragMove(e.touches[0].clientX,e.touches[0].clientY);}},{passive:false});
  svg.addEventListener('mousemove',function(e){if(dragging)dragMove(e.clientX,e.clientY);});
  function dragEnd(){if(dragging)dragging._pinned=false;dragging=null;}
  svg.addEventListener('touchend',dragEnd);svg.addEventListener('mouseup',dragEnd);svg.addEventListener('mouseleave',dragEnd);

  // Pan
  var panning=false,panStart={x:0,y:0};
  svg.addEventListener('touchstart',function(e){if(e.touches.length===1&&!dragging){panning=true;panStart={x:e.touches[0].clientX-px,y:e.touches[0].clientY-py};}});
  svg.addEventListener('touchmove',function(e){if(panning&&e.touches.length===1&&!dragging){px=e.touches[0].clientX-panStart.x;py=e.touches[0].clientY-panStart.y;upd();}});
  svg.addEventListener('touchend',function(){panning=false;});
  svg.addEventListener('mousedown',function(e){if(e.target===svg||e.target===g){panning=true;panStart={x:e.clientX-px,y:e.clientY-py};}});
  svg.addEventListener('mousemove',function(e){if(panning&&!dragging){px=e.clientX-panStart.x;py=e.clientY-panStart.y;upd();}});
  svg.addEventListener('mouseup',function(){panning=false;});

  function render(){
    if(alpha>0.001)tick();
    for(var i=0;i<eEls.length;i++){var s=nMap[eEls[i].s],t=nMap[eEls[i].t];if(s&&t){eEls[i].el.setAttribute('x1',s.x);eEls[i].el.setAttribute('y1',s.y);eEls[i].el.setAttribute('x2',t.x);eEls[i].el.setAttribute('y2',t.y);}}
    for(var j=0;j<nEls.length;j++){nEls[j].el.setAttribute('transform','translate('+nEls[j].n.x+','+nEls[j].n.y+')');}
    window._codexGraph.raf=requestAnimationFrame(render);
  }
  window._codexGraph={raf:0};
  window._codexGraphZoom=function(f){var rect=svg.getBoundingClientRect();var mx=rect.width/2,my=rect.height/2;var nz=Math.max(0.2,Math.min(6,zm*f));px=mx-(mx-px)*nz/zm;py=my-(my-py)*nz/zm;zm=nz;upd();};
  window._codexGraphReset=function(){zm=1;px=0;py=0;upd();};
  window._codexGraphReheat=function(){alpha=1.0;nodes.forEach(function(n){n.vx=(Math.random()-0.5)*2;n.vy=(Math.random()-0.5)*2;n._pinned=false;});};
  window._codexGraphUpdate=function(newData){
    // Update physics constants from new settings
    var ns=newData.settings||{};
    REP=400+(ns.repelForce||0.5)*2400;
    SPR=0.001+(ns.linkForce||0.5)*0.014;
    SLEN=40+(ns.linkDistance||0.5)*200;
    CGRAV=0.001+(ns.centerForce||0.5)*0.02;
    var newScale=ns.nodeSize||1.0;

    // Build set of visible node IDs from filtered data
    var visibleNodes={};
    newData.nodes.forEach(function(n){visibleNodes[n.id]=true;});
    var visibleEdges={};
    newData.edges.forEach(function(e){visibleEdges[e.source+'->'+e.target]=true;});

    // Update node visibility and size
    nEls.forEach(function(ne){
      var vis=!!visibleNodes[ne.n.id];
      ne.el.style.display=vis?'':'none';
      if(vis&&newScale!==NODE_SCALE){
        ne.n.r=(4+Math.min(ne.n.deg,12)*1.2)*newScale;
        ne.ci.setAttribute('r',ne.n.r);
      }
    });
    NODE_SCALE=newScale;

    // Update edge visibility
    eEls.forEach(function(ee){
      var vis=!!visibleEdges[ee.s+'->'+ee.t]||!!visibleEdges[ee.t+'->'+ee.s];
      // Also hide if either endpoint is hidden
      if(vis) vis=!!visibleNodes[ee.s]&&!!visibleNodes[ee.t];
      ee.el.style.display=vis?'':'none';
    });

    // Reheat slightly so physics changes take effect
    alpha=Math.max(alpha,0.5);
  };
  render();
}
"#;
