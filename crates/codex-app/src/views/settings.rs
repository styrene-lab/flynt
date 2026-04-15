use codex_core::models::{AppearanceConfig, FontSizePreset, SyncConfig, VaultConfig};
use dioxus::prelude::*;
use crate::{
    state::ThemeName,
    bootstrap::AppContext,
};

// ── Theme catalogue ───────────────────────────────────────────────────────────
// Each entry describes a theme well enough to render a preview card without
// activating it. Hex values here are display-only — component CSS still uses vars.

#[derive(PartialEq, Eq)]
struct ThemeEntry {
    id:      &'static str,
    label:   &'static str,
    bg:      &'static str,
    surface: &'static str,
    primary: &'static str,
    text:    &'static str,
}

const THEMES: &[ThemeEntry] = &[
    ThemeEntry {
        id:      "alpharius",
        label:   "Alpharius",
        bg:      "#06080e",
        surface: "#0e1622",
        primary: "#2ab4c8",
        text:    "#c4d8e4",
    },
    // Future themes registered here; CSS file added to app.css @imports.
];

// ── Settings view ─────────────────────────────────────────────────────────────

#[component]
pub fn SettingsView() -> Element {
    let ctx = use_context::<AppContext>();

    // Appearance — reactive, applied immediately via context signals.
    let mut theme    = use_context::<Signal<ThemeName>>();
    let mut font_sz  = use_context::<Signal<FontSizePreset>>();

    // Vault + sync — local form state; persisted on explicit Save.
    let mut vault_name   = use_signal(|| ctx.vault.config.vault_name.clone());
    let mut sync_config  = use_signal(|| ctx.vault.config.sync.clone());
    let mut save_msg     = use_signal(|| Option::<(&'static str, &'static str)>::None);

    // Closure: build & persist current config.
    let vault = ctx.vault.clone();
    let save = move |_| {
        let config = VaultConfig {
            vault_name:  vault_name.read().clone(),
            sync:        sync_config.read().clone(),
            appearance:  AppearanceConfig {
                theme:     theme.read().0.clone(),
                font_size: *font_sz.read(),
            },
        };
        match vault.save_config(&config) {
            Ok(())  => *save_msg.write() = Some(("ok",    "Settings saved.")),
            Err(e)  => {
                tracing::error!("save_config: {e}");
                *save_msg.write() = Some(("err", "Save failed — check logs."));
            }
        }
    };

    rsx! {
        div { class: "settings-root",
            div { class: "settings-scroll",

                // ── Appearance ───────────────────────────────────────────────
                SettingsSection { heading: "Appearance",
                    SettingsRow { label: "Theme",
                        div { class: "theme-grid",
                            for entry in THEMES {
                                ThemeCard {
                                    entry,
                                    active: theme.read().0 == entry.id,
                                    on_select: move |id: String| {
                                        *theme.write() = ThemeName(id);
                                    },
                                }
                            }
                        }
                    }

                    SettingsRow { label: "Font size",
                        div { class: "font-size-row",
                            for preset in [FontSizePreset::Small, FontSizePreset::Medium,
                                           FontSizePreset::Large, FontSizePreset::XLarge] {
                                button {
                                    class: if *font_sz.read() == preset {
                                        "font-size-btn active"
                                    } else {
                                        "font-size-btn"
                                    },
                                    onclick: move |_| *font_sz.write() = preset,
                                    "{preset.label()}"
                                }
                            }
                        }
                    }
                }

                // ── Vault ────────────────────────────────────────────────────
                SettingsSection { heading: "Vault",
                    SettingsRow { label: "Name",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{vault_name}",
                            oninput: move |e| *vault_name.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Location",
                        span { class: "settings-path muted",
                            "{ctx.vault.root.display()}"
                        }
                    }
                }

                // ── Sync ─────────────────────────────────────────────────────
                SettingsSection { heading: "Sync",
                    SettingsRow { label: "Backend",
                        div { class: "radio-group",
                            SyncRadio {
                                label: "None",
                                active: matches!(*sync_config.read(), SyncConfig::None),
                                on_select: move |_| *sync_config.write() = SyncConfig::None,
                            }
                            SyncRadio {
                                label: "iCloud",
                                active: matches!(*sync_config.read(), SyncConfig::ICloud),
                                on_select: move |_| *sync_config.write() = SyncConfig::ICloud,
                            }
                            SyncRadio {
                                label: "Git",
                                active: matches!(*sync_config.read(), SyncConfig::Git { .. }),
                                on_select: move |_| *sync_config.write() = SyncConfig::Git {
                                    remote: "origin".into(),
                                    branch: "main".into(),
                                    auto_commit_seconds: 60,
                                },
                            }
                        }
                    }

                    // Conditional Git fields
                    if let SyncConfig::Git { remote, branch, auto_commit_seconds }
                        = sync_config.read().clone()
                    {
                        SettingsRow { label: "Remote URL",
                            input {
                                class: "input settings-input",
                                r#type: "text",
                                value: "{remote}",
                                oninput: move |e| {
                                    if let SyncConfig::Git { ref mut remote, .. } =
                                        *sync_config.write()
                                    {
                                        *remote = e.value();
                                    }
                                },
                            }
                        }
                        SettingsRow { label: "Branch",
                            input {
                                class: "input settings-input",
                                r#type: "text",
                                value: "{branch}",
                                oninput: move |e| {
                                    if let SyncConfig::Git { ref mut branch, .. } =
                                        *sync_config.write()
                                    {
                                        *branch = e.value();
                                    }
                                },
                            }
                        }
                        SettingsRow { label: "Auto-commit (sec)",
                            input {
                                class: "input settings-input settings-input-narrow",
                                r#type: "number",
                                min: "0",
                                value: "{auto_commit_seconds}",
                                oninput: move |e| {
                                    let secs = e.value().parse().unwrap_or(0);
                                    if let SyncConfig::Git {
                                        ref mut auto_commit_seconds, ..
                                    } = *sync_config.write() {
                                        *auto_commit_seconds = secs;
                                    }
                                },
                            }
                            span { class: "settings-hint muted", "(0 = manual only)" }
                        }
                    }
                }

                // ── Save bar ─────────────────────────────────────────────────
                div { class: "settings-save-bar",
                    button { class: "btn btn-primary", onclick: save, "Save changes" }
                    if let Some((kind, msg)) = *save_msg.read() {
                        span {
                            class: if kind == "ok" { "save-msg ok" } else { "save-msg err" },
                            "{msg}"
                        }
                    }
                }
            }
        }
    }
}

