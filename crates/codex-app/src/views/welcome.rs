use crate::bootstrap::{LauncherProfile, PendingVaultSetup};
use dioxus::prelude::*;

#[component]
pub fn WelcomeView(
    launcher_profile: LauncherProfile,
    on_choose_existing: EventHandler<()>,
    on_create_local: EventHandler<()>,
    on_clone_remote: EventHandler<()>,
    on_import_markdown: EventHandler<()>,
    on_seed_demo_publication: EventHandler<()>,
) -> Element {
    let recent_count = launcher_profile.recent_vaults.len();
    let pending_label = launcher_profile.pending_setup.as_ref().map(|pending| match pending {
        PendingVaultSetup::OpenExisting { path } => format!("Open {}", path.display()),
        PendingVaultSetup::CreateLocal { path, name } => format!("Create {name} at {}", path.display()),
        PendingVaultSetup::LinkGithub { local_path, repo, branch } => {
            format!("Clone {repo} ({branch}) -> {}", local_path.display())
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
                        "Start from an existing vault, create a new one, clone a remote, or import markdown. Your notes stay as plain markdown — always."
                    }
                }

                div { class: "welcome-grid",
                    WelcomeCard {
                        primary: true,
                        icon: ">>",
                        title: "Open existing vault",
                        body: "Adopt an existing Obsidian or Codex vault in place. Codex indexes it locally and keeps markdown as the source of truth.",
                        action: "Choose folder",
                        meta: "Best for migrating from Obsidian or another markdown editor",
                        on_click: move |_| on_choose_existing.call(()),
                    }
                    WelcomeCard {
                        primary: false,
                        icon: "+",
                        title: "Create local vault",
                        body: "Start a fresh local markdown knowledge base. You can add Git sync later from Settings.",
                        action: "Create vault",
                        meta: "Local-first, no account required",
                        on_click: move |_| on_create_local.call(()),
                    }
                    WelcomeCard {
                        primary: false,
                        icon: "",
                        title: "Clone remote vault",
                        body: "Clone an existing Git repository as your vault. Keeps devices in sync automatically via background commits.",
                        action: "Clone repo",
                        meta: "Requires SSH key or Git credentials configured on this machine",
                        on_click: move |_| on_clone_remote.call(()),
                    }
                    WelcomeCard {
                        primary: false,
                        icon: "<-",
                        title: "Import markdown folder",
                        body: "Copy markdown files from another location into your vault while preserving wikilinks and structure.",
                        action: "Import",
                        meta: "Non-destructive copy into current vault",
                        on_click: move |_| on_import_markdown.call(()),
                    }
                    WelcomeCard {
                        primary: false,
                        icon: ":::",
                        title: "Seed demo publication",
                        body: "Scaffold an Astro-based site that publishes from a Codex vault. Good for seeing what publication looks like.",
                        action: "Seed demo",
                        meta: "Creates a publication target repo",
                        on_click: move |_| on_seed_demo_publication.call(()),
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
