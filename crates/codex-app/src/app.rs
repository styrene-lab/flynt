use dioxus::prelude::*;

#[component]
pub fn App() -> Element {
    rsx! {
        div { class: "codex-root",
            h1 { "Codex" }
            p { "Vault not yet loaded." }
        }
    }
}
