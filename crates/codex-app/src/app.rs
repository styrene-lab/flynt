use codex_core::models::DocumentId;
use dioxus::prelude::*;
use crate::{
    bootstrap::bootstrap_from_env,
    components::{AgentRail, Sidebar, Toolbar},
    state::{Route, SyncStatus},
    views::{GraphView, KanbanView, NotesView, SettingsView},
};

/// Name of the active theme — provided via context so any component can read
/// (or eventually swap) the theme without prop drilling.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeName(pub String);

#[component]
pub fn App() -> Element {
    use_context_provider(bootstrap_from_env);

    // Theme name drives the data-theme attribute on the root div.
    // Any component can call use_context::<Signal<ThemeName>>() to swap it.
    let theme = use_context_provider(|| Signal::new(ThemeName("alpharius".into())));

    let active_route = use_signal(|| Route::Notes);
    let mut show_agent = use_signal(|| false);
    let sync_status = use_signal(|| SyncStatus::Idle);
    let selected_doc: Signal<Option<DocumentId>> = use_signal(|| None);

    rsx! {
        document::Stylesheet { href: asset!("/assets/app.css") }

        div {
            class: "codex-shell",
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
