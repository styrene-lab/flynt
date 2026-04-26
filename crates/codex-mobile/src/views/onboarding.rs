use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Welcome,
    RepoInput,
    Cloning,
    Done,
    Error,
}

#[component]
pub fn OnboardingView(on_complete: EventHandler<std::path::PathBuf>) -> Element {
    let mut step = use_signal(|| Step::Welcome);
    let mut repo_url = use_signal(|| String::new());
    let mut branch = use_signal(|| "main".to_string());
    let mut token = use_signal(|| String::new());
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);
    let mut vault_name = use_signal(|| String::new());

    rsx! {
        div { class: "onboarding",
            match *step.read() {
                Step::Welcome => rsx! {
                    div { class: "onboarding-card",
                        h1 { class: "onboarding-title", "Welcome to Codex" }
                        p { class: "onboarding-desc",
                            "Your local-first knowledge vault. Connect a GitHub repository to sync your notes across devices, or create a new local vault."
                        }

                        div { class: "onboarding-actions",
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| *step.write() = Step::RepoInput,
                                "Clone from GitHub"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| {
                                    // Create a local vault directly
                                    let vault_root = crate::bootstrap::default_vault_root();
                                    on_complete.call(vault_root);
                                },
                                "Create local vault"
                            }
                        }
                    }
                },

                Step::RepoInput => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Connect Repository" }
                        p { class: "onboarding-desc",
                            "Enter your GitHub repository URL and a personal access token with repo scope."
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

                                    // Derive vault name from repo URL
                                    let name = url
                                        .rsplit('/')
                                        .next()
                                        .unwrap_or("vault")
                                        .trim_end_matches(".git")
                                        .to_string();
                                    *vault_name.write() = name.clone();

                                    *step.write() = Step::Cloning;
                                    *error_msg.write() = None;

                                    let dest = crate::bootstrap::default_vault_root();
                                    spawn(async move {
                                        match tokio::task::spawn_blocking(move || {
                                            crate::oauth::clone_with_token(&url, &br, &dest, &tk)
                                        }).await {
                                            Ok(Ok(())) => {
                                                *step.write() = Step::Done;
                                            }
                                            Ok(Err(e)) => {
                                                *error_msg.write() = Some(format!("{e}"));
                                                *step.write() = Step::Error;
                                            }
                                            Err(e) => {
                                                *error_msg.write() = Some(format!("Clone task failed: {e}"));
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

                Step::Cloning => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Cloning…" }
                        p { class: "onboarding-desc",
                            "Downloading your vault from GitHub. This may take a moment."
                        }
                        div { class: "onboarding-spinner" }
                    }
                },

                Step::Done => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Vault ready" }
                        p { class: "onboarding-desc",
                            "Your vault \"{vault_name}\" has been cloned and is ready to use."
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
                        h2 { class: "onboarding-title onboarding-error-title", "Clone failed" }
                        if let Some(ref err) = *error_msg.read() {
                            p { class: "onboarding-error", "{err}" }
                        }
                        div { class: "onboarding-actions",
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| *step.write() = Step::RepoInput,
                                "Try again"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| *step.write() = Step::Welcome,
                                "Back"
                            }
                        }
                    }
                },
            }
        }
    }
}
