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

    // Initialize Excalidraw when component mounts
    let path_for_save = path.clone();
    use_effect(move || {
        let data = content.read().clone();
        let escaped = serde_json::to_string(&data).unwrap_or("\"{}\"".into());

        let js = format!(r#"
            (function() {{
                function init() {{
                    const container = document.getElementById('codex-excalidraw');
                    if (!container || !window.CodexExcalidraw) {{
                        setTimeout(init, 50);
                        return;
                    }}
                    window.CodexExcalidraw.mount('codex-excalidraw', {escaped}, function(data) {{
                        // Store latest scene data for save
                        window._excalidrawLatest = data;
                    }});
                }}
                init();
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
                }
            }
        });
    });

    rsx! {
        div { class: "excalidraw-pane",
            div { class: "excalidraw-topbar",
                span { class: "excalidraw-title",
                    "{path.file_stem().and_then(|s| s.to_str()).unwrap_or(\"Drawing\")}"
                }
                if !save_state.read().is_empty() {
                    span { class: "save-status saved", "{save_state}" }
                }
            }
            div { id: "codex-excalidraw", class: "excalidraw-container" }
        }
    }
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
