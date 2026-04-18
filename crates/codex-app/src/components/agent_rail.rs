use crate::bootstrap::AppContext;
use codex_core::models::{CodexOperatorSettings, OmegonProfile};
use dioxus::prelude::*;
use std::path::PathBuf;

/// Find the omegon binary on disk.
fn find_omegon_binary() -> Option<PathBuf> {
    // Check common locations
    let candidates = [
        dirs::home_dir().map(|h| h.join(".local/bin/omegon")),
        Some(PathBuf::from("/usr/local/bin/omegon")),
        dirs::home_dir().map(|h| h.join(".cargo/bin/omegon")),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    // Try PATH via `command -v`
    std::process::Command::new("which")
        .arg("omegon")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| PathBuf::from(s.trim()))
}

#[derive(Clone, PartialEq)]
struct ChatMessage {
    role: String, // "user" or "assistant"
    content: String,
}

#[component]
pub fn AgentRail() -> Element {
    let ctx = use_context::<AppContext>();
    let operator_settings = use_context::<Signal<CodexOperatorSettings>>().read().clone();
    let project_profile = use_context::<Signal<OmegonProfile>>().read().clone();
    let omegon = ctx.omegon();

    let mut input = use_signal(String::new);
    let mut messages: Signal<Vec<ChatMessage>> = use_signal(Vec::new);
    let mut is_running = use_signal(|| false);

    let omegon_binary = find_omegon_binary();
    let binary_found = omegon_binary.is_some();

    let active_persona = if operator_settings.active_persona.trim().is_empty() {
        "off".to_string()
    } else {
        operator_settings.active_persona.clone()
    };
    let model_summary = project_profile
        .last_used_model
        .as_ref()
        .map(|model| format!("{}/{}", model.provider, model.model_id))
        .unwrap_or_else(|| "default".to_string());

    let vault_root = ctx.vault_root();

    rsx! {
        div { class: "agent-rail",
            // ── Status bar ───────────────────────────────────────
            div { class: "agent-status-bar",
                div { class: "agent-status-row",
                    span { class: "agent-status-label", "Omegon" }
                    span {
                        class: if binary_found { "agent-status-badge connected" } else { "agent-status-badge disconnected" },
                        if binary_found { "ready" } else { "not found" }
                    }
                }
                div { class: "agent-status-detail",
                    span { "Persona: {active_persona}" }
                    span { " · Model: {model_summary}" }
                }
            }

            // ── Chat messages ────────────────────────────────────
            div { class: "agent-messages",
                if messages.read().is_empty() {
                    div { class: "agent-empty",
                        p { "Ask Omegon about your vault, notes, or projects." }
                        div { class: "agent-suggestions",
                            button {
                                class: "btn btn-ghost btn-xs",
                                onclick: move |_| *input.write() = "Summarize the current note".into(),
                                "Summarize note"
                            }
                            button {
                                class: "btn btn-ghost btn-xs",
                                onclick: move |_| *input.write() = "What are the most connected documents in my vault?".into(),
                                "Top connections"
                            }
                            button {
                                class: "btn btn-ghost btn-xs",
                                onclick: move |_| *input.write() = "Suggest tags for this document".into(),
                                "Suggest tags"
                            }
                        }
                    }
                } else {
                    for msg in messages.read().iter() {
                        div {
                            class: if msg.role == "user" { "agent-msg user" } else { "agent-msg assistant" },
                            div { class: "agent-msg-role",
                                if msg.role == "user" { "You" } else { "Omegon" }
                            }
                            div { class: "agent-msg-content", "{msg.content}" }
                        }
                    }
                    if *is_running.read() {
                        div { class: "agent-msg assistant",
                            div { class: "agent-msg-role", "Omegon" }
                            div { class: "agent-msg-content typing", "Thinking..." }
                        }
                    }
                }
            }

            // ── Input ────────────────────────────────────────────
            div { class: "agent-input-area",
                textarea {
                    class: "agent-textarea",
                    placeholder: if binary_found { "Ask Omegon..." } else { "Omegon binary not found" },
                    value: "{input}",
                    disabled: !binary_found || *is_running.read(),
                    oninput: move |e| *input.write() = e.value(),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter && !e.modifiers().shift() {
                            e.prevent_default();
                            let prompt = input.read().trim().to_string();
                            if prompt.is_empty() || !binary_found { return; }

                            let binary = omegon_binary.clone().unwrap();
                            let vault = vault_root.clone();

                            // Add user message
                            messages.write().push(ChatMessage {
                                role: "user".into(),
                                content: prompt.clone(),
                            });
                            *input.write() = String::new();
                            *is_running.write() = true;

                            spawn(async move {
                                let result = tokio::task::spawn_blocking(move || {
                                    std::process::Command::new(&binary)
                                        .arg("--prompt")
                                        .arg(&prompt)
                                        .arg("--cwd")
                                        .arg(&vault)
                                        .arg("--max-turns")
                                        .arg("10")
                                        .output()
                                }).await;

                                let response = match result {
                                    Ok(Ok(output)) => {
                                        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                                        if !stdout.trim().is_empty() {
                                            stdout.trim().to_string()
                                        } else if !stderr.trim().is_empty() {
                                            format!("(stderr) {}", stderr.trim())
                                        } else {
                                            "(no output)".to_string()
                                        }
                                    }
                                    Ok(Err(e)) => format!("Error: {e}"),
                                    Err(e) => format!("Error: {e}"),
                                };

                                messages.write().push(ChatMessage {
                                    role: "assistant".into(),
                                    content: response,
                                });
                                *is_running.write() = false;
                            });
                        }
                    },
                }
                div { class: "agent-input-hint",
                    "Enter to send · Shift+Enter for newline"
                }
            }
        }
    }
}
