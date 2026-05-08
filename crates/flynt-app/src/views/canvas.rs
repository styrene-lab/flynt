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
/// into the vault on first launch so flynt-agent can read it for
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
        if parts.is_empty() { None } else { Some(parts.join("; ")) }
    }

    let presets: serde_json::Value = serde_json::from_str(TWEAKCN_PRESETS)
        .unwrap_or(serde_json::Value::Null);

    render(&presets, theme_id)
        .or_else(|| render(&presets, "default"))
        .unwrap_or_else(|| {
            "--background: #0c0c0c; --foreground: #f5f5f5; --primary: #6c8cff; --border: #2a2a2a; --radius: 6px".into()
        })
}

/// Build the srcdoc HTML for a single cell. Pure function — unit tested.
///
/// Inlines Tailwind, theme tokens, and the cell's CSS into `<head>`, then
/// the cell's HTML and optional JS into `<body>`. Inlining (vs. a
/// `vault://` link) keeps the canvas portable: it renders identically
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
<style>:root {{ {theme} }} html,body {{ margin:0; padding:0; background:var(--background); color:var(--foreground); font-family:system-ui,sans-serif; }} {css}</style>\
</head><body>{html}<script>{js}</script></body></html>"
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

/// Create a new canvas: a `.canvas` data file plus a `.md` wrapper that
/// embeds it. Returns the `.md` path (indexable by Flynt). Mirrors
/// `create_drawing` for excalidraw.
pub fn create_canvas(vault_root: &std::path::Path, name: &str) -> anyhow::Result<PathBuf> {
    let canvases_dir = vault_root.join("canvases");
    std::fs::create_dir_all(&canvases_dir)?;

    let canvas_file = format!("{name}.canvas");
    let canvas_abs = canvases_dir.join(&canvas_file);
    Canvas::default().save(&canvas_abs)?;

    let md_file = format!("{name}.md");
    let md_rel = PathBuf::from("canvases").join(&md_file);
    let md_abs = vault_root.join(&md_rel);
    let md_content = format!("+++\ntitle = \"{name}\"\ntags = [\"canvas\"]\n+++\n\n![[{canvas_file}]]\n");
    std::fs::write(&md_abs, md_content)?;

    Ok(md_rel)
}

#[component]
pub fn CanvasView(path: PathBuf) -> Element {
    let ctx = use_context::<AppContext>();

    // Refresh counter — bumped whenever the vault watcher reports a write
    // to our .canvas file (typically by the agent via canvas_set_cells).
    // Hook into use_memo's deps so reload happens automatically.
    let mut refresh = use_signal(|| 0u64);

    {
        let vault_events = ctx.vault_events();
        let watch_path = path.clone();
        use_effect(move || {
            let mut rx = vault_events.subscribe();
            let watch_path = watch_path.clone();
            spawn(async move {
                while let Ok(event) = rx.recv().await {
                    let changed = match event {
                        flynt_store::watcher::VaultChangeEvent::FileCreated(p)
                        | flynt_store::watcher::VaultChangeEvent::FileModified(p) => Some(p),
                        flynt_store::watcher::VaultChangeEvent::FileDeleted(_) => None,
                    };
                    let Some(changed) = changed else { continue; };
                    // Match by suffix — events carry absolute paths; our path
                    // is vault-relative. Keeps the comparison robust to
                    // canonicalization differences.
                    if changed.ends_with(&watch_path) {
                        *refresh.write() += 1;
                    }
                }
            });
        });
    }

    let path_load = path.clone();
    let parsed = use_memo(move || {
        let _ = refresh();
        let vault = ctx.vault();
        let abs = vault.root.join(&path_load);
        Canvas::load(&abs).map_err(|e| e.to_string())
    });

    let parsed_ref = parsed.read();
    let canvas = match &*parsed_ref {
        Ok(c) => c,
        Err(e) => {
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
                            let srcdoc = build_srcdoc(cell, &canvas.theme, TAILWIND_CSS);
                            rsx! {
                                div { key: "{cell.id}", class: "canvas-cell", style: "{cell_style}",
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
        assert_eq!(canvas_embed_path("![[test.canvas]]\n"), Some("test.canvas".into()));
    }

    #[test]
    fn rejects_regular_note_with_canvas_embed() {
        let content = "+++\ntitle = \"Note\"\n+++\n\nText before.\n\n![[hero.canvas]]\n\nText after.\n";
        assert_eq!(canvas_embed_path(content), None);
    }

    #[test]
    fn rejects_non_canvas_embed() {
        assert_eq!(canvas_embed_path("![[image.png]]"), None);
    }

    #[test]
    fn is_canvas_extension() {
        assert!(is_canvas(std::path::Path::new("canvases/x.canvas")));
        assert!(!is_canvas(std::path::Path::new("notes/x.md")));
    }

    fn cell_with(html: &str, css: &str, js: Option<&str>) -> Cell {
        Cell {
            id: "t".into(), x: 0, y: 0, w: 1, h: 1,
            html: html.into(), css: css.into(),
            js: js.map(|s| s.into()),
        }
    }

    #[test]
    fn build_srcdoc_inlines_tailwind_and_theme_and_cell_css() {
        let cell = cell_with("<button class=\"btn\">Hi</button>", ".btn { color: red; }", None);
        let out = build_srcdoc(&cell, "default", "/* tw-marker */");
        assert!(out.contains("/* tw-marker */"), "tailwind must be inlined");
        assert!(out.contains("--background:"), "theme vars must be inlined");
        assert!(out.contains(".btn { color: red; }"), "cell css must be inlined");
        assert!(out.contains("<button class=\"btn\">Hi</button>"), "cell html must be in body");
    }

    #[test]
    fn build_srcdoc_handles_missing_js() {
        let cell = cell_with("<div>x</div>", "", None);
        let out = build_srcdoc(&cell, "default", "");
        // Empty <script></script> is fine — the document still parses.
        assert!(out.contains("<script></script>"));
    }

    #[test]
    fn build_srcdoc_includes_cell_js() {
        let cell = cell_with("<div></div>", "", Some("console.log('hi')"));
        let out = build_srcdoc(&cell, "default", "");
        assert!(out.contains("console.log('hi')"));
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
        let tmp = TempDir::new().unwrap();
        let md_path = create_canvas(tmp.path(), "Hero").unwrap();

        assert!(md_path.to_string_lossy().ends_with(".md"));
        let md_abs = tmp.path().join(&md_path);
        let canvas_abs = tmp.path().join("canvases/Hero.canvas");
        assert!(md_abs.exists());
        assert!(canvas_abs.exists());

        let md_content = std::fs::read_to_string(&md_abs).unwrap();
        assert_eq!(canvas_embed_path(&md_content), Some("Hero.canvas".into()));

        let scene: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&canvas_abs).unwrap()).unwrap();
        assert_eq!(scene["version"], 1);
        assert_eq!(scene["grid"]["cols"], 12);
    }
}
