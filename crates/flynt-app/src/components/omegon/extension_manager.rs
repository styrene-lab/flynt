//! Extension manager — list extensions, render schema-driven config and secret UIs.
//!
//! Combines filesystem scanning (for install/remove) with ACP `_extensions/list`
//! (for config schema + secret status) to provide a unified extensions panel.

use crate::acp::AcpSession;
use crate::bootstrap::AppContext;
use crate::components::omegon::extension_config::{ExtensionConfigPanel, parse_extensions_list};
use dioxus::prelude::*;
use std::rc::Rc;

/// Parsed extension manifest (minimal fields for display).
#[derive(Debug, Clone)]
struct ExtensionInfo {
    name: String,
    version: String,
    description: String,
}

fn discover_extensions(extensions_dir: &std::path::Path) -> Vec<ExtensionInfo> {
    let mut extensions = Vec::new();
    let Ok(entries) = std::fs::read_dir(extensions_dir) else {
        return extensions;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest_path = dir.join("manifest.toml");
        if !manifest_path.exists() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(parsed) = raw.parse::<toml::Table>() else {
            continue;
        };
        let ext = parsed.get("extension").and_then(|v| v.as_table());
        let name = ext
            .and_then(|e| e.get("name").and_then(|v| v.as_str()))
            .unwrap_or_else(|| {
                dir.file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or("unknown")
            })
            .to_string();
        let version = ext
            .and_then(|e| e.get("version").and_then(|v| v.as_str()))
            .unwrap_or("?")
            .to_string();
        let description = ext
            .and_then(|e| e.get("description").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();

        extensions.push(ExtensionInfo {
            name,
            version,
            description,
        });
    }
    extensions.sort_by(|a, b| a.name.cmp(&b.name));
    extensions
}

#[component]
pub fn ExtensionManagerSection() -> Element {
    let ctx = use_context::<AppContext>();
    let mut refresh = use_signal(|| 0u64);
    let shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();

    let extensions = use_resource(move || {
        let _ = refresh.read();
        let extensions_dir = ctx.omegon().extensions_dir.clone();
        async move {
            tokio::task::spawn_blocking(move || discover_extensions(&extensions_dir))
                .await
                .unwrap_or_default()
        }
    });

    // Fetch ACP extension data (config schemas + secret status)
    let acp_data = use_resource(move || {
        let _ = refresh.read();
        let sess = shared_session.read().clone();
        async move {
            if let Some(s) = sess {
                s.extensions_list().await.ok()
            } else {
                None
            }
        }
    });

    let ext_data_list = acp_data
        .read()
        .as_ref()
        .and_then(|opt| opt.as_ref())
        .map(|v| parse_extensions_list(v))
        .unwrap_or_default();

    let mut action_msg: Signal<Option<(&'static str, String)>> = use_signal(|| None);
    let mut install_uri = use_signal(String::new);
    let mut show_install = use_signal(|| false);

    // For the empty-state CTA — clicking "Browse the Armory" should
    // flip the active settings page to Armory.
    let mut settings_page = use_context::<Signal<crate::state::SettingsPage>>();

    let installed_count = extensions.read().as_ref().map(|v| v.len()).unwrap_or(0);

    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "Extensions" }

            if installed_count == 0 && extensions.read().is_some() {
                // Empty state: no extensions installed yet. Surface the
                // Armory rather than leave the operator staring at a
                // bare panel wondering what to do.
                div { class: "extensions-empty-state",
                    div { class: "extensions-empty-icon", "\u{1F4E6}" }
                    h3 { class: "extensions-empty-title", "No extensions installed" }
                    p { class: "extensions-empty-body",
                        "Extensions add tools, integrations, and data sources to omegon — anything from a Linear connector to a custom code-search backend. Browse the Armory to install one."
                    }
                    div { class: "extensions-empty-actions",
                        button {
                            class: "btn btn-primary",
                            onclick: move |_| {
                                *settings_page.write() = crate::state::SettingsPage::OmegonArmory;
                            },
                            "Browse the Armory"
                        }
                        button {
                            class: "btn btn-ghost",
                            onclick: move |_| { *show_install.write() = true; },
                            "Install by URI\u{2026}"
                        }
                    }
                }
            }

            div { class: "settings-rows",
                for ext in extensions.read().as_ref().unwrap_or(&vec![]).iter() {
                    {
                        let ext_data = ext_data_list.iter().find(|d| d.name == ext.name).cloned();
                        let is_enabled = ext_data.as_ref().map(|d| d.enabled).unwrap_or(true);
                        rsx! {
                            div { class: if is_enabled { "extension-card" } else { "extension-card extension-card-disabled" },
                                div { class: "extension-card-header",
                                    div { class: "extension-card-identity",
                                        span { class: "extension-card-name", "{ext.name}" }
                                        span { class: "provider-status-text", "v{ext.version}" }
                                        if !is_enabled {
                                            span { class: "extension-card-badge disabled", "disabled" }
                                        }
                                    }
                                    div { class: "extension-card-actions",
                                        if is_enabled {
                                            button {
                                                class: "btn btn-ghost btn-sm",
                                                onclick: {
                                                    let name = ext.name.clone();
                                                    let sess = shared_session.read().clone();
                                                    move |_| {
                                                        let name = name.clone();
                                                        let sess = sess.clone();
                                                        spawn(async move {
                                                            if let Some(s) = sess {
                                                                let _ = s.extensions_disable(&name).await;
                                                                *refresh.write() += 1;
                                                            }
                                                        });
                                                    }
                                                },
                                                "Disable"
                                            }
                                        } else {
                                            button {
                                                class: "btn btn-ghost btn-sm",
                                                onclick: {
                                                    let name = ext.name.clone();
                                                    let sess = shared_session.read().clone();
                                                    move |_| {
                                                        let name = name.clone();
                                                        let sess = sess.clone();
                                                        spawn(async move {
                                                            if let Some(s) = sess {
                                                                let _ = s.extensions_enable(&name).await;
                                                                *refresh.write() += 1;
                                                            }
                                                        });
                                                    }
                                                },
                                                "Enable"
                                            }
                                        }
                                        button {
                                            class: "btn btn-ghost btn-sm provider-remove-btn",
                                            onclick: {
                                                let name = ext.name.clone();
                                                let sess = shared_session.read().clone();
                                                move |_| {
                                                    let name = name.clone();
                                                    let sess = sess.clone();
                                                    spawn(async move {
                                                        if let Some(s) = sess {
                                                            match s.extensions_remove(&name).await {
                                                                Ok(_) => *refresh.write() += 1,
                                                                Err(e) => *action_msg.write() = Some(("err", format!("Remove failed: {e}"))),
                                                            }
                                                        }
                                                    });
                                                }
                                            },
                                            "Remove"
                                        }
                                    }
                                }
                                if !ext.description.is_empty() {
                                    span { class: "settings-hint muted", "{ext.description}" }
                                }
                                if let Some(data) = ext_data {
                                    ExtensionConfigPanel {
                                        ext: data,
                                        session: shared_session,
                                    }
                                }
                            }
                        }
                    }
                }
                if extensions.read().as_ref().map(|v| v.is_empty()).unwrap_or(true) {
                    div { class: "settings-row",
                        span { class: "settings-hint muted", "No extensions installed" }
                    }
                }
            }

            // ── Install / Update actions ──
            div { class: "extension-actions",
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        let v = *show_install.read();
                        *show_install.write() = !v;
                    },
                    if *show_install.read() { "Cancel" } else { "Install extension" }
                }
                button {
                    class: "btn btn-ghost",
                    onclick: {
                        let sess = shared_session.read().clone();
                        move |_| {
                            let sess = sess.clone();
                            spawn(async move {
                                if let Some(s) = sess {
                                    match s.extensions_update(None).await {
                                        Ok(_) => {
                                            *action_msg.write() = Some(("ok", "Extensions updated".into()));
                                            *refresh.write() += 1;
                                        }
                                        Err(e) => *action_msg.write() = Some(("err", format!("Update failed: {e}"))),
                                    }
                                }
                            });
                        }
                    },
                    "Update all"
                }
                if let Some((kind, msg)) = &*action_msg.read() {
                    span {
                        class: if *kind == "ok" { "save-msg ok" } else { "save-msg err" },
                        "{msg}"
                    }
                }
            }
            if *show_install.read() {
                div { class: "extension-install-form",
                    input {
                        class: "input settings-input",
                        r#type: "text",
                        placeholder: "Local path, git URL, or .tar.gz",
                        value: "{install_uri}",
                        oninput: move |e| *install_uri.write() = e.value(),
                    }
                    button {
                        class: "btn btn-primary btn-sm",
                        onclick: {
                            let sess = shared_session.read().clone();
                            move |_| {
                                let uri = install_uri.read().clone();
                                let sess = sess.clone();
                                if uri.trim().is_empty() { return; }
                                spawn(async move {
                                    if let Some(s) = sess {
                                        match s.extensions_install(&uri).await {
                                            Ok(_) => {
                                                *action_msg.write() = Some(("ok", format!("Installed from {uri}")));
                                                *install_uri.write() = String::new();
                                                *show_install.write() = false;
                                                *refresh.write() += 1;
                                            }
                                            Err(e) => *action_msg.write() = Some(("err", format!("Install failed: {e}"))),
                                        }
                                    }
                                });
                            }
                        },
                        "Install"
                    }
                    span { class: "settings-hint muted", "Accepts local paths, git URLs (https/ssh), or .tar.gz archives" }
                }
            }
        }
    }
}
