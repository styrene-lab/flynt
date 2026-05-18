//! Design canvas — a grid of HTML/CSS cells the agent can author and the
//! user can review. Each `.canvas` file is JSON describing a grid plus a
//! list of cells; cells render as sandboxed iframes with vendored Tailwind
//! and theme tokens injected. Mirrors the `.excalidraw` document pattern:
//! data file + sibling `.md` wrapper that embeds it.

use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use flynt_core::canvas::{Canvas, Cell};
use std::path::PathBuf;

/// Tailwind CSS bundled into the app binary. Phase 4 ships a placeholder
/// stub; the maintainer regenerates the real precompiled CSS via a
/// one-shot tailwindcss standalone-binary run (no Node, no per-machine
/// build). The renderer pipeline is otherwise complete — drop the real
/// file in and Tailwind classes light up.
const TAILWIND_CSS: &str = include_str!("../../assets/vendor/tailwind.css");

/// Vendored tweakcn-style theme presets. Compiled into the app binary so
/// theme switching is instant and offline. The JSON map is also copied
/// into the project on first launch so flynt-agent can read it for
/// canvas_apply_theme suggestions (phase 5).
const TWEAKCN_PRESETS: &str = include_str!("../../assets/vendor/tweakcn-presets.json");

/// Resolve theme tokens for a given theme id. Falls back to "default" if
/// the requested theme is unknown so the canvas always renders. Returns
/// CSS-variable declarations ready to drop inside a `:root { ... }` block.
fn theme_vars(theme_id: &str) -> String {
    fn render(presets: &serde_json::Value, theme_id: &str) -> Option<String> {
        let vars = presets.get(theme_id)?.get("vars")?.as_object()?;
        let parts: Vec<String> = vars
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| format!("{k}: {s}")))
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("; "))
        }
    }

    let presets: serde_json::Value =
        serde_json::from_str(TWEAKCN_PRESETS).unwrap_or(serde_json::Value::Null);

    render(&presets, theme_id)
        .or_else(|| render(&presets, "default"))
        .unwrap_or_else(|| {
            "--background: #0c0c0c; --foreground: #f5f5f5; --primary: #6c8cff; --border: #2a2a2a; --radius: 6px".into()
        })
}

