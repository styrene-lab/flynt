//! Node-flow viewer/editor — renders `.flow` files in the webview via
//! react-flow.
//!
//! Phase 3: editable. Operator can drag nodes, draw new edges, delete
//! selected elements via Backspace/Delete; changes flow back to disk via
//! a debounced (~500ms) save loop. Cmd+S triggers an immediate flush.
//!
//! Phase 4-followup: file-watch reactivity. When an external process
//! (agent tool, second editor, file sync) writes to the open `.flow`
//! file, the viewer reloads automatically. Own-save echoes are filtered
//! out (the watcher fires for our own writes too).
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
//! 4. Save status (`""` | `"saving"` | `"saved"` | `"refreshed"` |
//!    `"error"`) is held in a Dioxus signal and rendered as a small
//!    badge above the canvas.
//!
//! ## Watcher subscription
//!
//! Subscribed to `ctx.project_events()`. When an event fires for our
//! path, the subscriber waits 200ms (debounce, also covers the
//! save→fsync→FSEvents lag), checks `last_self_save_at` to suppress
//! own-save echoes (1500ms window), reloads the file, and writes the
//! new content into the `loaded` signal — which the mount effect
//! observes and remounts the bundle.
//!
//! Path canonicalization: we canonicalize the project ROOT once at
//! subscribe-time (always exists) and join the relative path against
//! it, rather than canonicalizing the file directly (which fails
//! before the file is created — common when an agent will produce it).
//! FSEvents emits canonicalized paths so this keeps comparison cheap.
//!
//! ## Known limitations
//!
//! - **Single-tab assumption**: the mount target id is hardcoded
//!   (`flynt-flow`). Two `.flow` tabs open simultaneously will collide.
//! - **External writes win when idle**: if the operator has unsaved
//!   local changes when an agent's write lands, the operator's
//!   in-memory state is replaced. The working contract is "agent
//!   waits for tool result before issuing the next write," which
//!   keeps the operator + agent from racing in practice.
//! - **Save-in-flight reloads are dropped**: if an external write
//!   arrives while our save loop is mid-write, the reload is skipped
//!   (otherwise the operator's pending content races with disk and
//!   the agent's write loses anyway, with extra UI flicker). Net
//!   effect: a narrow class of agent writes can be silently
//!   overwritten by a concurrently-completing operator save. Operator
//!   can resync by triggering any further save (or by closing/
//!   reopening the tab).

use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use flynt_store::watcher::ProjectChangeEvent;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Whether a path points at a `.flow` file. Used by the notes view to
/// dispatch to `FlowView` instead of the markdown editor.
pub fn is_flow(path: &std::path::Path) -> bool {
    path.extension().map(|e| e == "flow").unwrap_or(false)
}

const EMPTY_FLOW_JSON: &str = r#"{"meta":{},"nodes":[],"edges":[]}"#;

/// How long after an own-save to suppress incoming watcher events. The
/// FSEvents round-trip is typically <500ms; 1500ms is a comfortable
/// margin without making the viewer feel laggy when an external write
/// arrives a moment after our own save.
const SELF_SAVE_ECHO_WINDOW: Duration = Duration::from_millis(1500);

/// Loaded body + the id we need to preserve on save so the indexer
/// keeps the same document identity. `PartialEq` so a `Signal` doesn't
/// re-trigger dependents when reloads produce identical content.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LoadedFlow {
    /// JSON body to hand to the bundle.
    body_json: String,
    /// Document id stamped into frontmatter on save. `None` when the
    /// file didn't exist (newly created or unreadable) — first save
    /// allocates a fresh id.
    id: Option<Uuid>,
}

/// Read + parse a `.flow` file into the shape we hand the bundle.
/// Errors collapse to an empty body + None id; the bundle still mounts
/// (renders an empty canvas) and the operator can fix the file
/// out-of-band. Logs at warn level so a parse regression is visible
/// in the trace stream.
fn read_and_parse_flow(project_root: &Path, rel_path: &Path) -> LoadedFlow {
    let abs = project_root.join(rel_path);
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
}