// ── Sub-components ────────────────────────────────────────────────────────────

#[component]
fn SettingsSection(heading: &'static str, children: Element) -> Element {
    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "{heading}" }
            div { class: "settings-rows", {children} }
        }
    }
}

#[component]
fn SettingsRow(label: &'static str, children: Element) -> Element {
    rsx! {
        div { class: "settings-row",
            span { class: "settings-label", "{label}" }
            div { class: "settings-control", {children} }
        }
    }
}

#[component]
fn ThemeCard(
    entry:     &'static ThemeEntry,
    active:    bool,
    on_select: EventHandler<String>,
) -> Element {
    rsx! {
        button {
            class: if active { "theme-card active" } else { "theme-card" },
            onclick: move |_| on_select.call(entry.id.to_string()),
            // Mini colour preview — inline style is justified here: we're rendering
            // the theme's own raw hex tokens as a swatch, not styling a component.
            div {
                class: "theme-preview",
                style: "background:{entry.bg}; border-color:{entry.primary};",
                div {
                    class: "theme-preview-bar",
                    style: "background:{entry.surface};",
                }
                div {
                    class: "theme-preview-dot",
                    style: "background:{entry.primary};",
                }
                span {
                    class: "theme-preview-text",
                    style: "color:{entry.text};",
                    "Aa"
                }
            }
            span { class: "theme-name", "{entry.label}" }
            if active {
                span { class: "theme-active-badge", "✓" }
            }
        }
    }
}

#[component]
fn SyncRadio(
    label:     &'static str,
    active:    bool,
    on_select: EventHandler<()>,
) -> Element {
    rsx! {
        button {
            class: if active { "radio-btn active" } else { "radio-btn" },
            onclick: move |_| on_select.call(()),
            div { class: if active { "radio-dot active" } else { "radio-dot" } }
            "{label}"
        }
    }
}
