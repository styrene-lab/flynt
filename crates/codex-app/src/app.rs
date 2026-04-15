use codex_core::models::DocumentId;
use dioxus::prelude::*;
use crate::{
    bootstrap::bootstrap_from_env,
    components::{AgentRail, Sidebar, Toolbar},
    state::{Route, SyncStatus},
    views::{GraphView, KanbanView, NotesView, SettingsView},
};

/// Active theme name — context-provided so any component can read or swap it.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeName(pub String);

#[component]
pub fn App() -> Element {
    // Bootstrap vault; use_context_provider returns the value for use here too.
    let ctx = use_context_provider(bootstrap_from_env);

    // Seed appearance signals from persisted config so they survive restarts.
    let theme = use_context_provider(|| {
        Signal::new(ThemeName(ctx.vault.config.appearance.theme.clone()))
    });
    let font_size = use_context_provider(|| {
        Signal::new(ctx.vault.config.appearance.font_size)
    });

    let active_route = use_signal(|| Route::Notes);
    let show_agent = use_signal(|| false);
    let sync_status = use_signal(|| SyncStatus::Idle);
    let selected_doc: Signal<Option<DocumentId>> = use_signal(|| None);

    rsx! {
        // Fonts — Inter for UI/prose, Fira Code for mono
        document::Link {
            rel: "preconnect",
            href: "https://fonts.googleapis.com",
        }
        document::Link {
            rel: "preconnect",
            href: "https://fonts.gstatic.com",
            crossorigin: "anonymous",
        }
        document::Link {
            rel: "stylesheet",
            href: "https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=Fira+Code:wght@400;500&display=swap",
        }
        document::Stylesheet { href: asset!("/assets/themes/alpharius.css") }
        document::Stylesheet { href: asset!("/assets/styles/reset.css") }
        document::Stylesheet { href: asset!("/assets/styles/layout.css") }
        document::Stylesheet { href: asset!("/assets/styles/components.css") }
        document::Stylesheet { href: asset!("/assets/styles/markdown.css") }
        document::Stylesheet { href: asset!("/assets/styles/settings.css") }
        document::Stylesheet { href: asset!("/assets/styles/kanban.css") }

        div {
            class: "codex-shell {font_size.read().css_class()}",
            "data-theme": "{theme.read().0}",

            Toolbar {
                sync_status: sync_status,
                show_agent,
                selected_doc,
                active_route,
            }
            div { class: "codex-body",
                Sidebar { active_route, selected_doc }
                div { class: "main-content",
                    match *active_route.read() {
                        Route::Notes    => rsx! { NotesView { selected_doc } },
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
