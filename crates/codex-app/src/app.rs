use crate::{
    bootstrap::{bootstrap_from_env, OmegonRuntimeContext, PendingVaultSetup},
    components::{AgentRail, Sidebar, TabBar, Toolbar},
    state::{Route, SyncStatus, TabState, ThemeName},
    views::{GraphView, KanbanView, NotesView, SearchView, SettingsView, WelcomeView},
};
use dioxus::prelude::*;

#[component]
pub fn App() -> Element {
    use_context_provider(bootstrap_from_env);

    let ctx = use_context::<crate::bootstrap::AppContext>();

    let theme = use_context_provider(|| {
        Signal::new(ThemeName(ctx.vault.config.appearance.theme.clone()))
    });
    let font_size = use_context_provider(|| Signal::new(ctx.vault.config.appearance.font_size));
    use_context_provider(|| Signal::new(ctx.omegon.load_project_profile()));
    use_context_provider(|| Signal::new(ctx.omegon.load_operator_settings()));
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
    let show_agent = use_signal(|| false);
    let sync_status = use_signal(|| SyncStatus::Idle);

    // Shared search query — lives here so toolbar and search view share it
    let search_query: Signal<String> = use_signal(String::new);

    let mut launcher_profile = use_signal(OmegonRuntimeContext::load_launcher_profile);

    rsx! {
        document::Link {
            rel: "stylesheet",
            href: "https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/styles/base16/ocean.min.css",
        }
        document::Script {
            src: "https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/highlight.min.js",
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
                            let current_vault_root = ctx.vault.root.clone();
                            let choose_existing_root = current_vault_root.clone();
                            let imported_root = current_vault_root.join("references/imported");
                            let on_choose_existing = move |_| {
                                let mut profile = launcher_profile();
                                profile.pending_setup = Some(PendingVaultSetup::OpenExisting {
                                    path: choose_existing_root.clone(),
                                });
                                profile.last_vault_root = Some(choose_existing_root.clone());
                                profile.wizard_completed = true;
                                if !profile.recent_vaults.contains(&choose_existing_root) {
                                    profile.recent_vaults.push(choose_existing_root.clone());
                                }
                                let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                launcher_profile.set(profile);
                                *active_route.write() = Route::Notes;
                            };
                            let on_create_local = move |_| {
                                let local_path = dirs::document_dir()
                                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                                    .join("Black Meridian");
                                if let Ok(_vault) = OmegonRuntimeContext::initialize_vault(
                                    &local_path,
                                    "Black Meridian",
                                    codex_core::models::SyncConfig::None,
                                ) {
                                    let mut profile = launcher_profile();
                                    profile.pending_setup = Some(PendingVaultSetup::CreateLocal {
                                        path: local_path.clone(),
                                        name: "Black Meridian".into(),
                                    });
                                    profile.last_vault_root = Some(local_path.clone());
                                    profile.wizard_completed = true;
                                    if !profile.recent_vaults.contains(&local_path) {
                                        profile.recent_vaults.push(local_path);
                                    }
                                    let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                    launcher_profile.set(profile);
                                    *active_route.write() = Route::Notes;
                                }
                            };
                            let on_link_github = move |_| {
                                let local_path = dirs::document_dir()
                                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                                    .join("Black Meridian");
                                if let Ok(_vault) = OmegonRuntimeContext::initialize_github_linked_vault(
                                    &local_path,
                                    "Black Meridian",
                                    "https://github.com/black-meridian/codex-vault.git",
                                    "main",
                                ) {
                                    let mut profile = launcher_profile();
                                    profile.pending_setup = Some(PendingVaultSetup::LinkGithub {
                                        local_path: local_path.clone(),
                                        repo: "https://github.com/black-meridian/codex-vault.git".into(),
                                        branch: "main".into(),
                                    });
                                    profile.last_vault_root = Some(local_path.clone());
                                    profile.wizard_completed = true;
                                    if !profile.recent_vaults.contains(&local_path) {
                                        profile.recent_vaults.push(local_path);
                                    }
                                    let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                                    launcher_profile.set(profile);
                                    *active_route.write() = Route::Notes;
                                }
                            };
                            let imported_root = current_vault_root.join("references/imported");
                            let on_import_markdown = move |_| {
                                let mut profile = launcher_profile();
                                profile.pending_setup = Some(PendingVaultSetup::CreateLocal {
                                    path: imported_root.clone(),
                                    name: "Imported References".into(),
                                });
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
