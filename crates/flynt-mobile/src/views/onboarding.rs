use dioxus::prelude::*;
use flynt_core::manifest::ProjectManifest;
use std::path::PathBuf;

#[derive(Clone, PartialEq)]
enum Step {
    Welcome,
    SyncChoice,
    RepoInput,
    ManifestInput,
    ManifestProjects,
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
    let mut project_name = use_signal(|| String::new());

    // Manifest state
    let mut manifest: Signal<Option<ProjectManifest>> = use_signal(|| None);
    let mut selected_project: Signal<Option<usize>> = use_signal(|| None);
    let mut manifest_dir: Signal<Option<PathBuf>> = use_signal(|| None);

    rsx! {
        div { class: "onboarding",
            match step.read().clone() {
                // ── Welcome: use-case driven ────────────────────────
                Step::Welcome => rsx! {
                    div { class: "onboarding-card",
                        h1 { class: "onboarding-title", "Flynt" }
                        p { class: "onboarding-desc",
                            "Your notes, ideas, and projects — always yours."
                        }

                        div { class: "onboarding-paths",
                            button {
                                class: "onboarding-path primary",
                                onclick: move |_| {
                                    let project_root = crate::bootstrap::default_project_root();
                                    on_complete.call(project_root);
                                },
                                span { class: "onboarding-path-title", "Start writing" }
                                span { class: "onboarding-path-desc", "Create a local notebook" }
                            }

                            button {
                                class: "onboarding-path",
                                onclick: move |_| *step.write() = Step::SyncChoice,
                                span { class: "onboarding-path-title", "Sync across devices" }
                                span { class: "onboarding-path-desc", "Keep notes in sync between devices" }
                            }

                            button {
                                class: "onboarding-path",
                                onclick: move |_| *step.write() = Step::RepoInput,
                                span { class: "onboarding-path-title", "Join a shared project" }
                                span { class: "onboarding-path-desc", "Paste a link someone shared with you" }
                            }
                        }
                    }
                },

                // ── Sync choice ─────────────────────────────────────
                Step::SyncChoice => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Sync your notes" }

                        div { class: "onboarding-paths",
                            button {
                                class: "onboarding-path",
                                onclick: move |_| *step.write() = Step::RepoInput,
                                span { class: "onboarding-path-title", "Connect a notebook" }
                                span { class: "onboarding-path-desc", "Sync one notebook from an online service" }
                            }

                            button {
                                class: "onboarding-path",
                                onclick: move |_| *step.write() = Step::ManifestInput,
                                span { class: "onboarding-path-title", "Connect all my notebooks" }
                                span { class: "onboarding-path-desc", "I have several notebooks and want to access them all" }
                            }
                        }

                        button {
                            class: "btn btn-ghost",
                            onclick: move |_| *step.write() = Step::Welcome,
                            "Back"
                        }
                    }
                },

                // ── Manifest flow ───────────────────────────────────
                Step::ManifestInput => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Connect Your Notebooks" }
                        p { class: "onboarding-desc",
                            "Enter the URL for your notebook collection and your access token."
                        }

                        div { class: "onboarding-form",
                            label { class: "onboarding-field",
                                span { "Manifest URL" }
                                input {
                                    class: "input",
                                    r#type: "url",
                                    value: "{repo_url}",
                                    placeholder: "https://github.com/you/flynt-manifest.git",
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

                                    let dest = crate::bootstrap::default_project_root()
                                        .parent()
                                        .unwrap_or(&PathBuf::from("."))
                                        .join(".flynt-manifest");
                                    let dest_for_result = dest.clone();
                                    spawn(async move {
                                        match tokio::task::spawn_blocking(move || {
                                            let _ = std::fs::remove_dir_all(&dest);
                                            crate::oauth::clone_with_token(&url, "main", &dest, &tk)?;
                                            flynt_core::manifest::load_manifest(&dest)
                                        }).await {
                                            Ok(Ok(m)) => {
                                                *manifest.write() = Some(m);
                                                *manifest_dir.write() = Some(dest_for_result);
                                                *step.write() = Step::ManifestProjects;
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
                                onclick: move |_| *step.write() = Step::SyncChoice,
                                "Back"
                            }
                        }
                    }
                },

                Step::ManifestProjects => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Your Projects" }
                        if let Some(ref m) = *manifest.read() {
                            if !m.identity.name.is_empty() {
                                p { class: "onboarding-desc",
                                    "Welcome back, {m.identity.name}. Pick a project to sync."
                                }
                            }
                            div { class: "onboarding-project-list",
                                for (idx, project) in m.projects.iter().enumerate() {
                                    {
                                        let is_selected = *selected_project.read() == Some(idx);
                                        let name = project.name.clone();
                                        let role = project.role.label();
                                        rsx! {
                                            button {
                                                key: "project-{idx}",
                                                class: if is_selected { "onboarding-project-item selected" } else { "onboarding-project-item" },
                                                onclick: move |_| *selected_project.write() = Some(idx),
                                                div { class: "onboarding-project-name", "{name}" }
                                                div { class: "onboarding-project-meta",
                                                    span { class: "onboarding-project-role", "{role}" }
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
                                disabled: selected_project.read().is_none(),
                                onclick: move |_| {
                                    let Some(idx) = *selected_project.read() else { return };
                                    let Some(ref m) = *manifest.read() else { return };
                                    let project = m.projects[idx].clone();
                                    let tk = token.read().trim().to_string();

                                    *project_name.write() = project.name.clone();
                                    *step.write() = Step::Cloning;
                                    *error_msg.write() = None;

                                    let dest = crate::bootstrap::default_project_root();
                                    let mdir = manifest_dir.read().clone();
                                    let mut manifest_clone = m.clone();

                                    spawn(async move {
                                        match tokio::task::spawn_blocking(move || {
                                            crate::oauth::clone_with_token(
                                                &project.repo, &project.branch, &dest, &tk
                                            )?;
                                            if let Some(ref mdir) = mdir {
                                                if let Some(v) = manifest_clone.projects.iter_mut()
                                                    .find(|v| v.name == project.name)
                                                {
                                                    v.local_path = Some(dest.clone());
                                                }
                                                let _ = flynt_core::manifest::save_local_manifest(
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
                                "Sync this project"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| *step.write() = Step::ManifestInput,
                                "Back"
                            }
                        }
                    }
                },

                // ── Single project / join shared ──────────────────────
                Step::RepoInput => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Connect a notebook" }
                        p { class: "onboarding-desc",
                            "Paste the link you were given, or enter your notebook's URL."
                        }

                        div { class: "onboarding-form",
                            label { class: "onboarding-field",
                                span { "Project URL" }
                                input {
                                    class: "input",
                                    r#type: "url",
                                    value: "{repo_url}",
                                    placeholder: "https://github.com/you/my-project.git",
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
                                span { "Access token" }
                                input {
                                    class: "input",
                                    r#type: "password",
                                    value: "{token}",
                                    placeholder: "ghp_...",
                                    oninput: move |e| *token.write() = e.value(),
                                }
                                span { class: "onboarding-hint",
                                    "Ask the person who shared this with you if you need a token."
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

                                    let name = url.rsplit('/').next()
                                        .unwrap_or("project")
                                        .trim_end_matches(".git")
                                        .to_string();
                                    *project_name.write() = name;
                                    *step.write() = Step::Cloning;
                                    *error_msg.write() = None;

                                    let dest = crate::bootstrap::default_project_root();
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

                // ── Shared states ───────────────────────────────────
                Step::Cloning => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "Setting up…" }
                        p { class: "onboarding-desc", "Downloading your project. This may take a moment." }
                        div { class: "onboarding-spinner" }
                    }
                },

                Step::Done => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title", "You're all set" }
                        p { class: "onboarding-desc", "Your project \"{project_name}\" is ready." }
                        button {
                            class: "btn btn-primary",
                            onclick: move |_| {
                                let project_root = crate::bootstrap::default_project_root();
                                on_complete.call(project_root);
                            },
                            "Open project"
                        }
                    }
                },

                Step::Error => rsx! {
                    div { class: "onboarding-card",
                        h2 { class: "onboarding-title onboarding-error-title", "Something went wrong" }
                        if let Some(ref err) = *error_msg.read() {
                            p { class: "onboarding-error", "{err}" }
                        }
                        button {
                            class: "btn btn-primary",
                            onclick: move |_| *step.write() = Step::Welcome,
                            "Start over"
                        }
                    }
                },
            }
        }
    }
}
