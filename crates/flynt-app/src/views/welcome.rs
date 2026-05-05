use crate::bootstrap::LauncherProfile;
use dioxus::prelude::*;

#[component]
pub fn WelcomeView(
    launcher_profile: LauncherProfile,
    on_get_started: EventHandler<()>,
    on_choose_existing: EventHandler<()>,
    on_clone_remote: EventHandler<()>,
    on_import_markdown: EventHandler<()>,
    on_cloud_vault: EventHandler<std::path::PathBuf>,
) -> Element {
    let mut show_advanced = use_signal(|| false);
    let mut show_sync_options = use_signal(|| false);
    let cloud_providers = flynt_store::sync::cloud::detect_providers();
    let has_cloud = !cloud_providers.is_empty();
    // Auto-expand git section if no cloud providers available
    let mut show_git_options = use_signal(move || !has_cloud);
    let has_existing_vault = launcher_profile.last_vault_root.is_some()
        || !launcher_profile.known_vaults.is_empty();

    rsx! {
        div { class: "view-welcome",
            div { class: "welcome-shell",
                div { class: "welcome-hero",
                    h1 { class: "welcome-title", "Flynt" }
                    p {
                        class: "welcome-subtitle",
                        "Your notes, ideas, and projects — stored as plain files, always yours."
                    }
                }

                // ── Primary paths ─────────────────────────────────────
                div { class: "welcome-paths",

                    // Path 1: Start writing or return to existing notebook
                    button {
                        class: "welcome-path-card primary",
                        onclick: move |_| on_get_started.call(()),
                        div { class: "welcome-path-icon", "\u{270F}" }
                        div { class: "welcome-path-content",
                            if has_existing_vault {
                                span { class: "welcome-path-title", "Open your notebook" }
                                span { class: "welcome-path-desc",
                                    "Return to your notes."
                                }
                            } else {
                                span { class: "welcome-path-title", "Start writing" }
                                span { class: "welcome-path-desc",
                                    "Create a notebook in your Documents folder. You can set up sync later."
                                }
                            }
                        }
                    }

                    // Path 2: Sync across devices (tier 2/3)
                    button {
                        class: "welcome-path-card",
                        onclick: move |_| {
                            let v = *show_sync_options.read();
                            *show_sync_options.write() = !v;
                        },
                        div { class: "welcome-path-icon", "\u{1F504}" }
                        div { class: "welcome-path-content",
                            span { class: "welcome-path-title", "Sync across devices" }
                            span { class: "welcome-path-desc",
                                "Keep your notes in sync between your Mac, iPhone, and other computers."
                            }
                        }
                    }

                    if *show_sync_options.read() {
                        div { class: "welcome-sync-options",

                            // ── Cloud providers (Ethel) ─────────────────
                            {
                                let providers = flynt_store::sync::cloud::detect_providers();
                                if !providers.is_empty() {
                                    rsx! {
                                        div { class: "welcome-sync-group",
                                            span { class: "welcome-sync-group-label", "Your cloud storage" }
                                            div { class: "welcome-cloud-grid",
                                                for provider in providers {
                                                    {
                                                        let label = provider.label.to_string();
                                                        let desc = provider.description.to_string();
                                                        rsx! {
                                                            button {
                                                                class: "welcome-cloud-btn",
                                                                onclick: move |_| {
                                                                    let vault_path = flynt_store::sync::cloud::vault_path_for_provider(&provider, "Flynt");
                                                                    if vault_path.join(".flynt").exists() {
                                                                        // Already exists — just open it
                                                                        on_cloud_vault.call(vault_path);
                                                                    } else {
                                                                        match flynt_store::sync::cloud::create_cloud_vault(&provider, "Flynt") {
                                                                            Ok(root) => on_cloud_vault.call(root),
                                                                            Err(e) => tracing::error!("Cloud vault failed: {e}"),
                                                                        }
                                                                    }
                                                                },
                                                                span { class: "welcome-cloud-label", "{label}" }
                                                                span { class: "welcome-cloud-desc", "{desc}" }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    rsx! {}
                                }
                            }

                            // ── Git hosting (tier 1 + 2) ────────────────
                            div { class: "welcome-sync-group",
                                if has_cloud {
                                    button {
                                        class: "welcome-sync-group-toggle",
                                        onclick: move |_| {
                                            let v = *show_git_options.read();
                                            *show_git_options.write() = !v;
                                        },
                                        span { class: "welcome-sync-group-label",
                                            if *show_git_options.read() { "More options \u{25B4}" } else { "More options \u{25BE}" }
                                        }
                                    }
                                } else {
                                    span { class: "welcome-sync-group-label", "Online sync" }
                                }

                                if *show_git_options.read() {
                                    div { class: "welcome-git-expanded",
                                        // Clone form for people who already have an account
                                        button {
                                            class: "welcome-option",
                                            onclick: move |_| on_clone_remote.call(()),
                                            span { class: "welcome-option-title", "Connect a notebook" }
                                            span { class: "welcome-option-desc",
                                                "I have an account and a notebook URL"
                                            }
                                        }

                                        // Education + signup for people who don't
                                        div { class: "welcome-git-explainer",
                                            p { class: "welcome-git-what",
                                                "Don't have an account? Git hosting keeps a complete history of your notes "
                                                "and is the most reliable way to sync across all your devices. "
                                                "Create a free account to get started:"
                                            }
                                            div { class: "welcome-git-providers",
                                                button {
                                                    class: "welcome-git-btn",
                                                    onclick: move |_| {
                                                        let _ = open::that("https://codeberg.org/repo/create");
                                                    },
                                                    span { class: "welcome-git-label", "Codeberg" }
                                                    span { class: "welcome-git-hint", "Open source, community-run" }
                                                }
                                                button {
                                                    class: "welcome-git-btn",
                                                    onclick: move |_| {
                                                        let _ = open::that("https://github.com/new");
                                                    },
                                                    span { class: "welcome-git-label", "GitHub" }
                                                    span { class: "welcome-git-hint", "Largest platform, free for personal use" }
                                                }
                                            }
                                            p { class: "welcome-git-after",
                                                "Create an account (or sign in), then create a new repository. "
                                                "Copy the URL and click \"Connect a notebook\" above."
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Path 3: Join a shared vault (tier 2/3)
                    button {
                        class: "welcome-path-card",
                        onclick: move |_| on_clone_remote.call(()),
                        div { class: "welcome-path-icon", "\u{1F91D}" }
                        div { class: "welcome-path-content",
                            span { class: "welcome-path-title", "Join a shared vault" }
                            span { class: "welcome-path-desc",
                                "Someone shared a vault with you? Paste the link they gave you."
                            }
                        }
                    }
                }

                // ── Advanced (tier 1) ─────────────────────────────────
                div { class: "welcome-advanced",
                    button {
                        class: "welcome-toggle-advanced",
                        onclick: move |_| { let v = *show_advanced.read(); *show_advanced.write() = !v; },
                        if *show_advanced.read() {
                            "Hide advanced options \u{25B4}"
                        } else {
                            "Advanced \u{25BE}"
                        }
                    }

                    if *show_advanced.read() {
                        div { class: "welcome-options",
                            button {
                                class: "welcome-option",
                                onclick: move |_| on_choose_existing.call(()),
                                span { class: "welcome-option-title", "Open an existing folder" }
                                span { class: "welcome-option-desc",
                                    "Use a folder of markdown files you already have (Obsidian vaults work too)"
                                }
                            }
                            button {
                                class: "welcome-option",
                                onclick: move |_| on_clone_remote.call(()),
                                span { class: "welcome-option-title", "Clone from Git" }
                                span { class: "welcome-option-desc",
                                    "Clone a repository and keep it synced"
                                }
                            }
                            button {
                                class: "welcome-option",
                                onclick: move |_| on_import_markdown.call(()),
                                span { class: "welcome-option-title", "Import markdown files" }
                                span { class: "welcome-option-desc",
                                    "Copy files from another location into a new notebook"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
