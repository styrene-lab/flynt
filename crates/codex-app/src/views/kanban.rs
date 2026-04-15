use dioxus::prelude::*;

#[component]
pub fn KanbanView() -> Element {
    rsx! {
        div { class: "view-kanban",
            h2 { class: "view-heading", "Kanban" }
            p { class: "placeholder", "Coming soon." }
        }
    }
}
