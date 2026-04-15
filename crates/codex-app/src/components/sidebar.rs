use codex_core::{models::{DocumentId, DocumentMeta}, store::VaultStore};
use dioxus::prelude::*;
use std::collections::BTreeMap;
use crate::{bootstrap::AppContext, state::Route};

// ── Sidebar ───────────────────────────────────────────────────────────────────

#[component]
pub fn Sidebar(
    mut active_route: Signal<Route>,
    mut selected_doc:  Signal<Option<DocumentId>>,
) -> Element {
    let ctx     = use_context::<AppContext>();
    let mut refresh = use_signal(|| 0_u64);

    // Subscribe to vault events → bump refresh → reload doc list
    let vault_events = ctx.vault_events.clone();
    use_effect(move || {
        let mut rx = vault_events.subscribe();
        spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(_)  => *refresh.write() += 1,
                    Err(_) => break,
                }
            }
        });
    });

    let docs = use_resource(move || {
        let _ = refresh();
        let c = ctx.clone();
        async move {
            tokio::task::spawn_blocking(move || c.vault.store.list_documents().unwrap_or_default())
                .await
                .unwrap_or_default()
        }
    });

    rsx! {
        nav { class: "sidebar",
            // ── Document tree ────────────────────────────────────────────────
            div { class: "sidebar-section",
                div { class: "sidebar-section-header",
                    span { class: "sidebar-heading", "Notes" }
                    button {
                        class: "sidebar-new-btn",
                        title: "New note",
                        onclick: move |_| {
                            // TODO: open new-note dialog
                        },
                        "+"
                    }
                }
                match &*docs.read() {
                    None => rsx! { span { class: "sidebar-item muted", "Loading…" } },
                    Some(list) if list.is_empty() => rsx! {
                        span { class: "sidebar-item muted", "No documents" }
                    },
                    Some(list) => rsx! {
                        { tree_view(list, selected_doc) }
                    },
                }
            }

            // ── Nav buttons ──────────────────────────────────────────────────
            div { class: "sidebar-nav",
                button {
                    class: if *active_route.read() == Route::Notes    { "nav-btn active" } else { "nav-btn" },
                    title: "Notes",
                    onclick: move |_| *active_route.write() = Route::Notes,
                    "📝"
                }
                button {
                    class: if *active_route.read() == Route::Kanban   { "nav-btn active" } else { "nav-btn" },
                    title: "Kanban",
                    onclick: move |_| *active_route.write() = Route::Kanban,
                    "📋"
                }
                button {
                    class: if *active_route.read() == Route::Graph    { "nav-btn active" } else { "nav-btn" },
                    title: "Graph",
                    onclick: move |_| *active_route.write() = Route::Graph,
                    "🕸"
                }
                button {
                    class: if *active_route.read() == Route::Settings { "nav-btn active" } else { "nav-btn" },
                    title: "Settings",
                    onclick: move |_| *active_route.write() = Route::Settings,
                    "⚙️"
                }
            }
        }
    }
}

// ── Tree rendering ────────────────────────────────────────────────────────────

/// Render documents as a folder tree, grouped by path prefix.
fn tree_view(docs: &[DocumentMeta], selected_doc: Signal<Option<DocumentId>>) -> Element {
    // Group by first path component (folder or root)
    let mut folders: BTreeMap<String, Vec<DocumentMeta>> = BTreeMap::new();

    for doc in docs {
        let components: Vec<_> = doc.path.components().collect();
        let folder = if components.len() > 1 {
            components[0].as_os_str().to_string_lossy().into_owned()
        } else {
            String::new() // root level
        };
        folders.entry(folder).or_default().push(doc.clone());
    }

    rsx! {
        // Root-level docs first (no folder prefix)
        for doc in folders.get("").cloned().unwrap_or_default().iter().cloned() {
            DocItem { meta: doc, selected_doc, indent: 0 }
        }
        // Folders
        for (folder, folder_docs) in folders.iter().filter(|(k, _)| !k.is_empty()) {
            FolderGroup {
                name: folder.clone(),
                docs: folder_docs.clone(),
                selected_doc,
            }
        }
    }
}

#[component]
fn FolderGroup(
    name:         String,
    docs:         Vec<DocumentMeta>,
    selected_doc: Signal<Option<DocumentId>>,
) -> Element {
    let mut open = use_signal(|| true); // expanded by default

    rsx! {
        div { class: "sidebar-folder",
            button {
                class: "sidebar-folder-header",
                onclick: move |_| { let v = *open.read(); *open.write() = !v; },
                span { class: "folder-chevron", if *open.read() { "▾" } else { "▸" } }
                span { class: "folder-name", "{name}" }
                span { class: "folder-count muted", "{docs.len()}" }
            }
            if *open.read() {
                div { class: "sidebar-folder-contents",
                    for doc in docs.iter().cloned() {
                        DocItem { meta: doc, selected_doc, indent: 1 }
                    }
                }
            }
        }
    }
}

#[component]
fn DocItem(
    meta:         DocumentMeta,
    mut selected_doc: Signal<Option<DocumentId>>,
    indent:       u32,
) -> Element {
    let is_active = selected_doc.read().as_ref() == Some(&meta.id);
    let id = meta.id.clone();
    let style = if indent > 0 {
        format!("padding-left: calc(var(--space-3) + {}px)", indent * 12)
    } else {
        String::new()
    };
    rsx! {
        button {
            class: if is_active { "sidebar-item active" } else { "sidebar-item" },
            style: "{style}",
            onclick: move |_| *selected_doc.write() = Some(id.clone()),
            span { class: "doc-title", "{meta.title}" }
        }
    }
}
