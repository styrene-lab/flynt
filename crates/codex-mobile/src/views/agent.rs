use dioxus::prelude::*;

/// Mobile agent chat — placeholder for Omegon integration.
/// On iOS, Omegon can't be spawned as a local process.
/// Future options:
/// - Connect to `omegon serve` on local network (localhost:7842)
/// - Direct Anthropic API calls with vault context
/// - Cloud relay via Omegon cloud agent
#[component]
pub fn AgentView() -> Element {
    let mut input = use_signal(String::new);
    let mut messages: Signal<Vec<(bool, String)>> = use_signal(Vec::new); // (is_user, text)

    rsx! {
        div { class: "agent-mobile",
            div { class: "agent-mobile-header",
                h2 { "Omegon" }
                span { class: "agent-mobile-status", "local network" }
            }

            div { class: "agent-mobile-messages",
                if messages.read().is_empty() {
                    div { class: "agent-mobile-empty",
                        p { "Ask Omegon about your vault." }
                        p { class: "muted", "Connects to Omegon daemon on your Mac (port 7842)." }
                    }
                } else {
                    for (idx, (is_user, text)) in messages.read().iter().enumerate() {
                        div {
                            key: "msg-{idx}",
                            class: if *is_user { "agent-m-msg user" } else { "agent-m-msg assistant" },
                            div { class: "agent-m-role", if *is_user { "You" } else { "Omegon" } }
                            div { class: "agent-m-text", "{text}" }
                        }
                    }
                }
            }

            div { class: "agent-mobile-input",
                textarea {
                    class: "agent-m-textarea",
                    placeholder: "Ask Omegon…",
                    value: "{input}",
                    oninput: move |e| *input.write() = e.value(),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter && !e.modifiers().shift() {
                            e.prevent_default();
                            let prompt = input.read().trim().to_string();
                            if prompt.is_empty() { return; }

                            messages.write().push((true, prompt.clone()));
                            *input.write() = String::new();

                            // TODO: Connect to omegon serve or direct API
                            messages.write().push((false,
                                "Omegon mobile agent is not yet connected. Run `omegon serve` on your Mac.".into()
                            ));
                        }
                    },
                }
            }
        }
    }
}
