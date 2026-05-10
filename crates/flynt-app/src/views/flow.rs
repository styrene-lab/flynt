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
//!
//! ## Known limitations carried over from ExcalidrawView
//!
//! - Single-tab assumption: the mount target id is hardcoded
//!   (`flynt-flow`). Two `.flow` tabs open simultaneously will collide
//!   on `getElementById`. Multi-tab support is a cross-view refactor.
//! - File-watch reactivity: an external write to the open file (e.g.,
//!   from an agent tool in Phase 4) does not refresh the view; the
//!   operator must close and reopen the tab. Wiring the project
//!   watcher into the memo lands when Phase 4 actually exercises it.

use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use std::path::PathBuf;

/// Whether a path points at a `.flow` file. Used by the notes view to
/// dispatch to `FlowView` instead of the markdown editor.
pub fn is_flow(path: &std::path::Path) -> bool {
    path.extension().map(|e| e == "flow").unwrap_or(false)
}

const EMPTY_FLOW_JSON: &str = r#"{"meta":{},"nodes":[],"edges":[]}"#;

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
            Err(_) => return EMPTY_FLOW_JSON.to_string(),
        };
        match flynt_flow::parse_flow(&raw) {
            Ok(doc) => serde_json::to_string(&doc.flow)
                .unwrap_or_else(|_| EMPTY_FLOW_JSON.to_string()),
            Err(e) => {
                tracing::warn!(error = %e, path = %abs.display(), "failed to parse .flow file");
                EMPTY_FLOW_JSON.to_string()
            }
        }
    });

    // Define the cleanup-on-unmount JS once. Pre-existing pattern in
    // `ExcalidrawView` defined `window._excalidrawCleanup` but never
    // invoked it — layout overrides leaked across navigation, the React
    // tree never unmounted. We do better here by invoking the cleanup
    // from `use_drop` below so it actually runs when the view unmounts.
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
                var mc = document.querySelector('.main-content');
                if (mc) { mc.style.overflow = ''; mc.style.display = ''; mc.style.flexDirection = ''; }
                var np = document.querySelector('.notes-pane');
                if (np) { np.style.overflow = ''; np.style.padding = ''; np.style.display = ''; np.style.flexDirection = ''; np.style.flex = ''; np.style.minHeight = ''; }
            };
            "#,
        );
    });

    // Run the cleanup when the view unmounts (route change, tab close,
    // app shutdown). Without this, repeated open/close of `.flow` tabs
    // accumulated React roots in the DOM and leaked the layout overrides.
    use_drop(|| {
        document::eval(
            r#"
            if (typeof window._flowCleanup === 'function') {
                try { window._flowCleanup(); } catch (e) { console.warn('[FlowView] cleanup error', e); }
            }
            "#,
        );
    });

    // Mount the bundle once the DOM has the target node and the bundle
    // global is ready. Polling pattern matches `FlyntExcalidraw` with
    // one improvement: bounded retries so a missing/broken bundle fails
    // loudly rather than spinning forever (caught a 1.2MB bundle file
    // server stall during testing — without the cap it just hangs).
    use_effect(move || {
        let data = flow_json.read().clone();
        // Double-encode: serde_json wraps the JSON-string in another
        // JSON-string literal so it can be safely embedded in JS source.
        // The bundle does `JSON.parse(escaped)` to recover the original.
        let escaped = serde_json::to_string(&data).unwrap_or_else(|_| "\"{}\"".into());

        let js = format!(
            r#"
            (function() {{
                var attempts = 0;
                var MAX_ATTEMPTS = 100; // ≈10s at 100ms — well past app boot
                function tryMount() {{
                    if (++attempts > MAX_ATTEMPTS) {{
                        console.error('[FlowView] bundle did not become available after',
                            MAX_ATTEMPTS, 'attempts — flow.bundle.js may be missing or broken');
                        return;
                    }}
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
