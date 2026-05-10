//! Node-flow viewer/editor — renders `.flow` files in the webview via
//! react-flow.
//!
//! Phase 3: editable. Operator can drag nodes, draw new edges, delete
//! selected elements via Backspace/Delete; changes flow back to disk via
//! a debounced (~500ms) save loop. Cmd+S triggers an immediate flush.
//!
//! The bundle (`assets/vendor/flow.bundle.js`) is built externally and
//! committed; see `crates/flynt-app/build/flow/README.md` for the build
//! commands. It exposes `window.FlyntFlow.{mount, unmount}` — the API
//! shape mirrors `window.FlyntExcalidraw` so the mount pattern below is
//! a near-copy of the Excalidraw view.
//!
//! ## Save loop architecture
//!
//! 1. JS bundle's `onChange` callback fires (debounced) when the
//!    operator mutates the graph.
//! 2. The callback hands a JSON-stringified `Flow` body to a queue
//!    (`window._flowSaveQueue`).
//! 3. The Rust side runs a draining loop in a tokio task: poll the
//!    queue every 200ms via `dioxus.send`, parse, build a `Flow`, write
//!    via `flynt_flow::save_flow`. The document id from the original
//!    parse is reused so the indexer's identity stays stable.
//! 4. Save status (`""` | `"saving"` | `"saved"` | `"error"`) is held
//!    in a Dioxus signal and rendered as a small badge above the canvas.
//!
//! ## Known limitations carried over from ExcalidrawView
//!
//! - Single-tab assumption: the mount target id is hardcoded
//!   (`flynt-flow`). Two `.flow` tabs open simultaneously will collide.
//! - File-watch reactivity: an external write (e.g., Phase 4 agent
//!   tool) does not refresh the open viewer; the operator must close
//!   and reopen.
//! - Last-writer-wins: if the agent and operator edit the same file
//!   concurrently, the in-memory state of whoever flushes second wins.
//!   No CRDT, no merge UI yet.

use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use std::path::PathBuf;
use uuid::Uuid;

/// Whether a path points at a `.flow` file. Used by the notes view to
/// dispatch to `FlowView` instead of the markdown editor.
pub fn is_flow(path: &std::path::Path) -> bool {
    path.extension().map(|e| e == "flow").unwrap_or(false)
}

const EMPTY_FLOW_JSON: &str = r#"{"meta":{},"nodes":[],"edges":[]}"#;

/// Loaded body + the id we need to preserve on save so the indexer
/// keeps the same document identity. `PartialEq` is required by Dioxus'
/// `Memo` so dependents only re-run when content actually changes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LoadedFlow {
    /// JSON body to hand to the bundle.
    body_json: String,
    /// Document id stamped into frontmatter on save. `None` when the
    /// file didn't exist (newly created or unreadable) — first save
    /// allocates a fresh id.
    id: Option<Uuid>,
}

