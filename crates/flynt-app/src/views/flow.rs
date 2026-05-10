//! Node-flow viewer — renders `.flow` files in the webview via react-flow.
//!
//! Phase 2 is read-only: the bundle mounts, the operator can pan/zoom/select
//! and inspect the graph the agent (or a human) authored, but no edits flow
//! back to disk. Phase 3 will add a debounced save loop mirroring how
//! `ExcalidrawView` handles changes.
//!
//! The bundle (`assets/vendor/flow.bundle.js`) is built externally and
//! committed; see `crates/flynt-app/build/flow/README.md` for the build
//! commands. It exposes `window.FlyntFlow.{mount, unmount}` — the API
//! shape mirrors `window.FlyntExcalidraw` so the mount pattern below is
//! a near-copy of the Excalidraw view's first 80 lines.

use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use std::path::PathBuf;

/// Whether a path points at a `.flow` file. Used by the notes view to
/// dispatch to `FlowView` instead of the markdown editor.
pub fn is_flow(path: &std::path::Path) -> bool {
    path.extension().map(|e| e == "flow").unwrap_or(false)
}

#[component]
pub fn FlowView(path: PathBuf) -> Element {
    let ctx = use_context::<AppContext>();
    let path_load = path.clone();

    // Load and parse the flow file. We hand the JSON body to the bundle;
    // the frontmatter is already parsed by the indexer, so we don't
    // re-surface it here. Errors → empty body so the bundle renders an
    // empty canvas rather than failing silently.
    let flow_json = use_memo(move || {
        let project = ctx.project();
        let abs = project.root.join(&path_load);
        let raw = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => return r#"{"meta":{},"nodes":[],"edges":[]}"#.to_string(),
        };
        match flynt_flow::parse_flow(&raw) {
            Ok(doc) => serde_json::to_string(&doc.flow)
                .unwrap_or_else(|_| r#"{"meta":{},"nodes":[],"edges":[]}"#.to_string()),
            Err(e) => {
                tracing::warn!(error = %e, path = %abs.display(), "failed to parse .flow file");
                r#"{"meta":{},"nodes":[],"edges":[]}"#.to_string()
            }
        }
    });

    // Force-fit the layout so react-flow can measure its container.
    // Same pattern as `ExcalidrawView::use_effect` — the host page's
    // layout primitives don't grant the bundle the height it needs by
    // default; the dispatched `resize` event nudges react-flow to remount.
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

            // Cleanup hook fires on view unmount via dioxus' effect drop.
            window._flowCleanup = function() {
                if (window.FlyntFlow) { try { window.FlyntFlow.unmount(); } catch(e) {} }
                var mc = document.querySelector('.main-content');
                if (mc) { mc.style.overflow = ''; mc.style.display = ''; mc.style.flexDirection = ''; }
                var np = document.querySelector('.notes-pane');
                if (np) { np.style.overflow = ''; np.style.padding = ''; np.style.display = ''; np.style.flexDirection = ''; np.style.flex = ''; np.style.minHeight = ''; }
            };
            "#,
        );
    });

    // Mount the bundle once the DOM has the target node and the bundle
    // global is ready. Polling pattern matches `FlyntExcalidraw` — the
    // bundle script is loaded eagerly via `document::Script` in app.rs,
    // but the global may attach a tick later than the view renders.
    use_effect(move || {
        let data = flow_json.read().clone();
        // Embed the JSON as a JS string literal — JSON.parse handles the
        // unescape, and we avoid template-string interpolation footguns.
        let escaped = serde_json::to_string(&data).unwrap_or_else(|_| "\"{}\"".into());

        let js = format!(
            r#"
            (function() {{
                function tryMount() {{
                    var container = document.getElementById('flynt-flow');
                    if (!container) {{ setTimeout(tryMount, 50); return; }}
                    if (!window.FlyntFlow) {{ setTimeout(tryMount, 100); return; }}
                    window.FlyntFlow.mount('flynt-flow', {escaped}, {{ readOnly: true }});
                }}
                tryMount();
            }})();
            "#
        );
        document::eval(&js);
    });

    rsx! {
        div {
            class: "flow-pane",
            style: "display:flex;flex-direction:column;flex:1;min-height:0;width:100%;position:relative;background:#020617;",
            div {
                id: "flynt-flow",
                style: "flex:1;min-height:0;width:100%;",
            }
        }
    }
}
