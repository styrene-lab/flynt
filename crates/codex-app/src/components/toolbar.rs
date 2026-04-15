use dioxus::prelude::*;
use crate::state::{Route, SyncStatus};

#[component]
pub fn Toolbar(sync_status: SyncStatus, mut show_agent: Signal<bool>) -> Element {
    let status_class = match &sync_status {
        SyncStatus::Idle => "sync-status",
        SyncStatus::Syncing => "sync-status syncing",
        SyncStatus::Conflict(_) => "sync-status conflict",
    };
    let status_label = match &sync_status {
        SyncStatus::Idle => "Idle".to_string(),
        SyncStatus::Syncing => "Syncing…".to_string(),
        SyncStatus::Conflict(n) => format!("{n} conflict{}", if *n == 1 { "" } else { "s" }),
    };

    rsx! {
        div { class: "toolbar",
            span { class: "app-title", "Codex" }
            span { class: status_class, "{status_label}" }
            button {
                class: "agent-toggle",
                onclick: move |_| {
                    let current = *show_agent.read();
                    *show_agent.write() = !current;
                },
                "Omegon"
            }
        }
    }
}
