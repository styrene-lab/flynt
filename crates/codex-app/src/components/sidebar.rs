use dioxus::prelude::*;
use crate::state::Route;

#[component]
pub fn Sidebar(mut active_route: Signal<Route>) -> Element {
    rsx! {
        nav { class: "sidebar",
            div { class: "sidebar-section",
                span { class: "sidebar-heading", "Notes" }
                span { class: "sidebar-item placeholder", "No documents yet" }
            }
            div { class: "sidebar-section",
                span { class: "sidebar-heading", "Boards" }
                span { class: "sidebar-item placeholder", "No boards yet" }
            }
            div { class: "sidebar-nav",
                button {
                    class: if *active_route.read() == Route::Notes { "nav-btn active" } else { "nav-btn" },
                    onclick: move |_| *active_route.write() = Route::Notes,
                    "📝"
                }
                button {
                    class: if *active_route.read() == Route::Kanban { "nav-btn active" } else { "nav-btn" },
                    onclick: move |_| *active_route.write() = Route::Kanban,
                    "📋"
                }
                button {
                    class: if *active_route.read() == Route::Graph { "nav-btn active" } else { "nav-btn" },
                    onclick: move |_| *active_route.write() = Route::Graph,
                    "🕸"
                }
                button {
                    class: if *active_route.read() == Route::Settings { "nav-btn active" } else { "nav-btn" },
                    onclick: move |_| *active_route.write() = Route::Settings,
                    "⚙️"
                }
            }
        }
    }
}
