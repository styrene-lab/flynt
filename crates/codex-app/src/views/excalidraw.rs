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

    // Initialize Excalidraw when component mounts — lazy-loads the bundle
    let path_for_save = path.clone();
    use_effect(move || {
        let data = content.read().clone();
        let escaped = serde_json::to_string(&data).unwrap_or("\"{}\"".into());

        let js = format!(r#"
            (function() {{
                function loadAndInit() {{
                    const container = document.getElementById('codex-excalidraw');
                    if (!container) {{ setTimeout(loadAndInit, 50); return; }}

                    if (window.CodexExcalidraw) {{
                        // Already loaded — mount directly
                        window.CodexExcalidraw.mount('codex-excalidraw', {escaped}, function(data) {{
                            window._excalidrawLatest = data;
                        }});
                        return;
                    }}

                    // Lazy-load CSS
                    if (!document.getElementById('excalidraw-css')) {{
                        const link = document.createElement('link');
                        link.id = 'excalidraw-css';
                        link.rel = 'stylesheet';
                        link.href = '/assets/vendor/excalidraw.css';
                        document.head.appendChild(link);
                    }}

                    // Lazy-load JS bundle
                    const script = document.createElement('script');
                    script.src = '/assets/vendor/excalidraw.bundle.js';
                    script.onload = function() {{
                        // Wait for CodexExcalidraw to be defined
                        function waitForAPI() {{
                            if (window.CodexExcalidraw) {{
                                window.CodexExcalidraw.mount('codex-excalidraw', {escaped}, function(data) {{
                                    window._excalidrawLatest = data;
                                }});
                            }} else {{
                                setTimeout(waitForAPI, 50);
                            }}
                        }}
                        waitForAPI();
                    }};
                    document.head.appendChild(script);
                }}
                loadAndInit();
            }})();
        "#);
        document::eval(&js);
    });

    // Save handler — Cmd+S triggers save via the bridge
    let path_save = path_for_save.clone();
    use_effect(move || {
        let js = r#"
            document.addEventListener('keydown', function _excSave(e) {
                if ((e.metaKey || e.ctrlKey) && e.key === 's') {
                    e.preventDefault();
                    if (window._excalidrawLatest) {
                        window._codexNotify && window._codexNotify('excalidraw-save', window._excalidrawLatest);
                    }
                }
            });
        "#;
        document::eval(js);

        // Poll for save messages
        let mut eval = document::eval(r#"
            if (!window._excalidrawSaveQueue) {
                window._excalidrawSaveQueue = [];
                const origNotify = window._codexNotify;
                window._codexNotify = function(type, data) {
                    if (type === 'excalidraw-save') {
                        window._excalidrawSaveQueue.push(data);
                    } else if (origNotify) {
                        origNotify(type, data);
                    }
                };
            }
            async function _excDrain() {
                while (true) {
                    if (window._excalidrawSaveQueue && window._excalidrawSaveQueue.length > 0) {
                        const data = window._excalidrawSaveQueue.shift();
                        dioxus.send(data);
                    } else {
                        await new Promise(r => setTimeout(r, 100));
                    }
                }
            }
            _excDrain();
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

                    // Auto-export SVG — use the saved data directly
                    // (no eval race — we already have the latest scene JSON)
                    let svg_path = abs.with_extension("svg");
                    // Defer SVG export slightly to let Excalidraw update internal state
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let mut svg_eval = document::eval(r#"
                        (async function() {
                            if (window.CodexExcalidraw && window.CodexExcalidraw._api) {
                                const svg = await window.CodexExcalidraw.exportSvg();
                                dioxus.send(svg || '');
                            } else { dioxus.send(''); }
                        })();
                    "#);
                    if let Ok(svg) = svg_eval.recv::<String>().await {
                        if !svg.is_empty() {
                            let _ = std::fs::write(&svg_path, &svg);
                        }
                    }
                }
            }
        });
    });

    let path_for_svg = path.clone();
    let path_for_png = path.clone();

    rsx! {
        div { class: "excalidraw-pane",
            div { class: "excalidraw-topbar",
                span { class: "excalidraw-title",
                    "{path.file_stem().and_then(|s| s.to_str()).unwrap_or(\"Drawing\")}"
                }
                div { class: "excalidraw-actions",
                    if !save_state.read().is_empty() {
                        span { class: "save-status saved", "{save_state}" }
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
            div { id: "codex-excalidraw", class: "excalidraw-container" }
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

/// Create a new empty .excalidraw file and return its path.
pub fn create_drawing(vault_root: &std::path::Path, name: &str) -> anyhow::Result<PathBuf> {
    let filename = format!("{name}.excalidraw");
    let path = PathBuf::from(&filename);
    let abs = vault_root.join(&path);
    let content = r#"{"type":"excalidraw","version":2,"elements":[],"appState":{"viewBackgroundColor":"transparent","theme":"dark"}}"#;
    std::fs::write(&abs, content)?;
    Ok(path)
}
