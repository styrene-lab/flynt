//! Command Palette — fuzzy-searchable command overlay.
//!
//! Two modes:
//!   ⌘P — Command mode: fuzzy search through commands + notes
//!   ⌘K — Agent mode: natural language delegation to Omegon

use crate::acp::AcpSession;
use crate::bootstrap::AppContext;
use crate::state::{Route, TabState};
use flynt_core::store::VaultStore;
use dioxus::prelude::*;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PaletteMode {
    Command,
    Agent,
}

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
        "view-welcome" => *active_route.write() = Route::Welcome,
        "new-note" => {
            let c = ctx;
            let mut ts = *tab_state;
            let mut ar = *active_route;
            spawn(async move {
                let project = c.project();
                // Generate unique filename to avoid collisions
                let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
                let title = format!("Untitled {ts_suffix}");
                let filename = format!("{title}.md");
                let path = std::path::PathBuf::from(&filename);
                let content = format!("+++\ntitle = \"{title}\"\ntags = []\n+++\n\n");
                if project.save_document_content(&path, &content).is_ok() {
                    let _ = project.reindex();
                    if let Ok(Some(doc)) = project.store.find_document_by_slug(&title.to_lowercase()) {
                        ts.write().open(doc.id, title);
                        *ar.write() = Route::Notes;
                    }
                }
            });
        }
        "icloud-project" => {
            match flynt_store::sync::icloud::create_icloud_vault("Flynt") {
                Ok(root) => {
                    let _ = crate::bootstrap::OmegonRuntimeContext::spawn_new_instance_for_vault(&root);
                }
                Err(e) => {
                    tracing::error!("iCloud project creation failed: {e}");
                }
            }
        }
        other if other.starts_with("template:") => {
            if let Some(tmpl_name) = other.strip_prefix("template:") {
                let templates = flynt_core::templates::list_templates(&ctx.project().root);
                if let Some(tmpl) = templates.iter().find(|t| t.name == tmpl_name) {
                    let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
                    let title = format!("{} {ts_suffix}", tmpl.name);
                    let vault_name = ctx.project().config.vault_name.clone();
                    let content = flynt_core::templates::expand(&tmpl.content, &title, &vault_name);
                    let filename = format!("{title}.md");
                    let path = std::path::PathBuf::from(&filename);
                    let c = ctx;
                    let mut ts = *tab_state;
                    let mut ar = *active_route;
                    spawn(async move {
                        let project = c.project();
                        if project.save_document_content(&path, &content).is_ok() {
                            let _ = project.reindex();
                            if let Ok(Some(doc)) = project.store.find_document_by_slug(&title.to_lowercase()) {
                                ts.write().open(doc.id, title);
                                *ar.write() = Route::Notes;
                            }
                        }
                    });
                }
            }
        }
        "insert-drawing" => {
            // Only works when CM6 editor is active (Notes view with a note open)
            if *active_route.read() != Route::Notes {
                return; // Not on notes view
            }
            let project = ctx.project();
            let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
            let name = format!("Drawing {ts_suffix}");
            if let Ok(_path) = crate::views::excalidraw::create_drawing(&project.root, &name) {
                let embed = format!("![[{name}.excalidraw]]");
                let js = format!(
                    "if(window._flyntCM){{const t=window._flyntCM.state.selection.main.head;window._flyntCM.dispatch({{changes:{{from:t,insert:{escaped}}}}});}}else{{alert('Open a note first to insert a drawing.')}}",
                    escaped = serde_json::to_string(&embed).unwrap_or_default()
                );
                document::eval(&js);
            }
        }
        "new-drawing" => {
            let c = ctx;
            let mut ts = *tab_state;
            let mut ar = *active_route;
            spawn(async move {
                let project = c.project();
                let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
                let name = format!("Drawing {ts_suffix}");
                if let Ok(_md_path) = crate::views::excalidraw::create_drawing(&project.root, &name) {
                    let _ = project.reindex();
                    let slug = name.to_lowercase();
                    if let Ok(Some(doc)) = project.store.find_document_by_slug(&slug) {
                        ts.write().open(doc.id, name);
                    }
                    *ar.write() = Route::Notes;
                }
            });
        }
        "new-canvas" => {
            let c = ctx;
            let mut ts = *tab_state;
            let mut ar = *active_route;
            spawn(async move {
                let project = c.project();
                let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
                let name = format!("Canvas {ts_suffix}");
                if let Ok(_md_path) = crate::views::canvas::create_canvas(&project.root, &name) {
                    let _ = project.reindex();
                    let slug = name.to_lowercase();
                    if let Ok(Some(doc)) = project.store.find_document_by_slug(&slug) {
                        ts.write().open(doc.id, name);
                    }
                    *ar.write() = Route::Notes;
                }
            });
        }
        "daily-note" => {
            let c = ctx.clone();
            let mut ts = *tab_state;
            let mut ar = *active_route;
            spawn(async move {
                let project = c.project();
                let date = flynt_core::daily::today();
                let path = flynt_core::daily::daily_note_path(date);
                let abs = project.root.join(&path);
                if !abs.exists() {
                    let templates = flynt_core::templates::list_templates(&project.root);
                    let tmpl = templates.iter().find(|t| t.name.to_lowercase() == "daily");
                    let content = flynt_core::daily::daily_note_content(date, tmpl.map(|t| t.content.as_str()));
                    if let Some(parent) = abs.parent() { let _ = std::fs::create_dir_all(parent); }
                    let _ = project.save_document_content(&path, &content);
                    let _ = project.reindex();
                }
                let title = date.format("%A, %B %-d, %Y").to_string();
                if let Ok(Some(doc)) = project.store.find_document_by_slug(&date.format("%Y-%m-%d").to_string()) {
                    ts.write().open(doc.id, title);
                    *ar.write() = Route::Notes;
                }
            });
        }
        "sync-now" => {
            let c = ctx;
            spawn(async move {
                let project = c.project();
                if let flynt_core::models::SyncConfig::Git { remote, branch, .. } = &project.config.sync {
                    let git = flynt_store::sync::git::GitSync::new(
                        project.root.clone(),
                        remote.clone(),
                        branch.clone(),
                    );
                    if let Err(e) = git.auto_commit("[flynt] manual sync") {
                        tracing::warn!("sync commit failed: {e}");
                    }
                    if let Err(e) = flynt_core::sync::SyncBackend::sync(&git) {
                        tracing::warn!("sync failed: {e}");
                    }
                }
            });
        }
        "create-tag" => {
            let c = ctx;
            spawn(async move {
                let project = c.project();
                if let flynt_core::models::SyncConfig::Git { remote, branch, .. } = &project.config.sync {
                    let git = flynt_store::sync::git::GitSync::new(
                        project.root.clone(), remote.clone(), branch.clone(),
                    );
                    // Auto-commit first so the tag captures current state
                    let _ = git.auto_commit("[flynt] snapshot");
                    let tag_name = format!("snapshot-{}", chrono::Local::now().format("%Y%m%d-%H%M%S%3f"));
                    match git.create_tag(&tag_name, Some("Flynt project snapshot")) {
                        Ok(()) => {
                            let _ = git.push_tags();
                            tracing::info!("Created snapshot: {tag_name}");
                        }
                        Err(e) => tracing::warn!("Snapshot failed: {e}"),
                    }
                }
            });
        }
        "toggle-agent" => {
            // Handled by the toolbar — the palette just triggers a show_agent toggle.
            // The signal isn't accessible here, but the menu handler in app.rs handles it.
        }
        other if other.starts_with("open:") => {
            if let Some(uuid_str) = other.strip_prefix("open:") {
                if let Ok(uuid) = uuid_str.parse::<uuid::Uuid>() {
                    tab_state.write().open(flynt_core::models::DocumentId(uuid), label.to_string());
                    *active_route.write() = Route::Notes;
                }
            }
        }
        _ => {}
    }
}

