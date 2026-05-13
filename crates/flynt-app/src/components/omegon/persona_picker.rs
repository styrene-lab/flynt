//! Persona picker — dropdown of personas known to the live omegon session.
//!
//! Replaces the bare text input that required operators to know the
//! exact persona id. The picker queries `persona_list` over the ACP
//! control channel and offers an "off" sentinel for the no-persona
//! state. A "Custom…" tail option drops into a free-text input for
//! the rare case where the operator is referencing a persona the
//! omegon host hasn't surfaced yet (typo-safe escape hatch).

use crate::acp::AcpSession;
use dioxus::prelude::*;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
struct PersonaEntry {
    id: String,
    /// Pretty label. Falls back to id if the registry doesn't carry one.
    name: String,
    /// Short description for the hint row under the dropdown.
    description: String,
}

fn parse_persona_list(v: &serde_json::Value) -> Vec<PersonaEntry> {
    // Accept either a bare array or `{ personas: [...] }` — the omegon
    // RPC schema isn't pinned, so probe both.
    let arr = v
        .as_array()
        .or_else(|| v.get("personas").and_then(|x| x.as_array()))
        .or_else(|| v.get("results").and_then(|x| x.as_array()));
    let Some(arr) = arr else { return Vec::new() };
    arr.iter()
        .filter_map(|p| {
            let id = p.get("id")
                .or_else(|| p.get("name"))
                .and_then(|s| s.as_str())?
                .to_string();
            let name = p
                .get("name")
                .and_then(|s| s.as_str())
                .unwrap_or(&id)
                .to_string();
            let description = p
                .get("description")
                .or_else(|| p.get("directive"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            Some(PersonaEntry { id, name, description })
        })
        .collect()
}

/// The "no persona" sentinel id. Omegon treats `"off"` and the empty
/// string interchangeably; we use `"off"` because that's what
/// `omegon_settings` already writes when the operator clears it.
const PERSONA_OFF: &str = "off";

/// Sentinel selected when the operator picks "Custom…" in the dropdown.
/// The picker swaps in a text input; the actual saved value is whatever
/// they type there.
const PERSONA_CUSTOM: &str = "__custom__";

#[component]
pub fn PersonaPicker(current: String, on_change: EventHandler<String>) -> Element {
    let shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();
    let mut refresh = use_signal(|| 0u64);

    let personas = use_resource(move || {
        let _ = refresh.read();
        let sess = shared_session.read().clone();
        async move {
            let Some(s) = sess else { return Vec::new() };
            s.persona_list().await.map(|v| parse_persona_list(&v)).unwrap_or_default()
        }
    });

    // The current value matches one of the known personas, the "off"
    // sentinel, or none-of-the-above (operator typed something custom).
    let known_ids: Vec<String> = personas
        .read()
        .as_ref()
        .map(|v| v.iter().map(|p| p.id.clone()).collect())
        .unwrap_or_default();
    let current_is_known = current == PERSONA_OFF
        || current.is_empty()
        || known_ids.contains(&current);

    // Track whether the operator deliberately opened the custom-input.
    // Without this, picking "Custom…" then deleting the text would flip
    // the dropdown straight back to "off" on every render.
    let mut custom_mode = use_signal(|| !current_is_known && !current.is_empty());

    // The dropdown value: either the actual id, the off sentinel,
    // or the custom sentinel when the operator picked it.
    let selected_value: String = if *custom_mode.read() {
        PERSONA_CUSTOM.to_string()
    } else if current.is_empty() {
        PERSONA_OFF.to_string()
    } else {
        current.clone()
    };

    // Selected entry's description, for the hint row.
    let hint = personas
        .read()
        .as_ref()
        .and_then(|list| list.iter().find(|p| p.id == current).cloned())
        .map(|p| p.description)
        .unwrap_or_default();

    rsx! {
        div { class: "persona-picker",
            select {
                class: "input settings-input",
                value: "{selected_value}",
                onchange: move |e| {
                    let v = e.value();
                    if v == PERSONA_CUSTOM {
                        *custom_mode.write() = true;
                    } else {
                        *custom_mode.write() = false;
                        if v == PERSONA_OFF {
                            on_change.call(PERSONA_OFF.to_string());
                        } else {
                            on_change.call(v);
                        }
                    }
                },
                option { value: "{PERSONA_OFF}", "Off (no persona)" }
                match personas.read().as_ref() {
                    Some(list) if !list.is_empty() => rsx! {
                        for p in list.iter().cloned() {
                            option { value: "{p.id}", "{p.name}" }
                        }
                    },
                    Some(_) => rsx! {
                        option { disabled: true, value: "", "(no personas installed — install via armory or omegon persona create)" }
                    },
                    None => rsx! {
                        option { disabled: true, value: "", "Loading…" }
                    },
                }
                option { value: "{PERSONA_CUSTOM}", "Custom…" }
            }

            if *custom_mode.read() {
                input {
                    class: "input settings-input persona-custom-input",
                    r#type: "text",
                    value: "{current}",
                    placeholder: "Persona id",
                    oninput: move |e| on_change.call(e.value()),
                }
            }

            if !hint.is_empty() {
                div { class: "persona-hint muted", "{hint}" }
            }

            button {
                class: "btn btn-ghost btn-xs persona-refresh",
                onclick: move |_| { *refresh.write() += 1; },
                title: "Re-query omegon for installed personas",
                "Refresh"
            }
        }
    }
}
