use codex_core::{models::{DocumentId, DocumentMeta}, store::VaultStore};
use dioxus::prelude::*;
use crate::{bootstrap::AppContext, state::Route};

#[component]
pub fn Sidebar(
    mut active_route: Signal<Route>,
    mut selected_doc: Signal<Option<DocumentId>>,
) -> Element {
    let ctx = use_context::<AppContext>();

    // Load document list; re-runs whenever the component re-renders with a changed signal.
    let docs = use_resource(move || {
        let vault = ctx.vault.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                vault.store.list_documents().unwrap_or_default()
            })
            .await
            .unwrap_or_default()
        }
    });

    rsx! {
        nav { class: "sidebar",
            // ── Document list ─────────────────────────────────────────────
            div { class: "sidebar-section",
                span { class: "sidebar-heading", "Notes" }
                match &*docs.read() {
                    None => rsx! { span { class: "sidebar-item muted", "Loading…" } },
                    Some(list) if list.is_empty() => rsx! {
                        span { class: "sidebar-item muted", "No documents yet" }
                    },
                    Some(list) => rsx! {
                        for meta in list.iter().cloned() {
                            DocItem { meta, selected_doc }
                        }
                    },
                }
            }

            // ── Nav buttons ───────────────────────────────────────────────
            div { class: "sidebar-nav",
                button {
                    class: if *active_route.read() == Route::Notes { "nav-btn active" } else { "nav-btn" },
                    onclick: move |_| *active_route.write() = Route::Notes,
                    "📝"
                }
                button {
                    class: if *active_route.read() == Route::Kanban { "nav-btn active" } else { "nav-btn" },
                    onclick: move |_| *active_route.write() = Route::Kanban,
                    "📋"
                }
                button {
                    class: if *active_route.read() == Route::Graph { "nav-btn active" } else { "nav-btn" },
                    onclick: move |_| *active_route.write() = Route::Graph,
                    "🕸"
                }
                button {
                    class: if *active_route.read() == Route::Settings { "nav-btn active" } else { "nav-btn" },
                    onclick: move |_| *active_route.write() = Route::Settings,
                    "⚙️"
                }
            }
        }
    }
}

/// Single document row in the sidebar list.
#[component]
fn DocItem(meta: DocumentMeta, mut selected_doc: Signal<Option<DocumentId>>) -> Element {
    let is_active = selected_doc.read().as_ref() == Some(&meta.id);
    let id = meta.id.clone();
    rsx! {
        button {
            class: if is_active { "sidebar-item active" } else { "sidebar-item" },
            onclick: move |_| *selected_doc.write() = Some(id.clone()),
            span { class: "doc-title", "{meta.title}" }
        }
    }
}
