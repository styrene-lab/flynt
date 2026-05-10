//! Excalidraw drawing editor — renders .excalidraw files in the webview.

use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use std::path::PathBuf;

/// Check if a document path is an Excalidraw file.
pub fn is_excalidraw(path: &std::path::Path) -> bool {
    path.extension().map(|e| e == "excalidraw").unwrap_or(false)
}

#[component]
pub fn ExcalidrawView(path: PathBuf) -> Element {
    let ctx = use_context::<AppContext>();
    let mut save_state = use_signal(|| "");
    let path_load = path.clone();

    // Load the file content
    let content = use_memo(move || {
        let project = ctx.project();
        let abs = project.root.join(&path_load);
        std::fs::read_to_string(&abs).unwrap_or_else(|_| {
            // New drawing — empty scene
            r#"{"type":"excalidraw","version":2,"elements":[],"appState":{}}"#.to_string()
        })
    });

    // Force parent layout for excalidraw — reapply until stable
    use_effect(move || {
        document::eval(r#"
            function fixLayout() {
                var mc = document.querySelector('.main-content');
                if (mc) { mc.style.overflow = 'hidden'; mc.style.display = 'flex'; mc.style.flexDirection = 'column'; }
                var np = document.querySelector('.notes-pane');
                if (np) { np.style.overflow = 'hidden'; np.style.padding = '0'; np.style.display = 'flex'; np.style.flexDirection = 'column'; np.style.flex = '1'; np.style.minHeight = '0'; }
            }
            fixLayout();
            // Hide tab bar in drawing mode — it eats space and isn't useful
            var tabBar = document.querySelector('.tab-bar');
            if (tabBar) tabBar.style.display = 'none';

            // Force Excalidraw to re-measure after layout settles
            requestAnimationFrame(function() {
                fixLayout();
                window.dispatchEvent(new Event('resize'));
            });
            setTimeout(function() { fixLayout(); window.dispatchEvent(new Event('resize')); }, 300);

            window.addEventListener('resize', fixLayout);

            // Restore tab bar when leaving drawing mode (cleanup)
            window._excalidrawCleanup = function() {
                // Unmount React to free memory
                if (window.FlyntExcalidraw && window.FlyntExcalidraw._root) {
                    try { window.FlyntExcalidraw._root.unmount(); } catch(e) {}
                    window.FlyntExcalidraw._root = null;
                    window.FlyntExcalidraw._api = null;
                }
                window._excalidrawLatest = null;
                window._excSaveQueue = [];
                var tb = document.querySelector('.tab-bar');
                if (tb) tb.style.display = '';
                var mc = document.querySelector('.main-content');
                if (mc) { mc.style.overflow = ''; mc.style.display = ''; mc.style.flexDirection = ''; }
                var np = document.querySelector('.notes-pane');
                if (np) { np.style.overflow = ''; np.style.padding = ''; np.style.display = ''; np.style.flexDirection = ''; np.style.flex = ''; np.style.minHeight = ''; }
            };
        "#);
    });

    // Initialize Excalidraw when component mounts — bundle is loaded eagerly in app.rs
    let path_for_save = path.clone();
    use_effect(move || {
        let data = content.read().clone();
        let escaped = serde_json::to_string(&data).unwrap_or("\"{}\"".into());

        let js = format!(r#"
            (function() {{
                function tryMount() {{
                    const container = document.getElementById('flynt-excalidraw');
                    if (!container) {{ setTimeout(tryMount, 50); return; }}
                    if (!window.FlyntExcalidraw) {{ setTimeout(tryMount, 100); return; }}

                    window.FlyntExcalidraw.mount('flynt-excalidraw', {escaped}, function(data) {{
                        window._excalidrawLatest = data;
                    }});
                }}
                tryMount();
            }})();
        "#);
        document::eval(&js);
    });

    // Auto-save: debounced 2-second timer after each change, plus Cmd+S for immediate save
    let path_save = path_for_save.clone();
    use_effect(move || {
        // Set up auto-save: polls for changes every 2 seconds
        let mut eval = document::eval(r#"
            window._excSaveDirty = false;
            window._excSaveQueue = window._excSaveQueue || [];

            // Mark dirty on every change
            const origOnChange = window.FlyntExcalidraw?._onChange;
            if (window.FlyntExcalidraw) {
                const prevMount = window.FlyntExcalidraw.mount.bind(window.FlyntExcalidraw);
                // The onChange callback is set during mount — we intercept it
            }

            // Cmd+S: immediate save
            document.addEventListener('keydown', function(e) {
                if ((e.metaKey || e.ctrlKey) && e.key === 's') {
                    e.preventDefault();
                    if (window._excalidrawLatest) {
                        window._excSaveQueue.push(window._excalidrawLatest);
                    }
                }
            });

            // Auto-save loop: check every 2 seconds if there are pending changes
            let lastSaved = '';
            async function autoSaveLoop() {
                while (true) {
                    await new Promise(r => setTimeout(r, 2000));
                    if (window._excalidrawLatest && window._excalidrawLatest !== lastSaved) {
                        lastSaved = window._excalidrawLatest;
                        window._excSaveQueue.push(lastSaved);
                    }
                }
            }
            autoSaveLoop();

            // Drain queue to Rust
            async function drain() {
                while (true) {
                    if (window._excSaveQueue.length > 0) {
                        dioxus.send(window._excSaveQueue.shift());
                    } else {
                        await new Promise(r => setTimeout(r, 200));
                    }
                }
            }
            drain();
        "#);

        let p = path_save.clone();
        let c = ctx;
        spawn(async move {
            loop {
                let Ok(data) = eval.recv::<String>().await else { break; };
                let project = c.project();
                let abs = project.root.join(&p);
                if std::fs::write(&abs, &data).is_ok() {
                    *save_state.write() = "saved";

                    // Auto-export SVG for inline embeds in notes
                    let svg_path = abs.with_extension("svg");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let mut svg_eval = document::eval(r#"
                        (async function() {
                            if (window.FlyntExcalidraw && window.FlyntExcalidraw._api) {
                                const svg = await window.FlyntExcalidraw.exportSvg();
                                dioxus.send(svg || '');
                            } else { dioxus.send(''); }
                        })();
                    "#);
                    if let Ok(svg) = svg_eval.recv::<String>().await {
                        if !svg.is_empty() {
                            let _ = std::fs::write(&svg_path, &svg);
                        }
                    }

                    // Clear "saved" after 2 seconds
                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                    if *save_state.read() == "saved" {
                        *save_state.write() = "";
                    }
                }
            }
        });
    });

    let path_for_svg = path.clone();
    let path_for_png = path.clone();

    rsx! {
        div {
            class: "excalidraw-pane",
            style: "display:flex;flex-direction:column;flex:1;min-height:0;width:100%;position:relative;",
            // Floating export buttons — top right overlay
            div {
                class: "excalidraw-overlay-actions",
                span {
                    class: if !save_state.read().is_empty() { "excalidraw-save-status visible" } else { "excalidraw-save-status" },
                    "{save_state}"
                }
                    button {
                        class: "btn btn-ghost btn-xs",
                        title: "Export as SVG",
                        onclick: move |_| {
                            let p = path_for_svg.clone();
                            let c = ctx;
                            spawn(async move {
                                let mut eval = document::eval(r#"
                                    (async function() {
                                        if (window.FlyntExcalidraw && window.FlyntExcalidraw._api) {
                                            const svg = await window.FlyntExcalidraw.exportSvg();
                                            dioxus.send(svg || '');
                                        } else {
                                            dioxus.send('');
                                        }
                                    })();
                                "#);
                                if let Ok(svg) = eval.recv::<String>().await {
                                    if !svg.is_empty() {
                                        let project = c.project();
                                        let svg_path = p.with_extension("svg");
                                        let abs = project.root.join(&svg_path);
                                        if std::fs::write(&abs, &svg).is_ok() {
                                            *save_state.write() = "SVG exported";
                                            #[cfg(target_os = "macos")]
                                            { let _ = std::process::Command::new("open").arg("-R").arg(&abs).spawn(); }
                                            #[cfg(target_os = "linux")]
                                            { if let Some(dir) = abs.parent() { let _ = std::process::Command::new("xdg-open").arg(dir).spawn(); } }
                                        }
                                    }
                                }
                            });
                        },
                        "Export SVG"
                    }
                    button {
                        class: "btn btn-ghost btn-xs",
                        title: "Export as PNG",
                        onclick: move |_| {
                            let p = path_for_png.clone();
                            let c = ctx;
                            spawn(async move {
                                // Export via canvas → PNG data URL
                                let mut eval = document::eval(r#"
                                    (async function() {
                                        if (!window.FlyntExcalidraw || !window.FlyntExcalidraw._api) {
                                            dioxus.send('');
                                            return;
                                        }
                                        const api = window.FlyntExcalidraw._api;
                                        const elements = api.getSceneElements();
                                        const appState = api.getAppState();
                                        // Use exportToBlob from the excalidraw package
                                        try {
                                            const svg = await window.FlyntExcalidraw.exportSvg();
                                            // Convert SVG to PNG via canvas
                                            const img = new Image();
                                            const blob = new Blob([svg], {type: 'image/svg+xml'});
                                            const url = URL.createObjectURL(blob);
                                            img.onload = function() {
                                                const canvas = document.createElement('canvas');
                                                canvas.width = img.width * 2;
                                                canvas.height = img.height * 2;
                                                const ctx = canvas.getContext('2d');
                                                ctx.scale(2, 2);
                                                ctx.drawImage(img, 0, 0);
                                                URL.revokeObjectURL(url);
                                                const dataUrl = canvas.toDataURL('image/png');
                                                // Send just the base64 part
                                                dioxus.send(dataUrl.split(',')[1] || '');
                                            };
                                            img.onerror = function() { dioxus.send(''); };
                                            img.src = url;
                                        } catch(e) {
                                            dioxus.send('');
                                        }
                                    })();
                                "#);
                                if let Ok(b64) = eval.recv::<String>().await {
                                    if !b64.is_empty() {
                                        if let Ok(bytes) = base64_decode(&b64) {
                                            let project = c.project();
                                            let png_path = p.with_extension("png");
                                            let abs = project.root.join(&png_path);
                                            if std::fs::write(&abs, &bytes).is_ok() {
                                                *save_state.write() = "PNG exported";
                                                #[cfg(target_os = "macos")]
                                            { let _ = std::process::Command::new("open").arg("-R").arg(&abs).spawn(); }
                                            #[cfg(target_os = "linux")]
                                            { if let Some(dir) = abs.parent() { let _ = std::process::Command::new("xdg-open").arg(dir).spawn(); } }
                                            }
                                        }
                                    }
                                }
                            });
                        },
                        "Export PNG"
                    }
                }
            div {
                id: "flynt-excalidraw",
                class: "excalidraw-container",
                style: "flex:1;min-height:0;width:100%;",
            }
        }
    }
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    // Simple base64 decoder — no external crate needed
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in input.as_bytes() {
        if b == b'=' || b == b'\n' || b == b'\r' || b == b' ' || b == b'\t' { continue; }
        let val = TABLE.iter().position(|&c| c == b).ok_or("invalid base64")? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Create a new excalidraw drawing: a `.excalidraw` data file plus
/// a `.md` wrapper that embeds it. Returns the `.md` path (indexable by Flynt).
pub fn create_drawing(project_root: &std::path::Path, name: &str) -> anyhow::Result<PathBuf> {
    let drawings_dir = project_root.join("drawings");
    std::fs::create_dir_all(&drawings_dir)?;

    // Create the .excalidraw data file
    let excalidraw_file = format!("{name}.excalidraw");
    let excalidraw_abs = drawings_dir.join(&excalidraw_file);
    let scene = r#"{"type":"excalidraw","version":2,"elements":[],"appState":{"viewBackgroundColor":"transparent","theme":"dark"}}"#;
    std::fs::write(&excalidraw_abs, scene)?;

    // Create a .md wrapper so the document is indexable and openable as a tab
    let md_file = format!("{name}.md");
    let md_rel = PathBuf::from("drawings").join(&md_file);
    let md_abs = project_root.join(&md_rel);
    let md_content = format!("+++\ntitle = \"{name}\"\ntags = [\"drawing\"]\n+++\n\n![[{excalidraw_file}]]\n");
    std::fs::write(&md_abs, md_content)?;

    Ok(md_rel)
}

/// Check if a document's content is purely an excalidraw embed wrapper.
/// Returns the excalidraw file path if so.
pub fn excalidraw_embed_path(content: &str) -> Option<String> {
    // Strip TOML frontmatter (+++...+++) to get the body
    let body = if let Some(rest) = content.strip_prefix("+++\n") {
        if let Some(end) = rest.find("\n+++") {
            rest[end + 4..].trim()
        } else {
            content.trim()
        }
    } else {
        content.trim()
    };

    // Body should be exactly one non-empty line: ![[something.excalidraw]]
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() == 1 {
        let line = lines[0].trim();
        if line.starts_with("![[") && line.ends_with(".excalidraw]]") {
            let inner = &line[3..line.len()-2];
            return Some(inner.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── excalidraw_embed_path ───────────────────────────────────────────

    #[test]
    fn detects_excalidraw_wrapper_with_frontmatter() {
        let content = "+++\ntitle = \"My Drawing\"\ntags = [\"drawing\"]\n+++\n\n![[diagram.excalidraw]]\n";
        assert_eq!(excalidraw_embed_path(content), Some("diagram.excalidraw".into()));
    }

    #[test]
    fn detects_excalidraw_wrapper_minimal() {
        assert_eq!(excalidraw_embed_path("![[test.excalidraw]]\n"), Some("test.excalidraw".into()));
    }

    #[test]
    fn rejects_regular_note_with_excalidraw_embed() {
        let content = "+++\ntitle = \"Note\"\n+++\n\nSome text before.\n\n![[drawing.excalidraw]]\n\nSome text after.\n";
        assert_eq!(excalidraw_embed_path(content), None, "should reject notes with text + embed");
    }

    #[test]
    fn rejects_plain_text() {
        assert_eq!(excalidraw_embed_path("Just a regular note."), None);
    }

    #[test]
    fn rejects_non_excalidraw_embed() {
        assert_eq!(excalidraw_embed_path("![[image.png]]"), None);
    }

    #[test]
    fn rejects_empty_content() {
        assert_eq!(excalidraw_embed_path(""), None);
    }

    #[test]
    fn rejects_frontmatter_only() {
        assert_eq!(excalidraw_embed_path("+++\ntitle = \"Empty\"\n+++\n"), None);
    }

    #[test]
    fn detects_excalidraw_with_expanded_frontmatter() {
        let content = "+++\nid = \"abc-123\"\ntitle = \"Drawing\"\ntags = [\"drawing\"]\naliases = []\nimported_reference = false\n\n[publication]\nenabled = false\nvisibility = \"private\"\n+++\n\n![[Drawing.excalidraw]]\n";
        assert_eq!(excalidraw_embed_path(content), Some("Drawing.excalidraw".into()));
    }

    #[test]
    fn handles_whitespace_around_embed() {
        let content = "+++\ntitle = \"Drawing\"\n+++\n\n  ![[spaced.excalidraw]]  \n";
        assert_eq!(excalidraw_embed_path(content), Some("spaced.excalidraw".into()));
    }

    // ── is_excalidraw ───────────────────────────────────────────────────

    #[test]
    fn is_excalidraw_true() {
        assert!(is_excalidraw(std::path::Path::new("drawings/test.excalidraw")));
        assert!(is_excalidraw(std::path::Path::new("test.excalidraw")));
    }

    #[test]
    fn is_excalidraw_false() {
        assert!(!is_excalidraw(std::path::Path::new("note.md")));
        assert!(!is_excalidraw(std::path::Path::new("image.png")));
        assert!(!is_excalidraw(std::path::Path::new("no-extension")));
    }

    // ── create_drawing ──────────────────────────────────────────────────

    #[test]
    fn create_drawing_produces_both_files() {
        let tmp = TempDir::new().unwrap();
        let md_path = create_drawing(tmp.path(), "Test Diagram").unwrap();

        // .md wrapper exists and is indexable
        assert!(md_path.to_string_lossy().ends_with(".md"));
        let md_abs = tmp.path().join(&md_path);
        assert!(md_abs.exists());

        // .excalidraw data file exists
        let excalidraw_abs = tmp.path().join("drawings/Test Diagram.excalidraw");
        assert!(excalidraw_abs.exists());

        // .md content embeds the .excalidraw file
        let md_content = std::fs::read_to_string(&md_abs).unwrap();
        assert!(md_content.contains("![[Test Diagram.excalidraw]]"));

        // .md content is detected as an excalidraw wrapper
        assert_eq!(
            excalidraw_embed_path(&md_content),
            Some("Test Diagram.excalidraw".into()),
            "the .md wrapper must be detected by excalidraw_embed_path"
        );
    }

    #[test]
    fn create_drawing_valid_scene_json() {
        let tmp = TempDir::new().unwrap();
        create_drawing(tmp.path(), "Valid").unwrap();

        let scene = std::fs::read_to_string(tmp.path().join("drawings/Valid.excalidraw")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&scene).unwrap();
        assert_eq!(parsed["type"], "excalidraw");
        assert_eq!(parsed["version"], 2);
        assert_eq!(parsed["appState"]["theme"], "dark");
    }

    #[test]
    fn create_drawing_multiple_in_same_dir() {
        let tmp = TempDir::new().unwrap();
        create_drawing(tmp.path(), "First").unwrap();
        create_drawing(tmp.path(), "Second").unwrap();

        assert!(tmp.path().join("drawings/First.excalidraw").exists());
        assert!(tmp.path().join("drawings/Second.excalidraw").exists());
        assert!(tmp.path().join("drawings/First.md").exists());
        assert!(tmp.path().join("drawings/Second.md").exists());
    }

    // ── base64_decode ───────────────────────────────────────────────────

    #[test]
    fn base64_decode_basic() {
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello");
    }

    #[test]
    fn base64_decode_no_padding() {
        assert_eq!(base64_decode("SGVsbG8").unwrap(), b"Hello");
    }

    #[test]
    fn base64_decode_with_whitespace() {
        assert_eq!(base64_decode("SGVs\nbG8=").unwrap(), b"Hello");
    }

    #[test]
    fn base64_decode_empty() {
        assert_eq!(base64_decode("").unwrap(), b"");
    }

    #[test]
    fn base64_decode_invalid_char() {
        assert!(base64_decode("!!!").is_err());
    }

    // ── round-trip: create → detect → delete ────────────────────────────

    #[test]
    fn full_drawing_lifecycle() {
        let tmp = TempDir::new().unwrap();

        // Create
        let md_path = create_drawing(tmp.path(), "Lifecycle Test").unwrap();
        let md_abs = tmp.path().join(&md_path);
        let excalidraw_abs = tmp.path().join("drawings/Lifecycle Test.excalidraw");
        assert!(md_abs.exists());
        assert!(excalidraw_abs.exists());

        // Detect
        let content = std::fs::read_to_string(&md_abs).unwrap();
        let detected = excalidraw_embed_path(&content);
        assert_eq!(detected, Some("Lifecycle Test.excalidraw".into()));

        // Delete (simulate sidebar delete — both files)
        std::fs::remove_file(&md_abs).unwrap();
        std::fs::remove_file(&excalidraw_abs).unwrap();
        assert!(!md_abs.exists());
        assert!(!excalidraw_abs.exists());
    }
}
