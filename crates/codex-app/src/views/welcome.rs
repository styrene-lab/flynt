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
    let has_existing_vault = launcher_profile.last_vault_root.is_some()
        || !launcher_profile.known_vaults.is_empty();

    rsx! {
        div { class: "view-welcome",
            div { class: "welcome-shell",
                div { class: "welcome-hero",
                    h1 { class: "welcome-title", "Codex" }
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
                            // Auto-detected cloud providers — zero config
                            for provider in codex_store::sync::cloud::detect_providers() {
                                {
                                    let label = provider.label.to_string();
                                    let desc = provider.description.to_string();
                                    rsx! {
                                        button {
                                            class: "welcome-option",
                                            onclick: move |_| {
                                                match codex_store::sync::cloud::create_cloud_vault(&provider, "Codex") {
                                                    Ok(root) => on_cloud_vault.call(root),
                                                    Err(e) => tracing::error!("Cloud vault failed: {e}"),
                                                }
                                            },
                                            span { class: "welcome-option-title", "{label}" }
                                            span { class: "welcome-option-desc", "{desc}" }
                                        }
                                    }
                                }
                            }
                            div { class: "welcome-git-section",
                                span { class: "welcome-option-title", "Git hosting" }
                                span { class: "welcome-option-desc",
                                    "Create a free account, then connect your notebook."
                                }
                                div { class: "welcome-git-providers",
                                    button {
                                        class: "welcome-git-btn",
                                        onclick: move |_| {
                                            let _ = open::that("https://codeberg.org/user/login");
                                        },
                                        span { class: "welcome-git-label", "Codeberg" }
                                        span { class: "welcome-git-hint", "Open source, community-run" }
                                    }
                                    button {
                                        class: "welcome-git-btn",
                                        onclick: move |_| {
                                            let _ = open::that("https://github.com/signup");
                                        },
                                        span { class: "welcome-git-label", "GitHub" }
                                        span { class: "welcome-git-hint", "Largest hosting platform" }
                                    }
                                }
                                button {
                                    class: "welcome-option compact",
                                    onclick: move |_| on_clone_remote.call(()),
                                    span { class: "welcome-option-title", "I already have an account" }
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
