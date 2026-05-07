use flynt_core::daemon::{
    AgentDaemonConfig, DaemonState, InboundCapability,
};
use dioxus::prelude::*;

use crate::bootstrap::AppContext;

#[component]
pub fn DaemonSettingsSection(
    config: Signal<AgentDaemonConfig>,
) -> Element {
    let ctx = use_context::<AppContext>();
    let daemon = ctx.daemon();
    let mut enabled = use_signal(|| config.read().enabled);
    let mut auto_start = use_signal(|| config.read().auto_start);
    let mut model = use_signal(|| config.read().model.clone().unwrap_or_default());
    let mut posture = use_signal(|| config.read().posture.clone().unwrap_or_default());
    let mut persona = use_signal(|| config.read().persona.clone().unwrap_or_default());
    let mut port = use_signal(|| config.read().port.to_string());
    let mut capabilities = use_signal(|| config.read().capabilities.clone());

    // Poll daemon state by ticking a counter every 2s
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            *tick.write() += 1;
        }
    });

    // Read current state
    let current_state = {
        let _ = tick.read(); // reactive dep
        daemon.state()
    };

    let status_class = match &current_state {
        DaemonState::Running { .. } => "daemon-status running",
        DaemonState::Starting => "daemon-status starting",
        DaemonState::Unhealthy(_) => "daemon-status unhealthy",
        DaemonState::Disabled => "daemon-status disabled",
        DaemonState::Stopped => "daemon-status stopped",
        DaemonState::AuspexManaged { .. } => "daemon-status auspex",
    };
    let status_text = current_state.to_string();
    let is_running = current_state.is_running();
    let is_stopped = current_state.is_stopped();

    // Sync local signals back to the config signal on change
    let mut sync_config = move || {
        let mut cfg = config.write();
        cfg.enabled = *enabled.read();
        cfg.auto_start = *auto_start.read();
        cfg.model = {
            let m = model.read().trim().to_string();
            if m.is_empty() { None } else { Some(m) }
        };
        cfg.posture = {
            let p = posture.read().trim().to_string();
            if p.is_empty() { None } else { Some(p) }
        };
        cfg.persona = {
            let p = persona.read().trim().to_string();
            if p.is_empty() { None } else { Some(p) }
        };
        cfg.port = port.read().parse::<u16>().unwrap_or(7842);
        cfg.capabilities = capabilities.read().clone();
    };

    let daemon_start = daemon.clone();
    let daemon_stop = daemon.clone();
    let daemon_restart = daemon.clone();

    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "Agent Daemon" }
            div { class: "settings-rows",

                // Status indicator
                div { class: "settings-row",
                    span { class: "settings-label", "Status" }
                    div { class: "settings-control",
                        div { class: "daemon-status-row",
                            span { class: status_class }
                            span { class: "daemon-status-text", "{status_text}" }
                        }
                    }
                }

                // Enable / Auto-start
                div { class: "settings-row",
                    span { class: "settings-label", "Enabled" }
                    div { class: "settings-control",
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: *enabled.read(),
                                onchange: move |e| {
                                    *enabled.write() = e.checked();
                                    sync_config();
                                },
                            }
                            "Enable daemon for this vault"
                        }
                    }
                }

                div { class: "settings-row",
                    span { class: "settings-label", "Auto-start" }
                    div { class: "settings-control",
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: *auto_start.read(),
                                onchange: move |e| {
                                    *auto_start.write() = e.checked();
                                    sync_config();
                                },
                            }
                            "Start daemon when app launches"
                        }
                    }
                }

                // Controls
                div { class: "settings-row",
                    span { class: "settings-label", "Controls" }
                    div { class: "settings-control",
                        div { class: "row gap-2",
                            button {
                                class: "btn btn-primary btn-sm",
                                disabled: is_running || !*enabled.read(),
                                onclick: move |_| {
                                    let d = daemon_start.clone();
                                    spawn(async move { let _ = d.start().await; });
                                },
                                "Start"
                            }
                            button {
                                class: "btn btn-ghost btn-sm",
                                disabled: is_stopped,
                                onclick: move |_| {
                                    let d = daemon_stop.clone();
                                    spawn(async move { let _ = d.stop().await; });
                                },
                                "Stop"
                            }
                            button {
                                class: "btn btn-ghost btn-sm",
                                disabled: is_stopped,
                                onclick: move |_| {
                                    let d = daemon_restart.clone();
                                    spawn(async move { let _ = d.restart().await; });
                                },
                                "Restart"
                            }
                        }
                    }
                }

                // Model
                div { class: "settings-row",
                    span { class: "settings-label", "Model" }
                    div { class: "settings-control",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{model}",
                            placeholder: "anthropic:claude-sonnet-4-7",
                            oninput: move |e| {
                                *model.write() = e.value();
                                sync_config();
                            },
                        }
                    }
                }

                // Posture
                div { class: "settings-row",
                    span { class: "settings-label", "Posture" }
                    div { class: "settings-control",
                        div { class: "radio-group",
                            for (value, label) in [
                                ("", "Default"),
                                ("fabricator", "Fabricator"),
                                ("architect", "Architect"),
                                ("explorator", "Explorator"),
                                ("devastator", "Devastator"),
                            ] {
                                button {
                                    class: if posture.read().as_str() == value { "radio-btn active" } else { "radio-btn" },
                                    onclick: move |_| {
                                        *posture.write() = value.to_string();
                                        sync_config();
                                    },
                                    "{label}"
                                }
                            }
                        }
                    }
                }

                // Persona
                div { class: "settings-row",
                    span { class: "settings-label", "Persona" }
                    div { class: "settings-control",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{persona}",
                            placeholder: "Agent persona name",
                            oninput: move |e| {
                                *persona.write() = e.value();
                                sync_config();
                            },
                        }
                    }
                }

                // Port
                div { class: "settings-row",
                    span { class: "settings-label", "Port" }
                    div { class: "settings-control",
                        input {
                            class: "input settings-input settings-input-sm",
                            r#type: "number",
                            value: "{port}",
                            placeholder: "7842",
                            oninput: move |e| {
                                *port.write() = e.value();
                                sync_config();
                            },
                        }
                    }
                }

                // Capabilities
                div { class: "settings-row",
                    span { class: "settings-label", "Capabilities" }
                    div { class: "settings-control",
                        div { class: "capabilities-grid",
                            for cap in InboundCapability::all() {
                                {
                                    let cap_clone = cap.clone();
                                    let is_active = capabilities.read().contains(&cap);
                                    rsx! {
                                        label { class: "checkbox-label",
                                            input {
                                                r#type: "checkbox",
                                                checked: is_active,
                                                onchange: move |e| {
                                                    let mut caps = capabilities.write();
                                                    if e.checked() {
                                                        if !caps.contains(&cap_clone) {
                                                            caps.push(cap_clone.clone());
                                                        }
                                                    } else {
                                                        caps.retain(|c| c != &cap_clone);
                                                    }
                                                    drop(caps);
                                                    sync_config();
                                                },
                                            }
                                            "{cap.label()}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Vox channel config has moved to the Extensions section (VoxExtensionSettings)
            }
        }
    }
}

/// Reusable Vox channel row with enable toggle + detail field.
#[component]
pub fn VoxChannelRow(
    label: &'static str,
    enabled: bool,
    on_toggle: EventHandler<bool>,
    detail: String,
    detail_label: &'static str,
    on_detail_change: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "settings-row vox-channel-row",
            span { class: "settings-label settings-label-indent", "{label}" }
            div { class: "settings-control",
                div { class: "row gap-2",
                    label { class: "checkbox-label",
                        input {
                            r#type: "checkbox",
                            checked: enabled,
                            onchange: move |e| on_toggle.call(e.checked()),
                        }
                        "Enabled"
                    }
                    if enabled {
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{detail}",
                            placeholder: "{detail_label}…",
                            oninput: move |e| on_detail_change.call(e.value()),
                        }
                    }
                }
            }
        }
    }
}
