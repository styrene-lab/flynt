use flynt_core::{models::DocumentMeta, store::VaultStore};
use dioxus::prelude::*;
use std::{collections::BTreeMap, path::PathBuf};
use crate::{
    bootstrap::{AppContext, OmegonRuntimeContext},
    state::{Route, TabState},
};
use rfd::FileDialog;

// ── Sidebar ───────────────────────────────────────────────────────────────────

#[component]
pub fn Sidebar(mut active_route: Signal<Route>) -> Element {
    let ctx     = use_context::<AppContext>();
    let mut refresh = use_context_provider(|| Signal::new(0_u64));

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
            list.retain(|doc| {
                let path = doc.path.to_string_lossy();
                !path.starts_with("ai/delegations/")
                    && !path.starts_with("ai/memory/")
                    && !path.starts_with("references/comms/")
            });
            list.sort_by(|a, b| a.path.cmp(&b.path));
            list
        }
    });

    let mut creating = use_signal(|| false);
    let mut new_name = use_signal(String::new);
    let mut create_err = use_signal(|| Option::<String>::None);

    rsx! {
        nav { class: "sidebar",
            // ── Vault selector (compact) ──────────────────────
            VaultSelector {}

            // ── File tree ─────────────────────────────────────
            div { class: "file-tree",
                div { class: "file-tree-header",
                    button {
                        class: "file-tree-new-btn",
                        title: "New note (\u{2318}N)",
                        onclick: move |_| {
                            let was = *creating.read();
                            creating.set(!was);
                            if !was {
                                new_name.set(String::new());
                                create_err.set(None);
                            }
                        },
                        "+"
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
                    None => rsx! { span { class: "tree-item muted", "Loading…" } },
                    Some(list) if list.is_empty() => rsx! {
                        div { class: "tree-empty",
                            "Empty vault — press + to create a note"
                        }
                    },
                    Some(list) => rsx! { { build_tree(list) } },
                }
            }

            // ── Nav (pinned bottom) ───────────────────────────
            div { class: "sidebar-nav",
                button {
                    class: if *active_route.read() == Route::Notes    { "nav-btn active" } else { "nav-btn" },
                    title: "Notes",
                    onclick: move |_| *active_route.write() = Route::Notes,
                    span { class: "nav-icon", dangerous_inner_html: crate::icons::ICON_SCROLL }
                }
                button {
                    class: if *active_route.read() == Route::Kanban   { "nav-btn active" } else { "nav-btn" },
                    title: "Kanban",
                    onclick: move |_| *active_route.write() = Route::Kanban,
                    span { class: "nav-icon", dangerous_inner_html: crate::icons::ICON_BOARD }
                }
                button {
                    class: if *active_route.read() == Route::Graph    { "nav-btn active" } else { "nav-btn" },
                    title: "Graph",
                    onclick: move |_| *active_route.write() = Route::Graph,
                    span { class: "nav-icon", dangerous_inner_html: crate::icons::ICON_GRAPH }
                }
                button {
                    class: if *active_route.read() == Route::Settings { "nav-btn active" } else { "nav-btn" },
                    title: "Settings",
                    onclick: move |_| *active_route.write() = Route::Settings,
                    span { class: "nav-icon", dangerous_inner_html: crate::icons::ICON_SETTINGS }
                }
            }
        }
    }
}

// ── File tree builder ─────────────────────────────────────────────────────────

/// Recursive tree node — folders contain sub-folders and files.
#[derive(Clone, PartialEq)]
enum TreeNode {
    Folder {
        name: String,
        children: BTreeMap<String, TreeNode>,
    },
    File(DocumentMeta),
}

impl TreeNode {
    fn file_count(&self) -> usize {
        match self {
            Self::File(_) => 1,
            Self::Folder { children, .. } => children.values().map(|c| c.file_count()).sum(),
        }
    }
}