/// Process one capture request end-to-end. Returns a fully populated
/// `CaptureResponse` regardless of success — failures land as
/// `captured: false` with `error` set so the agent can react rather than
/// hang. Runs on the Dioxus async runtime; uses `document::eval` to
/// execute the cell-measurement protocol against the live DOM.
async fn process_capture_request(
    req: crate::canvas_capture::CaptureRequest,
    canvas_rel: &std::path::Path,
    resp_dir: &std::path::Path,
) -> crate::canvas_capture::CaptureResponse {
    use crate::canvas_capture::*;

    let request_id = req.request_id.clone();
    let _ = canvas_rel; // reserved for future per-canvas filtering

    // ── Step 1: ask the page for canvas-pane bounds + per-cell metrics ──
    // One JS round-trip: walks the DOM, posts a measurement message to each
    // cell's iframe, awaits responses with a 600ms total timeout, then
    // serializes the lot back to Rust via dioxus.send.
    let measurement_js = r#"
        (async function(){
            const pane = document.querySelector('.canvas-pane');
            if (!pane) { dioxus.send(JSON.stringify({error: 'canvas-pane element not found'})); return; }
            const paneRect = pane.getBoundingClientRect();
            // Window position in screen coords. window.screenX/Y are relative to
            // the browser-frame; in wry/Dioxus desktop they map to the OS window
            // origin in screen coordinates.
            const winX = window.screenX || 0;
            const winY = window.screenY || 0;

            const cells = Array.from(document.querySelectorAll('.canvas-cell'));
            const pendingByRequestId = new Map();
            const results = [];

            function onMessage(e) {
                const d = e.data;
                if (d && d.flyntMeasureResponse) {
                    const resolver = pendingByRequestId.get(d.request_id);
                    if (resolver) {
                        pendingByRequestId.delete(d.request_id);
                        resolver(d);
                    }
                }
            }
            window.addEventListener('message', onMessage);

            for (const cellEl of cells) {
                const id = cellEl.getAttribute('key') || cellEl.getAttribute('data-cell-id') || '';
                const iframe = cellEl.querySelector('iframe');
                const r = cellEl.getBoundingClientRect();
                const cellMetric = {
                    id: cellEl.querySelector('iframe')?.title?.replace('canvas cell ', '') || id,
                    cell_box: { x: r.x, y: r.y, w: r.width, h: r.height },
                    content_box: null,
                };
                if (iframe && iframe.contentWindow) {
                    const reqId = Math.random().toString(36).slice(2);
                    const promise = new Promise((resolve) => {
                        pendingByRequestId.set(reqId, resolve);
                    });
                    try {
                        iframe.contentWindow.postMessage({
                            flyntMeasure: true, cell_id: cellMetric.id, request_id: reqId,
                        }, '*');
                    } catch (e) {}
                    const timeout = new Promise((resolve) => setTimeout(() => resolve(null), 400));
                    const resp = await Promise.race([promise, timeout]);
                    if (resp) {
                        cellMetric.content_box = {
                            x: r.x, y: r.y,
                            w: resp.content_width || 0,
                            h: resp.content_height || 0,
                        };
                    }
                }
                results.push(cellMetric);
            }
            window.removeEventListener('message', onMessage);

            dioxus.send(JSON.stringify({
                pane_window_relative: { x: paneRect.x, y: paneRect.y, w: paneRect.width, h: paneRect.height },
                pane_screen_relative: { x: winX + paneRect.x, y: winY + paneRect.y, w: paneRect.width, h: paneRect.height },
                window_pos: { x: winX, y: winY },
                cells: results,
            }));
        })();
    "#;

    let mut eval = dioxus::prelude::document::eval(measurement_js);
    let measurement_json: String = match eval.recv::<String>().await {
        Ok(s) => s,
        Err(e) => {
            return CaptureResponse {
                request_id,
                image_path: String::new(),
                image_base64: String::new(),
                image_width: 0,
                image_height: 0,
                viewport_box: BoxXywh {
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                },
                cells: vec![],
                scale_factor: 1.0,
                captured: false,
                error: Some(format!("dioxus.send recv failed: {e}")),
            };
        }
    };

    let measurement: serde_json::Value = match serde_json::from_str(&measurement_json) {
        Ok(v) => v,
        Err(e) => return error_response(&request_id, format!("measurement json parse: {e}")),
    };
    if let Some(err) = measurement.get("error").and_then(|v| v.as_str()) {
        return error_response(&request_id, format!("measurement: {err}"));
    }

    let pane = match measurement.get("pane_window_relative") {
        Some(v) => v,
        None => return error_response(&request_id, "missing pane_window_relative".into()),
    };
    let pane_box = BoxXywh {
        x: pane["x"].as_f64().unwrap_or(0.0) as f32,
        y: pane["y"].as_f64().unwrap_or(0.0) as f32,
        w: pane["w"].as_f64().unwrap_or(0.0) as f32,
        h: pane["h"].as_f64().unwrap_or(0.0) as f32,
    };

    // ── Step 2: capture via xcap, crop to pane bounds ──
    let (png_bytes, img_w, img_h) = match capture_pane(pane_box) {
        Ok(t) => t,
        Err(e) => return error_response(&request_id, format!("xcap capture: {e}")),
    };

    // ── Step 3: persist + base64-encode ──
    let png_path = resp_dir.join(format!("{request_id}.png"));
    if let Err(e) = std::fs::write(&png_path, &png_bytes) {
        tracing::warn!("capture PNG write failed: {e}");
    }
    use base64::Engine;
    let image_base64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

    // ── Step 4: shape per-cell metrics with fill_ratio ──
    let cells: Vec<CellMetric> = measurement
        .get("cells")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let id = c
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let cb = c
                .get("cell_box")
                .map(|b| BoxXywh {
                    x: b["x"].as_f64().unwrap_or(0.0) as f32,
                    y: b["y"].as_f64().unwrap_or(0.0) as f32,
                    w: b["w"].as_f64().unwrap_or(0.0) as f32,
                    h: b["h"].as_f64().unwrap_or(0.0) as f32,
                })
                .unwrap_or(BoxXywh {
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                });
            let content = c.get("content_box").and_then(|b| {
                if b.is_null() {
                    return None;
                }
                Some(BoxXywh {
                    x: b["x"].as_f64().unwrap_or(0.0) as f32,
                    y: b["y"].as_f64().unwrap_or(0.0) as f32,
                    w: b["w"].as_f64().unwrap_or(0.0) as f32,
                    h: b["h"].as_f64().unwrap_or(0.0) as f32,
                })
            });
            let fill_ratio = content.as_ref().and_then(|cb_inner| {
                if cb.h > 0.0 {
                    Some(cb_inner.h / cb.h)
                } else {
                    None
                }
            });
            CellMetric {
                id,
                cell_box: cb,
                content_box: content,
                fill_ratio,
            }
        })
        .collect();

    let scale = find_flynt_window()
        .and_then(|w| w.current_monitor().ok())
        .and_then(|m| m.scale_factor().ok())
        .unwrap_or(1.0);

    CaptureResponse {
        request_id,
        image_path: png_path.to_string_lossy().to_string(),
        image_base64,
        image_width: img_w,
        image_height: img_h,
        viewport_box: pane_box,
        cells,
        scale_factor: scale,
        captured: true,
        error: None,
    }
}

