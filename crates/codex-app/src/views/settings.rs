use crate::{
    bootstrap::AppContext,
    state::ThemeName,
};
use codex_core::models::{
    AppearanceConfig, CodexOperatorSettings, FontSizePreset, OmegonProfile, OmegonProfileModel,
    SyncConfig, VaultConfig,
};
use dioxus::prelude::*;

// ── Theme catalogue ───────────────────────────────────────────────────────────
// Each entry describes a theme well enough to render a preview card without
// activating it. Hex values here are display-only — component CSS still uses vars.

#[derive(PartialEq, Eq)]
struct ThemeEntry {
    id: &'static str,
    label: &'static str,
    bg: &'static str,
    surface: &'static str,
    primary: &'static str,
    text: &'static str,
}

const THEMES: &[ThemeEntry] = &[
    ThemeEntry {
        id: "alpharius",
        label: "Alpharius",
        bg: "#06080e",
        surface: "#0e1622",
        primary: "#2ab4c8",
        text: "#c4d8e4",
    },
    // Future themes registered here; CSS file added to app.css @imports.
];

// ── Settings view ─────────────────────────────────────────────────────────────

#[component]
pub fn SettingsView() -> Element {
    let ctx = use_context::<AppContext>();

    // Appearance — reactive, applied immediately via context signals.
    let mut theme = use_context::<Signal<ThemeName>>();
    let mut font_sz = use_context::<Signal<FontSizePreset>>();

    // Vault + sync — local form state; persisted on explicit Save.
    let mut vault_name = use_signal(|| ctx.vault.config.vault_name.clone());
    let mut sync_config = use_signal(|| ctx.vault.config.sync.clone());

    let mut project_profile_state = use_context::<Signal<OmegonProfile>>();
    let mut operator_settings_state = use_context::<Signal<CodexOperatorSettings>>();
    let initial_profile = project_profile_state.read().clone();
    let initial_operator = operator_settings_state.read().clone();

    // Omegon-compatible persisted profile.
    let mut model_provider = use_signal(|| {
        initial_profile
            .last_used_model
            .as_ref()
            .map(|model| model.provider.clone())
            .unwrap_or_default()
    });
    let mut model_id = use_signal(|| {
        initial_profile
            .last_used_model
            .as_ref()
            .map(|model| model.model_id.clone())
            .unwrap_or_default()
    });
    let mut thinking_level =
        use_signal(|| initial_profile.thinking_level.clone().unwrap_or_default());
    let mut max_turns = use_signal(|| {
        initial_profile
            .max_turns
            .map(|turns| turns.to_string())
            .unwrap_or_default()
    });

    // Codex-owned operator preferences.
    let mut active_persona = use_signal(|| initial_operator.active_persona.clone());
    let mut rail_extension = use_signal(|| initial_operator.rail_extension.clone());
    let mut vox_enabled = use_signal(|| initial_operator.vox.enabled);
    let mut vox_tts_enabled = use_signal(|| initial_operator.vox.tts_enabled);
    let mut vox_voice = use_signal(|| initial_operator.vox.voice.clone());

    let mut save_msg = use_signal(|| Option::<(&'static str, &'static str)>::None);

    let vault = ctx.vault.clone();
    let local_runtime = ctx.vault.config.local_runtime.clone();
    let omegon = ctx.omegon.clone();
    let save = move |_| {
        let config = VaultConfig {
            vault_name: vault_name.read().clone(),
            sync: sync_config.read().clone(),
            appearance: AppearanceConfig {
                theme: theme.read().0.clone(),
                font_size: *font_sz.read(),
            },
            local_runtime: local_runtime.clone(),
        };

        let last_used_model = if model_provider.read().trim().is_empty() || model_id.read().trim().is_empty() {
            None
        } else {
            Some(OmegonProfileModel {
                provider: model_provider.read().trim().to_string(),
                model_id: model_id.read().trim().to_string(),
            })
        };

        let thinking_level_value = {
            let value = thinking_level.read().clone();
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        };

        let max_turns_value = {
            let value = max_turns.read().clone();
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                match value.parse::<u32>() {
                    Ok(parsed) => Some(parsed),
                    Err(_) => {
                        *save_msg.write() = Some(("err", "Max turns must be a whole number."));
                        return;
                    }
                }
            }
        };

        let profile = OmegonProfile {
            last_used_model,
            thinking_level: thinking_level_value,
            max_turns: max_turns_value,
            ..OmegonProfile::default()
        };

        let operator_settings = CodexOperatorSettings {
            active_persona: active_persona.read().trim().to_string(),
            enabled_skills: initial_operator.enabled_skills.clone(),
            preferred_extensions: initial_operator.preferred_extensions.clone(),
            rail_extension: rail_extension.read().trim().to_string(),
            vox: codex_core::models::VoxSettings {
                enabled: *vox_enabled.read(),
                tts_enabled: *vox_tts_enabled.read(),
                voice: vox_voice.read().trim().to_string(),
            },
        };

        match vault.save_config(&config) {
            Ok(()) => {}
            Err(e) => {
                tracing::error!("save_config: {e}");
                *save_msg.write() = Some(("err", "Save failed — check logs."));
                return;
            }
        }

        if let Err(e) = omegon.save_project_profile(&profile) {
            tracing::error!("save_project_profile: {e}");
            *save_msg.write() = Some(("err", "Profile save failed — check logs."));
            return;
        }

        if let Err(e) = omegon.save_operator_settings(&operator_settings) {
            tracing::error!("save_operator_settings: {e}");
            *save_msg.write() = Some(("err", "Operator settings save failed — check logs."));
            return;
        }

        *project_profile_state.write() = profile;
        *operator_settings_state.write() = operator_settings;
        *save_msg.write() = Some(("ok", "Settings saved."));
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

                    if let SyncConfig::Git { remote, branch, auto_commit_seconds } = sync_config.read().clone() {
                        SettingsRow { label: "Remote URL",
                            input {
                                class: "input settings-input",
                                r#type: "text",
                                value: "{remote}",
                                oninput: move |e| {
                                    if let SyncConfig::Git { ref mut remote, .. } = *sync_config.write() {
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
                                    if let SyncConfig::Git { ref mut branch, .. } = *sync_config.write() {
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
                                    if let SyncConfig::Git { ref mut auto_commit_seconds, .. } = *sync_config.write() {
                                        *auto_commit_seconds = secs;
                                    }
                                },
                            }
                            span { class: "settings-hint muted", "(0 = manual only)" }
                        }
                    }
                }

                // ── Omegon profile ───────────────────────────────────────────
                SettingsSection { heading: "Omegon profile",
                    SettingsRow { label: "Model provider",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{model_provider}",
                            placeholder: "anthropic",
                            oninput: move |e| *model_provider.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Model ID",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{model_id}",
                            placeholder: "claude-sonnet-4-6",
                            oninput: move |e| *model_id.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Thinking level",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{thinking_level}",
                            placeholder: "medium",
                            oninput: move |e| *thinking_level.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Max turns",
                        input {
                            class: "input settings-input settings-input-narrow",
                            r#type: "number",
                            min: "1",
                            value: "{max_turns}",
                            placeholder: "24",
                            oninput: move |e| *max_turns.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Project profile",
                        span { class: "settings-path muted", "{ctx.omegon.project_profile_path.display()}" }
                    }
                }

                // ── Operator ─────────────────────────────────────────────────
                SettingsSection { heading: "Operator",
                    SettingsRow { label: "Active persona",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{active_persona}",
                            placeholder: "off",
                            oninput: move |e| *active_persona.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Rail extension",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{rail_extension}",
                            placeholder: "vox",
                            oninput: move |e| *rail_extension.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Vox enabled",
                        input {
                            r#type: "checkbox",
                            checked: *vox_enabled.read(),
                            onchange: move |_| {
                                let current = *vox_enabled.read();
                                *vox_enabled.write() = !current;
                            },
                        }
                    }
                    SettingsRow { label: "Vox TTS",
                        input {
                            r#type: "checkbox",
                            checked: *vox_tts_enabled.read(),
                            onchange: move |_| {
                                let current = *vox_tts_enabled.read();
                                *vox_tts_enabled.write() = !current;
                            },
                        }
                    }
                    SettingsRow { label: "Vox voice",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{vox_voice}",
                            placeholder: "default",
                            oninput: move |e| *vox_voice.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Operator settings",
                        span { class: "settings-path muted", "{ctx.omegon.operator_settings_path.display()}" }
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
fn ThemeCard(entry: &'static ThemeEntry, active: bool, on_select: EventHandler<String>) -> Element {
    rsx! {
        button {
            class: if active { "theme-card active" } else { "theme-card" },
            onclick: move |_| on_select.call(entry.id.to_string()),
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
fn SyncRadio(label: &'static str, active: bool, on_select: EventHandler<()>) -> Element {
    rsx! {
        button {
            class: if active { "radio-btn active" } else { "radio-btn" },
            onclick: move |_| on_select.call(()),
            div { class: if active { "radio-dot active" } else { "radio-dot" } }
            "{label}"
        }
    }
}
