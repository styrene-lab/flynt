use codex_core::models::DocumentId;
use dioxus::prelude::*;
use crate::{
    bootstrap::bootstrap_from_env,
    components::{AgentRail, Sidebar, Toolbar},
    state::{Route, SyncStatus},
    views::{GraphView, KanbanView, NotesView, SettingsView},
};

#[component]
pub fn App() -> Element {
    // Bootstrap once: opens vault, reindexes, starts file watcher.
    // use_context_provider only runs on first render.
    use_context_provider(bootstrap_from_env);

    let active_route = use_signal(|| Route::Notes);
    let mut show_agent = use_signal(|| false);
    let sync_status = use_signal(|| SyncStatus::Idle);
    // Currently-open document — shared between sidebar (selection) and notes view (render).
    let selected_doc: Signal<Option<DocumentId>> = use_signal(|| None);

    rsx! {
        div { class: "codex-shell",
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
