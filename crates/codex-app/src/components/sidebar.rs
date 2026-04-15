use dioxus::prelude::*;

#[component]
pub fn Sidebar() -> Element {
    rsx! { nav { class: "sidebar", "Sidebar placeholder" } }
}
