use crate::bootstrap::LauncherProfile;
use dioxus::prelude::*;

#[component]
pub fn WelcomeView(
    launcher_profile: LauncherProfile,
    on_get_started: EventHandler<()>,
    on_choose_existing: EventHandler<()>,
    on_clone_remote: EventHandler<()>,
    on_import_markdown: EventHandler<()>,
    on_icloud: EventHandler<()>,
) -> Element {
    let mut show_advanced = use_signal(|| false);
    let mut show_sync_options = use_signal(|| false);

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

                    // Path 1: Just start writing (tier 3)
                    button {
                        class: "welcome-path-card primary",
                        onclick: move |_| on_get_started.call(()),
                        div { class: "welcome-path-icon", "\u{270F}" }
                        div { class: "welcome-path-content",
                            span { class: "welcome-path-title", "Start writing" }
                            span { class: "welcome-path-desc",
                                "Create a notebook in your Documents folder. You can set up sync later."
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
                            button {
                                class: "welcome-option",
                                onclick: move |_| on_icloud.call(()),
                                span { class: "welcome-option-title", "iCloud" }
                                span { class: "welcome-option-desc",
                                    "Syncs automatically between Apple devices. No account setup needed."
                                }
                            }
                            button {
                                class: "welcome-option",
                                onclick: move |_| on_clone_remote.call(()),
                                span { class: "welcome-option-title", "Online account" }
                                span { class: "welcome-option-desc",
                                    "Sync via GitHub, a Styrene Hub, or any compatible service. Works across all platforms."
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
