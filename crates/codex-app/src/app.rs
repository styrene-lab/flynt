use dioxus::prelude::*;
use crate::{
    bootstrap::bootstrap_from_env,
    components::{AgentRail, Sidebar, TabBar, Toolbar},
    state::{Route, SyncStatus, TabState, ThemeName},
    views::{GraphView, KanbanView, NotesView, SearchView, SettingsView},
};

#[component]
pub fn App() -> Element {
    use_context_provider(bootstrap_from_env);

    let ctx = use_context::<crate::bootstrap::AppContext>();

    let theme = use_context_provider(|| {
        Signal::new(ThemeName(ctx.vault.config.appearance.theme.clone()))
    });
    let font_size = use_context_provider(|| {
        Signal::new(ctx.vault.config.appearance.font_size)
    });

    // Tab state — provided via context so sidebar, tab bar, and notes share it
    use_context_provider(|| Signal::new(TabState::default()));

    // Route — provided via context so search view can navigate back
    let active_route = use_context_provider(|| Signal::new(Route::default()));
    let mut show_agent = use_signal(|| false);
    let sync_status = use_signal(|| SyncStatus::Idle);

    // Shared search query — lives here so toolbar and search view share it
    let search_query: Signal<String> = use_signal(String::new);

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
