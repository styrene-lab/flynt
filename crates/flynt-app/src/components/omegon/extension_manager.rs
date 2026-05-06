//! Extension manager — list, install, remove Omegon extensions.
//!
//! Scans `~/.omegon/extensions/` for directories containing `manifest.toml`,
//! displays them with status badges, and provides install/remove controls.

use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use std::path::PathBuf;

/// Parsed extension manifest (minimal fields for display).
#[derive(Debug, Clone)]
struct ExtensionInfo {
    name: String,
    version: String,
    description: String,
    path: PathBuf,
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
            .unwrap_or_else(|| dir.file_name().unwrap_or_default().to_str().unwrap_or("unknown"))
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
            path: dir,
        });
    }
    extensions.sort_by(|a, b| a.name.cmp(&b.name));
    extensions
}

#[component]
pub fn ExtensionManagerSection() -> Element {
    let ctx = use_context::<AppContext>();
    let mut refresh = use_signal(|| 0u64);

    let extensions = use_resource(move || {
        let _ = refresh.read();
        let extensions_dir = ctx.omegon().extensions_dir.clone();
        async move {
            tokio::task::spawn_blocking(move || discover_extensions(&extensions_dir))
                .await
                .unwrap_or_default()
        }
    });

    let mut remove_error: Signal<Option<String>> = use_signal(|| None);

    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "Extensions" }
            div { class: "settings-rows",
                for ext in extensions.read().as_ref().unwrap_or(&vec![]).iter() {
                    div { class: "settings-row provider-row",
                        span { class: "settings-label", "{ext.name}" }
                        div { class: "settings-control",
                            div { class: "provider-status-row",
                                span { class: "provider-status authenticated" }
                                span { class: "provider-status-text", "v{ext.version}" }
                            }
                            if !ext.description.is_empty() {
                                span { class: "settings-hint muted", "{ext.description}" }
                            }
                            div { class: "row gap-2",
                                button {
                                    class: "btn btn-ghost btn-sm provider-remove-btn",
                                    onclick: {
                                        let path = ext.path.clone();
                                        let name = ext.name.clone();
                                        move |_| {
                                            match std::fs::remove_dir_all(&path) {
                                                Ok(()) => {
                                                    tracing::info!("Removed extension: {name}");
                                                    *refresh.write() += 1;
                                                }
                                                Err(e) => {
                                                    tracing::error!("Failed to remove extension {name}: {e}");
                                                    *remove_error.write() = Some(format!("Failed to remove {name}: {e}"));
                                                }
                                            }
                                        }
                                    },
                                    "Remove"
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
            if let Some(ref err) = *remove_error.read() {
                span { class: "text-error", "{err}" }
            }
        }
    }
}