/// Build a fully nested tree from flat document list using all path components.
fn build_tree(docs: &[DocumentMeta]) -> Element {
    let mut root: BTreeMap<String, TreeNode> = BTreeMap::new();

    for doc in docs {
        let components: Vec<_> = doc.path.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();

        if components.len() <= 1 {
            // Root-level file — use title as sort key
            root.entry(format!("\x7F{}", doc.title))
                .or_insert(TreeNode::File(doc.clone()));
        } else {
            // Walk/create nested folder path
            let folder_parts = &components[..components.len() - 1];
            let mut current = &mut root;

            for part in folder_parts {
                let entry = current.entry(part.clone()).or_insert_with(|| TreeNode::Folder {
                    name: part.clone(),
                    children: BTreeMap::new(),
                });
                current = match entry {
                    TreeNode::Folder { children, .. } => children,
                    _ => unreachable!(),
                };
            }
            current.entry(format!("\x7F{}", doc.title))
                .or_insert(TreeNode::File(doc.clone()));
        }
    }

    rsx! { { render_tree_level(&root, 0) } }
}

/// Recursively render a tree level. Not a #[component] — just a function
/// that returns Element, avoiding Dioxus Props derive issues with complex types.
fn render_tree_level(nodes: &BTreeMap<String, TreeNode>, depth: u32) -> Element {
    let entries: Vec<_> = nodes.iter().collect();

    rsx! {
        for (_key, node) in entries.iter() {
            match *node {
                TreeNode::Folder { name, children } => {
                    { render_folder(name, children, depth) }
                },
                TreeNode::File(doc) => rsx! {
                    TreeFile { meta: doc.clone(), depth }
                },
            }
        }
    }
}

fn render_folder(name: &str, children: &BTreeMap<String, TreeNode>, depth: u32) -> Element {
    let name = name.to_string();
    let children = children.clone();
    let count: usize = children.values().map(|c| c.file_count()).sum();
    let mut open = use_signal(|| false);
    let indent = depth as f32 * 12.0;

    rsx! {
        button {
            class: "tree-item tree-folder",
            style: "padding-left: {indent + 8.0}px;",
            onclick: move |_| { let v = *open.read(); *open.write() = !v; },
            span { class: "tree-chevron", if *open.read() { "\u{25BE}" } else { "\u{25B8}" } }
            span { class: "tree-name", "{name}" }
            span { class: "tree-count", "{count}" }
        }
        if *open.read() {
            { render_tree_level(&children, depth + 1) }
        }
    }
}

