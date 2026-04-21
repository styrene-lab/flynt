use crate::{
    bootstrap::{bootstrap_from_env, runtime_state_for_vault_root, AppContext, OmegonRuntimeContext, PendingVaultSetup},
    components::{initial_note_id_for_vault, AgentRail, CommandPalette, Sidebar, TabBar, Toolbar},
    state::{Route, SyncStatus, TabState, ThemeName},
    views::{GraphView, KanbanView, NotesView, SearchView, SettingsView, WelcomeView},
};
use codex_core::store::VaultStore;
use dioxus::prelude::*;
use rfd::FileDialog;
use std::path::PathBuf;

#[component]
pub fn App() -> Element {
    let initial_runtime = bootstrap_from_env();
    let runtime = use_signal(|| initial_runtime.clone());
    let ctx = AppContext { runtime };
    use_context_provider(|| ctx.clone());

    let current_runtime = ctx.runtime.read().clone();

    let theme = use_context_provider(|| {
        Signal::new(ThemeName(current_runtime.vault.config.appearance.theme.clone()))
    });
    let font_size = use_context_provider(|| Signal::new(current_runtime.vault.config.appearance.font_size));
    use_context_provider(|| Signal::new(current_runtime.omegon.load_project_profile()));
    use_context_provider(|| Signal::new(current_runtime.omegon.load_operator_settings()));
    use_context_provider(|| Signal::new(None::<tokio::process::Child>));
    use_context_provider(|| Signal::new(None::<u32>));
    use_context_provider(|| Signal::new(None::<String>));

    // Tab state — provided via context so sidebar, tab bar, and notes share it
    use_context_provider(|| Signal::new(TabState::default()));

    // Route — provided via context so search view can navigate back
    let mut active_route = use_context_provider(|| {
        let launcher_profile = OmegonRuntimeContext::load_launcher_profile();
        let route = if launcher_profile.wizard_completed || launcher_profile.last_vault_root.is_some() {
            Route::Notes
        } else {
            Route::Welcome
        };
        Signal::new(route)
    });
    let mut tab_state = use_context::<Signal<TabState>>();
    let show_agent = use_signal(|| false);
    let sync_status = use_signal(|| SyncStatus::Idle);
    let mut palette_open = use_signal(|| false);

    // Shared search query — lives here so toolbar and search view share it
    let search_query: Signal<String> = use_signal(String::new);

    // ── Native menu event handler ────────────────────────────────────────
    let ctx_menu_handler = ctx.clone();
    let mut show_agent_menu = show_agent;
    dioxus::desktop::use_muda_event_handler(move |event| {
        match event.id().0.as_str() {
            crate::menu::VIEW_NOTES => *active_route.write() = Route::Notes,
            crate::menu::VIEW_BOARD => *active_route.write() = Route::Kanban,
            crate::menu::VIEW_GRAPH => *active_route.write() = Route::Graph,
            crate::menu::VIEW_SETTINGS => *active_route.write() = Route::Settings,
            crate::menu::TOGGLE_AGENT => {
                let v = *show_agent_menu.read();
                *show_agent_menu.write() = !v;
            }
            crate::menu::CLOSE_TAB => {
                let active = tab_state.read().active;
                if !tab_state.read().tabs.is_empty() {
                    tab_state.write().close(active);
                }
            }
            crate::menu::NEW_NOTE => {
                let c = ctx_menu_handler;
                spawn(async move {
                    let vault = c.vault();
                    let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
                    let title = format!("Untitled {ts_suffix}");
                    let filename = format!("{title}.md");
                    let path = std::path::PathBuf::from(&filename);
                    let content = format!("+++\ntitle = \"{title}\"\ntags = []\n+++\n\n");
                    if vault.save_document_content(&path, &content).is_ok() {
                        let _ = vault.reindex();
                        if let Ok(Some(doc)) = vault.store.find_document_by_slug(&title.to_lowercase()) {
                            tab_state.write().open(doc.id, title);
                            *active_route.write() = Route::Notes;
                        }
                    }
                });
            }
            crate::menu::DAILY_NOTE => {
                let c = ctx_menu_handler;
                let mut ts = tab_state;
                let mut ar = active_route;
                spawn(async move {
                    let vault = c.vault();
                    let date = codex_core::daily::today();
                    let path = codex_core::daily::daily_note_path(date);
                    let abs = vault.root.join(&path);
                    if !abs.exists() {
                        // Load daily template if it exists
                        let templates = codex_core::templates::list_templates(&vault.root);
                        let tmpl = templates.iter().find(|t| t.name.to_lowercase() == "daily");
                        let content = codex_core::daily::daily_note_content(
                            date,
                            tmpl.map(|t| t.content.as_str()),
                        );
                        if let Some(parent) = abs.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
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
            crate::menu::NEW_DRAWING => {
                let c = ctx_menu_handler;
                let mut ts = tab_state;
                let mut ar = active_route;
                spawn(async move {
                    let vault = c.vault();
                    let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
                    let name = format!("Drawing {ts_suffix}");
                    if let Ok(_path) = crate::views::excalidraw::create_drawing(&vault.root, &name) {
                        let _ = vault.reindex();
                        let slug = name.to_lowercase();
                        if let Ok(Some(doc)) = vault.store.find_document_by_slug(&slug) {
                            ts.write().open(doc.id, name);
                        }
                        *ar.write() = Route::Notes;
                    }
                });
            }
            crate::menu::OPEN_VAULT => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    let _ = OmegonRuntimeContext::spawn_new_instance_for_vault(&path);
                }
            }
            _ => {}
        }
    });

    let mut launcher_profile = use_signal(OmegonRuntimeContext::load_launcher_profile);

    // Clone dialog state
    let mut clone_dialog_open = use_signal(|| false);
    let mut clone_url: Signal<String> = use_signal(|| "git@github.com:".to_string());
    let mut clone_branch: Signal<String> = use_signal(|| "main".to_string());
    let mut clone_error: Signal<Option<String>> = use_signal(|| None);
    let mut clone_busy = use_signal(|| false);

    // Welcome screen error banner
    let mut welcome_error: Signal<Option<String>> = use_signal(|| None);

    let ctx_for_switch = ctx.clone();
    let _switch_runtime = move |selected_root: PathBuf| {
        let mut ctx = ctx_for_switch.clone();
        ctx.set_runtime(runtime_state_for_vault_root(selected_root));
    };

    rsx! {
        // Prevent flash of unstyled content — hide body until theme loads
        document::Style { "body {{ opacity: 0; transition: opacity 0.1s; }} body.ready {{ opacity: 1; }}" }

        document::Link {
            rel: "stylesheet",
            href: "https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/styles/base16/ocean.min.css",
        }
        document::Script {
            src: "https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/highlight.min.js",
        }
        document::Script {
            src: asset!("/assets/vendor/codemirror.bundle.js"),
        }
        // Excalidraw loaded lazily — see views/excalidraw.rs
        // Bundle copied into app by build-release.sh post-build step
        document::Stylesheet { href: asset!("/assets/themes/alpharius.css") }
        document::Stylesheet { href: asset!("/assets/styles/reset.css") }
        document::Stylesheet { href: asset!("/assets/styles/layout.css") }
        document::Stylesheet { href: asset!("/assets/styles/components.css") }
        document::Stylesheet { href: asset!("/assets/styles/markdown.css") }
        document::Stylesheet { href: asset!("/assets/styles/settings.css") }
        document::Stylesheet { href: asset!("/assets/styles/kanban.css") }
        document::Stylesheet { href: asset!("/assets/styles/tabs.css") }
        document::Stylesheet { href: asset!("/assets/styles/search.css") }
        document::Stylesheet { href: asset!("/assets/styles/graph.css") }
        document::Stylesheet { href: asset!("/assets/styles/welcome.css") }
        // Reveal body after stylesheets are loaded
        document::Script { "document.body.classList.add('ready');" }

        div {
            class: "codex-shell {font_size.read().css_class()}",
            "data-theme": "{theme.read().0}",
            tabindex: "0",
            onkeydown: move |e| {
                // ⌘P — command palette
                if (e.modifiers().meta() || e.modifiers().ctrl()) && e.key() == Key::Character("p".to_string()) {
                    e.prevent_default();
                    let v = *palette_open.read();
                    *palette_open.write() = !v;
                }
            },

            CommandPalette { open: palette_open }

            Toolbar {
                sync_status,
                show_agent,
                search_query,
                active_route,
            }
            div { class: "codex-body",
                Sidebar { active_route: active_route }
                div { class: "main-content",
                    // Tab bar above the content area (only in Notes mode)
                    if *active_route.read() == Route::Notes {
                        TabBar {}
                    }
                    match *active_route.read() {
                        Route::Welcome => {
                            let mut choose_ctx = ctx.clone();
                            let mut create_ctx = ctx.clone();
                            let import_ctx = ctx.clone();
                            let on_choose_existing = move |_| {
                                *welcome_error.write() = None;
                                let Some(selected_root) = FileDialog::new().pick_folder() else {
                                    return;
                                };
                                if !selected_root.is_dir() {
                                    *welcome_error.write() = Some("Please select a folder, not a file.".into());
                                    return;
                                }
                                if let Err(e) = OmegonRuntimeContext::initialize_vault(
                                    &selected_root,
                                    selected_root
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("Codex"),
                                    codex_core::models::SyncConfig::None,
                                ) {
                                    *welcome_error.write() = Some(format!("Could not open vault: {e}"));
                                    return;
                                }
                                let mut profile = launcher_profile();
                                profile.pending_setup = Some(PendingVaultSetup::OpenExisting {
                                    path: selected_root.clone(),
                                });
                                profile.last_vault_root = Some(selected_root.clone());
                                profile.wizard_completed = true;
                                if !profile.recent_vaults.contains(&selected_root) {
                                    profile.recent_vaults.push(selected_root.clone());
                                }
                                let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                launcher_profile.set(profile);
                                choose_ctx.set_runtime(runtime_state_for_vault_root(selected_root.clone()));
                                if let Some(note_id) = initial_note_id_for_vault(&selected_root) {
                                    if let Ok(parsed) = uuid::Uuid::parse_str(&note_id) {
                                        tab_state.write().open(
                                            codex_core::models::DocumentId(parsed),
                                            "Notes".into(),
                                        );
                                    }
                                }
                                *active_route.write() = Route::Notes;
                            };
                            let on_create_local = move |_| {
                                *welcome_error.write() = None;
                                let Some(local_path) = FileDialog::new()
                                    .set_directory(
                                        dirs::document_dir()
                                            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."))),
                                    )
                                    .pick_folder()
                                else {
                                    return;
                                };
                                let name = local_path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("Black Meridian")
                                    .to_string();
                                match OmegonRuntimeContext::initialize_vault(
                                    &local_path,
                                    &name,
                                    codex_core::models::SyncConfig::None,
                                ) {
                                    Ok(_vault) => {
                                        let mut profile = launcher_profile();
                                        profile.pending_setup = Some(PendingVaultSetup::CreateLocal {
                                            path: local_path.clone(),
                                            name: name.clone(),
                                        });
                                        profile.last_vault_root = Some(local_path.clone());
                                        profile.wizard_completed = true;
                                        if !profile.recent_vaults.contains(&local_path) {
                                            profile.recent_vaults.push(local_path.clone());
                                        }
                                        let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                        launcher_profile.set(profile);
                                        create_ctx.set_runtime(runtime_state_for_vault_root(local_path.clone()));
                                        *active_route.write() = Route::Notes;
                                    }
                                    Err(e) => {
                                        *welcome_error.write() = Some(format!("Could not create vault: {e}"));
                                    }
                                }
                            };
                            let on_clone_remote = move |_| {
                                *clone_dialog_open.write() = true;
                                *clone_error.write() = None;
                            };
                            let on_import_markdown = move |_| {
                                *welcome_error.write() = None;
                                let Some(source_root) = FileDialog::new().pick_folder() else {
                                    return;
                                };
                                match import_ctx.vault().import_markdown_tree(&source_root) {
                                    Ok(_count) => {
                                        let mut profile = launcher_profile();
                                        profile.wizard_completed = true;
                                        let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                        launcher_profile.set(profile);
                                        *active_route.write() = Route::Notes;
                                    }
                                    Err(e) => {
                                        *welcome_error.write() = Some(format!("Import failed: {e}"));
                                    }
                                }
                            };
                            let on_seed_demo_publication = move |_| {
                                *welcome_error.write() = None;
                                let Some(repo_root) = FileDialog::new()
                                    .set_directory(
                                        dirs::document_dir()
                                            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."))),
                                    )
                                    .pick_folder()
                                else {
                                    return;
                                };
                                if let Err(e) = OmegonRuntimeContext::seed_demo_publication_repo(&repo_root) {
                                    *welcome_error.write() = Some(format!("Could not create demo: {e}"));
                                    return;
                                }
                                let site_name = repo_root
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("codex-publication-demo")
                                    .to_string();
                                let mut profile = launcher_profile();
                                profile.pending_setup = Some(PendingVaultSetup::SeedDemoPublication {
                                    repo_root: repo_root.clone(),
                                    site_name,
                                });
                                profile.wizard_completed = true;
                                let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                launcher_profile.set(profile);
                            };
                            rsx! {
                                if let Some(err) = welcome_error.read().as_ref() {
                                    div { class: "welcome-error-banner",
                                        span { class: "welcome-error-text", "{err}" }
                                        button {
                                            class: "welcome-error-dismiss",
                                            onclick: move |_| *welcome_error.write() = None,
                                            "Dismiss"
                                        }
                                    }
                                }
                                WelcomeView {
                                    launcher_profile: launcher_profile(),
                                    on_choose_existing,
                                    on_create_local,
                                    on_clone_remote,
                                    on_import_markdown,
                                    on_seed_demo_publication,
                                }
                            }
                        },
                        Route::Notes    => rsx! { NotesView {} },

                        Route::Search   => rsx! { SearchView { search_query } },
                        Route::Kanban   => rsx! { KanbanView {} },
                        Route::Graph    => rsx! { GraphView {} },
                        Route::Settings => rsx! { SettingsView {} },
                    }
                }
                // Clone remote vault dialog
                if *clone_dialog_open.read() {
                    div { class: "modal-overlay",
                        onclick: move |_| *clone_dialog_open.write() = false,
                        div { class: "modal-dialog",
                            onclick: move |e| e.stop_propagation(),
                            h2 { "Clone remote vault" }
                            p { class: "modal-hint", "Enter the Git repository URL and branch. SSH (git@...) and HTTPS both work." }

                            div { class: "modal-field",
                                label { "Repository URL" }
                                input {
                                    r#type: "text",
                                    value: "{clone_url}",
                                    placeholder: "git@github.com:user/vault.git",
                                    oninput: move |e| *clone_url.write() = e.value(),
                                }
                            }
                            div { class: "modal-field",
                                label { "Branch" }
                                input {
                                    r#type: "text",
                                    value: "{clone_branch}",
                                    placeholder: "main",
                                    oninput: move |e| *clone_branch.write() = e.value(),
                                }
                            }

                            if let Some(err) = clone_error.read().as_ref() {
                                div { class: "modal-error", "{err}" }
                            }

                            div { class: "modal-actions",
                                button {
                                    class: "modal-btn secondary",
                                    onclick: move |_| *clone_dialog_open.write() = false,
                                    "Cancel"
                                }
                                button {
                                    class: "modal-btn primary",
                                    disabled: *clone_busy.read(),
                                    onclick: {
                                        let mut github_ctx = ctx.clone();
                                        move |_| {
                                            let url = clone_url.read().trim().to_string();
                                            let branch = clone_branch.read().trim().to_string();
                                            if url.is_empty() {
                                                *clone_error.write() = Some("Repository URL is required".into());
                                                return;
                                            }
                                            let branch = if branch.is_empty() { "main".to_string() } else { branch };

                                            // Derive a folder name from the URL
                                            let repo_name = url
                                                .rsplit('/')
                                                .next()
                                                .unwrap_or("vault")
                                                .trim_end_matches(".git")
                                                .to_string();
                                            let dest = dirs::document_dir()
                                                .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
                                                .join(&repo_name);

                                            *clone_busy.write() = true;
                                            *clone_error.write() = None;

                                            match OmegonRuntimeContext::clone_remote_vault(&dest, &url, &branch) {
                                                Ok(_vault) => {
                                                    let mut profile = launcher_profile();
                                                    profile.pending_setup = Some(PendingVaultSetup::LinkGithub {
                                                        local_path: dest.clone(),
                                                        repo: url,
                                                        branch,
                                                    });
                                                    profile.last_vault_root = Some(dest.clone());
                                                    profile.wizard_completed = true;
                                                    if !profile.recent_vaults.contains(&dest) {
                                                        profile.recent_vaults.push(dest.clone());
                                                    }
                                                    let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                                    launcher_profile.set(profile);
                                                    github_ctx.set_runtime(runtime_state_for_vault_root(dest));
                                                    *clone_dialog_open.write() = false;
                                                    *active_route.write() = Route::Notes;
                                                }
                                                Err(e) => {
                                                    *clone_error.write() = Some(format!("{e:#}"));
                                                }
                                            }
                                            *clone_busy.write() = false;
                                        }
                                    },
                                    if *clone_busy.read() { "Cloning..." } else { "Clone" }
                                }
                            }
                        }
                    }
                }

                if show_agent() {
                    AgentRail {}
                }
            }
        }
    }
}
