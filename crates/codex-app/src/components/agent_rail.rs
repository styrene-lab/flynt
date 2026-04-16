use dioxus::prelude::*;
use crate::bootstrap::AppContext;

#[component]
pub fn AgentRail() -> Element {
    let ctx = use_context::<AppContext>();
    let mut input = use_signal(|| String::new());
    let mut profile = use_signal(|| String::from("codex"));
    let mut extension = use_signal(|| String::from("vox"));

    let project_profile_exists = ctx.omegon.project_profile_path.exists();
    let global_profile_exists = ctx.omegon.global_profile_path.exists();
    let vox_installed = ctx.omegon.vox_manifest_path.exists();

    rsx! {
        div { class: "agent-rail",
            div { class: "agent-rail-header", "Omegon" }

            div { class: "agent-messages",
                div { class: "placeholder",
                    strong { "Native integration" }
                    p { "Codex will use Omegon's real native extension runtime under ~/.omegon/extensions. MCP is not part of this path." }
                    ul {
                        li { "Home: {ctx.omegon.home_dir.display()}" }
                        li {
                            "Project profile: {ctx.omegon.project_profile_path.display()}"
                            if project_profile_exists { " ✓" } else { " (missing)" }
                        }
                        li {
                            "Global profile: {ctx.omegon.global_profile_path.display()}"
                            if global_profile_exists { " ✓" } else { " (missing)" }
                        }
                        li {
                            "Vox manifest: {ctx.omegon.vox_manifest_path.display()}"
                            if vox_installed { " ✓" } else { " (missing)" }
                        }
                    }
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
                    title: if vox_installed { "Runtime discovered; RPC wiring not implemented yet" } else { "Install vox into ~/.omegon/extensions/vox first" },
                    if vox_installed { "Runtime discovered" } else { "Vox not installed" }
                }
            }
        }
    }
}
