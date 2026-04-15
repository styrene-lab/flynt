use codex_core::models::{DocumentId, FontSizePreset};
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
    let mut show_agent = use_signal(|| false);
    let sync_status = use_signal(|| SyncStatus::Idle);
    let selected_doc: Signal<Option<DocumentId>> = use_signal(|| None);

    rsx! {
        document::Stylesheet { href: asset!("/assets/app.css") }

        div {
            class: "codex-shell {font_size.read().css_class()}",
            "data-theme": "{theme.read().0}",

            Toolbar {
                sync_status: sync_status.read().clone(),
                show_agent,
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
