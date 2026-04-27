use codex_core::manifest::{ManifestVault, VaultManifest};
use dioxus::prelude::*;
use std::path::PathBuf;

#[derive(Clone, PartialEq)]
enum Step {
    Welcome,
    RepoInput,
    ManifestInput,
    ManifestVaults,
    Cloning,
    Done,
    Error,
}

#[component]
pub fn OnboardingView(on_complete: EventHandler<PathBuf>) -> Element {
    let mut step = use_signal(|| Step::Welcome);
    let mut repo_url = use_signal(|| String::new());
    let mut branch = use_signal(|| "main".to_string());
    let mut token = use_signal(|| String::new());
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);
    let mut vault_name = use_signal(|| String::new());

    // Manifest state
    let mut manifest: Signal<Option<VaultManifest>> = use_signal(|| None);
    let mut selected_vault: Signal<Option<usize>> = use_signal(|| None);
    let mut manifest_dir: Signal<Option<PathBuf>> = use_signal(|| None);

    rsx! {
        div { class: "onboarding",
            match step.read().clone() {
                Step::Welcome => rsx! {
                    div { class: "onboarding-card",
                        h1 { class: "onboarding-title", "Welcome to Codex" }
                        p { class: "onboarding-desc",
                            "Your local-first knowledge vault. Sync notes across devices with git, or start fresh."
                        }

                        div { class: "onboarding-actions",
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| *step.write() = Step::ManifestInput,
                                "Connect vaults"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| *step.write() = Step::RepoInput,
                                "Clone single repo"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| {
                                    let vault_root = crate::bootstrap::default_vault_root();
                                    on_complete.call(vault_root);
                                },
                                "Create local vault"
                            }
                        }
                    }
                },

                // ── Manifest flow ───────────────────────────────────────
                Step::ManifestInput => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Connect Your Vaults" }
                        p { class: "onboarding-desc",
                            "Enter the URL of your vault manifest repository. This is a small repo that lists all your vaults."
                        }

                        div { class: "onboarding-form",
                            label { class: "onboarding-field",
                                span { "Manifest repo URL" }
                                input {
                                    class: "input",
                                    r#type: "url",
                                    value: "{repo_url}",
                                    placeholder: "https://github.com/user/codex-manifest.git",
                                    oninput: move |e| *repo_url.write() = e.value(),
                                }
                            }
                            label { class: "onboarding-field",
                                span { "Token" }
                                input {
                                    class: "input",
                                    r#type: "password",
                                    value: "{token}",
                                    placeholder: "ghp_...",
                                    oninput: move |e| *token.write() = e.value(),
                                }
                            }
                        }

                        div { class: "onboarding-actions",
                            button {
                                class: "btn btn-primary",
                                disabled: repo_url.read().trim().is_empty() || token.read().trim().is_empty(),
                                onclick: move |_| {
                                    let url = repo_url.read().trim().to_string();
                                    let tk = token.read().trim().to_string();
                                    *step.write() = Step::Cloning;
                                    *error_msg.write() = None;

                                    // Clone manifest repo to a temp location
                                    let dest = crate::bootstrap::default_vault_root()
                                        .parent()
                                        .unwrap_or(&PathBuf::from("."))
                                        .join(".codex-manifest");
                                    let dest_for_result = dest.clone();
                                    spawn(async move {
                                        match tokio::task::spawn_blocking(move || {
                                            // Remove existing manifest dir if present
                                            let _ = std::fs::remove_dir_all(&dest);
                                            crate::oauth::clone_with_token(&url, "main", &dest, &tk)?;
                                            codex_core::manifest::load_manifest(&dest)
                                        }).await {
                                            Ok(Ok(m)) => {
                                                *manifest.write() = Some(m);
                                                *manifest_dir.write() = Some(dest_for_result);
                                                *step.write() = Step::ManifestVaults;
                                            }
                                            Ok(Err(e)) => {
                                                *error_msg.write() = Some(format!("{e}"));
                                                *step.write() = Step::Error;
                                            }
                                            Err(e) => {
                                                *error_msg.write() = Some(format!("{e}"));
                                                *step.write() = Step::Error;
                                            }
                                        }
                                    });
                                },
                                "Connect"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| *step.write() = Step::Welcome,
                                "Back"
                            }
                        }
                    }
                },

                Step::ManifestVaults => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Your Vaults" }
                        if let Some(ref m) = *manifest.read() {
                            if !m.identity.name.is_empty() {
                                p { class: "onboarding-desc",
                                    "Signed in as {m.identity.name}. Select a vault to sync to this device."
                                }
                            }
                            div { class: "onboarding-vault-list",
                                for (idx, vault) in m.vaults.iter().enumerate() {
                                    {
                                        let is_selected = *selected_vault.read() == Some(idx);
                                        let vault_name_display = vault.name.clone();
                                        let role_label = vault.role.label();
                                        let hub_label = vault.hub.as_deref().unwrap_or("git");
                                        rsx! {
                                            button {
                                                key: "vault-{idx}",
                                                class: if is_selected { "onboarding-vault-item selected" } else { "onboarding-vault-item" },
                                                onclick: move |_| *selected_vault.write() = Some(idx),
                                                div { class: "onboarding-vault-name", "{vault_name_display}" }
                                                div { class: "onboarding-vault-meta",
                                                    span { class: "onboarding-vault-role", "{role_label}" }
                                                    span { class: "onboarding-vault-hub", "{hub_label}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        div { class: "onboarding-actions",
                            button {
                                class: "btn btn-primary",
                                disabled: selected_vault.read().is_none(),
                                onclick: move |_| {
                                    let Some(idx) = *selected_vault.read() else { return };
                                    let Some(ref m) = *manifest.read() else { return };
                                    let vault = m.vaults[idx].clone();
                                    let tk = token.read().trim().to_string();

                                    *vault_name.write() = vault.name.clone();
                                    *step.write() = Step::Cloning;
                                    *error_msg.write() = None;

                                    let dest = crate::bootstrap::default_vault_root();
                                    let mdir = manifest_dir.read().clone();
                                    let mut manifest_clone = m.clone();

                                    spawn(async move {
                                        match tokio::task::spawn_blocking(move || {
                                            crate::oauth::clone_with_token(
                                                &vault.repo, &vault.branch, &dest, &tk
                                            )?;
                                            // Update manifest sidecar with local path
                                            if let Some(ref mdir) = mdir {
                                                if let Some(v) = manifest_clone.vaults.iter_mut()
                                                    .find(|v| v.name == vault.name)
                                                {
                                                    v.local_path = Some(dest.clone());
                                                }
                                                let _ = codex_core::manifest::save_local_manifest(
                                                    mdir, &manifest_clone
                                                );
                                            }
                                            Ok::<_, anyhow::Error>(())
                                        }).await {
                                            Ok(Ok(())) => *step.write() = Step::Done,
                                            Ok(Err(e)) => {
                                                *error_msg.write() = Some(format!("{e}"));
                                                *step.write() = Step::Error;
                                            }
                                            Err(e) => {
                                                *error_msg.write() = Some(format!("{e}"));
                                                *step.write() = Step::Error;
                                            }
                                        }
                                    });
                                },
                                "Clone selected vault"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| *step.write() = Step::ManifestInput,
                                "Back"
                            }
                        }
                    }
                },

                // ── Single repo flow (unchanged) ────────────────────────
                Step::RepoInput => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Clone Repository" }
                        p { class: "onboarding-desc",
                            "Enter a git repository URL and a personal access token."
                        }

                        div { class: "onboarding-form",
                            label { class: "onboarding-field",
                                span { "Repository URL" }
                                input {
                                    class: "input",
                                    r#type: "url",
                                    value: "{repo_url}",
                                    placeholder: "https://github.com/user/vault.git",
                                    oninput: move |e| *repo_url.write() = e.value(),
                                }
                            }
                            label { class: "onboarding-field",
                                span { "Branch" }
                                input {
                                    class: "input",
                                    r#type: "text",
                                    value: "{branch}",
                                    placeholder: "main",
                                    oninput: move |e| *branch.write() = e.value(),
                                }
                            }
                            label { class: "onboarding-field",
                                span { "Personal access token" }
                                input {
                                    class: "input",
                                    r#type: "password",
                                    value: "{token}",
                                    placeholder: "ghp_...",
                                    oninput: move |e| *token.write() = e.value(),
                                }
                                span { class: "onboarding-hint",
                                    "Create at github.com/settings/tokens with 'repo' scope"
                                }
                            }
                        }

                        div { class: "onboarding-actions",
                            button {
                                class: "btn btn-primary",
                                disabled: repo_url.read().trim().is_empty() || token.read().trim().is_empty(),
                                onclick: move |_| {
                                    let url = repo_url.read().trim().to_string();
                                    let br = branch.read().trim().to_string();
                                    let tk = token.read().trim().to_string();

                                    let name = url
                                        .rsplit('/')
                                        .next()
                                        .unwrap_or("vault")
                                        .trim_end_matches(".git")
                                        .to_string();
                                    *vault_name.write() = name;
                                    *step.write() = Step::Cloning;
                                    *error_msg.write() = None;

                                    let dest = crate::bootstrap::default_vault_root();
                                    spawn(async move {
                                        match tokio::task::spawn_blocking(move || {
                                            crate::oauth::clone_with_token(&url, &br, &dest, &tk)
                                        }).await {
                                            Ok(Ok(())) => *step.write() = Step::Done,
                                            Ok(Err(e)) => {
                                                *error_msg.write() = Some(format!("{e}"));
                                                *step.write() = Step::Error;
                                            }
                                            Err(e) => {
                                                *error_msg.write() = Some(format!("{e}"));
                                                *step.write() = Step::Error;
                                            }
                                        }
                                    });
                                },
                                "Clone"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| *step.write() = Step::Welcome,
                                "Back"
                            }
                        }
                    }
                },

                // ── Shared states ───────────────────────────────────────
                Step::Cloning => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Cloning…" }
                        p { class: "onboarding-desc",
                            "Downloading your vault. This may take a moment."
                        }
                        div { class: "onboarding-spinner" }
                    }
                },

                Step::Done => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Vault ready" }
                        p { class: "onboarding-desc",
                            "Your vault \"{vault_name}\" is ready to use."
                        }
                        button {
                            class: "btn btn-primary",
                            onclick: move |_| {
                                let vault_root = crate::bootstrap::default_vault_root();
                                on_complete.call(vault_root);
                            },
                            "Open vault"
                        }
                    }
                },

                Step::Error => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title onboarding-error-title", "Something went wrong" }
                        if let Some(ref err) = *error_msg.read() {
                            p { class: "onboarding-error", "{err}" }
                        }
                        div { class: "onboarding-actions",
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| *step.write() = Step::Welcome,
                                "Start over"
                            }
                        }
                    }
                },
            }
        }
    }
}
