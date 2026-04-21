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
        let vault = ctx.vault();
        let abs = vault.root.join(&path_load);
        std::fs::read_to_string(&abs).unwrap_or_else(|_| {
            // New drawing — empty scene
            r#"{"type":"excalidraw","version":2,"elements":[],"appState":{}}"#.to_string()
        })
    });

    // Force parent layout for excalidraw — CSS :has() may not work in all WebKit versions
    use_effect(move || {
        document::eval(r#"
            setTimeout(function() {
                var mc = document.querySelector('.main-content');
                if (mc) { mc.style.overflow = 'hidden'; mc.style.display = 'flex'; mc.style.flexDirection = 'column'; }
                var np = document.querySelector('.notes-pane');
                if (np) { np.style.overflow = 'hidden'; np.style.padding = '0'; np.style.display = 'flex'; np.style.flexDirection = 'column'; np.style.flex = '1'; np.style.minHeight = '0'; }
            }, 50);
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
                    const container = document.getElementById('codex-excalidraw');
                    if (!container) {{ setTimeout(tryMount, 50); return; }}
                    if (!window.CodexExcalidraw) {{ setTimeout(tryMount, 100); return; }}

                    window.CodexExcalidraw.mount('codex-excalidraw', {escaped}, function(data) {{
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
            const origOnChange = window.CodexExcalidraw?._onChange;
            if (window.CodexExcalidraw) {
                const prevMount = window.CodexExcalidraw.mount.bind(window.CodexExcalidraw);
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
                let vault = c.vault();
                let abs = vault.root.join(&p);
                if std::fs::write(&abs, &data).is_ok() {
                    *save_state.write() = "saved";
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
            style: "display:flex;flex-direction:column;flex:1;min-height:0;width:100%;",
            div { class: "excalidraw-topbar",
                span { class: "excalidraw-title",
                    "{path.file_stem().and_then(|s| s.to_str()).unwrap_or(\"Drawing\")}"
                }
                div { class: "excalidraw-actions",
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
                                        if (window.CodexExcalidraw && window.CodexExcalidraw._api) {
                                            const svg = await window.CodexExcalidraw.exportSvg();
                                            dioxus.send(svg || '');
                                        } else {
                                            dioxus.send('');
                                        }
                                    })();
                                "#);
                                if let Ok(svg) = eval.recv::<String>().await {
                                    if !svg.is_empty() {
                                        let vault = c.vault();
                                        let svg_path = p.with_extension("svg");
                                        let abs = vault.root.join(&svg_path);
                                        if std::fs::write(&abs, &svg).is_ok() {
                                            *save_state.write() = "SVG exported";
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
                                        if (!window.CodexExcalidraw || !window.CodexExcalidraw._api) {
                                            dioxus.send('');
                                            return;
                                        }
                                        const api = window.CodexExcalidraw._api;
                                        const elements = api.getSceneElements();
                                        const appState = api.getAppState();
                                        // Use exportToBlob from the excalidraw package
                                        try {
                                            const svg = await window.CodexExcalidraw.exportSvg();
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
                                            let vault = c.vault();
                                            let png_path = p.with_extension("png");
                                            let abs = vault.root.join(&png_path);
                                            if std::fs::write(&abs, &bytes).is_ok() {
                                                *save_state.write() = "PNG exported";
                                            }
                                        }
                                    }
                                }
                            });
                        },
                        "Export PNG"
                    }
                }
            }
            div {
                id: "codex-excalidraw",
                class: "excalidraw-container",
                style: "flex:1;min-height:0;width:100%;position:relative;",
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
/// a `.md` wrapper that embeds it. Returns the `.md` path (indexable by Codex).
pub fn create_drawing(vault_root: &std::path::Path, name: &str) -> anyhow::Result<PathBuf> {
    let drawings_dir = vault_root.join("drawings");
    std::fs::create_dir_all(&drawings_dir)?;

    // Create the .excalidraw data file
    let excalidraw_file = format!("{name}.excalidraw");
    let excalidraw_abs = drawings_dir.join(&excalidraw_file);
    let scene = r#"{"type":"excalidraw","version":2,"elements":[],"appState":{"viewBackgroundColor":"transparent","theme":"dark"}}"#;
    std::fs::write(&excalidraw_abs, scene)?;

    // Create a .md wrapper so the document is indexable and openable as a tab
    let md_file = format!("{name}.md");
    let md_rel = PathBuf::from("drawings").join(&md_file);
    let md_abs = vault_root.join(&md_rel);
    let md_content = format!("+++\ntitle = \"{name}\"\ntags = [\"drawing\"]\n+++\n\n![[{excalidraw_file}]]\n");
    std::fs::write(&md_abs, md_content)?;

    Ok(md_rel)
}

/// Check if a document's content is purely an excalidraw embed wrapper.
/// Returns the excalidraw file path if so.
pub fn excalidraw_embed_path(content: &str) -> Option<String> {
    let body = content.lines()
        .skip_while(|l| l.starts_with("+++") || (!l.is_empty() && !l.starts_with("!")))
        .filter(|l| !l.trim().is_empty() && !l.starts_with("+++"))
        .collect::<Vec<_>>();

    if body.len() == 1 {
        let line = body[0].trim();
        if line.starts_with("![[") && line.ends_with(".excalidraw]]") {
            let inner = &line[3..line.len()-2];
            return Some(inner.to_string());
        }
    }
    None
}
