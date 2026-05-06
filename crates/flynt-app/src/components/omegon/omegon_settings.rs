//! Main Omegon agent configuration section for the Settings view.
//!
//! Replaces the inline "Omegon Profile" and "Operator" sections with a
//! unified component backed by the config bridge. Changes to session-level
//! fields (model, thinking, posture) apply immediately to the live ACP
//! session via `set_config()`.

use std::rc::Rc;
use crate::acp::{AcpSession, ConfigOption};
use crate::bootstrap::AppContext;
use super::config_bridge::UnifiedOmegonConfig;
use dioxus::prelude::*;

#[component]
pub fn OmegonSettingsSection() -> Element {
    let ctx = use_context::<AppContext>();
    let shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();
    let config_options = use_context::<Signal<Vec<ConfigOption>>>();

    // Load unified config from disk
    let omegon = ctx.omegon();
    let profile = omegon.load_project_profile();
    let operator = omegon.load_operator_settings();
    let mut config = use_signal(|| UnifiedOmegonConfig::load(&profile, &operator));

    let mut save_msg: Signal<Option<(&str, &str)>> = use_signal(|| None);
    let mut show_advanced = use_signal(|| false);

    // Resolve available model options from ACP session config
    let model_options: Vec<(String, String)> = config_options
        .read()
        .iter()
        .find(|o| o.id == "model")
        .map(|o| o.options.iter().map(|v| (v.value.clone(), v.name.clone())).collect())
        .unwrap_or_default();

    let thinking_options: Vec<(String, String)> = config_options
        .read()
        .iter()
        .find(|o| o.id == "thinking")
        .map(|o| o.options.iter().map(|v| (v.value.clone(), v.name.clone())).collect())
        .unwrap_or_else(|| vec![
            ("off".into(), "Off".into()),
            ("minimal".into(), "Minimal".into()),
            ("low".into(), "Low".into()),
            ("medium".into(), "Medium".into()),
            ("high".into(), "High".into()),
        ]);

    let save_handler = move |_| {
        let cfg = config.read().clone();
        let omegon = ctx.omegon();
        let mut profile = omegon.load_project_profile();
        let mut operator = omegon.load_operator_settings();
        cfg.save_to(&mut profile, &mut operator);

        if let Err(e) = omegon.save_project_profile(&profile) {
            tracing::error!("Failed to save profile: {e}");
            *save_msg.write() = Some(("err", "Save failed — check logs."));
            return;
        }
        if let Err(e) = omegon.save_operator_settings(&operator) {
            tracing::error!("Failed to save operator settings: {e}");
            *save_msg.write() = Some(("err", "Save failed — check logs."));
            return;
        }

        // Live-apply session fields to ACP
        if let Some(sess) = shared_session.read().clone() {
            let model = cfg.model.clone();
            let thinking = cfg.thinking.clone();
            let posture = cfg.posture.clone();
            spawn(async move {
                sess.set_config("model", &model).await;
                sess.set_config("thinking", &thinking).await;
                sess.set_config("posture", &posture).await;
            });
        }

        tracing::info!("Omegon config saved");
        *save_msg.write() = Some(("ok", "Saved."));
    };

    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "Omegon Agent" }

            div { class: "settings-rows",
                // ── Model ──
                SettingsRow { label: "Model",
                    select {
                        class: "input settings-input",
                        value: "{config.read().model}",
                        onchange: move |e| config.write().model = e.value(),
                        if model_options.is_empty() {
                            option { value: "{config.read().model}", "{config.read().model}" }
                        }
                        for (value, name) in &model_options {
                            option { value: "{value}", "{name}" }
                        }
                    }
                }

                // ── Thinking Level ──
                SettingsRow { label: "Thinking level",
                    select {
                        class: "input settings-input",
                        value: "{config.read().thinking}",
                        onchange: move |e| config.write().thinking = e.value(),
                        for (value, name) in &thinking_options {
                            option { value: "{value}", "{name}" }
                        }
                    }
                }

                // ── Posture ──
                SettingsRow { label: "Posture",
                    super::PosturePicker {
                        current: config.read().posture.clone(),
                        on_change: move |v: String| config.write().posture = v,
                        vault_root: ctx.vault_root(),
                    }
                }

                // ── Max Turns ──
                SettingsRow { label: "Max turns",
                    input {
                        class: "input settings-input settings-input-narrow",
                        r#type: "number",
                        min: "1",
                        max: "200",
                        value: "{config.read().max_turns}",
                        oninput: move |e| {
                            if let Ok(v) = e.value().parse::<u32>() {
                                config.write().max_turns = v.max(1).min(200);
                            }
                        },
                    }
                }

                // ── Persona ──
                SettingsRow { label: "Persona",
                    input {
                        class: "input settings-input",
                        r#type: "text",
                        value: "{config.read().active_persona}",
                        placeholder: "off",
                        oninput: move |e| config.write().active_persona = e.value(),
                    }
                }

                // ── Agent ID ──
                SettingsRow { label: "Agent ID",
                    input {
                        class: "input settings-input",
                        r#type: "text",
                        value: "{config.read().agent_id.clone().unwrap_or_default()}",
                        placeholder: "default (no override)",
                        oninput: move |e| {
                            let v = e.value().trim().to_string();
                            config.write().agent_id = if v.is_empty() { None } else { Some(v) };
                        },
                    }
                }
            }

            // ── Advanced ──
            div { class: "settings-advanced-toggle",
                button {
                    class: "settings-toggle-btn",
                    onclick: move |_| { let v = *show_advanced.read(); *show_advanced.write() = !v; },
                    if *show_advanced.read() {
                        "Hide advanced settings \u{25B4}"
                    } else {
                        "Show advanced settings \u{25BE}"
                    }
                }
            }

            if *show_advanced.read() {
                div { class: "settings-rows",
                    // ── Provider Order ──
                    SettingsRow { label: "Provider order",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{config.read().provider_order.join(\", \")}",
                            placeholder: "anthropic, openai, ollama",
                            oninput: move |e| {
                                config.write().provider_order = e.value()
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            },
                        }
                        span { class: "settings-hint muted", "Comma-separated, first = preferred" }
                    }

                    // ── Avoid Providers ──
                    SettingsRow { label: "Avoid providers",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{config.read().avoid_providers.join(\", \")}",
                            placeholder: "none",
                            oninput: move |e| {
                                config.write().avoid_providers = e.value()
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            },
                        }
                    }

                    // ── Embedding URL ──
                    SettingsRow { label: "Embedding URL",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{config.read().embed_url.clone().unwrap_or_default()}",
                            placeholder: "http://localhost:11434",
                            oninput: move |e| {
                                let v = e.value().trim().to_string();
                                config.write().embed_url = if v.is_empty() { None } else { Some(v) };
                            },
                        }
                    }

                    // ── Embedding Model ──
                    SettingsRow { label: "Embedding model",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{config.read().embed_model.clone().unwrap_or_default()}",
                            placeholder: "nomic-embed-text",
                            oninput: move |e| {
                                let v = e.value().trim().to_string();
                                config.write().embed_model = if v.is_empty() { None } else { Some(v) };
                            },
                        }
                    }

                    // ── Context Floor Pin ──
                    SettingsRow { label: "Context floor pin",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{config.read().context_floor_pin.clone().unwrap_or_default()}",
                            placeholder: "none",
                            oninput: move |e| {
                                let v = e.value().trim().to_string();
                                config.write().context_floor_pin = if v.is_empty() { None } else { Some(v) };
                            },
                        }
                    }
                }
            }

            // ── Save button ──
            div { class: "settings-save-row",
                button {
                    class: "btn btn-primary",
                    onclick: save_handler,
                    "Save agent settings"
                }
                if let Some((kind, msg)) = save_msg.read().as_ref() {
                    span { class: if *kind == "err" { "text-error" } else { "text-success" }, "{msg}" }
                }
            }
        }
    }
}

/// Reusable settings row (label + control). Same pattern as settings.rs.
#[component]
fn SettingsRow(label: &'static str, children: Element) -> Element {
    rsx! {
        div { class: "settings-row",
            span { class: "settings-label", "{label}" }
            div { class: "settings-control", {children} }
        }
    }
}
