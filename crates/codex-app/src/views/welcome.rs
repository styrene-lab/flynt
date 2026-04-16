use crate::bootstrap::{LauncherProfile, PendingVaultSetup};
use dioxus::prelude::*;

#[component]
pub fn WelcomeView(
    launcher_profile: LauncherProfile,
    on_choose_existing: EventHandler<()>,
    on_create_local: EventHandler<()>,
    on_link_github: EventHandler<()>,
    on_import_markdown: EventHandler<()>,
) -> Element {
    let recent_count = launcher_profile.recent_vaults.len();
    let pending_label = launcher_profile.pending_setup.as_ref().map(|pending| match pending {
        PendingVaultSetup::OpenExisting { path } => format!("Open {}", path.display()),
        PendingVaultSetup::CreateLocal { path, name } => format!("Create {name} at {}", path.display()),
        PendingVaultSetup::LinkGithub { local_path, repo, branch } => {
            format!("Link {repo} ({branch}) at {}", local_path.display())
        }
        PendingVaultSetup::PublishPreview { output_path, repo, branch } => {
            format!("Preview at {} for {repo} ({branch})", output_path.display())
        }
        PendingVaultSetup::SeedDemoPublication { repo_root, site_name } => {
            format!("Seeded {site_name} demo at {}", repo_root.display())
        }
    });

    rsx! {
        div { class: "view-welcome",
            div { class: "welcome-shell",
                div { class: "welcome-hero",
                    div { class: "welcome-kicker", "Knowledge kernel" }
                    h1 { class: "welcome-title", "Welcome to Codex" }
                    p {
                        class: "welcome-subtitle",
                        "Start from an existing Black Meridian or Obsidian vault, create a clean local vault, link a GitHub-backed remote, or import markdown references into Codex. Markdown remains the canonical reasoning layer."
                    }
                }

                div { class: "welcome-grid",
                    WelcomeCard {
                        primary: true,
                        icon: "📂",
                        title: "Open existing vault",
                        body: "Adopt an existing Obsidian or Codex vault in place. Codex will index it locally and keep markdown as the source of truth.",
                        action: "Choose folder",
                        meta: "Best for Black Meridian or any existing knowledge base",
                        on_click: move |_| on_choose_existing.call(()),
                    }
                    WelcomeCard {
                        primary: false,
                        icon: "✍️",
                        title: "Create local vault",
                        body: "Start a fresh local markdown knowledge base with Codex defaults and local-first storage boundaries.",
                        action: "Create vault",
                        meta: "Creates a clean local vault root",
                        on_click: move |_| on_create_local.call(()),
                    }
                    WelcomeCard {
                        primary: false,
                        icon: "",
                        title: "Link GitHub remote",
                        body: "Create a local vault configured for Git sync so publication and collaboration can ride on a browser-auth GitHub workflow later.",
                        action: "Link remote",
                        meta: "Simple Git-backed remote setup",
                        on_click: move |_| on_link_github.call(()),
                    }
                    WelcomeCard {
                        primary: false,
                        icon: "⬇",
                        title: "Import markdown vault",
                        body: "Bring external markdown or Obsidian-style content into Codex as references while preserving wikilinks and provenance.",
                        action: "Import references",
                        meta: "Uses the markdown-as-truth import pipeline",
                        on_click: move |_| on_import_markdown.call(()),
                    }
                }

                div { class: "welcome-status-panel",
                    div { class: "welcome-status-card",
                        div { class: "welcome-status-label", "Last vault" }
                        div {
                            class: "welcome-status-value",
                            if let Some(last_root) = launcher_profile.last_vault_root.as_ref() {
                                "{last_root.display()}"
                            } else {
                                "No vault selected yet"
                            }
                        }
                    }
                    div { class: "welcome-status-card",
                        div { class: "welcome-status-label", "Recent vaults" }
                        div { class: "welcome-status-value", "{recent_count}" }
                    }
                    div { class: "welcome-status-card",
                        div { class: "welcome-status-label", "Pending setup" }
                        div {
                            class: "welcome-status-value",
                            if let Some(label) = pending_label.as_ref() {
                                "{label}"
                            } else {
                                "No pending action"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn WelcomeCard(
    icon: &'static str,
    title: &'static str,
    body: &'static str,
    action: &'static str,
    meta: &'static str,
    on_click: EventHandler<MouseEvent>,
    primary: bool,
) -> Element {
    rsx! {
        button {
            class: if primary { "welcome-card primary" } else { "welcome-card" },
            onclick: move |event| on_click.call(event),
            div { class: "welcome-card-icon", "{icon}" }
            div { class: "welcome-card-title", "{title}" }
            p { class: "welcome-card-body", "{body}" }
            div { class: "welcome-card-actions",
                span { class: "welcome-card-action", "{action}" }
                span { class: "welcome-card-meta", "{meta}" }
            }
        }
    }
}
