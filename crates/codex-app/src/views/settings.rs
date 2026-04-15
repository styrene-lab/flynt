use dioxus::prelude::*;

#[component]
pub fn SettingsView() -> Element {
    rsx! {
        div { class: "view-settings",
            h2 { class: "view-heading", "Settings" }
            p { class: "placeholder", "Coming soon." }
        }
    }
}
