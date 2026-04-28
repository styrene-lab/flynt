use codex_core::identity;
use dioxus::prelude::*;
use crate::bootstrap::AppContext;

#[component]
pub fn IdentitySettingsSection() -> Element {
    let ctx = use_context::<AppContext>();
    let mut status = use_signal(identity::probe_identity);
    let mut passphrase = use_signal(String::new);
    let mut confirm_passphrase = use_signal(String::new);
    let mut unlocked: Signal<Option<UnlockedDisplay>> = use_signal(|| None);
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);
    let mut creating = use_signal(|| false);
    let mut unlocking = use_signal(|| false);

    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "Identity" }
            div { class: "settings-rows",

                // Status
                div { class: "settings-row",
                    span { class: "settings-label", "Status" }
                    div { class: "settings-control",
                        div { class: "identity-status-row",
                            span {
                                class: if status.read().available { "identity-dot active" } else { "identity-dot" },
                            }
                            span { class: "identity-status-text",
                                if status.read().available {
                                    "{status.read().tier}"
                                } else {
                                    "No identity"
                                }
                            }
                        }
                    }
                }

                if status.read().available {
                    // ── Unlock existing identity ────────────────
                    if unlocked.read().is_none() {
                        div { class: "settings-row",
                            span { class: "settings-label", "Unlock" }
                            div { class: "settings-control",
                                div { class: "identity-form",
                                    input {
                                        class: "input settings-input",
                                        r#type: "password",
                                        value: "{passphrase}",
                                        placeholder: "Passphrase",
                                        oninput: move |e| *passphrase.write() = e.value(),
                                        onkeydown: move |e| {
                                            if e.key() == Key::Enter {
                                                let pp = passphrase.read().clone();
                                                *unlocking.write() = true;
                                                *error_msg.write() = None;
                                                spawn(async move {
                                                    match tokio::task::spawn_blocking(move || identity::unlock_identity(&pp)).await {
                                                        Ok(Ok(id)) => {
                                                            *unlocked.write() = Some(UnlockedDisplay {
                                                                fingerprint: id.fingerprint,
                                                                ssh_pubkey: id.ssh_pubkey,
                                                            });
                                                        }
                                                        Ok(Err(e)) => *error_msg.write() = Some(format!("{e}")),
                                                        Err(e) => *error_msg.write() = Some(format!("{e}")),
                                                    }
                                                    *unlocking.write() = false;
                                                    *passphrase.write() = String::new();
                                                });
                                            }
                                        },
                                    }
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        disabled: passphrase.read().is_empty() || *unlocking.read(),
                                        onclick: move |_| {
                                            let pp = passphrase.read().clone();
                                            *unlocking.write() = true;
                                            *error_msg.write() = None;
                                            spawn(async move {
                                                match tokio::task::spawn_blocking(move || identity::unlock_identity(&pp)).await {
                                                    Ok(Ok(id)) => {
                                                        *unlocked.write() = Some(UnlockedDisplay {
                                                            fingerprint: id.fingerprint,
                                                            ssh_pubkey: id.ssh_pubkey,
                                                        });
                                                    }
                                                    Ok(Err(e)) => *error_msg.write() = Some(format!("{e}")),
                                                    Err(e) => *error_msg.write() = Some(format!("{e}")),
                                                }
                                                *unlocking.write() = false;
                                                *passphrase.write() = String::new();
                                            });
                                        },
                                        if *unlocking.read() { "Unlocking…" } else { "Unlock" }
                                    }
                                }
                            }
                        }
                    }

                    // ── Show unlocked identity details ──────────
                    if let Some(ref id) = *unlocked.read() {
                        div { class: "settings-row",
                            span { class: "settings-label", "Fingerprint" }
                            div { class: "settings-control",
                                code { class: "identity-fingerprint", "{id.fingerprint}" }
                            }
                        }
                        div { class: "settings-row",
                            span { class: "settings-label", "SSH public key" }
                            div { class: "settings-control",
                                div { class: "identity-ssh-key",
                                    code { class: "identity-key-text", "{id.ssh_pubkey}" }
                                    button {
                                        class: "btn btn-ghost btn-xs",
                                        onclick: {
                                            let key = id.ssh_pubkey.clone();
                                            move |_| {
                                                let js = format!(
                                                    "navigator.clipboard.writeText({})",
                                                    serde_json::to_string(&key).unwrap_or_default()
                                                );
                                                document::eval(&js);
                                            }
                                        },
                                        "Copy"
                                    }
                                }
                                span { class: "settings-hint",
                                    "Add this key to your Codeberg or GitHub account for passwordless git access"
                                }
                            }
                        }
                        div { class: "settings-row",
                            span { class: "settings-label", "Git signing" }
                            div { class: "settings-control",
                                button {
                                    class: "btn btn-ghost btn-sm",
                                    onclick: {
                                        let ssh_key = id.ssh_pubkey.clone();
                                        move |_| {
                                            let vault_root = ctx.vault_root();
                                            let key = ssh_key.clone();
                                            match identity::configure_git_signing(&vault_root, &key) {
                                                Ok(()) => *error_msg.write() = Some("Git signing enabled for this vault".into()),
                                                Err(e) => *error_msg.write() = Some(format!("Failed: {e}")),
                                            }
                                        }
                                    },
                                    "Enable git signing for this vault"
                                }
                            }
                        }
                    }
                } else {
                    // ── Create new identity ─────────────────────
                    div { class: "settings-row",
                        span { class: "settings-label", "Create" }
                        div { class: "settings-control",
                            div { class: "identity-form",
                                input {
                                    class: "input settings-input",
                                    r#type: "password",
                                    value: "{passphrase}",
                                    placeholder: "Choose a passphrase",
                                    oninput: move |e| *passphrase.write() = e.value(),
                                }
                                input {
                                    class: "input settings-input",
                                    r#type: "password",
                                    value: "{confirm_passphrase}",
                                    placeholder: "Confirm passphrase",
                                    oninput: move |e| *confirm_passphrase.write() = e.value(),
                                }
                                button {
                                    class: "btn btn-primary btn-sm",
                                    disabled: passphrase.read().is_empty()
                                        || *passphrase.read() != *confirm_passphrase.read()
                                        || *creating.read(),
                                    onclick: move |_| {
                                        let pp = passphrase.read().clone();
                                        *creating.write() = true;
                                        *error_msg.write() = None;
                                        spawn(async move {
                                            match tokio::task::spawn_blocking(move || identity::create_identity(&pp)).await {
                                                Ok(Ok(_path)) => {
                                                    *status.write() = identity::probe_identity();
                                                    *error_msg.write() = Some("Identity created. Unlock it to see your keys.".into());
                                                }
                                                Ok(Err(e)) => *error_msg.write() = Some(format!("{e}")),
                                                Err(e) => *error_msg.write() = Some(format!("{e}")),
                                            }
                                            *creating.write() = false;
                                            *passphrase.write() = String::new();
                                            *confirm_passphrase.write() = String::new();
                                        });
                                    },
                                    if *creating.read() { "Creating…" } else { "Create identity" }
                                }
                            }
                            span { class: "settings-hint",
                                "Your identity derives all keys from a single passphrase. Choose a strong one."
                            }
                        }
                    }
                }

                // Error/success messages
                if let Some(ref msg) = *error_msg.read() {
                    div { class: "settings-row",
                        span { class: "settings-label", "" }
                        div { class: "settings-control",
                            span {
                                class: if msg.contains("created") || msg.contains("enabled") { "save-msg ok" } else { "save-msg err" },
                                "{msg}"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct UnlockedDisplay {
    fingerprint: String,
    ssh_pubkey: String,
}
