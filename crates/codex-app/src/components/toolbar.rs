use codex_core::{models::DocumentMeta, store::VaultStore};
use dioxus::prelude::*;
use crate::{bootstrap::AppContext, state::{Route, SyncStatus, TabState}};

#[component]
pub fn Toolbar(
    sync_status:      Signal<SyncStatus>,
    mut show_agent:   Signal<bool>,
    mut active_route: Signal<Route>,
    mut search_query: Signal<String>,
) -> Element {
    let ctx           = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut results:  Signal<Vec<DocumentMeta>> = use_signal(Vec::new);
    let mut focused = use_signal(|| false);

    // Update shared search_query and fetch inline dropdown results
    let ctx_search = ctx.clone();
    let on_input = move |e: Event<FormData>| {
        let q = e.value();
        *search_query.write() = q.clone();
        if q.trim().is_empty() { *results.write() = Vec::new(); return; }
        let c = ctx_search.clone();
        spawn(async move {
            let hits = tokio::task::spawn_blocking(move || {
                c.vault.store.search_documents(&q)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|r| DocumentMeta {
                        id:         r.document_id,
                        path:       r.path,
                        title:      r.title,
                        tags:       Vec::new(),
                        updated_at: chrono::Utc::now(),
                    })
                    .collect::<Vec<_>>()
            }).await.unwrap_or_default();
            *results.write() = hits;
        });
    };

    let sync_label = match *sync_status.read() {
        SyncStatus::Idle        => "",
        SyncStatus::Syncing     => "⟳",
        SyncStatus::Conflict(_) => "⚠",
    };

    rsx! {
        div { class: "toolbar",
            span { class: "toolbar-vault-name", "{ctx.vault.config.vault_name}" }

            div { class: "toolbar-search-wrap",
                input {
                    class: "toolbar-search",
                    r#type: "text",
                    placeholder: "Search notes…  ↵ for full results",
                    value: "{search_query}",
                    oninput:  on_input,
                    onfocus:  move |_| *focused.write() = true,
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            *active_route.write() = Route::Search;
                            *focused.write()  = false;
                            *results.write()  = Vec::new();
                        }
                        if e.key() == Key::Escape {
                            *focused.write()  = false;
                            *results.write()  = Vec::new();
                        }
                    },
                    onblur: move |_| {
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                            *focused.write() = false;
                            *results.write() = Vec::new();
                        });
                    },
                }

                // Quick-pick dropdown — rich card with path
                if *focused.read() && !results.read().is_empty() {
                    div { class: "search-overlay",
                        for meta in results.read().iter().cloned() {
                            {
                                let id    = meta.id.clone();
                                let title = meta.title.clone();
                                let t2    = title.clone();
                                let path  = meta.path.to_string_lossy().to_string();
                                // breadcrumb = parent segments only
                                let breadcrumb: String = {
                                    let mut parts: Vec<&str> = path.split('/').collect();
                                    if parts.len() > 1 { parts.pop(); }
                                    parts.join(" › ")
                                };
                                rsx! {
                                    button {
                                        class: "search-overlay-item",
                                        onmousedown: move |_| {
                                            tab_state.write().open(id.clone(), t2.clone());
                                            *active_route.write() = Route::Notes;
                                            *focused.write() = false;
                                            *results.write() = Vec::new();
                                        },
                                        span { class: "search-overlay-title", "{title}" }
                                        if !breadcrumb.is_empty() {
                                            span { class: "search-overlay-path", "{breadcrumb}" }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "search-overlay-enter",
                            "Press ↵ to see all results"
                        }
                    }
                }
            }

            div { class: "toolbar-right",
                if !sync_label.is_empty() {
                    span { class: "sync-badge", "{sync_label}" }
                }
                button {
                    class: if *show_agent.read() { "btn btn-ghost active" } else { "btn btn-ghost" },
                    title: "Toggle agent rail",
                    onclick: move |_| { let v = *show_agent.read(); *show_agent.write() = !v; },
                    "✦"
                }
            }
        }
    }
}
