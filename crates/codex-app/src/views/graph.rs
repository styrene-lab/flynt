use dioxus::prelude::*;

#[component]
pub fn GraphView() -> Element {
    rsx! {
        div { class: "view-graph",
            h2 { class: "view-heading", "Graph" }
            p { class: "placeholder", "Coming soon." }
        }
    }
}
