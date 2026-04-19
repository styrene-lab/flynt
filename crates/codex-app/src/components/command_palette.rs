//! Command Palette — fuzzy-searchable command overlay (⌘P).

use crate::bootstrap::AppContext;
use crate::state::{Route, TabState};
use codex_core::store::VaultStore;
use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
struct Cmd {
    id: String,
    label: String,
    category: String,
}

fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() { return true; }
    let mut hi = haystack.chars();
    for nc in needle.chars() {
        loop {
            match hi.next() {
                Some(hc) if hc == nc => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

fn execute_command(
    id: &str,
    label: &str,
    ctx: AppContext,
    tab_state: &mut Signal<TabState>,
    active_route: &mut Signal<Route>,
) {
    match id {
        "view-notes" => *active_route.write() = Route::Notes,
        "view-board" => *active_route.write() = Route::Kanban,
        "view-graph" => *active_route.write() = Route::Graph,
        "view-settings" => *active_route.write() = Route::Settings,
        "new-note" => {
            let c = ctx;
            let mut ts = *tab_state;
            let mut ar = *active_route;
            spawn(async move {
                let vault = c.vault();
                // Generate unique filename to avoid collisions
                let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
                let title = format!("Untitled {ts_suffix}");
                let filename = format!("{title}.md");
                let path = std::path::PathBuf::from(&filename);
                let content = format!("+++\ntitle = \"{title}\"\ntags = []\n+++\n\n");
                if vault.save_document_content(&path, &content).is_ok() {
                    let _ = vault.reindex();
                    if let Ok(Some(doc)) = vault.store.find_document_by_slug(&title.to_lowercase()) {
                        ts.write().open(doc.id, title);
                        *ar.write() = Route::Notes;
                    }
                }
            });
        }
        "icloud-vault" => {
            match codex_store::sync::icloud::create_icloud_vault("Codex") {
                Ok(root) => {
                    let _ = crate::bootstrap::OmegonRuntimeContext::spawn_new_instance_for_vault(&root);
                }
                Err(e) => {
                    tracing::error!("iCloud vault creation failed: {e}");
                }
            }
        }
        other if other.starts_with("template:") => {
            if let Some(tmpl_name) = other.strip_prefix("template:") {
                let templates = codex_core::templates::list_templates(&ctx.vault().root);
                if let Some(tmpl) = templates.iter().find(|t| t.name == tmpl_name) {
                    let title = "Untitled";
                    let vault_name = &ctx.vault().config.vault_name;
                    let content = codex_core::templates::expand(&tmpl.content, title, vault_name);
                    let path = std::path::Path::new("Untitled.md");
                    let c = ctx;
                    let mut ts = *tab_state;
                    let mut ar = *active_route;
                    spawn(async move {
                        let vault = c.vault();
                        if vault.save_document_content(path, &content).is_ok() {
                            let _ = vault.reindex();
                            if let Ok(Some(doc)) = vault.store.find_document_by_slug("untitled") {
                                ts.write().open(doc.id, "Untitled".into());
                                *ar.write() = Route::Notes;
                            }
                        }
                    });
                }
            }
        }
        "new-drawing" => {
            let vault = ctx.vault();
            let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
            let name = format!("Drawing {ts_suffix}");
            if let Ok(_path) = crate::views::excalidraw::create_drawing(&vault.root, &name) {
                let _ = vault.reindex();
                // Open the drawing — for now just navigate to notes
                // The notes view will detect .excalidraw and render appropriately
                *active_route.write() = Route::Notes;
            }
        }
        "daily-note" => {
            let c = ctx.clone();
            let mut ts = *tab_state;
            let mut ar = *active_route;
            spawn(async move {
                let vault = c.vault();
                let date = codex_core::daily::today();
                let path = codex_core::daily::daily_note_path(date);
                let abs = vault.root.join(&path);
                if !abs.exists() {
                    let templates = codex_core::templates::list_templates(&vault.root);
                    let tmpl = templates.iter().find(|t| t.name.to_lowercase() == "daily");
                    let content = codex_core::daily::daily_note_content(date, tmpl.map(|t| t.content.as_str()));
                    if let Some(parent) = abs.parent() { let _ = std::fs::create_dir_all(parent); }
                    let _ = vault.save_document_content(&path, &content);
                    let _ = vault.reindex();
                }
                let title = date.format("%A, %B %-d, %Y").to_string();
                if let Ok(Some(doc)) = vault.store.find_document_by_slug(&date.format("%Y-%m-%d").to_string()) {
                    ts.write().open(doc.id, title);
                    *ar.write() = Route::Notes;
                }
            });
        }
        other if other.starts_with("open:") => {
            if let Some(uuid_str) = other.strip_prefix("open:") {
                if let Ok(uuid) = uuid_str.parse::<uuid::Uuid>() {
                    tab_state.write().open(codex_core::models::DocumentId(uuid), label.to_string());
                    *active_route.write() = Route::Notes;
                }
            }
        }
        _ => {}
    }
}

#[component]
pub fn CommandPalette(mut open: Signal<bool>) -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();

    let mut query = use_signal(String::new);
    let mut selected = use_signal(|| 0usize);

    // Build the full command list once (memoized — only recomputes when open changes)
    let all_commands = use_memo(move || {
        let _ = *open.read(); // reactive dependency — recompute when palette opens/closes
        let mut all: Vec<Cmd> = vec![
            Cmd { id: "view-notes".into(), label: "Notes".into(), category: "Navigate".into() },
            Cmd { id: "view-board".into(), label: "Board".into(), category: "Navigate".into() },
            Cmd { id: "view-graph".into(), label: "Graph".into(), category: "Navigate".into() },
            Cmd { id: "view-settings".into(), label: "Settings".into(), category: "Navigate".into() },
            Cmd { id: "new-note".into(), label: "New Note".into(), category: "Create".into() },
            Cmd { id: "new-board".into(), label: "New Board".into(), category: "Create".into() },
            Cmd { id: "daily-note".into(), label: "Today's Note".into(), category: "Create".into() },
        Cmd { id: "new-drawing".into(), label: "New Drawing".into(), category: "Create".into() },
            Cmd { id: "toggle-agent".into(), label: "Toggle Agent Panel".into(), category: "View".into() },
            Cmd { id: "sync-now".into(), label: "Sync Now".into(), category: "Action".into() },
        Cmd { id: "icloud-vault".into(), label: "Create Vault in iCloud".into(), category: "Create".into() },
        ];
        let templates = codex_core::templates::list_templates(&ctx.vault().root);
        for tmpl in &templates {
            all.push(Cmd {
                id: format!("template:{}", tmpl.name),
                label: format!("New from: {}", tmpl.name),
                category: "Template".into(),
            });
        }
        if let Ok(tags) = ctx.vault().list_tags() {
            for (tag, count) in &tags {
                all.push(Cmd {
                    id: format!("filter-tag:{}", tag),
                    label: format!("{} ({} notes)", tag, count),
                    category: "Tag".into(),
                });
            }
        }
        if let Ok(docs) = ctx.vault().store.list_documents() {
            for doc in docs {
                all.push(Cmd {
                    id: format!("open:{}", doc.id.0),
                    label: doc.title,
                    category: "Open".into(),
                });
            }
        }
        all
    });

    if !*open.read() {
        return rsx! {};
    }

    // Filter is cheap — just string matching on the cached list
    let q = query.read().to_lowercase();
    let filtered: Vec<Cmd> = all_commands.read().iter()
        .filter(|c| fuzzy_match(&c.label.to_lowercase(), &q))
        .cloned()
        .collect();
    let sel = (*selected.read()).min(filtered.len().saturating_sub(1));

    rsx! {
        div {
            class: "palette-overlay",
            onclick: move |_| {
                *open.write() = false;
                *query.write() = String::new();
            },
        }
        div { class: "palette",
            input {
                class: "palette-input",
                autofocus: true,
                placeholder: "Type a command or note name…",
                value: "{query}",
                oninput: move |e| {
                    *query.write() = e.value();
                    *selected.write() = 0;
                },
                onkeydown: move |e| {
                    let max = filtered.len().saturating_sub(1);
                    match e.key() {
                        Key::ArrowDown => {
                            e.prevent_default();
                            let s = *selected.read();
                            *selected.write() = if s >= max { 0 } else { s + 1 };
                        }
                        Key::ArrowUp => {
                            e.prevent_default();
                            let s = *selected.read();
                            *selected.write() = if s == 0 { max } else { s - 1 };
                        }
                        Key::Enter => {
                            if let Some(cmd) = filtered.get(sel) {
                                execute_command(&cmd.id, &cmd.label, ctx, &mut tab_state, &mut active_route);
                                *open.write() = false;
                                *query.write() = String::new();
                                *selected.write() = 0;
                            }
                        }
                        Key::Escape => {
                            *open.write() = false;
                            *query.write() = String::new();
                        }
                        _ => {}
                    }
                },
            }
            div { class: "palette-results",
                for (i, cmd) in filtered.iter().enumerate() {
                    {
                        let cmd_id = cmd.id.clone();
                        let cmd_label = cmd.label.clone();
                        rsx! {
                            button {
                                key: "{i}-{cmd_id}",
                                class: if i == sel { "palette-item selected" } else { "palette-item" },
                                onclick: move |_| {
                                    execute_command(&cmd_id, &cmd_label, ctx, &mut tab_state, &mut active_route);
                                    *open.write() = false;
                                    *query.write() = String::new();
                                    *selected.write() = 0;
                                },
                                span { class: "palette-category", "{cmd.category}" }
                                span { class: "palette-label", "{cmd.label}" }
                            }
                        }
                    }
                }
                if filtered.is_empty() {
                    div { class: "palette-empty", "No matching commands" }
                }
            }
        }
    }
}