#[component]
pub fn FlowView(path: PathBuf) -> Element {
    let ctx = use_context::<AppContext>();
    let path_load = path.clone();

    // Load + parse once per FlowView mount. We keep both the body JSON
    // (for the bundle) and the document id (for save).
    let loaded: Memo<LoadedFlow> = use_memo(move || {
        let project = ctx.project();
        let abs = project.root.join(&path_load);
        let raw = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => {
                return LoadedFlow {
                    body_json: EMPTY_FLOW_JSON.to_string(),
                    id: None,
                }
            }
        };
        match flynt_flow::parse_flow(&raw) {
            Ok(doc) => LoadedFlow {
                body_json: serde_json::to_string(&doc.flow)
                    .unwrap_or_else(|_| EMPTY_FLOW_JSON.to_string()),
                id: Some(doc.id),
            },
            Err(e) => {
                tracing::warn!(error = %e, path = %abs.display(), "failed to parse .flow file");
                LoadedFlow {
                    body_json: EMPTY_FLOW_JSON.to_string(),
                    id: None,
                }
            }
        }
    });

    // Status badge state. Three transitions: idle ("") → "saving" while
    // a write is in flight → "saved" for ~2s → idle. "error" sticks
    // until the next successful save.
    let mut save_state = use_signal(|| "");

    // Layout fixes + cleanup hook (same approach as Excalidraw).
    use_effect(move || {
        document::eval(
            r#"
            (function fix() {
                var mc = document.querySelector('.main-content');
                if (mc) { mc.style.overflow = 'hidden'; mc.style.display = 'flex'; mc.style.flexDirection = 'column'; }
                var np = document.querySelector('.notes-pane');
                if (np) { np.style.overflow = 'hidden'; np.style.padding = '0'; np.style.display = 'flex'; np.style.flexDirection = 'column'; np.style.flex = '1'; np.style.minHeight = '0'; }
                requestAnimationFrame(function() { window.dispatchEvent(new Event('resize')); });
                setTimeout(function() { window.dispatchEvent(new Event('resize')); }, 200);
            })();

            window._flowCleanup = function() {
                if (window.FlyntFlow) { try { window.FlyntFlow.unmount(); } catch(e) {} }
                window._flowSaveQueue = [];
                window._flowOnChange = null;
                // Signal the drain loop to exit on its next iteration.
                // Without this the loop runs forever (JS leak across
                // every FlowView open/close) — the Rust-side spawn task
                // is cancelled by Dioxus scope drop, but the JS poller
                // has no scope-binding.
                window._flowDrainActive = false;
                var mc = document.querySelector('.main-content');
                if (mc) { mc.style.overflow = ''; mc.style.display = ''; mc.style.flexDirection = ''; }
                var np = document.querySelector('.notes-pane');
                if (np) { np.style.overflow = ''; np.style.padding = ''; np.style.display = ''; np.style.flexDirection = ''; np.style.flex = ''; np.style.minHeight = ''; }
            };
            "#,
        );
    });

    use_drop(|| {
        document::eval(
            r#"
            if (typeof window._flowCleanup === 'function') {
                try { window._flowCleanup(); } catch (e) { console.warn('[FlowView] cleanup error', e); }
            }
            "#,
        );
    });

    // Mount the bundle with an onChange callback. The callback enqueues
    // the JSON body into `window._flowSaveQueue`; the Rust drain loop
    // (next effect) picks it up. Going through a queue rather than a
    // direct `dioxus.send` keeps the JS callback synchronous (so
    // react-flow's debounce doesn't await the Rust round-trip) and
    // gives us a single point to flush from at unmount.
    use_effect(move || {
        let data = loaded.read().body_json.clone();
        let escaped = serde_json::to_string(&data).unwrap_or_else(|_| "\"{}\"".into());

        let js = format!(
            r#"
            (function() {{
                window._flowSaveQueue = window._flowSaveQueue || [];
                window._flowOnChange = function(body) {{
                    window._flowSaveQueue.push(body);
                }};

                var attempts = 0;
                var MAX_ATTEMPTS = 100;
                function tryMount() {{
                    if (++attempts > MAX_ATTEMPTS) {{
                        console.error('[FlowView] bundle did not become available after',
                            MAX_ATTEMPTS, 'attempts — flow.bundle.js may be missing or broken');
                        return;
                    }}
                    var container = document.getElementById('flynt-flow');
                    if (!container) {{ setTimeout(tryMount, 50); return; }}
                    if (!window.FlyntFlow) {{ setTimeout(tryMount, 100); return; }}
                    window.FlyntFlow.mount('flynt-flow', {escaped}, {{
                        readOnly: false,
                        onChange: window._flowOnChange,
                    }});
                }}
                tryMount();
            }})();
            "#
        );
        document::eval(&js);
    });

    // Drain loop: pulls JSON bodies off the JS queue, writes them to
    // disk via flynt-flow's serializer. Keeps the bundle's onChange
    // synchronous and lets us batch redundant writes (only the latest
    // queued body matters; older ones drop).
    //
    // Lifetime contract — important for future maintainers:
    //   * The JS drain function is scope-less; we terminate it via the
    //     `window._flowDrainActive` sentinel set false in `_flowCleanup`.
    //   * The Rust `spawn` task is scope-bound to this component
    //     (Dioxus drops it on unmount), but it lives across multiple
    //     re-runs of this `use_effect` if anything inside the closure
    //     ever reads a signal that changes. **Don't read signals inside
    //     this `use_effect` body.** The current code reads them only
    //     inside the spawned future, which is fine.
    let path_save = path.clone();
    use_effect(move || {
        let mut bridge = document::eval(
            r#"
            window._flowDrainActive = true;
            (async function drain() {
                while (window._flowDrainActive) {
                    if (window._flowSaveQueue && window._flowSaveQueue.length > 0) {
                        // Keep only the latest queued body — older ones
                        // are stale snapshots of the same edit session.
                        var latest = window._flowSaveQueue[window._flowSaveQueue.length - 1];
                        window._flowSaveQueue = [];
                        try { dioxus.send(latest); }
                        catch (e) { console.warn('[FlowView] dioxus.send failed', e); }
                    } else {
                        await new Promise(function(r) { setTimeout(r, 200); });
                    }
                }
            })();
            "#,
        );

        let p = path_save.clone();
        let c = ctx;
        let l = loaded;
        spawn(async move {
            // Document id captured from the initial parse. If the file
            // didn't exist on first read, allocate fresh once and reuse
            // — the indexer treats the resulting identity as canonical.
            let mut doc_id: Option<Uuid> = l.read().id;

            loop {
                let Ok(body_json) = bridge.recv::<String>().await else {
                    break;
                };

                *save_state.write() = "saving";

                // Build a Flow from the body JSON. If the bundle ever
                // sends an unparseable payload we surface "error" rather
                // than silently dropping — Phase 3 trusts the bundle but
                // we want the bug to be visible.
                let flow: flynt_flow::Flow = match serde_json::from_str(&body_json) {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::error!(error = %e, "FlowView: bundle sent invalid JSON body");
                        *save_state.write() = "error";
                        continue;
                    }
                };

                let project = c.project();
                let abs = project.root.join(&p);
                let id = *doc_id.get_or_insert_with(Uuid::new_v4);

                let abs_for_blocking = abs.clone();
                let result = tokio::task::spawn_blocking(move || {
                    flynt_flow::save_flow(&abs_for_blocking, &flow, Some(id))
                })
                .await;

                match result {
                    Ok(Ok(())) => {
                        *save_state.write() = "saved";
                        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                        if *save_state.read() == "saved" {
                            *save_state.write() = "";
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::error!(error = %e, path = %abs.display(), "save_flow failed");
                        *save_state.write() = "error";
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "save task panicked");
                        *save_state.write() = "error";
                    }
                }
            }
        });
    });

    rsx! {
        div {
            class: "flow-pane",
            style: "display:flex;flex-direction:column;flex:1;min-height:0;width:100%;position:relative;background:#020617;",
            // Status badge — same visual idiom as the Excalidraw view's
            // "saved" pill so the muscle memory is consistent.
            div {
                class: "flow-overlay-actions",
                style: "position:absolute;top:8px;right:12px;z-index:5;pointer-events:none;",
                span {
                    style: format!(
                        "font-size:11px;color:#94a3b8;background:rgba(15,23,42,0.85);padding:3px 8px;border-radius:4px;border:1px solid #1e293b;opacity:{};transition:opacity .15s;",
                        if save_state.read().is_empty() { 0.0 } else { 1.0 }
                    ),
                    "{save_state}"
                }
            }
            div {
                id: "flynt-flow",
                style: "flex:1;min-height:0;width:100%;",
            }
        }
    }
}
