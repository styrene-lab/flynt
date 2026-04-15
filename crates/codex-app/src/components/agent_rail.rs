use dioxus::prelude::*;

#[component]
pub fn AgentRail() -> Element {
    let mut input = use_signal(|| String::new());

    rsx! {
        div { class: "agent-rail",
            div { class: "agent-rail-header", "Omegon" }
            div { class: "agent-messages",
                span { class: "placeholder", "Start a conversation…" }
            }
            div { class: "agent-input",
                textarea {
                    placeholder: "Ask Omegon anything about your vault…",
                    value: "{input}",
                    oninput: move |e| *input.write() = e.value(),
                }
                button { class: "agent-send", "Send" }
            }
        }
    }
}