#[component]
pub fn CommandPalette(mut open: Signal<bool>, mode: Signal<PaletteMode>) -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();

    let mut query = use_signal(String::new);
    let mut selected = use_signal(|| 0usize);
    let mut agent_status_msg: Signal<Option<&'static str>> = use_signal(|| None);
    let shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();

    // Build the full command list once (memoized — only recomputes when open changes)
    let all_commands = use_memo(move || {
        let _ = *open.read(); // reactive dependency — recompute when palette opens/closes
        let mut all: Vec<Cmd> = vec![
            Cmd { id: "view-notes".into(), label: "Notes".into(), category: "Navigate".into() },
            Cmd { id: "view-board".into(), label: "Tasks".into(), category: "Navigate".into() },
            Cmd { id: "view-graph".into(), label: "Graph".into(), category: "Navigate".into() },
            Cmd { id: "view-settings".into(), label: "Settings".into(), category: "Navigate".into() },
            Cmd { id: "view-welcome".into(), label: "Welcome".into(), category: "Navigate".into() },
            Cmd { id: "new-note".into(), label: "New Note".into(), category: "Create".into() },
            Cmd { id: "new-board".into(), label: "New Board".into(), category: "Create".into() },
            Cmd { id: "daily-note".into(), label: "Today's Note".into(), category: "Create".into() },
        Cmd { id: "new-drawing".into(), label: "New Drawing".into(), category: "Create".into() },
        Cmd { id: "new-canvas".into(), label: "New Canvas".into(), category: "Create".into() },
        Cmd { id: "insert-drawing".into(), label: "Insert Drawing Here".into(), category: "Create".into() },
            Cmd { id: "toggle-agent".into(), label: "Toggle Agent Panel".into(), category: "View".into() },
            Cmd { id: "sync-now".into(), label: "Sync Now".into(), category: "Action".into() },
            Cmd { id: "create-tag".into(), label: "Create Snapshot".into(), category: "Action".into() },
        Cmd { id: "icloud-project".into(), label: "Create Project in iCloud".into(), category: "Create".into() },
        ];
        let templates = flynt_core::templates::list_templates(&ctx.project().root);
        for tmpl in &templates {
            all.push(Cmd {
                id: format!("template:{}", tmpl.name),
                label: format!("New from: {}", tmpl.name),
                category: "Template".into(),
            });
        }
        if let Ok(tags) = ctx.project().list_tags() {
            for (tag, count) in &tags {
                all.push(Cmd {
                    id: format!("filter-tag:{}", tag),
                    label: format!("{} ({} notes)", tag, count),
                    category: "Tag".into(),
                });
            }
        }
        if let Ok(docs) = ctx.project().store.list_documents() {
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

    let current_mode = *mode.read();
    let mut close = move || {
        *open.write() = false;
        *query.write() = String::new();
        *selected.write() = 0;
        *agent_status_msg.write() = None;
    };

    rsx! {
        div {
            class: "palette-overlay",
            onclick: move |_| close(),
        }
        div { class: if current_mode == PaletteMode::Agent { "palette palette-agent" } else { "palette" },

            // Mode tabs — only show Agent tab if connected
            if shared_session.read().is_some() {
                div { class: "palette-mode-bar",
                    button {
                        class: if current_mode == PaletteMode::Command { "palette-mode-tab active" } else { "palette-mode-tab" },
                        onclick: move |_| *mode.write() = PaletteMode::Command,
                        "Commands"
                        span { class: "palette-shortcut", "\u{2318}P" }
                    }
                    button {
                        class: if current_mode == PaletteMode::Agent { "palette-mode-tab active" } else { "palette-mode-tab" },
                        onclick: move |_| *mode.write() = PaletteMode::Agent,
                        "Agent"
                        span { class: "palette-shortcut", "\u{2318}K" }
                    }
                }
            }

            match current_mode {
                PaletteMode::Command => {
                    // ── Command mode (existing behavior) ────────────────────
                    let q = query.read().to_lowercase();
                    let filtered: Vec<Cmd> = all_commands.read().iter()
                        .filter(|c| fuzzy_match(&c.label.to_lowercase(), &q))
                        .cloned()
                        .collect();
                    let sel = (*selected.read()).min(filtered.len().saturating_sub(1));

                    rsx! {
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
                                            close();
                                        }
                                    }
                                    Key::Escape => close(),
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
                                                close();
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
                },
                PaletteMode::Agent => {
                    // ── Agent delegation mode ───────────────────────────────
                    // Fire-and-forget: submit prompt to the shared ACP session,
                    // close the palette. Results flow into the project via watcher
                    // and appear in the agent rail transcript.
                    let has_session = shared_session.read().is_some();

                    rsx! {
                        input {
                            class: "palette-input palette-input-agent",
                            autofocus: true,
                            placeholder: if has_session {
                                "Delegate a task to the agent…"
                            } else {
                                "Agent not connected — open the agent panel first"
                            },
                            value: "{query}",
                            disabled: !has_session,
                            oninput: move |e| *query.write() = e.value(),
                            onkeydown: move |e| {
                                match e.key() {
                                    Key::Enter if !e.modifiers().shift() => {
                                        e.prevent_default();
                                        let prompt = query.read().trim().to_string();
                                        if prompt.is_empty() { return; }

                                        let Some(sess) = shared_session.read().clone() else { return };

                                        // Inject project context: active note title + route
                                        let context_prefix = {
                                            let ts = tab_state.read();
                                            let route = active_route.read();
                                            let mut ctx_parts = Vec::new();
                                            if let Some(title) = ts.active_title() {
                                                ctx_parts.push(format!("[Currently viewing: \"{title}\"]"));
                                            }
                                            match *route {
                                                Route::Kanban => ctx_parts.push("[On: Tasks view]".into()),
                                                Route::Graph => ctx_parts.push("[On: Graph view]".into()),
                                                Route::Settings => ctx_parts.push("[On: Settings]".into()),
                                                _ => {}
                                            }
                                            if ctx_parts.is_empty() {
                                                String::new()
                                            } else {
                                                format!("{}\n\n", ctx_parts.join(" "))
                                            }
                                        };
                                        let full_prompt = format!("{context_prefix}{prompt}");

                                        // Persist delegation to project for audit trail
                                        let project = ctx.project();
                                        let ts = chrono::Local::now();
                                        let del_path = format!(
                                            "ai/delegations/{}.md",
                                            ts.format("%Y%m%d-%H%M%S%3f")
                                        );
                                        let del_content = format!(
                                            "+++\ntitle = \"Delegation {}\"\ntags = [\"delegation\"]\n+++\n\n{}\n",
                                            ts.format("%H:%M"),
                                            prompt,
                                        );
                                        let _ = project.save_document_content(
                                            std::path::Path::new(&del_path),
                                            &del_content,
                                        );

                                        // Fire and forget — submit to the existing session
                                        sess.prompt(&full_prompt);

                                        // Brief confirmation, then close
                                        *agent_status_msg.write() = Some("Delegated");
                                        let mut open_sig = open;
                                        let mut query_sig = query;
                                        let mut msg_sig = agent_status_msg;
                                        spawn(async move {
                                            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                                            *open_sig.write() = false;
                                            *query_sig.write() = String::new();
                                            *msg_sig.write() = None;
                                        });
                                    }
                                    Key::Escape => close(),
                                    _ => {}
                                }
                            },
                        }

                        div { class: "palette-agent-body",
                            if let Some(msg) = *agent_status_msg.read() {
                                div { class: "palette-agent-delegated",
                                    span { class: "palette-agent-check", "\u{2713}" }
                                    span { "{msg}" }
                                }
                            } else if !has_session {
                                div { class: "palette-agent-hint palette-agent-hint-warn",
                                    "Toggle the agent panel (View > Agent) to connect, then use \u{2318}K to delegate."
                                }
                            } else {
                                div { class: "palette-agent-hint",
                                    "Describe what you need. The agent acts on your project and results appear in your notes."
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}
