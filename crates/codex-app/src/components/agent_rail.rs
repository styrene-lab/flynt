use dioxus::prelude::*;

#[component]
pub fn AgentRail() -> Element {
    let mut input = use_signal(|| String::new());
    let mut profile = use_signal(|| String::from("codex"));
    let mut extension = use_signal(|| String::from("vox"));

    rsx! {
        div { class: "agent-rail",
            div { class: "agent-rail-header", "Omegon" }

            div { class: "agent-messages",
                div { class: "placeholder",
                    strong { "Native integration" }
                    p { "Codex will host Omegon-native extensions here. MCP is not part of this path." }
                }
            }

            div { class: "agent-input",
                label {
                    class: "settings-field",
                    span { "Profile" }
                    input {
                        value: "{profile}",
                        placeholder: "omegon profile",
                        oninput: move |e| *profile.write() = e.value(),
                    }
                }

                label {
                    class: "settings-field",
                    span { "Extension" }
                    input {
                        value: "{extension}",
                        placeholder: "vox",
                        oninput: move |e| *extension.write() = e.value(),
                    }
                }

                textarea {
                    placeholder: "Send a prompt through the active Omegon-native extension…",
                    value: "{input}",
                    oninput: move |e| *input.write() = e.value(),
                }

                button {
                    class: "agent-send",
                    disabled: true,
                    title: "Execution wiring not implemented yet",
                    "Connect native extension"
                }
            }
        }
    }
}
