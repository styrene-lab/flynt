//! Armory — browse + install extensions from the omegon registry.
//!
//! The omegon host exposes `extensions/search` (registry browse) and
//! `extensions/install` (install by URI). This panel wires both into
//! a two-pane layout: filterable list on the left, detail + install
//! action on the right. It's the discovery sibling to
//! [`ExtensionManagerSection`] which manages what's already installed.

use crate::acp::AcpSession;
use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use std::rc::Rc;

/// One entry as returned by `extensions/search`. The registry payload
/// is loosely typed JSON; we extract the fields we need defensively
/// and tolerate missing keys (the registry may grow / change).
#[derive(Clone, Debug, PartialEq)]
struct ArmoryEntry {
    name: String,
    version: String,
    description: String,
    uri: String,
    author: String,
    tags: Vec<String>,
}

impl ArmoryEntry {
    fn from_json(v: &serde_json::Value) -> Option<Self> {
        // Required: a name + an install URI. Without those the entry
        // is unactionable.
        let name = v.get("name").and_then(|n| n.as_str())?.to_string();
        let uri = v
            .get("uri")
            .or_else(|| v.get("install_uri"))
            .or_else(|| v.get("repo"))
            .and_then(|u| u.as_str())?
            .to_string();
        Some(Self {
            name,
            uri,
            version: v
                .get("version")
                .and_then(|s| s.as_str())
                .unwrap_or("?")
                .to_string(),
            description: v
                .get("description")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            author: v
                .get("author")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            tags: v
                .get("tags")
                .and_then(|t| t.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

fn parse_search_response(v: &serde_json::Value) -> Vec<ArmoryEntry> {
    // The omegon RPC may return either a bare array or `{ results: [..] }`.
    // Defensive: try both shapes before giving up.
    let arr = v
        .as_array()
        .or_else(|| v.get("results").and_then(|r| r.as_array()))
        .or_else(|| v.get("extensions").and_then(|r| r.as_array()));
    let Some(arr) = arr else { return Vec::new() };
    arr.iter().filter_map(ArmoryEntry::from_json).collect()
}

#[component]
pub fn ArmorySection() -> Element {
    let ctx = use_context::<AppContext>();
    let mut shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();

    let mut search_query = use_signal(String::new);
    let mut selected: Signal<Option<ArmoryEntry>> = use_signal(|| None);
    let mut install_state = use_signal::<Option<(&'static str, String)>>(|| None);
    let mut refresh_tick = use_signal(|| 0u64);
    // Track "launching session" state for the in-modal omegon launcher,
    // plus any error that came back from AcpSession::connect.
    let mut launching = use_signal(|| false);
    let mut launch_err = use_signal::<Option<String>>(|| None);

    let session_present = shared_session.read().is_some();
    let omegon_binary = ctx.omegon().resolve_binary();
    let binary_exists = omegon_binary.exists();

    // Fetch the registry. Re-fires when search_query changes (we pass
    // it as the omegon-side filter) and when refresh_tick bumps.
    // Only runs when a session is alive — without one, we render a
    // dedicated "launch session" panel instead of leaking the error
    // into the list area.
    let entries = use_resource(move || {
        let _ = refresh_tick.read();
        let q = search_query.read().clone();
        let sess = shared_session.read().clone();
        async move {
            let Some(s) = sess else {
                return Err::<Vec<ArmoryEntry>, String>("no-session".into());
            };
            let query = if q.trim().is_empty() { None } else { Some(q.as_str()) };
            match s.extensions_search(query).await {
                Ok(v) => Ok(parse_search_response(&v)),
                Err(e) => Err(e.to_string()),
            }
        }
    });

    // No session: render an in-modal launcher. This avoids the
    // "exit settings → start session → come back" dance the operator
    // would otherwise have to do.
    if !session_present {
        let ctx_for_launch = ctx.clone();
        return rsx! {
            div { class: "armory-root",
                div { class: "armory-launch-panel",
                    div { class: "armory-launch-icon", "\u{26A1}" }
                    h3 { class: "armory-launch-title", "Omegon session required" }
                    p { class: "armory-launch-body",
                        "Browsing the Armory queries the live omegon host. Flynt hasn't connected to a session yet — start one to load the registry."
                    }
                    if !binary_exists {
                        div { class: "armory-launch-error",
                            "Omegon binary not found at "
                            code { "{omegon_binary.display()}" }
                            ". Install omegon (or set a binary path in Runtime settings) before launching a session."
                        }
                    }
                    if let Some(err) = launch_err.read().as_ref() {
                        div { class: "armory-launch-error", "{err}" }
                    }
                    div { class: "armory-launch-actions",
                        button {
                            class: "btn btn-primary",
                            disabled: !binary_exists || *launching.read(),
                            onclick: move |_| {
                                let ctx = ctx_for_launch.clone();
                                let binary = omegon_binary.clone();
                                *launching.write() = true;
                                *launch_err.write() = None;
                                spawn(async move {
                                    let project = ctx.project_root();
                                    let operator_settings = ctx.omegon().load_operator_settings();
                                    let agent_id = operator_settings.agent_id.clone();
                                    match AcpSession::connect(binary, project, agent_id).await {
                                        Ok((s, _rx)) => {
                                            // Note: the event-loop receiver is dropped here
                                            // because the agent rail owns the canonical
                                            // event loop. The session itself is reusable for
                                            // RPC calls (extensions/search, install), which
                                            // is all the Armory needs.
                                            *shared_session.write() = Some(Rc::new(s));
                                            *launching.write() = false;
                                        }
                                        Err(e) => {
                                            *launch_err.write() = Some(format!("Could not start omegon: {e}"));
                                            *launching.write() = false;
                                        }
                                    }
                                });
                            },
                            if *launching.read() {
                                "Starting\u{2026}"
                            } else {
                                "Launch omegon session"
                            }
                        }
                    }
                }
            }
        };
    }

    rsx! {
        div { class: "armory-root",
            // Header: search + refresh
            div { class: "armory-header",
                input {
                    class: "input armory-search",
                    r#type: "text",
                    value: "{search_query}",
                    placeholder: "Search the armory…",
                    oninput: move |e| {
                        *search_query.write() = e.value();
                    },
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| { *refresh_tick.write() += 1; },
                    title: "Re-query the omegon registry",
                    "Refresh"
                }
            }

            // Two-pane: list (left) + detail (right)
            div { class: "armory-split",
                div { class: "armory-list",
                    match &*entries.read() {
                        Some(Ok(items)) if items.is_empty() => {
                            let q = search_query.read().clone();
                            let has_query = !q.trim().is_empty();
                            rsx! {
                                div { class: "armory-empty",
                                    if has_query {
                                        div { class: "armory-empty-title", "No matches for \u{201C}{q}\u{201D}" }
                                        div { class: "armory-empty-body",
                                            "The omegon registry doesn't have an extension matching that query. Try a broader term or clear the search."
                                        }
                                        button {
                                            class: "btn btn-ghost btn-sm",
                                            onclick: move |_| { *search_query.write() = String::new(); },
                                            "Clear search"
                                        }
                                    } else {
                                        div { class: "armory-empty-title", "Registry is empty" }
                                        div { class: "armory-empty-body",
                                            "No extensions returned from the omegon registry. This usually means the host can't reach it — check the agent rail for omegon's status."
                                        }
                                    }
                                }
                            }
                        },
                        Some(Ok(items)) => rsx! {
                            for entry in items.iter().cloned() {
                                {
                                    let is_active = selected.read().as_ref() == Some(&entry);
                                    let entry_click = entry.clone();
                                    rsx! {
                                        button {
                                            class: if is_active { "armory-item active" } else { "armory-item" },
                                            onclick: move |_| {
                                                *selected.write() = Some(entry_click.clone());
                                                *install_state.write() = None;
                                            },
                                            div { class: "armory-item-name", "{entry.name}" }
                                            div { class: "armory-item-version", "v{entry.version}" }
                                            if !entry.description.is_empty() {
                                                div { class: "armory-item-desc", "{entry.description}" }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        Some(Err(msg)) => rsx! {
                            div { class: "armory-error", "{msg}" }
                        },
                        None => rsx! {
                            div { class: "armory-loading", "Loading…" }
                        },
                    }
                }

                div { class: "armory-detail",
                    match &*selected.read() {
                        Some(entry) => {
                            let install_uri = entry.uri.clone();
                            let install_name = entry.name.clone();
                            rsx! {
                                div { class: "armory-detail-head",
                                    div { class: "armory-detail-name", "{entry.name}" }
                                    div { class: "armory-detail-version", "v{entry.version}" }
                                }
                                if !entry.author.is_empty() {
                                    div { class: "armory-detail-author", "by {entry.author}" }
                                }
                                if !entry.tags.is_empty() {
                                    div { class: "armory-detail-tags",
                                        for tag in entry.tags.iter() {
                                            span { class: "armory-tag", "{tag}" }
                                        }
                                    }
                                }
                                if !entry.description.is_empty() {
                                    div { class: "armory-detail-desc", "{entry.description}" }
                                }
                                div { class: "armory-detail-uri",
                                    span { class: "muted", "URI: " }
                                    code { "{entry.uri}" }
                                }
                                div { class: "armory-detail-actions",
                                    button {
                                        class: "btn btn-primary",
                                        disabled: install_state.read().as_ref()
                                            .map(|(k, _)| *k == "running").unwrap_or(false),
                                        onclick: move |_| {
                                            let uri = install_uri.clone();
                                            let name = install_name.clone();
                                            let sess = shared_session.read().clone();
                                            *install_state.write() = Some(("running", format!("Installing {name}…")));
                                            spawn(async move {
                                                let Some(s) = sess else {
                                                    install_state.set(Some(("err", "no agent session — start omegon".into())));
                                                    return;
                                                };
                                                match s.extensions_install(&uri).await {
                                                    Ok(_) => {
                                                        install_state.set(Some(("ok", format!("Installed {name}. See Extensions to configure."))));
                                                    }
                                                    Err(e) => {
                                                        install_state.set(Some(("err", format!("Install failed: {e}"))));
                                                    }
                                                }
                                            });
                                        },
                                        "Install"
                                    }
                                }
                                if let Some((kind, msg)) = install_state.read().as_ref() {
                                    div {
                                        class: match *kind {
                                            "ok"      => "armory-status ok",
                                            "err"     => "armory-status err",
                                            "running" => "armory-status running",
                                            _         => "armory-status",
                                        },
                                        "{msg}"
                                    }
                                }
                            }
                        }
                        None => rsx! {
                            div { class: "armory-detail-empty muted",
                                "Pick an extension on the left to see details and install."
                            }
                        },
                    }
                }
            }
        }
    }
}
