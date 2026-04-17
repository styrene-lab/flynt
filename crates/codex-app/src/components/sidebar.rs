use codex_core::{models::DocumentMeta, store::VaultStore};
use dioxus::prelude::*;
use std::{collections::BTreeMap, path::PathBuf};
use crate::{
    bootstrap::{AppContext, KnownVault, OmegonRuntimeContext},
    state::{Route, TabState},
};
use rfd::FileDialog;

// ── Sidebar ───────────────────────────────────────────────────────────────────

#[component]
pub fn Sidebar(mut active_route: Signal<Route>) -> Element {
    let ctx     = use_context::<AppContext>();
    let mut refresh = use_signal(|| 0_u64);

    let vault_events = ctx.vault_events();
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
        let vault = ctx.vault();
        async move {
            let mut list = tokio::task::spawn_blocking(move || {
                vault.store.list_documents().unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            // Sort alphabetically for a clean sidebar
            list.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
            list
        }
    });

    let mut creating = use_signal(|| false);
    let mut new_name = use_signal(String::new);
    let mut create_err = use_signal(|| Option::<String>::None);

    rsx! {
        nav { class: "sidebar",
            div { class: "sidebar-section",
                div { class: "sidebar-section-header",
                    span { class: "sidebar-heading", "NOTES" }
                    button {
                        class: "sidebar-new-btn",
                        title: "New note",
                        onclick: move |_| {
                            let was = *creating.read();
                            creating.set(!was);
                            if !was {
                                new_name.set(String::new());
                                create_err.set(None);
                            }
                        },
                        if *creating.read() { "×" } else { "+" }
                    }
                }
                if *creating.read() {
                    NewNoteInput {
                        new_name,
                        create_err,
                        creating,
                        refresh,
                        active_route,
                    }
                }
                match &*docs.read() {
                    None => rsx! { span { class: "sidebar-item muted", "Loading…" } },
                    Some(list) if list.is_empty() => rsx! {
                        span { class: "sidebar-item muted", "No documents" }
                    },
                    Some(list) => rsx! { { tree_view(list) } },
                }
            }

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

            VaultSwitcher {}
        }
    }
}

// ── Tree ─────────────────────────────────────────────────────────────────────

fn tree_view(docs: &[DocumentMeta]) -> Element {
    let mut folders: BTreeMap<String, Vec<DocumentMeta>> = BTreeMap::new();
    for doc in docs {
        let components: Vec<_> = doc.path.components().collect();
        let folder = if components.len() > 1 {
            components[0].as_os_str().to_string_lossy().into_owned()
        } else {
            String::new()
        };
        folders.entry(folder).or_default().push(doc.clone());
    }

    rsx! {
        // Folder groups first (sorted alphabetically by BTreeMap)
        for (folder, folder_docs) in folders.iter().filter(|(k, _)| !k.is_empty()) {
            FolderGroup { name: folder.clone(), docs: folder_docs.clone() }
        }
        // Root-level files after folders, with a separator if folders exist
        if folders.keys().any(|k| !k.is_empty()) && folders.contains_key("") {
            div { class: "sidebar-divider" }
        }
        for doc in folders.get("").cloned().unwrap_or_default().iter().cloned() {
            DocItem { meta: doc, indent: 0 }
        }
    }
}

#[component]
fn FolderGroup(name: String, docs: Vec<DocumentMeta>) -> Element {
    let mut open = use_signal(|| false);
    rsx! {
        div { class: "sidebar-folder",
            button {
                class: "sidebar-folder-header",
                onclick: move |_| { let v = *open.read(); *open.write() = !v; },
                span { class: "folder-chevron", if *open.read() { "▾" } else { "▸" } }
                span { class: "folder-name", "{name}" }
                span { class: "folder-count", "{docs.len()}" }
            }
            if *open.read() {
                div { class: "sidebar-folder-contents",
                    for doc in docs.iter().cloned() {
                        DocItem { meta: doc, indent: 1 }
                    }
                }
            }
        }
    }
}

#[component]
fn DocItem(meta: DocumentMeta, indent: u32) -> Element {
    let mut tab_state    = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();

    let active_id = tab_state.read().active_id().cloned();
    let is_active = active_id.as_ref() == Some(&meta.id);

    let id    = meta.id.clone();
    let title = meta.title.clone();

    rsx! {
        button {
            class: if is_active { "sidebar-doc active" } else { "sidebar-doc" },
            class: if indent > 0 { "indent" } else { "" },
            onclick: move |_| {
                tab_state.write().open(id.clone(), title.clone());
                *active_route.write() = Route::Notes;
            },
            span { class: "doc-icon", "◇" }
            span { class: "doc-title", "{meta.title}" }
        }
    }
}

