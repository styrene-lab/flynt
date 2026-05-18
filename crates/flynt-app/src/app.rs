use crate::{
    bootstrap::{
        AppContext, OmegonRuntimeContext, PendingProjectSetup, bootstrap_from_env,
        runtime_state_for_project_root,
    },
    components::{AgentRail, CommandPalette, Sidebar, Toolbar, initial_note_id_for_project},
    state::{
        Route, SettingsOpen, SettingsPage, SyncActivityState, SyncRunOutcome, SyncStatus, TabState,
        ThemeName,
    },
    views::{GraphView, KanbanView, NotesView, SearchView, SettingsView, WelcomeView},
};
use dioxus::prelude::*;
use flynt_core::store::ProjectStore;
use rfd::FileDialog;
use std::path::PathBuf;

#[component]
pub fn App() -> Element {
    // bootstrap_from_env opens the project on disk + spawns a watcher +
    // initializes daemons + push pipeline. It must run ONCE per app
    // lifetime, not per render. The previous shape called it
    // unconditionally outside use_signal — so every App re-render (and
    // there are many: every set_runtime, every signal write that
    // cascades) re-opened /Documents/Flynt (the env-var default),
    // discarded the result, and leaked the spawned watcher / pipeline.
    // Symptoms: stray "Project opened at /Documents/Flynt" log lines
    // appearing seconds after a project switch.
    //
    // Moving the call inside the use_signal init closure binds it to
    // the component's mount lifecycle — runs once, persists across
    // re-renders.
    let runtime = use_signal(bootstrap_from_env);
    let ctx = AppContext { runtime };
    use_context_provider(|| ctx.clone());

    let current_runtime = ctx.runtime.read().clone();

    let operator_settings = current_runtime.omegon.load_operator_settings();
    let initial_theme = if operator_settings.ui_theme.active_theme.trim().is_empty() {
        current_runtime.project.config.appearance.theme.clone()
    } else {
        operator_settings.ui_theme.active_theme.clone()
    };
    let theme = use_context_provider(|| Signal::new(ThemeName(initial_theme)));
    let font_size =
        use_context_provider(|| Signal::new(current_runtime.project.config.appearance.font_size));
    use_context_provider(|| Signal::new(current_runtime.omegon.load_project_profile()));
    use_context_provider(|| Signal::new(operator_settings.clone()));
    use_context_provider(|| {
        Signal::new(crate::theme::ThemeLibrary::from_operator(
            &operator_settings,
        ))
    });
    use_context_provider(|| Signal::new(None::<tokio::process::Child>));
    use_context_provider(|| Signal::new(None::<u32>));
    use_context_provider(|| Signal::new(None::<String>));

    // Shared ACP session — populated by AgentRail, used by CommandPalette agent mode
    use_context_provider(|| Signal::new(None::<std::rc::Rc<crate::acp::AcpSession>>));

    // Shared Omegon setup refresh trigger. Bumped after installer actions,
    // binary override changes, or manual rechecks so setup surfaces
    // re-evaluate runtime readiness in place.
    use_context_provider(|| crate::omegon_setup::OmegonSetupRefresh(Signal::new(0)));

    // Shared ACP config options (model, thinking, posture) — populated by AgentRail,
    // consumed by OmegonSettingsSection for dropdown options
    use_context_provider(|| Signal::new(Vec::<crate::acp::ConfigOption>::new()));

    // Tab state — provided via context so sidebar, tab bar, and notes share it
    use_context_provider(|| Signal::new(TabState::default()));

    // Drawing mode flag — set by NotesView when showing ExcalidrawView
    let _is_drawing = use_context_provider(|| Signal::new(false));

    // Rename trigger — sidebar bumps this, NotesView watches and opens inline rename
    use_context_provider(|| Signal::new(crate::state::RenameTrigger(0)));

    // Note context inspector command bus — command palette bumps this,
    // NotesView applies the requested tab/toggle behavior if mounted.
    use_context_provider(|| Signal::new(crate::state::NoteInspectorCommand::default()));

    // Note history/recovery command bus — command palette and snapshot actions
    // use this to open the active note recovery modal.
    use_context_provider(|| Signal::new(crate::state::NoteHistoryCommand::default()));

    // Publication preview/export command bus — command palette can trigger
    // the notes workflow without knowing NotesView internals.
    use_context_provider(|| Signal::new(crate::state::PublicationPreviewCommand::default()));

    // Settings tab — which panel is shown in SettingsView
    use_context_provider(|| Signal::new(SettingsPage::default()));

    // Settings modal open state — global signal so any component can
    // toggle it (menu, command palette, sidebar gear button, agent rail).
    use_context_provider(|| Signal::new(SettingsOpen(false)));

    // Route — provided via context so search view can navigate back
    let mut active_route = use_context_provider(|| {
        let launcher_profile = OmegonRuntimeContext::load_launcher_profile();
        let route =
            if launcher_profile.wizard_completed || launcher_profile.last_project_root.is_some() {
                Route::Notes
            } else {
                Route::Welcome
            };
        Signal::new(route)
    });
    let mut tab_state = use_context::<Signal<TabState>>();
    let show_agent = use_signal(|| false);
    let mut sync_status = use_signal(|| SyncStatus::Idle);
    let mut sync_activity = use_context_provider(|| Signal::new(SyncActivityState::default()));

    // Poll sync status from the auto-sync watcher
    {
        let runtime_for_sync = ctx.runtime.clone();
        use_future(move || async move {
            let mut run_active = false;
            loop {
                let rx_opt = runtime_for_sync.read().sync_status_rx.clone();
                if let Some(rx) = rx_opt {
                    let status = rx.borrow().clone();
                    let ui_status = match &status {
                        flynt_store::sync::AutoSyncStatus::Idle => SyncStatus::Idle,
                        flynt_store::sync::AutoSyncStatus::Committing
                        | flynt_store::sync::AutoSyncStatus::Pulling
                        | flynt_store::sync::AutoSyncStatus::Pushing => SyncStatus::Syncing,
                        flynt_store::sync::AutoSyncStatus::Conflict(files) => {
                            SyncStatus::Conflict(files.len())
                        }
                        flynt_store::sync::AutoSyncStatus::Error(_) => SyncStatus::Syncing, // transient
                    };
                    let now = chrono::Utc::now();
                    match status {
                        flynt_store::sync::AutoSyncStatus::Idle => {
                            if run_active {
                                run_active = false;
                                let mut activity = sync_activity.write();
                                activity.current_phase = None;
                                activity.last_finished_at = Some(now);
                                activity.last_outcome = Some(SyncRunOutcome::Success);
                                activity.successful_runs =
                                    activity.successful_runs.saturating_add(1);
                            }
                        }
                        flynt_store::sync::AutoSyncStatus::Committing
                        | flynt_store::sync::AutoSyncStatus::Pulling
                        | flynt_store::sync::AutoSyncStatus::Pushing => {
                            if !run_active {
                                run_active = true;
                                let mut activity = sync_activity.write();
                                activity.last_started_at = Some(now);
                                activity.last_finished_at = None;
                                activity.last_outcome = None;
                            }
                            sync_activity.write().current_phase = Some(
                                match status {
                                    flynt_store::sync::AutoSyncStatus::Committing => "Committing",
                                    flynt_store::sync::AutoSyncStatus::Pulling => "Pulling",
                                    flynt_store::sync::AutoSyncStatus::Pushing => "Pushing",
                                    _ => unreachable!(),
                                }
                                .into(),
                            );
                        }
                        flynt_store::sync::AutoSyncStatus::Conflict(files) => {
                            run_active = false;
                            let mut activity = sync_activity.write();
                            activity.current_phase = None;
                            activity.last_finished_at = Some(now);
                            activity.last_outcome = Some(SyncRunOutcome::Conflict(files));
                            activity.failed_runs = activity.failed_runs.saturating_add(1);
                        }
                        flynt_store::sync::AutoSyncStatus::Error(error) => {
                            run_active = false;
                            let mut activity = sync_activity.write();
                            activity.current_phase = None;
                            activity.last_finished_at = Some(now);
                            activity.last_outcome = Some(SyncRunOutcome::Error(error));
                            activity.failed_runs = activity.failed_runs.saturating_add(1);
                        }
                    }
                    *sync_status.write() = ui_status;
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
    }
    let mut palette_open = use_signal(|| false);
    let mut palette_mode = use_signal(|| crate::components::command_palette::PaletteMode::Command);
    let shared_acp_session = use_context::<Signal<Option<std::rc::Rc<crate::acp::AcpSession>>>>();

    // Mirror tab + view state to <project>/.flynt-local/flynt/ui-state.json so the
    // embedded omegon agent can answer "what document am I looking at?" via
    // its get_ui_state tool. Re-fires whenever Dioxus detects tab_state /
    // active_route changes.
    {
        let ui_ctx = ctx.clone();
        use_effect(move || {
            let tabs = tab_state.read().clone();
            let route = active_route.read().clone();
            let project = ui_ctx.project();
            crate::ui_state::write_snapshot(&project, &tabs, &route);
        });
    }

    // Bootstrap canvas assets (tweakcn presets, shadcn primitives) into the
    // project's .flynt-local directory so flynt-agent can read them via the
    // canvas_* tool family. Idempotent and content-aware; safe to re-run on
    // every launch.
    {
        let assets_ctx = ctx.clone();
        use_effect(move || {
            let project = assets_ctx.project();
            crate::canvas_assets::bootstrap(&project.root);
        });
    }

    // Shared search query — lives here so toolbar and search view share it
    let search_query: Signal<String> = use_signal(String::new);

    // ── Native menu event handler ────────────────────────────────────────
    let ctx_menu_handler = ctx.clone();
    let mut show_agent_menu = show_agent;
    let mut settings_open_menu = use_context::<Signal<SettingsOpen>>();
    dioxus::desktop::use_muda_event_handler(move |event| {
        match event.id().0.as_str() {
            crate::menu::VIEW_NOTES => *active_route.write() = Route::Notes,
            crate::menu::VIEW_BOARD => *active_route.write() = Route::Kanban,
            crate::menu::VIEW_GRAPH => *active_route.write() = Route::Graph,
            crate::menu::VIEW_SETTINGS => *settings_open_menu.write() = SettingsOpen(true),
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
                    let project = c.project();
                    let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
                    let title = format!("Untitled {ts_suffix}");
                    let filename = format!("{title}.md");
                    let path = std::path::PathBuf::from(&filename);
                    let content = format!("+++\ntitle = \"{title}\"\ntags = []\n+++\n\n");
                    if project.save_document_content(&path, &content).is_ok() {
                        let _ = project.reindex();
                        if let Ok(Some(doc)) =
                            project.store.find_document_by_slug(&title.to_lowercase())
                        {
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
                    let project = c.project();
                    let date = flynt_core::daily::today();
                    let path = flynt_core::daily::daily_note_path(date);
                    let abs = project.root.join(&path);
                    if !abs.exists() {
                        // Load daily template if it exists
                        let templates = flynt_core::templates::list_templates(&project.root);
                        let tmpl = templates.iter().find(|t| t.name.to_lowercase() == "daily");
                        let content = flynt_core::daily::daily_note_content(
                            date,
                            tmpl.map(|t| t.content.as_str()),
                        );
                        if let Some(parent) = abs.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let _ = project.save_document_content(&path, &content);
                        let _ = project.reindex();
                    }
                    let title = date.format("%A, %B %-d, %Y").to_string();
                    if let Ok(Some(doc)) = project
                        .store
                        .find_document_by_slug(&date.format("%Y-%m-%d").to_string())
                    {
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
                    let project = c.project();
                    let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
                    let name = format!("Drawing {ts_suffix}");
                    if let Ok(_md_path) =
                        crate::views::excalidraw::create_drawing(&project.root, &name)
                    {
                        let _ = project.reindex();
                        let slug = name.to_lowercase();
                        if let Ok(Some(doc)) = project.store.find_document_by_slug(&slug) {
                            ts.write().open(doc.id, name);
                        }
                        *ar.write() = Route::Notes;
                    }
                });
            }
            crate::menu::NEW_CANVAS => {
                let c = ctx_menu_handler;
                let mut ts = tab_state;
                let mut ar = active_route;
                spawn(async move {
                    let project = c.project();
                    let ts_suffix = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
                    let name = format!("Canvas {ts_suffix}");
                    if let Ok(md_path) = crate::views::canvas::create_canvas(&project.root, &name) {
                        let _ = project.index_file(&project.root.join(&md_path));
                        let _ = c.project_events().send(
                            flynt_store::watcher::ProjectChangeEvent::FileCreated(
                                project.root.join(&md_path),
                            ),
                        );
                        let slug = name.to_lowercase();
                        if let Ok(Some(doc)) = project.store.find_document_by_slug(&slug) {
                            ts.write().open(doc.id, name);
                        }
                        *ar.write() = Route::Notes;
                    }
                });
            }
            crate::menu::OPEN_PROJECT => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    let _ = OmegonRuntimeContext::spawn_new_instance_for_project(&path);
                }
            }
            crate::menu::DELETE_NOTE => {
                let c = ctx_menu_handler;
                let mut ts = tab_state;
                spawn(async move {
                    let active_id = ts.read().active_id().cloned();
                    if let Some(doc_id) = active_id {
                        let project = c.project();
                        if let Ok(Some(doc)) = project.store.get_document(&doc_id) {
                            let abs = project.root.join(&doc.path);
                            if abs.exists() {
                                let _ = std::fs::remove_file(&abs);
                            }
                            let _ = project.store.delete_document(&doc_id);
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
        let project_events = ctx.project_events();
        let project_for_svg = ctx.project();
        use_future(move || {
            let mut rx = project_events.subscribe();
            let project = project_for_svg.clone();
            async move {
                loop {
                    let Ok(evt) = rx.recv().await else { break };
                    // Re-read viz config on each event so settings changes take effect immediately
                    let viz = project.config.visualization.clone();
                    let path = match evt {
                        flynt_store::watcher::ProjectChangeEvent::FileCreated(p)
                        | flynt_store::watcher::ProjectChangeEvent::FileModified(p) => p,
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
                            let mounted_key = path
                                .strip_prefix(&project.root)
                                .unwrap_or(&path)
                                .to_string_lossy()
                                .replace('\\', "/");
                            let mounted_key =
                                serde_json::to_string(&mounted_key).unwrap_or_default();
                            let js = format!(
                                r#"(async function() {{
                                    const changedDrawing = {mounted_key};
                                    if (window._excalidrawMountedKey) {{
                                        if (window._excalidrawMountedKey === changedDrawing &&
                                            window.FlyntExcalidraw &&
                                            window.FlyntExcalidraw._api &&
                                            window.FlyntExcalidraw.exportSvg) {{
                                            let svg = await window.FlyntExcalidraw.exportSvg();
                                            dioxus.send(svg || '');
                                            return;
                                        }}
                                        dioxus.send('');
                                        return;
                                    }}
                                    if (window.FlyntExcalidraw && window.FlyntExcalidraw.renderSceneToSvg) {{
                                        let svg = await window.FlyntExcalidraw.renderSceneToSvg({escaped});
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
                                let deadline =
                                    std::time::Instant::now() + std::time::Duration::from_secs(30);
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
                                        Ok(None) => std::thread::sleep(
                                            std::time::Duration::from_millis(100),
                                        ),
                                        Err(e) => return Err(e),
                                    }
                                }
                                child.wait_with_output()
                            })
                            .await;
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
                                        tracing::debug!(
                                            "d2 CLI not found — skipping render for {}",
                                            path.display()
                                        );
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
                        let _ = project.index_file(&md_path);
                    }
                }
            }
        });
    }

    let mut launcher_profile = use_signal(OmegonRuntimeContext::load_launcher_profile);

    // Clone dialog state
    let mut clone_dialog_open = use_signal(|| false);
    let mut clone_url: Signal<String> = use_signal(String::new);
    let mut clone_branch: Signal<String> = use_signal(|| "main".to_string());
    let mut clone_error: Signal<Option<String>> = use_signal(|| None);
    let mut clone_token: Signal<String> = use_signal(String::new);
    let mut clone_busy = use_signal(|| false);

    // Welcome screen error banner
    let mut welcome_error: Signal<Option<String>> = use_signal(|| None);

    let ctx_for_switch = ctx.clone();
    let _switch_runtime = move |selected_root: PathBuf| {
        let mut ctx = ctx_for_switch.clone();
        ctx.set_runtime(runtime_state_for_project_root(selected_root));
    };

    let shell_theme_style = {
        let library = use_context::<Signal<crate::theme::ThemeLibrary>>();
        library.read().active_vars(&theme.read().0)
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
            src: asset!("/assets/vendor/flynt-excalidraw-headless.js"),
        }
        document::Stylesheet { href: asset!("/assets/vendor/excalidraw.css") }
        // react-flow — loaded eagerly so .flow files mount instantly.
        // Bundle is built from crates/flynt-app/build/flow/ (see README).
        document::Script {
            src: asset!("/assets/vendor/flow.bundle.js"),
        }
        document::Stylesheet { href: asset!("/assets/themes/alpharius.css") }
        document::Stylesheet { href: asset!("/assets/styles/reset.css") }
        document::Stylesheet { href: asset!("/assets/styles/layout.css") }
        document::Stylesheet { href: asset!("/assets/styles/components.css") }
        document::Stylesheet { href: asset!("/assets/styles/markdown.css") }
        document::Stylesheet { href: asset!("/assets/styles/settings.css") }
        document::Stylesheet { href: asset!("/assets/styles/kanban.css") }
        document::Stylesheet { href: asset!("/assets/styles/task-strip.css") }
        document::Stylesheet { href: asset!("/assets/styles/tabs.css") }
        document::Stylesheet { href: asset!("/assets/styles/search.css") }
        document::Stylesheet { href: asset!("/assets/styles/graph.css") }
        document::Stylesheet { href: asset!("/assets/styles/welcome.css") }
        document::Stylesheet { href: asset!("/assets/styles/canvas.css") }
        // Reveal body after stylesheets are loaded
        document::Script { "document.body.classList.add('ready');" }

        // Global help-hint tooltip positioner. The tooltip itself is
        // `position: fixed` so it can escape overflow:hidden ancestors
        // (the settings modal in particular). This delegated listener
        // sets the tooltip's top/left to sit above the hovered icon,
        // clamping to viewport bounds so it never overflows.
        document::Script {
            r#"
            (function() {{
                if (window._flyntHelpHintWired) return;
                window._flyntHelpHintWired = true;
                const TIP_W = 240;
                const GAP = 6;
                document.addEventListener('mouseover', function(e) {{
                    const hint = e.target && e.target.closest && e.target.closest('.help-hint');
                    if (!hint) return;
                    const tip = hint.querySelector('.help-hint-tooltip');
                    if (!tip) return;
                    const r = hint.getBoundingClientRect();
                    const tipH = tip.offsetHeight || 60;
                    let top = r.top - tipH - GAP;
                    let left = r.left;
                    if (top < 8) top = r.bottom + GAP;
                    const maxLeft = window.innerWidth - TIP_W - 8;
                    if (left > maxLeft) left = maxLeft;
                    if (left < 8) left = 8;
                    tip.style.top = top + 'px';
                    tip.style.left = left + 'px';
                }});
            }})();
            "#
        }

        div {
            class: "flynt-shell {font_size.read().css_class()}",
            "data-theme": "{theme.read().0}",
            style: "{shell_theme_style}",
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
            div { class: "flynt-body",
                Sidebar { active_route: active_route }
                crate::components::SidebarDivider {}
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

                                // If we already have a project, switch to it
                                let existing = launcher_profile().last_project_root.clone();
                                if let Some(ref root) = existing {
                                    if root.exists() {
                                        start_ctx.set_runtime(runtime_state_for_project_root(root.clone()));
                                        *active_route.write() = Route::Notes;
                                        return;
                                    }
                                }

                                let project_root = dirs::document_dir()
                                    .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
                                    .join("Flynt");
                                match OmegonRuntimeContext::initialize_project(
                                    &project_root,
                                    "Flynt",
                                    flynt_core::models::SyncConfig::None,
                                ) {
                                    Ok(project) => {
                                        // Create a welcome note
                                        let welcome_path = std::path::PathBuf::from("Welcome.md");
                                        let welcome_content = include_str!("../assets/welcome-note.md");
                                        let _ = project.save_document_content(&welcome_path, welcome_content);
                                        let _ = project.reindex();

                                        let mut profile = launcher_profile();
                                        profile.last_project_root = Some(project_root.clone());
                                        profile.wizard_completed = true;
                                        if !profile.recent_projects.contains(&project_root) {
                                            profile.recent_projects.push(project_root.clone());
                                        }
                                        let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                        launcher_profile.set(profile);
                                        start_ctx.set_runtime(runtime_state_for_project_root(project_root));

                                        // Open the welcome note
                                        let project = start_ctx.project();
                                        if let Ok(Some(doc)) = project.store.find_document_by_slug("welcome") {
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
                                if let Err(e) = OmegonRuntimeContext::initialize_project_with_indexing(
                                    &selected_root,
                                    selected_root
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("Flynt"),
                                    flynt_core::models::SyncConfig::None,
                                    flynt_core::models::IndexingConfig { write_frontmatter: false, scopes: Vec::new() },
                                ) {
                                    *welcome_error.write() = Some(format!("Could not open project: {e}"));
                                    return;
                                }
                                let mut profile = launcher_profile();
                                profile.pending_setup = Some(PendingProjectSetup::OpenExisting {
                                    path: selected_root.clone(),
                                });
                                profile.last_project_root = Some(selected_root.clone());
                                profile.wizard_completed = true;
                                if !profile.recent_projects.contains(&selected_root) {
                                    profile.recent_projects.push(selected_root.clone());
                                }
                                let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                launcher_profile.set(profile);
                                choose_ctx.set_runtime(runtime_state_for_project_root(selected_root.clone()));
                                if let Some(note_id) = initial_note_id_for_project(&selected_root) {
                                    if let Ok(parsed) = uuid::Uuid::parse_str(&note_id) {
                                        tab_state.write().open(
                                            flynt_core::models::DocumentId(parsed),
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
                            let on_cloud_project = move |root: PathBuf| {
                                *welcome_error.write() = None;
                                let name = root.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("Flynt")
                                    .to_string();
                                // Initialize the project at the cloud location
                                match OmegonRuntimeContext::initialize_project(&root, &name, flynt_core::models::SyncConfig::None) {
                                    Ok(project) => {
                                        let welcome_path = std::path::PathBuf::from("Welcome.md");
                                        let welcome_content = include_str!("../assets/welcome-note.md");
                                        let _ = project.save_document_content(&welcome_path, welcome_content);
                                        let _ = project.reindex();

                                        let mut profile = launcher_profile();
                                        profile.last_project_root = Some(root.clone());
                                        profile.wizard_completed = true;
                                        OmegonRuntimeContext::register_known_project(&mut profile, &root, &name);
                                        let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                        launcher_profile.set(profile);
                                        let mut c = cloud_ctx.clone();
                                        c.set_runtime(runtime_state_for_project_root(root));
                                        *active_route.write() = Route::Notes;
                                    }
                                    Err(e) => {
                                        *welcome_error.write() = Some(format!("Could not create project: {e}"));
                                    }
                                }
                            };
                            let on_import_markdown = move |_| {
                                *welcome_error.write() = None;
                                let Some(source_root) = FileDialog::new().pick_folder() else {
                                    return;
                                };
                                match import_ctx.project().import_markdown_tree(&source_root) {
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
                                    on_cloud_project,
                                }
                            }
                        },
                        Route::Notes    => rsx! { NotesView {} },

                        Route::Search   => rsx! { SearchView { search_query } },
                        Route::Kanban   => rsx! { KanbanView {} },
                        Route::Graph    => rsx! { GraphView {} },
                    }
                }
                // Settings modal — overlays the active route so operators
                // can glance at a setting and return to their work without
                // losing the underlying view. Closes on Escape, backdrop
                // click, or the close button inside the modal.
                {
                    let settings_open_signal = use_context::<Signal<SettingsOpen>>();
                    let is_open = settings_open_signal.read().0;
                    let mut close_settings = settings_open_signal;
                    rsx! {
                        if is_open {
                            div {
                                class: "modal-overlay settings-modal-overlay",
                                onclick: move |_| *close_settings.write() = SettingsOpen(false),
                                onkeydown: move |e| {
                                    if e.key() == Key::Escape {
                                        *close_settings.write() = SettingsOpen(false);
                                    }
                                },
                                tabindex: 0,
                                div {
                                    class: "modal-dialog settings-modal-dialog",
                                    onclick: move |e| e.stop_propagation(),
                                    button {
                                        class: "settings-modal-close",
                                        title: "Close (Esc)",
                                        onclick: move |_| *close_settings.write() = SettingsOpen(false),
                                        "\u{00D7}"
                                    }
                                    SettingsView {}
                                }
                            }
                        }
                    }
                }
                // Clone remote project dialog
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
                                    placeholder: "https://github.com/you/your-project.git",
                                    oninput: move |e| *clone_url.write() = e.value(),
                                    onblur: move |_| {
                                        let url = clone_url.read().trim().to_string();
                                        if clone_token.read().is_empty() {
                                            if let Some(token) = flynt_core::providers::token_for_url(&url) {
                                                *clone_token.write() = token;
                                            }
                                        }
                                    },
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
                                                .unwrap_or("project")
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
                                            let persist_token = token.clone();
                                            spawn(async move {
                                                let clone_result = tokio::task::spawn_blocking(move || {
                                                    if token.is_empty() {
                                                        OmegonRuntimeContext::clone_remote_project(&dest, &url, &branch)
                                                    } else {
                                                        flynt_store::sync::GitSync::clone_repo_with_token(&url, &branch, &dest, &token)
                                                            .and_then(|_| flynt_store::project::Project::open(&dest).map_err(Into::into))
                                                    }.map(|_| (dest, url, branch))
                                                }).await;

                                                match clone_result {
                                                    Ok(Ok((dest, url, branch))) => {
                                                        // Persist the token so future sync operations use it
                                                        if !persist_token.is_empty() {
                                                            if let Some(provider_id) = flynt_core::providers::provider_for_url(&url) {
                                                                let _ = flynt_core::providers::save_api_key(provider_id, &persist_token);
                                                            }
                                                        }
                                                        let mut profile = launcher_profile();
                                                        profile.pending_setup = Some(PendingProjectSetup::LinkGithub {
                                                            local_path: dest.clone(),
                                                            repo: url,
                                                            branch,
                                                        });
                                                        profile.last_project_root = Some(dest.clone());
                                                        profile.wizard_completed = true;
                                                        if !profile.recent_projects.contains(&dest) {
                                                            profile.recent_projects.push(dest.clone());
                                                        }
                                                        let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                                        launcher_profile.set(profile);
                                                        github_ctx.set_runtime(runtime_state_for_project_root(dest));
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
                    crate::components::PanelDivider {}
                    AgentRail {}
                }
            }
        }
    }
}
