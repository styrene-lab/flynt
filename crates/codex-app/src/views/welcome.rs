use crate::bootstrap::{LauncherProfile, PendingVaultSetup};
use dioxus::prelude::*;

#[component]
pub fn WelcomeView(
    launcher_profile: LauncherProfile,
    on_get_started: EventHandler<()>,
    on_choose_existing: EventHandler<()>,
    on_clone_remote: EventHandler<()>,
    on_import_markdown: EventHandler<()>,
) -> Element {
    let mut show_advanced = use_signal(|| false);

    rsx! {
        div { class: "view-welcome",
            div { class: "welcome-shell",
                div { class: "welcome-hero",
                    h1 { class: "welcome-title", "Welcome to Codex" }
                    p {
                        class: "welcome-subtitle",
                        "A place for your notes, ideas, and projects. Everything is saved as plain files on your computer."
                    }
                }

                // Primary CTA — one big button
                div { class: "welcome-primary",
                    button {
                        class: "welcome-start-btn",
                        onclick: move |_| on_get_started.call(()),
                        "Get started"
                    }
                    p { class: "welcome-start-hint", "Creates a notebook in your Documents folder" }
                }

                // Advanced options — hidden by default
                div { class: "welcome-advanced",
                    button {
                        class: "welcome-toggle-advanced",
                        onclick: move |_| { let v = *show_advanced.read(); *show_advanced.write() = !v; },
                        if *show_advanced.read() {
                            "Hide options \u{25B4}"
                        } else {
                            "I already have notes \u{25BE}"
                        }
                    }

                    if *show_advanced.read() {
                        div { class: "welcome-options",
                            button {
                                class: "welcome-option",
                                onclick: move |_| on_choose_existing.call(()),
                                span { class: "welcome-option-title", "Open an existing folder" }
                                span { class: "welcome-option-desc", "Use a folder of markdown files you already have (works with Obsidian vaults)" }
                            }
                            button {
                                class: "welcome-option",
                                onclick: move |_| on_clone_remote.call(()),
                                span { class: "welcome-option-title", "Clone from Git" }
                                span { class: "welcome-option-desc", "Download a notebook from a Git repository and keep it synced" }
                            }
                            button {
                                class: "welcome-option",
                                onclick: move |_| on_import_markdown.call(()),
                                span { class: "welcome-option-title", "Import markdown files" }
                                span { class: "welcome-option-desc", "Copy files from another location into a new notebook" }
                            }
                        }
                    }
                }
            }
        }
    }
}