fn error_response(request_id: &str, error: String) -> crate::canvas_capture::CaptureResponse {
    use crate::canvas_capture::*;
    CaptureResponse {
        request_id: request_id.to_string(),
        image_path: String::new(),
        image_base64: String::new(),
        image_width: 0,
        image_height: 0,
        viewport_box: BoxXywh {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        },
        cells: vec![],
        scale_factor: 1.0,
        captured: false,
        error: Some(error),
    }
}

fn write_response(resp_dir: &std::path::Path, resp: &crate::canvas_capture::CaptureResponse) {
    let path = resp_dir.join(format!("{}.json", resp.request_id));
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string(resp) {
        if std::fs::write(&tmp, json.as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// JS injected into every cell's iframe srcdoc. Listens for the parent
/// posting `__flynt_measure__:<id>` and responds with the cell body's
/// natural dimensions. Lets us collect per-cell layout metrics without
/// `allow-same-origin` on the sandbox — postMessage is explicitly
/// cross-origin friendly even from opaque-origin iframes.
const MEASUREMENT_HOOK: &str = r#"
(function(){
  window.addEventListener('message', function(e){
    var d = e.data;
    if (typeof d === 'object' && d && d.flyntMeasure) {
      var b = document.body;
      var r = b ? b.getBoundingClientRect() : { width: 0, height: 0 };
      var resp = {
        flyntMeasureResponse: true,
        cell_id: d.cell_id,
        request_id: d.request_id,
        content_width: Math.ceil(b ? b.scrollWidth : 0),
        content_height: Math.ceil(b ? b.scrollHeight : 0),
        viewport_width: window.innerWidth,
        viewport_height: window.innerHeight,
      };
      try { e.source.postMessage(resp, '*'); }
      catch (err) { try { parent.postMessage(resp, '*'); } catch (e2) {} }
    }
  });
})();
"#;

/// Build the srcdoc HTML for a single cell. Pure function — unit tested.
///
/// Inlines Tailwind, theme tokens, and the cell's CSS into `<head>`, then
/// the cell's HTML and optional JS into `<body>`. Inlining (vs. a
/// `project://` link) keeps the canvas portable: it renders identically
/// across boundaries — exported, screenshotted, run on a different
/// machine, or run offline.
pub fn build_srcdoc(cell: &Cell, theme: &str, tailwind_css: &str) -> String {
    let theme = theme_vars(theme);
    let css = &cell.css;
    let html = &cell.html;
    let js = cell.js.as_deref().unwrap_or("");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<style>{tailwind_css}</style>\
<style>:root {{ {theme} }} html,body {{ margin:0; padding:0; height:100%; background:var(--background); color:var(--foreground); font-family:system-ui,sans-serif; }} body > * {{ box-sizing: border-box; }} {css}</style>\
</head><body>{html}<script>{MEASUREMENT_HOOK}\n{js}</script></body></html>",
        MEASUREMENT_HOOK = MEASUREMENT_HOOK
    )
}

/// Extension check for the raw canvas data file.
pub fn is_canvas(path: &std::path::Path) -> bool {
    path.extension().map(|e| e == "canvas").unwrap_or(false)
}

/// Detect a `.md` wrapper whose body is exactly one `![[...canvas]]` embed.
/// Returns the embedded canvas filename if so. Mirrors `excalidraw_embed_path`.
pub fn canvas_embed_path(content: &str) -> Option<String> {
    let body = if let Some(rest) = content.strip_prefix("+++\n") {
        if let Some(end) = rest.find("\n+++") {
            rest[end + 4..].trim()
        } else {
            content.trim()
        }
    } else {
        content.trim()
    };

    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() == 1 {
        let line = lines[0].trim();
        if line.starts_with("![[") && line.ends_with(".canvas]]") {
            let inner = &line[3..line.len() - 2];
            return Some(inner.to_string());
        }
    }
    None
}

/// Recover a canvas association even when the wrapper body has been
/// corrupted (e.g. by an unrelated note-view bug stomping the file).
/// Returns true if the document's frontmatter declares `tags = [...,
/// "canvas", ...]`. Combined with a sibling `<stem>.canvas` file
/// existing, this is enough to keep dispatching to CanvasView so a
/// transient body corruption doesn't strand the user's design work.
pub fn frontmatter_has_canvas_tag(content: &str) -> bool {
    let Some(rest) = content.strip_prefix("+++\n") else {
        return false;
    };
    let Some(end) = rest.find("\n+++") else {
        return false;
    };
    let frontmatter = &rest[..end];
    for line in frontmatter.lines() {
        let trimmed = line.trim_start();
        if let Some(rhs) = trimmed.strip_prefix("tags") {
            // tags = ["a", "b", "canvas"] — accept any whitespace/= between
            let after_eq = rhs.trim_start().strip_prefix('=').unwrap_or("");
            if after_eq.contains("\"canvas\"") {
                return true;
            }
        }
    }
    false
}

/// Re-export of `flynt_core::canvas::create_canvas` for callers in this
/// crate (menu handler, command palette). The actual implementation lives
/// in flynt-core so flynt-agent can call into the same code via the
/// `canvas_create` ACP tool.
pub use flynt_core::canvas::create_canvas;

#[component]
pub fn CanvasView(path: PathBuf) -> Element {
    let ctx = use_context::<AppContext>();

    // Refresh counter — bumped whenever the project watcher reports a write
    // to our .canvas file (typically by the agent via canvas_set_cells).
    // Hook into use_memo's deps so reload happens automatically.
    let mut refresh = use_signal(|| 0u64);

    {
        let project_events = ctx.project_events();
        let watch_path = path.clone();
        use_effect(move || {
            let mut rx = project_events.subscribe();
            let watch_path = watch_path.clone();
            spawn(async move {
                while let Ok(event) = rx.recv().await {
                    let changed = match event {
                        flynt_store::watcher::ProjectChangeEvent::FileCreated(p)
                        | flynt_store::watcher::ProjectChangeEvent::FileModified(p) => Some(p),
                        flynt_store::watcher::ProjectChangeEvent::FileDeleted(_) => None,
                    };
                    let Some(changed) = changed else {
                        continue;
                    };
                    // Match by suffix — events carry absolute paths; our path
                    // is project-relative. Keeps the comparison robust to
                    // canonicalization differences.
                    if changed.ends_with(&watch_path) {
                        *refresh.write() += 1;
                    }
                }
            });
        });
    }

    // Capture-request handler. Polls the request dir on a slow tick (200ms),
    // processes any pending request by querying iframe metrics via
    // postMessage, calling xcap to capture the canvas-pane, and writing the
    // response sidecar. Lives here because only flynt-app has the WebView
    // window context xcap needs. See `crate::canvas_capture` for the
    // request/response shape and the rationale.
    {
        let cap_ctx = ctx.clone();
        let canvas_path_for_handler = path.clone();
        use_effect(move || {
            let project_root = cap_ctx.project().root.clone();
            let canvas_rel = canvas_path_for_handler.clone();
            spawn(async move {
                use crate::canvas_capture::*;
                let req_dir = capture_request_dir(&project_root);
                let resp_dir = capture_response_dir(&project_root);
                let _ = std::fs::create_dir_all(&req_dir);
                let _ = std::fs::create_dir_all(&resp_dir);
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    let entries = match std::fs::read_dir(&req_dir) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|s| s.to_str()) != Some("json") {
                            continue;
                        }
                        // Read + immediately remove the request so retries
                        // from the tool don't double-process. Idempotent
                        // even if the response write fails midway.
                        let body = match std::fs::read_to_string(&path) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        let _ = std::fs::remove_file(&path);
                        let req: CaptureRequest = match serde_json::from_str(&body) {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::warn!("malformed capture request {}: {e}", path.display());
                                continue;
                            }
                        };
                        let resp = process_capture_request(req, &canvas_rel, &resp_dir).await;
                        write_response(&resp_dir, &resp);
                    }
                }
            });
        });
    }

    let path_load = path.clone();
    let parsed = use_memo(move || {
        let _ = refresh();
        let project = ctx.project();
        let abs = project.root.join(&path_load);
        Canvas::load(&abs).map_err(|e| e.to_string())
    });

    tracing::info!("CanvasView render: path={}", path.display());
    let parsed_ref = parsed.read();
    let canvas = match &*parsed_ref {
        Ok(c) => {
            tracing::info!(
                "CanvasView parsed: {} cells, theme={}",
                c.cells.len(),
                c.theme
            );
            c
        }
        Err(e) => {
            tracing::warn!("CanvasView parse error: {e}");
            return rsx! {
                div { class: "canvas-pane",
                    div { class: "canvas-toolbar",
                        span { class: "canvas-meta", "Canvas: {path.display()}" }
                        span { class: "canvas-error", "Parse error: {e}" }
                    }
                }
            };
        }
    };

    let grid_style = format!(
        "grid-template-columns: repeat({}, 1fr); grid-auto-rows: minmax(120px, auto); gap: {}px;",
        canvas.grid.cols.max(1),
        canvas.grid.gap,
    );

    rsx! {
        div { class: "canvas-pane",
            div { class: "canvas-toolbar",
                span { class: "canvas-meta", "Canvas: {path.display()}" }
                span { class: "canvas-meta",
                    "v{canvas.version} · theme={canvas.theme} · {canvas.grid.cols}×{canvas.grid.rows} · {canvas.cells.len()} cell(s)"
                }
            }
            if canvas.cells.is_empty() {
                div { class: "canvas-empty",
                    "Empty canvas. Ask the agent to design something here."
                }
            } else {
                div { class: "canvas-grid", style: "{grid_style}",
                    for cell in canvas.cells.iter() {
                        {
                            let cell_style = format!(
                                "grid-column: {} / span {}; grid-row: {} / span {};",
                                cell.x.saturating_add(1),
                                cell.w.max(1),
                                cell.y.saturating_add(1),
                                cell.h.max(1),
                            );
                            // One sandboxed iframe per cell. srcdoc inlines
                            // Tailwind + theme tokens + cell HTML/CSS/JS so
                            // each cell renders self-contained — style and JS
                            // can't bleed across cells or into the host page.
                            // Required dioxus-desktop >= 0.7.9 (earlier versions
                            // ship a navigation handler that cancels iframe
                            // content loads).
                            let srcdoc = build_srcdoc(cell, &canvas.theme, TAILWIND_CSS);
                            rsx! {
                                div {
                                    key: "{cell.id}",
                                    class: "canvas-cell",
                                    style: "{cell_style}",
                                    iframe {
                                        title: "canvas cell {cell.id}",
                                        "sandbox": "allow-scripts",
                                        "srcdoc": "{srcdoc}",
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detects_canvas_wrapper_with_frontmatter() {
        let content = "+++\ntitle = \"My Canvas\"\ntags = [\"canvas\"]\n+++\n\n![[hero.canvas]]\n";
        assert_eq!(canvas_embed_path(content), Some("hero.canvas".into()));
    }

    #[test]
    fn detects_canvas_wrapper_minimal() {
        assert_eq!(
            canvas_embed_path("![[test.canvas]]\n"),
            Some("test.canvas".into())
        );
    }

    #[test]
    fn rejects_regular_note_with_canvas_embed() {
        let content =
            "+++\ntitle = \"Note\"\n+++\n\nText before.\n\n![[hero.canvas]]\n\nText after.\n";
        assert_eq!(canvas_embed_path(content), None);
    }

    #[test]
    fn rejects_non_canvas_embed() {
        assert_eq!(canvas_embed_path("![[image.png]]"), None);
    }

    #[test]
    fn frontmatter_has_canvas_tag_detects_simple_tags() {
        let c = "+++\ntitle = \"Demo\"\ntags = [\"canvas\"]\n+++\n\nbody\n";
        assert!(frontmatter_has_canvas_tag(c));
    }

    #[test]
    fn frontmatter_has_canvas_tag_detects_in_multi_tag_array() {
        let c = "+++\ntags = [\"draft\", \"canvas\", \"design\"]\n+++\n\nbody\n";
        assert!(frontmatter_has_canvas_tag(c));
    }

    #[test]
    fn frontmatter_has_canvas_tag_rejects_missing_or_other_tags() {
        assert!(!frontmatter_has_canvas_tag("+++\ntags = []\n+++\n\nbody\n"));
        assert!(!frontmatter_has_canvas_tag(
            "+++\ntags = [\"draft\"]\n+++\n\nbody\n"
        ));
        assert!(!frontmatter_has_canvas_tag("plain text"));
    }

    #[test]
    fn frontmatter_has_canvas_tag_handles_no_frontmatter() {
        assert!(!frontmatter_has_canvas_tag("just a regular note"));
    }

    #[test]
    fn is_canvas_extension() {
        assert!(is_canvas(std::path::Path::new("canvases/x.canvas")));
        assert!(!is_canvas(std::path::Path::new("notes/x.md")));
    }

    fn cell_with(html: &str, css: &str, js: Option<&str>) -> Cell {
        Cell {
            id: "t".into(),
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            html: html.into(),
            css: css.into(),
            js: js.map(|s| s.into()),
        }
    }

    #[test]
    fn build_srcdoc_inlines_tailwind_and_theme_and_cell_css() {
        let cell = cell_with(
            "<button class=\"btn\">Hi</button>",
            ".btn { color: red; }",
            None,
        );
        let out = build_srcdoc(&cell, "default", "/* tw-marker */");
        assert!(out.contains("/* tw-marker */"), "tailwind must be inlined");
        assert!(out.contains("--background:"), "theme vars must be inlined");
        assert!(
            out.contains(".btn { color: red; }"),
            "cell css must be inlined"
        );
        assert!(
            out.contains("<button class=\"btn\">Hi</button>"),
            "cell html must be in body"
        );
    }

    #[test]
    fn build_srcdoc_handles_missing_js_with_measurement_hook_only() {
        // Even when cell.js is None we still inject the measurement hook,
        // so the script tag is never empty. This pins the new contract: the
        // measurement protocol is always wired regardless of cell-side JS.
        let cell = cell_with("<div>x</div>", "", None);
        let out = build_srcdoc(&cell, "default", "");
        assert!(out.contains("<script>"));
        assert!(
            out.contains("flyntMeasure"),
            "measurement hook must be injected"
        );
    }

    #[test]
    fn build_srcdoc_includes_cell_js() {
        let cell = cell_with("<div></div>", "", Some("console.log('hi')"));
        let out = build_srcdoc(&cell, "default", "");
        assert!(out.contains("console.log('hi')"));
    }

    #[test]
    fn build_srcdoc_sets_html_body_to_full_height() {
        // Regression: without `height:100%` on html and body, child elements'
        // `h-full` (height: 100%) collapses because there's no parent height to
        // resolve against. The agent's discipline of using h-full only works if
        // the renderer propagates height down. Phase 3 shipped without this and
        // every "tall cell with short content" rendered with dead space below.
        let cell = cell_with("<div class=\"h-full\">x</div>", "", None);
        let out = build_srcdoc(&cell, "default", "");
        assert!(out.contains("html,body"), "html/body selector present");
        assert!(
            out.contains("height:100%"),
            "html/body must set height:100%"
        );
    }

    #[test]
    fn build_srcdoc_starts_with_doctype() {
        let cell = cell_with("", "", None);
        let out = build_srcdoc(&cell, "default", "");
        assert!(out.starts_with("<!doctype html>"));
    }

    #[test]
    fn theme_vars_returns_css_declarations() {
        let vars = theme_vars("default");
        assert!(vars.contains("--background:"));
        assert!(vars.contains("--primary:"));
        // Semicolon-joined so it can drop directly inside :root { ... }
        assert!(vars.contains(";"));
    }

    #[test]
    fn create_canvas_produces_both_files() {
        // create_canvas is now defined in flynt-core; we keep this smoke
        // check here to confirm the re-export and the wrapper-detection
        // path round-trip end-to-end through the UI crate.
        let tmp = TempDir::new().unwrap();
        let md_path = create_canvas(tmp.path(), "Hero").unwrap();

        let md_abs = tmp.path().join(&md_path);
        let md_content = std::fs::read_to_string(&md_abs).unwrap();
        assert_eq!(canvas_embed_path(&md_content), Some("Hero.canvas".into()));
    }
}
