//! Generic extension config UI — renders config fields and secret status
//! from the ACP `_extensions/list` response schema.

use crate::acp::AcpSession;
use dioxus::prelude::*;
use std::rc::Rc;

/// A single extension's data from `_extensions/list`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtensionData {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub config_fields: Vec<ConfigFieldEntry>,
    pub required_secrets: Vec<SecretEntry>,
    pub optional_secrets: Vec<SecretEntry>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigFieldEntry {
    pub key: String,
    pub field_type: String,
    pub label: String,
    pub description: String,
    pub required: bool,
    pub default: Option<String>,
    pub placeholder: Option<String>,
    pub values: Vec<String>,
    pub current_value: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SecretEntry {
    pub name: String,
    pub resolved: bool,
    pub source: Option<String>,
}

/// Parse the `_extensions/list` response into structured data.
pub fn parse_extensions_list(value: &serde_json::Value) -> Vec<ExtensionData> {
    let Some(extensions) = value["extensions"].as_array() else {
        return Vec::new();
    };
    extensions.iter().filter_map(|ext| {
        let name = ext["name"].as_str()?.to_string();
        let version = ext["version"].as_str().unwrap_or("?").to_string();
        let description = ext["description"].as_str().unwrap_or("").to_string();
        let enabled = ext["enabled"].as_bool().unwrap_or(true);

        let config_fields = ext["config_schema"].as_object()
            .map(|schema| {
                schema.iter().map(|(key, field)| {
                    ConfigFieldEntry {
                        key: key.clone(),
                        field_type: field["type"].as_str().unwrap_or("string").to_string(),
                        label: field["label"].as_str().unwrap_or(key).to_string(),
                        description: field["description"].as_str().unwrap_or("").to_string(),
                        required: field["required"].as_bool().unwrap_or(false),
                        default: field["default"].as_str().map(String::from),
                        placeholder: field["placeholder"].as_str().map(String::from),
                        values: field["values"].as_array()
                            .map(|v| v.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                            .unwrap_or_default(),
                        current_value: field["current_value"].as_str().map(String::from),
                    }
                }).collect()
            })
            .unwrap_or_default();

        let parse_secrets = |key: &str| -> Vec<SecretEntry> {
            ext["secrets"][key].as_array()
                .map(|arr| arr.iter().filter_map(|s| {
                    Some(SecretEntry {
                        name: s["name"].as_str()?.to_string(),
                        resolved: s["resolved"].as_bool().unwrap_or(false),
                        source: s["source"].as_str().map(String::from),
                    })
                }).collect())
                .unwrap_or_default()
        };

        Some(ExtensionData {
            name, version, description, enabled, config_fields,
            required_secrets: parse_secrets("required"),
            optional_secrets: parse_secrets("optional"),
        })
    }).collect()
}

/// Renders config fields + secret status for a single extension.
#[component]
pub fn ExtensionConfigPanel(
    ext: ExtensionData,
    session: Signal<Option<Rc<AcpSession>>>,
) -> Element {
    let has_config = !ext.config_fields.is_empty();
    let has_secrets = !ext.required_secrets.is_empty() || !ext.optional_secrets.is_empty();

    if !has_config && !has_secrets {
        return rsx! {};
    }

    let ext_name = ext.name.clone();

    rsx! {
        div { class: "extension-config-panel",
            if has_config {
                div { class: "extension-config-section",
                    span { class: "extension-config-heading", "Configuration" }
                    for field in &ext.config_fields {
                        ExtensionConfigField {
                            ext_name: ext_name.clone(),
                            field: field.clone(),
                            session: session,
                        }
                    }
                }
            }

            if has_secrets {
                div { class: "extension-config-section",
                    span { class: "extension-config-heading", "Secrets" }
                    for secret in &ext.required_secrets {
                        ExtensionSecretRow {
                            ext_name: ext_name.clone(),
                            secret: secret.clone(),
                            optional: false,
                            session: session,
                        }
                    }
                    for secret in &ext.optional_secrets {
                        ExtensionSecretRow {
                            ext_name: ext_name.clone(),
                            secret: secret.clone(),
                            optional: true,
                            session: session,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ExtensionConfigField(
    ext_name: String,
    field: ConfigFieldEntry,
    session: Signal<Option<Rc<AcpSession>>>,
) -> Element {
    let initial = field.current_value.clone()
        .or_else(|| field.default.clone())
        .unwrap_or_default();
    let mut value = use_signal(|| initial);
    let mut status = use_signal(|| Option::<(&'static str, String)>::None);

    let ext_for_save = ext_name.clone();
    let key_for_save = field.key.clone();
    let do_save = move || {
        let ext = ext_for_save.clone();
        let key = key_for_save.clone();
        let val = value.read().clone();
        let sess = session.read().clone();
        spawn(async move {
            if let Some(s) = sess {
                match s.extensions_config_set(&ext, &key, &val).await {
                    Ok(resp) => {
                        if resp["ok"].as_bool() == Some(true) {
                            *status.write() = Some(("ok", "Saved".into()));
                        } else {
                            let err = resp["error"].as_str().unwrap_or("unknown error");
                            *status.write() = Some(("err", err.to_string()));
                        }
                    }
                    Err(e) => *status.write() = Some(("err", e.to_string())),
                }
            }
        });
    };

    rsx! {
        div { class: "extension-config-field",
            label { class: "extension-config-label",
                "{field.label}"
                if field.required {
                    span { class: "extension-config-required", " *" }
                }
            }
            if !field.description.is_empty() {
                span { class: "settings-hint muted", "{field.description}" }
            }
            div { class: "extension-config-input-row",
                match field.field_type.as_str() {
                    "boolean" => rsx! {
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: value.read().as_str() == "true",
                                onchange: {
                                    let do_save = do_save.clone();
                                    move |e: Event<FormData>| {
                                        *value.write() = if e.checked() { "true".into() } else { "false".into() };
                                        do_save();
                                    }
                                },
                            }
                            "Enabled"
                        }
                    },
                    "enum" => rsx! {
                        select {
                            class: "input settings-input",
                            value: "{value}",
                            onchange: {
                                let do_save = do_save.clone();
                                move |e: Event<FormData>| {
                                    *value.write() = e.value();
                                    do_save();
                                }
                            },
                            for opt in &field.values {
                                option {
                                    value: "{opt}",
                                    selected: *value.read() == *opt,
                                    "{opt}"
                                }
                            }
                        }
                    },
                    "text" => rsx! {
                        textarea {
                            class: "input settings-input",
                            value: "{value}",
                            rows: "4",
                            placeholder: field.placeholder.as_deref().unwrap_or(""),
                            oninput: move |e| *value.write() = e.value(),
                            onfocusout: {
                                let do_save = do_save.clone();
                                move |_| do_save()
                            },
                        }
                    },
                    _ => rsx! {
                        input {
                            class: "input settings-input",
                            r#type: if field.field_type == "number" { "number" } else { "text" },
                            value: "{value}",
                            placeholder: field.placeholder.as_deref().unwrap_or(""),
                            oninput: move |e| *value.write() = e.value(),
                            onfocusout: {
                                let do_save = do_save.clone();
                                move |_| do_save()
                            },
                        }
                    },
                }
                if let Some((kind, msg)) = &*status.read() {
                    span {
                        class: if *kind == "ok" { "save-msg ok" } else { "save-msg err" },
                        "{msg}"
                    }
                }
            }
        }
    }
}

#[component]
fn ExtensionSecretRow(
    ext_name: String,
    secret: SecretEntry,
    optional: bool,
    session: Signal<Option<Rc<AcpSession>>>,
) -> Element {
    let mut show_input = use_signal(|| false);
    let mut secret_value = use_signal(String::new);
    let mut status = use_signal(|| Option::<(&'static str, String)>::None);

    let status_class = if secret.resolved {
        "provider-status authenticated"
    } else if optional {
        "provider-status missing"
    } else {
        "provider-status missing"
    };

    let status_label = if secret.resolved {
        secret.source.as_deref().unwrap_or("configured")
    } else if optional {
        "optional"
    } else {
        "missing"
    };

    rsx! {
        div { class: "extension-secret-row",
            div { class: "extension-secret-status",
                span { class: status_class }
                span { class: "extension-secret-name", "{secret.name}" }
                span { class: "settings-hint muted", "{status_label}" }
            }
            div { class: "extension-secret-actions",
                if secret.resolved {
                    button {
                        class: "btn btn-ghost btn-sm",
                        onclick: {
                            let name = secret.name.clone();
                            move |_| {
                                let name = name.clone();
                                let sess = session.read().clone();
                                spawn(async move {
                                    if let Some(s) = sess {
                                        match s.extensions_secret_delete(&name).await {
                                            Ok(_) => *status.write() = Some(("ok", "Cleared".into())),
                                            Err(e) => *status.write() = Some(("err", e.to_string())),
                                        }
                                    }
                                });
                            }
                        },
                        "Clear"
                    }
                } else {
                    button {
                        class: "btn btn-ghost btn-sm",
                        onclick: move |_| {
                            let v = *show_input.read();
                            *show_input.write() = !v;
                        },
                        if *show_input.read() { "Cancel" } else { "Set" }
                    }
                }
                if let Some((kind, msg)) = &*status.read() {
                    span {
                        class: if *kind == "ok" { "save-msg ok" } else { "save-msg err" },
                        "{msg}"
                    }
                }
            }
            if *show_input.read() {
                div { class: "extension-secret-input",
                    input {
                        class: "input settings-input",
                        r#type: "password",
                        placeholder: "Enter secret value…",
                        value: "{secret_value}",
                        oninput: move |e| *secret_value.write() = e.value(),
                    }
                    button {
                        class: "btn btn-primary btn-sm",
                        onclick: {
                            let ext = ext_name.clone();
                            let name = secret.name.clone();
                            move |_| {
                                let ext = ext.clone();
                                let name = name.clone();
                                let val = secret_value.read().clone();
                                let sess = session.read().clone();
                                spawn(async move {
                                    if let Some(s) = sess {
                                        match s.extensions_secret_set(&ext, &name, &val).await {
                                            Ok(resp) => {
                                                if resp["ok"].as_bool() == Some(true) {
                                                    *status.write() = Some(("ok", "Stored in keychain".into()));
                                                    *show_input.write() = false;
                                                    *secret_value.write() = String::new();
                                                } else {
                                                    let err = resp["error"].as_str().unwrap_or("failed");
                                                    *status.write() = Some(("err", err.to_string()));
                                                }
                                            }
                                            Err(e) => *status.write() = Some(("err", e.to_string())),
                                        }
                                    }
                                });
                            }
                        },
                        "Store"
                    }
                }
            }
        }
    }
}
