use dioxus::prelude::*;

#[component]
pub fn NotesView() -> Element {
    rsx! {
        div { class: "view-notes",
            h2 { class: "view-heading", "Notes" }
            p { class: "placeholder", "Coming soon." }
        }
    }
}