#[component]
fn TreeFile(meta: DocumentMeta, depth: u32) -> Element {
    let ctx              = use_context::<AppContext>();
    let mut tab_state    = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();
    let mut refresh      = use_context::<Signal<u64>>();
    let mut rename_trigger = use_context::<Signal<crate::state::RenameTrigger>>();

    let active_id = tab_state.read().active_id().cloned();
    let is_active = active_id.as_ref() == Some(&meta.id);

    let id    = meta.id.clone();
    let title = meta.title.clone();
    let doc_path = meta.path.clone();
    let doc_title = meta.title.clone();
    let indent = depth as f32 * 12.0;

    let mut ctx_menu: Signal<Option<(f64, f64)>> = use_signal(|| None);

    rsx! {
        button {
            class: if is_active { "tree-item tree-file active" } else { "tree-item tree-file" },
            style: "padding-left: {indent + 20.0}px;",
            onclick: move |_| {
                tab_state.write().open(id.clone(), title.clone());
                *active_route.write() = Route::Notes;
            },
            oncontextmenu: move |e| {
                e.prevent_default();
                let coords = e.client_coordinates();
                *ctx_menu.write() = Some((coords.x, coords.y));
            },
            span { class: "tree-file-icon", "\u{25C7}" }
            span { class: "tree-name", "{meta.title}" }
        }

        if let Some((x, y)) = *ctx_menu.read() {
            {
                let path_for_delete = doc_path.clone();
                let title_for_tab = doc_title.clone();
                let id_for_tab = meta.id.clone();
                let kind_items = {
                    use flynt_core::datum::EntityKind;
                    let current_kind = meta.entity_kind.clone();
                    let mut items = Vec::new();
                    if !matches!(current_kind, Some(EntityKind::DesignNode)) {
                        items.push(crate::components::ContextMenuItem::new("kind-design_node", "Convert to Design Node"));
                    }
                    if !matches!(current_kind, Some(EntityKind::Project)) {
                        items.push(crate::components::ContextMenuItem::new("kind-project", "Convert to Project"));
                    }
                    if current_kind.is_some() {
                        items.push(crate::components::ContextMenuItem::new("kind-clear", "Remove Kind"));
                    }
                    if let Some(first) = items.first_mut() { *first = first.clone().sep(); }
                    items
                };
                rsx! {
                    crate::components::ContextMenu {
                        x, y,
                        items: {
                            let mut all = vec![
                                crate::components::ContextMenuItem::new("open-tab", "Open in New Tab"),
                                crate::components::ContextMenuItem::new("rename", "Rename\u{2026}"),
                                crate::components::ContextMenuItem::new("reveal", if cfg!(target_os = "macos") { "Reveal in Finder" } else { "Open in File Manager" }),
                            ];
                            all.extend(kind_items);
                            all.push(crate::components::ContextMenuItem::danger("delete", "Move to Trash").sep());
                            all
                        },
                        on_close: move |_| *ctx_menu.write() = None,
                        on_select: move |action: String| {
                            *ctx_menu.write() = None;
                            match action.as_str() {
                                "open-tab" => {
                                    tab_state.write().open(id_for_tab.clone(), title_for_tab.clone());
                                    *active_route.write() = Route::Notes;
                                }
                                "rename" => {
                                    tab_state.write().open(id_for_tab.clone(), title_for_tab.clone());
                                    *active_route.write() = Route::Notes;
                                    rename_trigger.write().0 += 1;
                                }
                                "reveal" => {
                                    let abs = ctx.vault().root.join(&path_for_delete);
                                    #[cfg(target_os = "macos")]
                                    { let _ = std::process::Command::new("open").arg("-R").arg(&abs).spawn(); }
                                    #[cfg(target_os = "linux")]
                                    { if let Some(dir) = abs.parent() { let _ = std::process::Command::new("xdg-open").arg(dir).spawn(); } }
                                }
                                a if a.starts_with("kind-") => {
                                    let kind_val = &a[5..];
                                    let p = path_for_delete.clone();
                                    let kind_opt = if kind_val == "clear" { None } else { Some(kind_val.to_string()) };
                                    spawn(async move {
                                        let vault = ctx.vault();
                                        let _ = tokio::task::spawn_blocking(move || {
                                            vault.set_document_kind(&p, kind_opt.as_deref())
                                        }).await;
                                        *refresh.write() += 1;
                                    });
                                }
                                "delete" => {
                                    let p = path_for_delete.clone();
                                    let doc_id = id_for_tab.clone();
                                    spawn(async move {
                                        let vault = ctx.vault();
                                        let abs = vault.root.join(&p);
                                        if abs.exists() {
                                            if let Ok(content) = std::fs::read_to_string(&abs) {
                                                if let Some(excalidraw_file) = crate::views::excalidraw::excalidraw_embed_path(&content) {
                                                    let doc_dir = p.parent().unwrap_or(std::path::Path::new(""));
                                                    let excalidraw_abs = vault.root.join(doc_dir).join(&excalidraw_file);
                                                    let _ = std::fs::remove_file(&excalidraw_abs);
                                                }
                                            }
                                            let _ = std::fs::remove_file(&abs);
                                        }
                                        let _ = vault.store.delete_document(&doc_id);
                                        let tabs = tab_state.read().tabs.clone();
                                        if let Some(idx) = tabs.iter().position(|(id, _)| id == &doc_id) {
                                            tab_state.write().close(idx);
                                        }
                                        let _ = vault.reindex();
                                        *refresh.write() += 1;
                                    });
                                }
                                _ => {}
                            }
                        },
                    }
                }
            }
        }
    }
}