#[component]
fn NewNoteInput(
    mut new_name: Signal<String>,
    mut create_err: Signal<Option<String>>,
    mut creating: Signal<bool>,
    mut refresh: Signal<u64>,
    mut active_route: Signal<Route>,
) -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();

    rsx! {
        div { class: "sidebar-new-note",
            input {
                class: "sidebar-new-note-input",
                placeholder: "Note name or path/name",
                value: "{new_name}",
                oninput: move |e| new_name.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Escape {
                        creating.set(false);
                        return;
                    }
                    if e.key() != Key::Enter {
                        return;
                    }
                    let raw = new_name.read().trim().to_string();
                    if raw.is_empty() {
                        return;
                    }
                    let rel = if raw.ends_with(".md") {
                        std::path::PathBuf::from(&raw)
                    } else {
                        std::path::PathBuf::from(format!("{raw}.md"))
                    };
                    let title = rel
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| raw.clone());
                    let vault = ctx.vault();
                    let ctx2 = ctx.clone();
                    let title2 = title.clone();
                    spawn(async move {
                        match tokio::task::spawn_blocking(move || vault.create_document(&rel, &title)).await {
                            Ok(Ok(())) => {
                                *refresh.write() += 1;
                                creating.set(false);
                                let vault = ctx2.vault();
                                if let Ok(Some(meta)) = tokio::task::spawn_blocking(
                                    move || vault.store.find_document_by_slug(&title2)
                                ).await.unwrap_or(Ok(None)) {
                                    tab_state.write().open(meta.id, meta.title);
                                    *active_route.write() = Route::Notes;
                                }
                            }
                            Ok(Err(e)) => create_err.set(Some(e.to_string())),
                            Err(e) => create_err.set(Some(e.to_string())),
                        }
                    });
                },
                autofocus: true,
            }
            if let Some(ref err) = *create_err.read() {
                span { class: "sidebar-new-note-err", "{err}" }
            }
        }
    }
}

#[component]
fn VaultSwitcher() -> Element {
    let ctx = use_context::<AppContext>();
    let mut profile = use_signal(OmegonRuntimeContext::load_launcher_profile);
    let current_root = ctx.vault_root();
    let current_name = ctx.vault().config.vault_name.clone();

    let open_vault = move |vault: KnownVault| {
        let _ = OmegonRuntimeContext::spawn_new_instance_for_vault(&vault.root);
    };

    let open_folder = move |_| {
        let Some(selected_root) = FileDialog::new().pick_folder() else {
            return;
        };
        let name = selected_root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Codex")
            .to_string();
        if OmegonRuntimeContext::initialize_vault(
            &selected_root,
            &name,
            codex_core::models::SyncConfig::None,
        )
        .is_ok()
        {
            let mut updated = OmegonRuntimeContext::load_launcher_profile();
            OmegonRuntimeContext::register_known_vault(&mut updated, &selected_root, &name);
            let _ = OmegonRuntimeContext::save_launcher_profile(&updated);
            profile.set(updated);
            let _ = OmegonRuntimeContext::spawn_new_instance_for_vault(&selected_root);
        }
    };

    rsx! {
        div { class: "sidebar-section vault-switcher",
            div { class: "sidebar-section-header",
                span { class: "sidebar-heading", "VAULTS" }
            }
            div { class: "vault-current",
                span { class: "vault-current-name", "{current_name}" }
                span { class: "vault-current-path", "{current_root.display()}" }
            }
            for vault in profile.read().known_vaults.iter().filter(|vault| vault.root != current_root).cloned() {
                button {
                    class: "sidebar-doc",
                    onclick: {
                        let vault = vault.clone();
                        move |_| open_vault(vault.clone())
                    },
                    span { class: "doc-icon", "◈" }
                    span { class: "doc-title", "{vault.name}" }
                }
            }
            button {
                class: "sidebar-doc muted",
                onclick: open_folder,
                span { class: "doc-icon", "+" }
                span { class: "doc-title", "Open another vault…" }
            }
        }
    }
}

pub fn initial_note_id_for_vault(vault_root: &PathBuf) -> Option<String> {
    let vault = crate::bootstrap::OmegonRuntimeContext::initialize_vault(
        vault_root,
        vault_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Codex"),
        codex_core::models::SyncConfig::None,
    ).ok()?;
    vault
        .store
        .list_documents()
        .ok()?
        .into_iter()
        .next()
        .map(|doc| doc.id.0.to_string())
}
