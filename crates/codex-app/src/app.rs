use crate::{
    bootstrap::{bootstrap_from_env, runtime_state_for_vault_root, AppContext, OmegonRuntimeContext, PendingVaultSetup},
    components::{initial_note_id_for_vault, AgentRail, CommandPalette, Sidebar, Toolbar},
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

    // Shared ACP session — populated by AgentRail, used by CommandPalette agent mode
    use_context_provider(|| Signal::new(None::<std::rc::Rc<crate::acp::AcpSession>>));

    // Tab state — provided via context so sidebar, tab bar, and notes share it
    use_context_provider(|| Signal::new(TabState::default()));

    // Drawing mode flag — set by NotesView when showing ExcalidrawView
    let _is_drawing = use_context_provider(|| Signal::new(false));

    // Rename trigger — sidebar bumps this, NotesView watches and opens inline rename
    use_context_provider(|| Signal::new(crate::state::RenameTrigger(0)));


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
    let mut sync_status = use_signal(|| SyncStatus::Idle);

    // Poll sync status from the auto-sync watcher
    {
        let runtime_for_sync = ctx.runtime.clone();
        use_future(move || async move {
            loop {
                let rx_opt = runtime_for_sync.read().sync_status_rx.clone();
                if let Some(rx) = rx_opt {
                    let status = rx.borrow().clone();
                    let ui_status = match status {
                        codex_store::sync::AutoSyncStatus::Idle => SyncStatus::Idle,
                        codex_store::sync::AutoSyncStatus::Committing
                        | codex_store::sync::AutoSyncStatus::Pulling
                        | codex_store::sync::AutoSyncStatus::Pushing => SyncStatus::Syncing,
                        codex_store::sync::AutoSyncStatus::Conflict(files) => SyncStatus::Conflict(files.len()),
                        codex_store::sync::AutoSyncStatus::Error(_) => SyncStatus::Syncing, // transient
                    };
                    *sync_status.write() = ui_status;
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
    }
    let mut palette_open = use_signal(|| false);
    let mut palette_mode = use_signal(|| crate::components::command_palette::PaletteMode::Command);
    let shared_acp_session = use_context::<Signal<Option<std::rc::Rc<crate::acp::AcpSession>>>>();

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
                    if let Ok(_md_path) = crate::views::excalidraw::create_drawing(&vault.root, &name) {
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
            crate::menu::DELETE_NOTE => {
                let c = ctx_menu_handler;
                let mut ts = tab_state;
                spawn(async move {
                    let active_id = ts.read().active_id().cloned();
                    if let Some(doc_id) = active_id {
                        let vault = c.vault();
                        if let Ok(Some(doc)) = vault.store.get_document(&doc_id) {
                            let abs = vault.root.join(&doc.path);
                            if abs.exists() {
                                let _ = std::fs::remove_file(&abs);
                            }
                            let _ = vault.store.delete_document(&doc_id);
                            let idx = ts.read().active;
                            ts.write().close(idx);
                        }
                    }
                });
            }
            _ => {}
        }
    });

    // Auto-render SVG when .excalidraw or .d2 files are created/modified
    {
        let vault_events = ctx.vault_events();
        let vault_for_svg = ctx.vault();
        use_future(move || {
            let mut rx = vault_events.subscribe();
            let vault = vault_for_svg.clone();
            async move {
                loop {
                    let Ok(evt) = rx.recv().await else { break };
                    // Re-read viz config on each event so settings changes take effect immediately
                    let viz = vault.config.visualization.clone();
                    let path = match evt {
                        codex_store::watcher::VaultChangeEvent::FileCreated(p)
                        | codex_store::watcher::VaultChangeEvent::FileModified(p) => p,
                        _ => continue,
                    };
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let svg_path = path.with_extension("svg");

                    match ext {
                        "excalidraw" if viz.excalidraw_auto_export => {
                            // Render via the webview's Excalidraw bundle
                            let scene_json = match std::fs::read_to_string(&path) {
                                Ok(s) => s,
                                Err(_) => continue,
                            };
                            let escaped = serde_json::to_string(&scene_json).unwrap_or_default();
                            let js = format!(
                                r#"(async function() {{
                                    if (window.CodexExcalidraw && window.CodexExcalidraw.renderSceneToSvg) {{
                                        let svg = await window.CodexExcalidraw.renderSceneToSvg({escaped});
                                        dioxus.send(svg || '');
                                    }} else {{ dioxus.send(''); }}
                                }})();"#
                            );
                            let mut eval = document::eval(&js);
                            if let Ok(svg) = eval.recv::<String>().await {
                                if !svg.is_empty() {
                                    let _ = std::fs::write(&svg_path, &svg);
                                }
                            }
                        }
                        "d2" if viz.d2_auto_render => {
                            // Render via d2 CLI with configured theme and layout
                            let input = path.clone();
                            let output = svg_path.clone();
                            let d2_bin = viz.d2_bin.clone().unwrap_or_else(|| "d2".into());
                            let theme = viz.d2_theme.to_string();
                            let layout = viz.d2_layout.clone();
                            let result = tokio::task::spawn_blocking(move || {
                                // Enrich PATH for GUI apps that inherit a stripped environment
                                let mut path_env = std::env::var("PATH").unwrap_or_default();
                                for extra in [
                                    "/opt/homebrew/bin",
                                    "/usr/local/bin",
                                    "/etc/profiles/per-user/default/bin",
                                ] {
                                    if !path_env.contains(extra) {
                                        path_env = format!("{extra}:{path_env}");
                                    }
                                }
                                // Also prepend user-specific Nix/Homebrew paths
                                if let Ok(home) = std::env::var("HOME") {
                                    for suffix in [".nix-profile/bin", ".local/bin"] {
                                        let p = format!("{home}/{suffix}");
                                        if !path_env.contains(&p) {
                                            path_env = format!("{p}:{path_env}");
                                        }
                                    }
                                }
                                // Spawn with timeout to prevent blocking the thread pool
                                let mut child = std::process::Command::new(&d2_bin)
                                    .env("PATH", &path_env)
                                    .args(["--theme", &theme, "--layout", &layout])
                                    .arg(&input)
                                    .arg(&output)
                                    .stdout(std::process::Stdio::piped())
                                    .stderr(std::process::Stdio::piped())
                                    .spawn()?;
                                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                                loop {
                                    match child.try_wait() {
                                        Ok(Some(_)) => break,
                                        Ok(None) if std::time::Instant::now() > deadline => {
                                            let _ = child.kill();
                                            return Err(std::io::Error::new(
                                                std::io::ErrorKind::TimedOut,
                                                "D2 render timed out after 30s",
                                            ));
                                        }
                                        Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
                                        Err(e) => return Err(e),
                                    }
                                }
                                child.wait_with_output()
                            }).await;
                            match result {
                                Ok(Ok(out)) if out.status.success() => {
                                    tracing::debug!("D2 rendered: {}", svg_path.display());
                                }
                                Ok(Ok(out)) => {
                                    let stderr = String::from_utf8_lossy(&out.stderr);
                                    tracing::warn!("D2 render failed: {stderr}");
                                }
                                Ok(Err(e)) => {
                                    if e.kind() == std::io::ErrorKind::NotFound {
                                        tracing::debug!("d2 CLI not found — skipping render for {}", path.display());
                                    } else {
                                        tracing::warn!("D2 render error: {e}");
                                    }
                                }
                                Err(e) => tracing::warn!("D2 task join error: {e}"),
                            }
                        }
                        _ => continue,
                    }

                    // Re-index the wrapper .md if it exists
                    let md_path = path.with_extension("md");
                    if md_path.exists() {
                        let _ = vault.index_file(&md_path);
                    }
                }
            }
        });
    }

    let mut launcher_profile = use_signal(OmegonRuntimeContext::load_launcher_profile);

    // Clone dialog state
    let mut clone_dialog_open = use_signal(|| false);
    let mut clone_url: Signal<String> = use_signal(|| "git@github.com:".to_string());
    let mut clone_branch: Signal<String> = use_signal(|| "main".to_string());
    let mut clone_error: Signal<Option<String>> = use_signal(|| None);
    let mut clone_token: Signal<String> = use_signal(String::new);
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
        // Excalidraw — loaded eagerly so it's available when drawings open
        document::Script {
            src: asset!("/assets/vendor/excalidraw.bundle.js"),
        }
        document::Script {
            src: asset!("/assets/vendor/codex-excalidraw-headless.js"),
        }
        document::Stylesheet { href: asset!("/assets/vendor/excalidraw.css") }
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
                // ⌘P — command palette (command mode)
                if (e.modifiers().meta() || e.modifiers().ctrl()) && e.key() == Key::Character("p".to_string()) {
                    e.prevent_default();
                    *palette_mode.write() = crate::components::command_palette::PaletteMode::Command;
                    let v = *palette_open.read();
                    *palette_open.write() = !v;
                }
                // ⌘K — command palette (agent delegation mode, only if agent connected)
                if (e.modifiers().meta() || e.modifiers().ctrl()) && e.key() == Key::Character("k".to_string()) {
                    e.prevent_default();
                    if shared_acp_session.read().is_some() {
                        *palette_mode.write() = crate::components::command_palette::PaletteMode::Agent;
                        *palette_open.write() = true;
                    }
                    // If no agent, Cmd+K is silently ignored — no confusing UI
                }
            },

            CommandPalette { open: palette_open, mode: palette_mode }

            Toolbar {
                sync_status,
                show_agent,
                search_query,
                active_route,
            }
            div { class: "codex-body",
                Sidebar { active_route: active_route }
                div { class: "main-content",
                    // Tab bar rendered inside NotesView to avoid race with is_drawing signal
                    match *active_route.read() {
                        Route::Welcome => {
                            let mut start_ctx = ctx.clone();
                            let mut choose_ctx = ctx.clone();
                            let import_ctx = ctx.clone();

                            // "Get started" / "Open your notebook"
                            let on_get_started = move |_| {
                                *welcome_error.write() = None;

                                // If we already have a vault, switch to it
                                let existing = launcher_profile().last_vault_root.clone();
                                if let Some(ref root) = existing {
                                    if root.exists() {
                                        start_ctx.set_runtime(runtime_state_for_vault_root(root.clone()));
                                        *active_route.write() = Route::Notes;
                                        return;
                                    }
                                }

                                let vault_root = dirs::document_dir()
                                    .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
                                    .join("Codex");
                                match OmegonRuntimeContext::initialize_vault(
                                    &vault_root,
                                    "Codex",
                                    codex_core::models::SyncConfig::None,
                                ) {
                                    Ok(vault) => {
                                        // Create a welcome note
                                        let welcome_path = std::path::PathBuf::from("Welcome.md");
                                        let welcome_content = include_str!("../assets/welcome-note.md");
                                        let _ = vault.save_document_content(&welcome_path, welcome_content);
                                        let _ = vault.reindex();

                                        let mut profile = launcher_profile();
                                        profile.last_vault_root = Some(vault_root.clone());
                                        profile.wizard_completed = true;
                                        if !profile.recent_vaults.contains(&vault_root) {
                                            profile.recent_vaults.push(vault_root.clone());
                                        }
                                        let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                        launcher_profile.set(profile);
                                        start_ctx.set_runtime(runtime_state_for_vault_root(vault_root));

                                        // Open the welcome note
                                        let vault = start_ctx.vault();
                                        if let Ok(Some(doc)) = vault.store.find_document_by_slug("welcome") {
                                            tab_state.write().open(doc.id, "Welcome".into());
                                        }
                                        *active_route.write() = Route::Notes;
                                    }
                                    Err(e) => {
                                        *welcome_error.write() = Some(format!("Could not create notebook: {e}"));
                                    }
                                }
                            };

                            let on_choose_existing = move |_| {
                                *welcome_error.write() = None;
                                let Some(selected_root) = FileDialog::new().pick_folder() else {
                                    return;
                                };
                                if !selected_root.is_dir() {
                                    *welcome_error.write() = Some("Please select a folder, not a file.".into());
                                    return;
                                }
                                // Existing folders: don't modify source files (no frontmatter injection)
                                if let Err(e) = OmegonRuntimeContext::initialize_vault_with_indexing(
                                    &selected_root,
                                    selected_root
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("Codex"),
                                    codex_core::models::SyncConfig::None,
                                    codex_core::models::IndexingConfig { write_frontmatter: false },
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
                            let on_clone_remote = move |_| {
                                *clone_dialog_open.write() = true;
                                *clone_error.write() = None;
                            };
                            let cloud_ctx = ctx.clone();
                            let on_cloud_vault = move |root: PathBuf| {
                                *welcome_error.write() = None;
                                let name = root.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("Codex")
                                    .to_string();
                                // Initialize the vault at the cloud location
                                match OmegonRuntimeContext::initialize_vault(&root, &name, codex_core::models::SyncConfig::None) {
                                    Ok(vault) => {
                                        let welcome_path = std::path::PathBuf::from("Welcome.md");
                                        let welcome_content = include_str!("../assets/welcome-note.md");
                                        let _ = vault.save_document_content(&welcome_path, welcome_content);
                                        let _ = vault.reindex();

                                        let mut profile = launcher_profile();
                                        profile.last_vault_root = Some(root.clone());
                                        profile.wizard_completed = true;
                                        OmegonRuntimeContext::register_known_vault(&mut profile, &root, &name);
                                        let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                        launcher_profile.set(profile);
                                        let mut c = cloud_ctx.clone();
                                        c.set_runtime(runtime_state_for_vault_root(root));
                                        *active_route.write() = Route::Notes;
                                    }
                                    Err(e) => {
                                        *welcome_error.write() = Some(format!("Could not create vault: {e}"));
                                    }
                                }
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
                                    on_get_started,
                                    on_choose_existing,
                                    on_clone_remote,
                                    on_import_markdown,
                                    on_cloud_vault,
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
                            h2 { "Connect a notebook" }
                            p { class: "modal-hint", "Enter the URL for your notebook from GitHub, Codeberg, Forgejo, or any git hosting service." }

                            div { class: "modal-field",
                                label { "Notebook URL" }
                                input {
                                    r#type: "text",
                                    value: "{clone_url}",
                                    placeholder: "https://codeberg.org/you/notebook.git",
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

                            div { class: "modal-field",
                                label { "Access token (optional)" }
                                input {
                                    r#type: "password",
                                    value: "{clone_token}",
                                    placeholder: "Only needed for private notebooks",
                                    oninput: move |e| *clone_token.write() = e.value(),
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

                                            if dest.exists() && dest.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false) {
                                                *clone_error.write() = Some(format!("Folder already exists: {}", dest.display()));
                                                return;
                                            }

                                            *clone_busy.write() = true;
                                            *clone_error.write() = None;

                                            let token = clone_token.read().trim().to_string();
                                            spawn(async move {
                                                let clone_result = tokio::task::spawn_blocking(move || {
                                                    if token.is_empty() {
                                                        OmegonRuntimeContext::clone_remote_vault(&dest, &url, &branch)
                                                    } else {
                                                        codex_store::sync::GitSync::clone_repo_with_token(&url, &branch, &dest, &token)
                                                            .and_then(|_| codex_store::vault::Vault::open(&dest).map_err(Into::into))
                                                    }.map(|_| (dest, url, branch))
                                                }).await;

                                                match clone_result {
                                                    Ok(Ok((dest, url, branch))) => {
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
                                                    Ok(Err(e)) => {
                                                        *clone_error.write() = Some(format!("{e:#}"));
                                                    }
                                                    Err(e) => {
                                                        *clone_error.write() = Some(format!("Clone failed: {e}"));
                                                    }
                                                }
                                                *clone_busy.write() = false;
                                            });
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
