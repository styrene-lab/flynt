use codex_core::store::VaultStore;
use dioxus::prelude::*;
use std::collections::BTreeMap;
use crate::{bootstrap::AppContext, state::{Route, TabState}};

#[component]
pub fn SearchView(mut search_query: Signal<String>) -> Element {
    let ctx              = use_context::<AppContext>();
    let mut tab_state    = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();

    let results = use_resource(move || {
        let q = search_query.read().clone();
        let c = ctx.clone();
        async move {
            if q.trim().is_empty() { return vec![]; }
            tokio::task::spawn_blocking(move || {
                c.vault.store.search_documents(&q).unwrap_or_default()
            })
            .await
            .unwrap_or_default()
        }
    });

    // Read once; clone into per-use bindings — avoids borrow-after-move in RSX
    let q_val   = search_query.read().clone();  // input value=""
    let q_match = q_val.clone();                // match guard comparisons
    let q_disp  = q_val.clone();                // display in empty-state text

    rsx! {
        div { class: "search-view",

            // ── Persistent search bar ────────────────────────────────────────
            div { class: "search-view-bar",
                span { class: "search-view-icon", "⌕" }
                input {
                    class: "search-view-input",
                    r#type: "text",
                    placeholder: "Search all notes…",
                    autofocus: true,
                    value: "{q_val}",
                    oninput: move |e| *search_query.write() = e.value(),
                    onkeydown: move |e| {
                        if e.key() == Key::Escape {
                            *active_route.write() = Route::Notes;
                        }
                    },
                }
            }

            // ── Content area ─────────────────────────────────────────────────
            div { class: "search-content",
                match &*results.read() {
                    None => rsx! {
                        div { class: "search-status muted", "Searching…" }
                    },

                    Some(list) if list.is_empty() && q_match.trim().is_empty() => rsx! {
                        div { class: "search-empty-state",
                            div { class: "search-empty-icon", "⌕" }
                            p { class: "search-empty-msg", "Type to search across all notes." }
                        }
                    },

                    Some(list) if list.is_empty() => rsx! {
                        div { class: "search-empty-state",
                            div { class: "search-empty-icon", "◎" }
                            p { class: "search-empty-msg",
                                "No results for \"{q_disp}\""
                            }
                            p { class: "muted search-empty-hint",
                                "Try different keywords or check the spelling."
                            }
                        }
                    },

                    Some(list) => {
                        let mut groups: BTreeMap<String, Vec<_>> = BTreeMap::new();
                        for r in list {
                            let comps: Vec<_> = r.path.components().collect();
                            let folder = if comps.len() > 1 {
                                comps[0].as_os_str().to_string_lossy().into_owned()
                            } else {
                                String::new()
                            };
                            groups.entry(folder).or_default().push(r.clone());
                        }

                        let total   = list.len();
                        let n_files = groups.values().map(|v| v.len()).sum::<usize>();
                        let res_word  = if total   == 1 { "result" } else { "results" };
                        let file_word = if n_files == 1 { "file"   } else { "files"   };

                        rsx! {
                            div { class: "search-stats-bar",
                                span { class: "search-stats-count", "{total}" }
                                span { class: "muted", " {res_word} in " }
                                span { class: "search-stats-count", "{n_files}" }
                                span { class: "muted", " {file_word}" }
                            }

                            div { class: "search-results-list",
                                for (folder, items) in &groups {
                                    div { class: "search-group",
                                        if !folder.is_empty() {
                                            div { class: "search-group-header",
                                                span { class: "search-folder-icon", "▶" }
                                                span { class: "search-group-name", "{folder}" }
                                                span { class: "search-group-badge", "{items.len()}" }
                                            }
                                        }

                                        for item in items {
                                            {
                                                let doc_id  = item.document_id.clone();
                                                let title   = item.title.clone();
                                                let t2      = title.clone();
                                                let path    = item.path.to_string_lossy().to_string();
                                                let excerpt = item.excerpt.clone();
                                                let breadcrumb: String = {
                                                    let mut parts: Vec<&str> = path.split('/').collect();
                                                    if parts.len() > 1 { parts.pop(); }
                                                    parts.join(" › ")
                                                };

                                                rsx! {
                                                    button {
                                                        class: "search-result-card",
                                                        onclick: move |_| {
                                                            tab_state.write().open(doc_id.clone(), t2.clone());
                                                            *active_route.write() = Route::Notes;
                                                        },
                                                        div { class: "src-header",
                                                            span { class: "src-file-icon", "📄" }
                                                            div { class: "src-meta",
                                                                span { class: "src-title", "{title}" }
                                                                if !breadcrumb.is_empty() {
                                                                    span { class: "src-path muted", "{breadcrumb}" }
                                                                }
                                                            }
                                                        }
                                                        if !excerpt.is_empty() {
                                                            div {
                                                                class: "src-excerpt",
                                                                dangerous_inner_html: "{excerpt}",
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}