// ── New note input ────────────────────────────────────────────────────────────

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
        div { class: "tree-new-note",
            input {
                class: "tree-new-note-input",
                placeholder: "path/name or name",
                value: "{new_name}",
                oninput: move |e| new_name.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Escape {
                        creating.set(false);
                        return;
                    }
                    if e.key() != Key::Enter { return; }
                    let raw = new_name.read().trim().to_string();
                    if raw.is_empty() { return; }
                    let rel = if raw.ends_with(".md") {
                        std::path::PathBuf::from(&raw)
                    } else {
                        std::path::PathBuf::from(format!("{raw}.md"))
                    };
                    let title = rel.file_stem()
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
                span { class: "tree-new-note-err", "{err}" }
            }
        }
    }
}

// ── Vault selector ────────────────────────────────────────────────────────────

#[component]
fn VaultSelector() -> Element {
    let mut ctx = use_context::<AppContext>();
    let mut active_route = use_context::<Signal<Route>>();
    let mut profile = use_signal(OmegonRuntimeContext::load_launcher_profile);
    let current_root = ctx.vault_root();
    let current_name = ctx.vault().config.vault_name.clone();

    let mut do_switch = move |root: std::path::PathBuf| {
        let new_runtime = crate::bootstrap::runtime_state_for_vault_root(root.clone());
        ctx.set_runtime(new_runtime);
        let mut updated = OmegonRuntimeContext::load_launcher_profile();
        updated.last_vault_root = Some(root);
        let _ = OmegonRuntimeContext::save_launcher_profile(&updated);
        profile.set(updated);
        *active_route.write() = Route::Notes;
    };

    let open_folder = move |_| {
        let Some(selected_root) = FileDialog::new().pick_folder() else { return; };
        let name = selected_root.file_name()
            .and_then(|v| v.to_str()).unwrap_or("Flynt").to_string();
        if OmegonRuntimeContext::initialize_vault(
            &selected_root, &name, flynt_core::models::SyncConfig::None,
        ).is_ok() {
            let mut updated = OmegonRuntimeContext::load_launcher_profile();
            OmegonRuntimeContext::register_known_vault(&mut updated, &selected_root, &name);
            let _ = OmegonRuntimeContext::save_launcher_profile(&updated);
            profile.set(updated);
            do_switch(selected_root);
        }
    };

    let mut expanded = use_signal(|| false);
    let other_vaults: Vec<_> = profile.read().known_vaults.iter()
        .filter(|v| v.root != current_root)
        .cloned()
        .collect();
    let has_others = !other_vaults.is_empty();

    rsx! {
        div { class: "vault-selector",
            button {
                class: "vault-selector-btn",
                onclick: move |_| { let v = *expanded.read(); *expanded.write() = !v; },
                span { class: "vault-selector-name", "{current_name}" }
                if has_others {
                    span { class: "vault-selector-arrow",
                        if *expanded.read() { "\u{25BE}" } else { "\u{25B8}" }
                    }
                }
            }
            if *expanded.read() {
                div { class: "vault-dropdown",
                    for vault in other_vaults {
                        {
                            let root = vault.root.clone();
                            rsx! {
                                button {
                                    class: "vault-dropdown-item",
                                    onclick: move |_| do_switch(root.clone()),
                                    "{vault.name}"
                                }
                            }
                        }
                    }
                    button {
                        class: "vault-dropdown-item muted",
                        onclick: open_folder,
                        "Open folder\u{2026}"
                    }
                }
            }
        }
    }
}

pub fn initial_note_id_for_vault(vault_root: &PathBuf) -> Option<String> {
    let vault = crate::bootstrap::OmegonRuntimeContext::initialize_vault(
        vault_root,
        vault_root.file_name().and_then(|name| name.to_str()).unwrap_or("Flynt"),
        flynt_core::models::SyncConfig::None,
    ).ok()?;
    vault.store.list_documents().ok()?.into_iter().next()
        .map(|doc| doc.id.0.to_string())
}
