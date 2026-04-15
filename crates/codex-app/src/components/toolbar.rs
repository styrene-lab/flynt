use codex_core::{models::{DocumentId, DocumentMeta}, store::VaultStore};
use dioxus::prelude::*;
use crate::{bootstrap::AppContext, state::{Route, SyncStatus}};

#[component]
pub fn Toolbar(
    sync_status:     Signal<SyncStatus>,
    mut show_agent: Signal<bool>,
    mut selected_doc: Signal<Option<DocumentId>>,
    mut active_route: Signal<Route>,
) -> Element {
    let ctx = use_context::<AppContext>();
    let mut query   = use_signal(String::new);
    let mut results: Signal<Vec<DocumentMeta>> = use_signal(Vec::new);
    let mut focused = use_signal(|| false);

    let ctx_search = ctx.clone();
    let on_input = move |e: Event<FormData>| {
        let q = e.value();
        *query.write() = q.clone();
        if q.trim().is_empty() {
            *results.write() = Vec::new();
            return;
        }
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
            })
            .await
            .unwrap_or_default();
            *results.write() = hits;
        });
    };

    let sync_label = match *sync_status.read() {
        SyncStatus::Idle       => "",
        SyncStatus::Syncing    => "⟳",
        SyncStatus::Conflict(_) => "⚠",
    };

    rsx! {
        div { class: "toolbar",
            // Vault name (left)
            span { class: "toolbar-vault-name", "{ctx.vault.config.vault_name}" }

            // Search (center)
            div { class: "toolbar-search-wrap",
                input {
                    class: "toolbar-search",
                    r#type: "search",
                    placeholder: "Search notes…",
                    value: "{query}",
                    oninput: on_input,
                    onfocus: move |_| *focused.write() = true,
                    onblur:  move |_| {
                        // Delay clear so click on result registers first.
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                            *focused.write() = false;
                            *results.write() = Vec::new();
                            *query.write()   = String::new();
                        });
                    },
                }
                if *focused.read() && !results.read().is_empty() {
                    div { class: "search-overlay",
                        for meta in results.read().iter().cloned() {
                            {
                                let id    = meta.id.clone();
                                let title = meta.title.clone();
                                rsx! {
                                    button {
                                        class: "search-result-item",
                                        onmousedown: move |_| {
                                            // mousedown fires before blur
                                            *selected_doc.write() = Some(id.clone());
                                            *active_route.write() = Route::Notes;
                                            *query.write()        = String::new();
                                            *results.write()      = Vec::new();
                                            *focused.write()      = false;
                                        },
                                        span { class: "result-title", "{title}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Right cluster
            div { class: "toolbar-right",
                if !sync_label.is_empty() {
                    span { class: "sync-badge", "{sync_label}" }
                }
                button {
                    class: if *show_agent.read() { "btn btn-ghost active" } else { "btn btn-ghost" },
                    title: "Toggle agent rail",
                    onclick: move |_| {
                        let v = *show_agent.read();
                        *show_agent.write() = !v;
                    },
                    "✦"
                }
            }
        }
    }
}