#[component]
pub fn FlowView(path: PathBuf) -> Element {
    let ctx = use_context::<AppContext>();
    let path_load = path.clone();

    // `loaded` is now a Signal (was Memo) so the watcher effect can write
    // to it when an external write lands. Initial value comes from the
    // first read; subsequent updates either originate from external
    // file-watch events or from the operator opening a different file
    // (which triggers a fresh FlowView mount).
    let mut loaded = use_signal(|| {
        let project = ctx.project();
        read_and_parse_flow(&project.root, &path_load)
    });

    // Status badge: idle ("") → "saving" while a write is in flight →
    // "saved" for ~2s → idle. "refreshed" fires when an external write
    // landed and we reloaded. "error" sticks until the next clean op.
    let mut save_state = use_signal(|| "");

    // Tracks when WE last wrote to disk. The watcher subscription uses
    // this to filter out own-save echoes (FSEvents fires for any write,
    // including ours). `Instant` isn't `PartialEq` for our purposes
    // since we only compare via elapsed; storing as Option keeps the
    // initial "haven't saved yet" state explicit.
    let last_self_save_at = use_signal(|| Option::<Instant>::None);

    // ── Layout fixes + cleanup hook ────────────────────────────────────────
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

    // ── Mount the bundle ───────────────────────────────────────────────────
    //
    // This effect re-runs whenever `loaded` changes — which is intended:
    // an external write should remount the bundle with the new content.
    // Operator's unsaved local edits are replaced (last-write-wins,
    // documented above).
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

    // ── Save loop ──────────────────────────────────────────────────────────
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
        let mut last_self_save = last_self_save_at;
        spawn(async move {
            // Document id captured from the initial parse. If the file
            // didn't exist on first read, allocate fresh once and reuse.
            let mut doc_id: Option<Uuid> = loaded.read().id;

            loop {
                let Ok(body_json) = bridge.recv::<String>().await else {
                    break;
                };

                *save_state.write() = "saving";

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
                        // Stamp the self-save timestamp BEFORE the badge
                        // transitions — the watcher subscription will
                        // fire imminently with this write's event, and
                        // we want the suppression window already open.
                        *last_self_save.write() = Some(Instant::now());
                        *save_state.write() = "saved";
                        tokio::time::sleep(Duration::from_millis(2000)).await;
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

    // ── Watcher subscription ───────────────────────────────────────────────
    //
    // Subscribe to project_events. On event for our path, debounce
    // (200ms — covers FSEvents emit lag and lets us coalesce rapid
    // writes), filter own-save echoes, then reload + push into `loaded`.
    let path_watch = path.clone();
    use_effect(move || {
        let project_events = ctx.project_events();
        let mut rx = project_events.subscribe();
        let p = path_watch.clone();
        let c = ctx;
        let last_self_save_ro = last_self_save_at;

        spawn(async move {
            // Pre-canonicalize the project ROOT once and join the
            // relative path to that. Canonicalizing `our_abs` directly
            // would fall back to a non-canonical path if the file
            // doesn't yet exist (e.g. operator opens a tab the agent
            // will create), and FSEvents would emit a canonicalized
            // path that wouldn't match. Project root is always present.
            let project = c.project();
            let canonical_root = std::fs::canonicalize(&project.root)
                .unwrap_or_else(|_| project.root.clone());
            let our_canonical = canonical_root.join(&p);

            // Helper: canonicalize an event path against the same root,
            // falling back to the path as-emitted. Centralized so the
            // recv + drain branches use identical comparison logic.
            let matches_ours = |evt_path: &Path, ours: &Path| -> bool {
                let pc = std::fs::canonicalize(evt_path)
                    .unwrap_or_else(|_| evt_path.to_path_buf());
                pc == *ours
            };

            loop {
                let Ok(first_evt) = rx.recv().await else { break };
                if !matches_ours(event_path(&first_evt), &our_canonical) {
                    continue;
                }

                // Debounce: drain any further events that arrive in the
                // next 200ms. Both matching and non-matching are
                // discarded from THIS subscriber's queue — non-matching
                // events stay in other subscribers' queues (broadcast
                // semantics), so the sidebar etc. still see them.
                tokio::time::sleep(Duration::from_millis(200)).await;
                while rx.try_recv().is_ok() {}

                // Own-save echo filter — FSEvents fires for our writes
                // too. If we just wrote within the window, skip.
                if let Some(t) = *last_self_save_ro.read() {
                    if t.elapsed() < SELF_SAVE_ECHO_WINDOW {
                        continue;
                    }
                }

                // Save-in-flight guard — if we're mid-save, reloading
                // would clobber the operator's pending content with
                // disk state, then the in-flight save would land and
                // overwrite again, eating the external write. Skip;
                // the operator can observe the agent's write by saving
                // (which clears save_state) and triggering a fresh
                // round of events.
                let saving = *save_state.read() == "saving";
                if saving {
                    tracing::debug!("FlowView: skipping watcher reload while save in flight");
                    continue;
                }

                // External write — reload from disk and push into
                // `loaded`. The mount effect observes the change and
                // remounts the bundle with the new content.
                let project = c.project();
                let new_loaded = read_and_parse_flow(&project.root, &p);
                let changed = *loaded.read() != new_loaded;
                if changed {
                    *loaded.write() = new_loaded;
                    *save_state.write() = "refreshed";
                    tokio::time::sleep(Duration::from_millis(2000)).await;
                    if *save_state.read() == "refreshed" {
                        *save_state.write() = "";
                    }
                }
            }
        });
    });

    rsx! {
        div {
            class: "flow-pane",
            style: "display:flex;flex-direction:column;flex:1;min-height:0;width:100%;position:relative;background:#020617;",
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

fn event_path(evt: &ProjectChangeEvent) -> &Path {
    match evt {
        ProjectChangeEvent::FileModified(p)
        | ProjectChangeEvent::FileCreated(p)
        | ProjectChangeEvent::FileDeleted(p) => p,
    }
}
