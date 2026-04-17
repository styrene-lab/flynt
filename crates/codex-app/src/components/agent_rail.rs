use crate::bootstrap::AppContext;
use codex_core::models::{CodexOperatorSettings, OmegonProfile};
use dioxus::prelude::*;

#[component]
pub fn AgentRail() -> Element {
    let ctx = use_context::<AppContext>();
    let operator_settings = use_context::<Signal<CodexOperatorSettings>>().read().clone();
    let project_profile = use_context::<Signal<OmegonProfile>>().read().clone();
    let omegon_pid = *use_context::<Signal<Option<u32>>>().read();
    let omegon_launch_error = use_context::<Signal<Option<String>>>().read().clone();
    let runtime_ready = omegon_pid.is_some() && omegon_launch_error.is_none();
    let omegon = ctx.omegon();

    let mut input = use_signal(String::new);

    let active_persona = if operator_settings.active_persona.trim().is_empty() {
        "off".to_string()
    } else {
        operator_settings.active_persona.clone()
    };
    let model_summary = project_profile
        .last_used_model
        .as_ref()
        .map(|model| format!("{}/{}", model.provider, model.model_id))
        .unwrap_or_else(|| "not configured".to_string());

    let project_profile_exists = omegon.project_profile_path.exists();
    let global_profile_exists = omegon.global_profile_path.exists();
    let vox_installed = omegon.vox_manifest_path.exists();

    rsx! {
        div { class: "agent-rail",
            div { class: "agent-rail-header", "Omegon" }

            div { class: "agent-messages",
                div { class: "placeholder",
                    strong { "Native integration" }
                    p { "Codex will use Omegon's real native extension runtime under ~/.omegon/extensions. MCP is not part of this path." }
                    ul {
                        li {
                            if let Some(pid) = omegon_pid {
                                "Runtime: running (pid {pid})"
                            } else if let Some(err) = omegon_launch_error.as_ref() {
                                "Runtime: launch failed ({err})"
                            } else {
                                "Runtime: not started"
                            }
                        }
                        li { "Persona: {active_persona}" }
                        li { "Model: {model_summary}" }
                        li { "Home: {omegon.home_dir.display()}" }
                        li {
                            "Project profile: {omegon.project_profile_path.display()}"
                            if project_profile_exists { " ✓" } else { " (missing)" }
                        }
                        li {
                            "Global profile: {omegon.global_profile_path.display()}"
                            if global_profile_exists { " ✓" } else { " (missing)" }
                        }
                        li {
                            "Vox manifest: {omegon.vox_manifest_path.display()}"
                            if vox_installed { " ✓" } else { " (missing)" }
                        }
                    }
                }
            }

            div { class: "agent-input",
                label {
                    class: "settings-field",
                    span { "Persona" }
                    input {
                        value: "{active_persona}",
                        disabled: true,
                        title: "Configured from Codex operator settings",
                    }
                }

                label {
                    class: "settings-field",
                    span { "Extension" }
                    input {
                        value: "{operator_settings.rail_extension}",
                        placeholder: "vox",
                        disabled: true,
                        title: "Configured from Codex operator settings",
                    }
                }

                textarea {
                    placeholder: "Send a prompt through the active Omegon-native extension…",
                    value: "{input}",
                    oninput: move |e| *input.write() = e.value(),
                }

                button {
                    class: "agent-send",
                    disabled: !runtime_ready || !vox_installed,
                    title: if !vox_installed {
                        "Install vox into ~/.omegon/extensions/vox first"
                    } else if runtime_ready {
                        "Background Omegon host is running; RPC wiring is the next slice"
                    } else {
                        "Open the rail to start Omegon, or inspect launch failure above"
                    },
                    if runtime_ready {
                        "Host running"
                    } else if vox_installed {
                        "Waiting for host"
                    } else {
                        "Vox not installed"
                    }
                }
            }
        }
    }
}
