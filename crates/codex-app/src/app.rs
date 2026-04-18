use crate::{
    bootstrap::{bootstrap_from_env, runtime_state_for_vault_root, AppContext, OmegonRuntimeContext, PendingVaultSetup},
    components::{initial_note_id_for_vault, AgentRail, Sidebar, TabBar, Toolbar},
    state::{Route, SyncStatus, TabState, ThemeName},
    views::{GraphView, KanbanView, NotesView, SearchView, SettingsView, WelcomeView},
};
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

    // Shared search query — lives here so toolbar and search view share it
    let search_query: Signal<String> = use_signal(String::new);

    let mut launcher_profile = use_signal(OmegonRuntimeContext::load_launcher_profile);

    let ctx_for_switch = ctx.clone();
    let _switch_runtime = move |selected_root: PathBuf| {
        let mut ctx = ctx_for_switch.clone();
        ctx.set_runtime(runtime_state_for_vault_root(selected_root));
    };

    rsx! {
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

        div {
            class: "codex-shell {font_size.read().css_class()}",
            "data-theme": "{theme.read().0}",

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
                            let mut github_ctx = ctx.clone();
                            let import_ctx = ctx.clone();
                            let on_choose_existing = move |_| {
                                let Some(selected_root) = FileDialog::new().pick_folder() else {
                                    return;
                                };
                                if OmegonRuntimeContext::initialize_vault(
                                    &selected_root,
                                    selected_root
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("Codex"),
                                    codex_core::models::SyncConfig::None,
                                )
                                .is_err()
                                {
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
                                let Some(local_path) = FileDialog::new()
                                    .set_directory(
                                        dirs::document_dir()
                                            .unwrap_or_else(|| std::path::PathBuf::from("/tmp")),
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
                                if let Ok(_vault) = OmegonRuntimeContext::initialize_vault(
                                    &local_path,
                                    &name,
                                    codex_core::models::SyncConfig::None,
                                ) {
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
                            };
                            let on_link_github = move |_| {
                                let Some(local_path) = FileDialog::new()
                                    .set_directory(
                                        dirs::document_dir()
                                            .unwrap_or_else(|| std::path::PathBuf::from("/tmp")),
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
                                if let Ok(_vault) = OmegonRuntimeContext::initialize_github_pages_publication(
                                    &local_path,
                                    &name,
                                    "https://github.com/black-meridian/codex-site.git",
                                    "gh-pages",
                                ) {
                                    let mut profile = launcher_profile();
                                    profile.pending_setup = Some(PendingVaultSetup::LinkGithub {
                                        local_path: local_path.clone(),
                                        repo: "https://github.com/black-meridian/codex-site.git".into(),
                                        branch: "gh-pages".into(),
                                    });
                                    profile.last_vault_root = Some(local_path.clone());
                                    profile.wizard_completed = true;
                                    if !profile.recent_vaults.contains(&local_path) {
                                        profile.recent_vaults.push(local_path.clone());
                                    }
                                    let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                    launcher_profile.set(profile);
                                    github_ctx.set_runtime(runtime_state_for_vault_root(local_path.clone()));
                                    *active_route.write() = Route::Notes;
                                }
                            };
                            let on_import_markdown = move |_| {
                                let Some(source_root) = FileDialog::new().pick_folder() else {
                                    return;
                                };
                                if import_ctx.vault().import_markdown_tree(&source_root).is_err() {
                                    return;
                                }
                                let mut profile = launcher_profile();
                                profile.pending_setup = Some(PendingVaultSetup::CreateLocal {
                                    path: source_root,
                                    name: "Imported References".into(),
                                });
                                profile.wizard_completed = true;
                                let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                launcher_profile.set(profile);
                                *active_route.write() = Route::Notes;
                            };
                            let on_seed_demo_publication = move |_| {
                                let Some(repo_root) = FileDialog::new()
                                    .set_directory(
                                        dirs::document_dir()
                                            .unwrap_or_else(|| std::path::PathBuf::from("/tmp")),
                                    )
                                    .pick_folder()
                                else {
                                    return;
                                };
                                if OmegonRuntimeContext::seed_demo_publication_repo(&repo_root).is_err() {
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
                                WelcomeView {
                                    launcher_profile: launcher_profile(),
                                    on_choose_existing,
                                    on_create_local,
                                    on_link_github,
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
                if show_agent() {
                    AgentRail {}
                }
            }
        }
    }
}
