use flynt_core::identity;
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

    let biometrics_available = identity::keychain_available();
    let is_keychain_identity = status.read().tier == "DeviceHsm";

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
                                    if is_keychain_identity {
                                        "Keychain (biometric)"
                                    } else {
                                        "{status.read().tier}"
                                    }
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
                        if is_keychain_identity {
                            // Biometric unlock — no passphrase needed
                            div { class: "settings-row",
                                span { class: "settings-label", "Unlock" }
                                div { class: "settings-control",
                                    div { class: "identity-form",
                                        button {
                                            class: "btn btn-primary btn-sm",
                                            disabled: *unlocking.read(),
                                            onclick: move |_| {
                                                *unlocking.write() = true;
                                                *error_msg.write() = None;
                                                spawn(async move {
                                                    match tokio::task::spawn_blocking(identity::unlock_keychain_identity).await {
                                                        Ok(Ok(id)) => {
                                                            *unlocked.write() = Some(UnlockedDisplay {
                                                                identity_hash: id.identity_hash,
                                                                ssh_auth_pubkey: id.ssh_auth_pubkey,
                                                                ssh_fingerprint: id.ssh_fingerprint,
                                                                git_signing_pubkey: id.git_signing_pubkey,
                                                            });
                                                        }
                                                        Ok(Err(e)) => *error_msg.write() = Some(format!("{e}")),
                                                        Err(e) => *error_msg.write() = Some(format!("{e}")),
                                                    }
                                                    *unlocking.write() = false;
                                                });
                                            },
                                            if *unlocking.read() { "Authenticating…" } else { "Authenticate with biometrics" }
                                        }
                                    }
                                    span { class: "settings-hint",
                                        "Your identity is protected by Face ID or Touch ID"
                                    }
                                }
                            }
                        } else {
                            // Passphrase unlock
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
                                                                    identity_hash: id.identity_hash,
                                                                    ssh_auth_pubkey: id.ssh_auth_pubkey,
                                                                    ssh_fingerprint: id.ssh_fingerprint,
                                                                    git_signing_pubkey: id.git_signing_pubkey,
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
                                                                identity_hash: id.identity_hash,
                                                                ssh_auth_pubkey: id.ssh_auth_pubkey,
                                                                ssh_fingerprint: id.ssh_fingerprint,
                                                                git_signing_pubkey: id.git_signing_pubkey,
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
                    }

                    // ── Show unlocked identity details ──────────
                    if let Some(ref id) = *unlocked.read() {
                        div { class: "settings-row",
                            span { class: "settings-label", "Identity" }
                            div { class: "settings-control",
                                code { class: "identity-fingerprint", "{id.identity_hash}" }
                                span { class: "settings-hint", "{id.ssh_fingerprint}" }
                            }
                        }
                        div { class: "settings-row",
                            span { class: "settings-label", "SSH auth key" }
                            div { class: "settings-control",
                                div { class: "identity-ssh-key",
                                    code { class: "identity-key-text", "{id.ssh_auth_pubkey}" }
                                    button {
                                        class: "btn btn-ghost btn-xs",
                                        onclick: {
                                            let key = id.ssh_auth_pubkey.clone();
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
                            span { class: "settings-label", "Git signing key" }
                            div { class: "settings-control",
                                div { class: "identity-ssh-key",
                                    code { class: "identity-key-text", "{id.git_signing_pubkey}" }
                                }
                                if ctx.vault_root().join(".git").exists() {
                                button {
                                    class: "btn btn-ghost btn-sm",
                                    onclick: {
                                        let ssh_key = id.git_signing_pubkey.clone();
                                        move |_| {
                                            let vault_root = ctx.vault_root();
                                            let key = ssh_key.clone();
                                            let profile = crate::bootstrap::OmegonRuntimeContext::load_launcher_profile();
                                            let manifest_id = profile.manifest_dir.as_ref()
                                                .and_then(|d| flynt_core::manifest::load_manifest(d).ok())
                                                .map(|m| m.identity);
                                            let git_name = manifest_id.as_ref()
                                                .filter(|i| !i.name.is_empty())
                                                .map(|i| i.name.as_str());
                                            let git_email = manifest_id.as_ref()
                                                .filter(|i| !i.email.is_empty())
                                                .map(|i| i.email.as_str());
                                            match identity::configure_git_signing(
                                                &vault_root, &key, git_name, git_email,
                                            ) {
                                                Ok(()) => *error_msg.write() = Some("Git signing enabled for this vault".into()),
                                                Err(e) => *error_msg.write() = Some(format!("Failed: {e}")),
                                            }
                                        }
                                    },
                                    "Enable git signing for this vault"
                                }
                                } else {
                                    span { class: "settings-hint", "Enable git sync first to use commit signing" }
                                }
                            }
                        }
                    }
                } else {
                    // ── Create new identity ─────────────────────
                    if biometrics_available {
                        // Biometric creation (Tier B) — primary option on Apple devices
                        div { class: "settings-row",
                            span { class: "settings-label", "Create" }
                            div { class: "settings-control",
                                div { class: "identity-form",
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        disabled: *creating.read(),
                                        onclick: move |_| {
                                            *creating.write() = true;
                                            *error_msg.write() = None;
                                            spawn(async move {
                                                match tokio::task::spawn_blocking(identity::create_keychain_identity).await {
                                                    Ok(Ok(())) => {
                                                        *status.write() = identity::probe_identity();
                                                        *error_msg.write() = Some("Identity created with biometric protection. Authenticate to see your keys.".into());
                                                    }
                                                    Ok(Err(e)) => *error_msg.write() = Some(format!("{e}")),
                                                    Err(e) => *error_msg.write() = Some(format!("{e}")),
                                                }
                                                *creating.write() = false;
                                            });
                                        },
                                        if *creating.read() { "Creating…" } else { "Protect with biometrics" }
                                    }
                                }
                                span { class: "settings-hint",
                                    "Your identity will be protected by Face ID or Touch ID. No passphrase needed."
                                }
                            }
                        }
                    }

                    // Passphrase creation (Tier D) — always available as fallback
                    div { class: "settings-row",
                        span { class: "settings-label",
                            if biometrics_available { "Or create with passphrase" } else { "Create" }
                        }
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
                                class: if msg.contains("created") || msg.contains("enabled") || msg.contains("Created") { "save-msg ok" } else { "save-msg err" },
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
    identity_hash: String,
    ssh_auth_pubkey: String,
    ssh_fingerprint: String,
    git_signing_pubkey: String,
}
