use crate::bootstrap::AppContext;
use codex_core::providers::{
    self, AuthMethod, CredentialStatus, ProviderInfo, PROVIDERS,
};
use dioxus::prelude::*;

#[component]
pub fn ProviderSettingsSection() -> Element {
    let mut refresh = use_signal(|| 0u64);

    // Probe providers on mount and on refresh
    let statuses = use_memo(move || {
        let _ = refresh.read();
        providers::probe_all()
    });

    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "Providers" }
            div { class: "settings-rows",
                for (provider, status) in statuses.read().iter() {
                    ProviderRow {
                        provider,
                        status: status.clone(),
                        on_change: move |_| *refresh.write() += 1,
                    }
                }
            }
        }
    }
}

#[component]
fn ProviderRow(
    provider: &'static ProviderInfo,
    status: CredentialStatus,
    on_change: EventHandler<()>,
) -> Element {
    let ctx = use_context::<AppContext>();
    let mut editing = use_signal(|| false);
    let mut key_input = use_signal(String::new);
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);

    let (status_class, status_text) = match &status {
        CredentialStatus::Authenticated { source } => {
            ("provider-status authenticated", format!("Authenticated ({source})"))
        }
        CredentialStatus::Expired => ("provider-status expired", "Expired".to_string()),
        CredentialStatus::Missing => ("provider-status missing", "Not configured".to_string()),
    };

    let is_authenticated = matches!(status, CredentialStatus::Authenticated { .. });
    let is_api_key = provider.auth_method == AuthMethod::ApiKey;

    rsx! {
        div { class: "settings-row provider-row",
            span { class: "settings-label", "{provider.label}" }
            div { class: "settings-control",
                div { class: "provider-status-row",
                    span { class: status_class }
                    span { class: "provider-status-text", "{status_text}" }
                }

                if *editing.read() {
                    // API key entry form
                    div { class: "provider-key-form",
                        input {
                            class: "input settings-input",
                            r#type: "password",
                            value: "{key_input}",
                            placeholder: if is_api_key { "API key…" } else { "OAuth token…" },
                            autofocus: true,
                            oninput: move |e| *key_input.write() = e.value(),
                            onkeydown: move |e| {
                                if e.key() == Key::Escape {
                                    *editing.write() = false;
                                    *key_input.write() = String::new();
                                }
                            },
                        }
                        div { class: "row gap-2",
                            button {
                                class: "btn btn-primary btn-sm",
                                disabled: key_input.read().trim().is_empty(),
                                onclick: move |_| {
                                    let key = key_input.read().trim().to_string();
                                    if key.is_empty() { return; }
                                    match providers::save_api_key(provider.id, &key) {
                                        Ok(()) => {
                                            *editing.write() = false;
                                            *key_input.write() = String::new();
                                            *error_msg.write() = None;
                                            on_change.call(());
                                        }
                                        Err(e) => {
                                            *error_msg.write() = Some(format!("{e}"));
                                        }
                                    }
                                },
                                "Save"
                            }
                            button {
                                class: "btn btn-ghost btn-sm",
                                onclick: move |_| {
                                    *editing.write() = false;
                                    *key_input.write() = String::new();
                                },
                                "Cancel"
                            }
                        }
                        if let Some(ref err) = *error_msg.read() {
                            span { class: "text-error", "{err}" }
                        }
                    }
                } else {
                    // Action buttons
                    div { class: "row gap-2",
                        if is_api_key {
                            button {
                                class: "btn btn-ghost btn-sm",
                                onclick: move |_| *editing.write() = true,
                                if is_authenticated { "Update key" } else { "Add key" }
                            }
                        } else {
                            // OAuth provider — launch browser flow
                            button {
                                class: "btn btn-ghost btn-sm",
                                onclick: move |_| {
                                    let runtime_cfg = ctx.vault().config.local_runtime.clone();
                                    let (bin, args) = providers::oauth_login_command(&runtime_cfg, provider.id);
                                    spawn(async move {
                                        match tokio::process::Command::new(&bin).args(&args).spawn() {
                                            Ok(_) => tracing::info!("OAuth login started for {}", args.last().unwrap_or(&String::new())),
                                            Err(e) => tracing::warn!("OAuth login failed: {e}"),
                                        }
                                    });
                                },
                                if is_authenticated { "Re-authenticate" } else { "Login" }
                            }
                            // Also allow manual token entry for OAuth providers
                            button {
                                class: "btn btn-ghost btn-sm",
                                onclick: move |_| *editing.write() = true,
                                "Paste token"
                            }
                        }

                        if is_authenticated {
                            button {
                                class: "btn btn-ghost btn-sm provider-remove-btn",
                                onclick: move |_| {
                                    let _ = providers::remove_credential(provider.id);
                                    on_change.call(());
                                },
                                "Remove"
                            }
                        }
                    }
                }
            }
        }
    }
}
