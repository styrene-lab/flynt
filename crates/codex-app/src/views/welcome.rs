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
    let has_pending_setup = launcher_profile.pending_setup.is_some();

    rsx! {
        div { class: "view-welcome",
            div { class: "welcome-shell",
                h1 { class: "view-heading", "Welcome to Codex" }
                p {
                    class: "placeholder",
                    "Markdown is the canonical knowledge kernel. Start from an existing vault, create a new vault, or import reference material into Codex."
                }

                div { class: "welcome-actions",
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| on_choose_existing.call(()),
                        "Open existing vault"
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| on_create_local.call(()),
                        "Create local vault"
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| on_link_github.call(()),
                        "Link GitHub remote"
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| on_import_markdown.call(()),
                        "Import markdown vault"
                    }
                }

                div { class: "welcome-status muted",
                    if let Some(last_root) = launcher_profile.last_vault_root.as_ref() {
                        p { "Last vault: {last_root.display()}" }
                    } else {
                        p { "No vault selected yet." }
                    }
                    p { "Recent vaults: {recent_count}" }
                    if has_pending_setup {
                        match launcher_profile.pending_setup.as_ref().unwrap() {
                            PendingVaultSetup::OpenExisting { path } => rsx! { p { "Pending: open {path.display()}" } },
                            PendingVaultSetup::CreateLocal { path, name } => rsx! { p { "Pending: create {name} at {path.display()}" } },
                            PendingVaultSetup::LinkGithub { local_path, repo, branch } => rsx! {
                                p { "Pending: link {repo} ({branch}) at {local_path.display()}" }
                            },
                        }
                    }
                }
            }
        }
    }
}
