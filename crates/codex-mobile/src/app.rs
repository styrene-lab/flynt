use std::time::Duration;
use dioxus::prelude::*;
use crate::bootstrap::MobileRuntime;
use crate::views::{agent, graph, kanban, notes};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Notes,
    Board,
    Graph,
    Agent,
    Settings,
}

#[component]
pub fn App() -> Element {
    let rt = match crate::bootstrap::bootstrap() {
        Ok(rt) => rt,
        Err(e) => {
            return rsx! {
                div { class: "error-screen",
                    h1 { "Failed to open vault" }
                    p { "{e}" }
                }
            };
        }
    };

    use_context_provider(|| Signal::new(rt.clone()));

    // Poll the share-extension inbox every 5 seconds
    let vault_for_inbox = rt.vault.clone();
    use_future(move || {
        let vault = vault_for_inbox.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                match crate::bootstrap::drain_inbox(&vault) {
                    Ok(0) => {}
                    Ok(n) => tracing::info!("Imported {n} notes from share inbox"),
                    Err(e) => tracing::warn!("Inbox drain error: {e}"),
                }
            }
        }
    });

    let mut tab = use_signal(|| Tab::Notes);
    let mut selected_note: Signal<Option<String>> = use_signal(|| None);

    rsx! {
        style { {include_str!("../assets/mobile.css")} }

        div { class: "mobile-app",
            div { class: "mobile-content",
                match *tab.read() {
                    Tab::Notes => {
                        match selected_note.read().clone() {
                            Some(doc_id) => rsx! {
                                notes::NoteDetail {
                                    doc_id,
                                    on_back: move |_| *selected_note.write() = None,
                                }
                            },
                            None => rsx! {
                                notes::NotesList {
                                    on_select: move |id: String| *selected_note.write() = Some(id),
                                }
                            },
                        }
                    },
                    Tab::Board => rsx! { kanban::KanbanView {} },
                    Tab::Graph => rsx! { graph::GraphView {} },
                    Tab::Agent => rsx! { agent::AgentView {} },
                    Tab::Settings => rsx! {
                        div { class: "settings-mobile",
                            h2 { "Settings" }
                            {
                                let rt = use_context::<Signal<MobileRuntime>>();
                                let vault_name = rt.read().vault.config.vault_name.clone();
                                let sync_label = match &rt.read().vault.config.sync {
                                    codex_core::models::SyncConfig::None => "Off".to_string(),
                                    codex_core::models::SyncConfig::Git { remote, branch, .. } => {
                                        format!("{remote}/{branch}")
                                    }
                                    codex_core::models::SyncConfig::ICloud => "iCloud".to_string(),
                                    codex_core::models::SyncConfig::S3 { bucket, .. } => {
                                        format!("S3: {bucket}")
                                    }
                                };
                                rsx! {
                                    div { class: "settings-section",
                                        div { class: "settings-row",
                                            span { class: "settings-label", "Vault" }
                                            span { class: "settings-value", "{vault_name}" }
                                        }
                                        div { class: "settings-row",
                                            span { class: "settings-label", "Sync" }
                                            span { class: "settings-value", "{sync_label}" }
                                        }
                                        div { class: "settings-row",
                                            span { class: "settings-label", "Path" }
                                            span { class: "settings-value settings-path", "{rt.read().vault_root.display()}" }
                                        }
                                    }
                                }
                            }
                        }
                    },
                }
            }

            div { class: "tab-bar",
                button {
                    class: if *tab.read() == Tab::Notes { "tab-btn active" } else { "tab-btn" },
                    onclick: move |_| { *tab.write() = Tab::Notes; *selected_note.write() = None; },
                    div { class: "tab-icon", dangerous_inner_html: crate::icons::ICON_SCROLL }
                    div { class: "tab-label", "Notes" }
                }
                button {
                    class: if *tab.read() == Tab::Board { "tab-btn active" } else { "tab-btn" },
                    onclick: move |_| *tab.write() = Tab::Board,
                    div { class: "tab-icon", dangerous_inner_html: crate::icons::ICON_BOARD }
                    div { class: "tab-label", "Board" }
                }
                button {
                    class: if *tab.read() == Tab::Graph { "tab-btn active" } else { "tab-btn" },
                    onclick: move |_| *tab.write() = Tab::Graph,
                    div { class: "tab-icon", dangerous_inner_html: crate::icons::ICON_GRAPH }
                    div { class: "tab-label", "Graph" }
                }
                button {
                    class: if *tab.read() == Tab::Agent { "tab-btn active" } else { "tab-btn" },
                    onclick: move |_| *tab.write() = Tab::Agent,
                    div { class: "tab-icon", dangerous_inner_html: crate::icons::ICON_OMEGON }
                    div { class: "tab-label", "Omegon" }
                }
                button {
                    class: if *tab.read() == Tab::Settings { "tab-btn active" } else { "tab-btn" },
                    onclick: move |_| *tab.write() = Tab::Settings,
                    div { class: "tab-icon", dangerous_inner_html: crate::icons::ICON_SETTINGS }
                    div { class: "tab-label", "Settings" }
                }
            }
        }
    }
}
