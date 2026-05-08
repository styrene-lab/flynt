//! Design canvas — a grid of HTML/CSS cells the agent can author and the
//! user can review. Each `.canvas` file is JSON describing a grid plus a
//! list of cells; cells render as sandboxed iframes with vendored Tailwind
//! and theme tokens injected. Mirrors the `.excalidraw` document pattern:
//! data file + sibling `.md` wrapper that embeds it.

use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use flynt_core::canvas::Canvas;
use std::path::PathBuf;

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
    let path_load = path.clone();

    // Parse via the typed Canvas model. A parse error becomes Err(message)
    // so the user sees what's wrong rather than an opaque blank pane.
    // Phase 3 replaces this stub with the iframe-per-cell renderer.
    let parsed = use_memo(move || {
        let vault = ctx.vault();
        let abs = vault.root.join(&path_load);
        Canvas::load(&abs).map_err(|e| e.to_string())
    });

    rsx! {
        div {
            class: "canvas-pane",
            style: "display:flex;flex-direction:column;flex:1;min-height:0;width:100%;padding:16px;font-family:monospace;color:var(--text-muted);gap:8px;",
            div { "Canvas: {path.display()}" }
            match &*parsed.read() {
                Ok(c) => rsx! {
                    div { style: "opacity:0.7;font-size:12px;",
                        "v{c.version} · theme={c.theme} · grid={c.grid.cols}×{c.grid.rows} gap={c.grid.gap}px · {c.cells.len()} cell(s)"
                    }
                },
                Err(e) => rsx! {
                    div { style: "color:var(--text-error,#f88);font-size:12px;", "Parse error: {e}" }
                },
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
