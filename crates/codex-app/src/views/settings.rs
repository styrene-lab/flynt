use crate::{
    bootstrap::{AppContext, OmegonRuntimeContext, PendingVaultSetup},
    state::ThemeName,
    views::PublicationRulesEditor,
};
use codex_core::models::{
    AppearanceConfig, CodexOperatorSettings, FontSizePreset, LocalRuntimeConfig,
    OmegonProfile, OmegonProfileModel, SyncConfig, VaultConfig,
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
    let mut vault_name = use_signal(|| ctx.vault().config.vault_name.clone());
    let mut sync_config = use_signal(|| ctx.vault().config.sync.clone());
    let mut local_state_root = use_signal(|| {
        ctx.vault()
            .config
            .local_runtime
            .local_state_root
            .as_ref()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .unwrap_or_default()
    });
    let mut codex_index_db_path = use_signal(|| {
        ctx.vault()
            .config
            .local_runtime
            .codex_index_db_path
            .as_ref()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .unwrap_or_default()
    });
    let mut omegon_runtime_root = use_signal(|| {
        ctx.vault()
            .config
            .local_runtime
            .omegon_runtime_root
            .as_ref()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .unwrap_or_default()
    });
    let mut omegon_mind_db_path = use_signal(|| {
        ctx.vault()
            .config
            .local_runtime
            .omegon_mind_db_path
            .as_ref()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .unwrap_or_default()
    });
    let mut styrene_identity_profile = use_signal(|| {
        ctx.vault()
            .config
            .local_runtime
            .styrene_identity_profile
            .clone()
            .unwrap_or_default()
    });

    let publication_default_visibility =
        use_signal(|| ctx.vault().config.publication.default_visibility);
    let publication_rules = use_signal(|| ctx.vault().config.publication.rules.clone());

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
    let publish_msg = use_signal(|| Option::<(&'static str, String)>::None);

    let vault = ctx.vault();
    let omegon = ctx.omegon();
    let omegon_for_save = omegon.clone();
    let publish_vault = ctx.vault();
    let mut publish_msg_signal = publish_msg;
    let publish_preview = move |_| {
        match OmegonRuntimeContext::export_publication_preview(&publish_vault) {
            Ok(output_path) => {
                let mut profile = OmegonRuntimeContext::load_launcher_profile();
                let target = OmegonRuntimeContext::publication_target(&publish_vault);
                profile.pending_setup = Some(PendingVaultSetup::PublishPreview {
                    output_path: output_path.clone(),
                    repo: target.as_ref().map(|target| target.repo.clone()).unwrap_or_default(),
                    branch: target.as_ref().map(|target| target.branch.clone()).unwrap_or_default(),
                });
                let _ = OmegonRuntimeContext::save_launcher_profile(&profile);
                *publish_msg_signal.write() = Some(("ok", format!("Local preview exported to {}", output_path.display())));
            }
            Err(err) => {
                *publish_msg_signal.write() = Some(("err", format!("Publish preview failed: {err}")));
            }
        }
    };
    let save = move |_| {
        // Validate git sync config
        if let codex_core::models::SyncConfig::Git { ref remote, ref branch, .. } = *sync_config.read() {
            if remote.trim().is_empty() {
                *save_msg.write() = Some(("err", "Git remote name cannot be empty."));
                return;
            }
            if branch.trim().is_empty() {
                *save_msg.write() = Some(("err", "Git branch name cannot be empty."));
                return;
            }
        }

        // Validate paths before saving
        for (_label, val) in [
            ("Local state root", local_state_root.read().clone()),
            ("Index DB path", codex_index_db_path.read().clone()),
            ("Omegon runtime root", omegon_runtime_root.read().clone()),
            ("Omegon mind DB path", omegon_mind_db_path.read().clone()),
        ] {
            let trimmed = val.trim().to_string();
            if !trimmed.is_empty() {
                let p = std::path::Path::new(&trimmed);
                if !p.is_absolute() {
                    *save_msg.write() = Some(("err", "Paths must be absolute (start with /)."));
                    return;
                }
            }
        }

        let local_runtime = LocalRuntimeConfig {
            local_state_root: path_from_input(local_state_root.read().as_str()),
            codex_index_db_path: path_from_input(codex_index_db_path.read().as_str()),
            omegon_runtime_root: path_from_input(omegon_runtime_root.read().as_str()),
            omegon_mind_db_path: path_from_input(omegon_mind_db_path.read().as_str()),
            styrene_identity_profile: string_from_input(styrene_identity_profile.read().as_str()),
            omegon_serve_host: None,
        };
        let config = VaultConfig {
            vault_name: vault_name.read().clone(),
            sync: sync_config.read().clone(),
            appearance: AppearanceConfig {
                theme: theme.read().0.clone(),
                font_size: *font_sz.read(),
            },
            local_runtime,
            publication: codex_core::models::PublicationPolicy {
                default_visibility: *publication_default_visibility.read(),
                rules: publication_rules.read().clone(),
            },
            security: ctx.vault().config.security.clone(),
            indexing: ctx.vault().config.indexing.clone(),
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
            acp_config: initial_operator.acp_config.clone(),
            agent_daemon: initial_operator.agent_daemon.clone(),
        };

        match vault.save_config(&config) {
            Ok(()) => {}
            Err(e) => {
                tracing::error!("save_config: {e}");
                *save_msg.write() = Some(("err", "Save failed — check logs."));
                return;
            }
        }

        if let Err(e) = omegon_for_save.save_project_profile(&profile) {
            tracing::error!("save_project_profile: {e}");
            *save_msg.write() = Some(("err", "Profile save failed — check logs."));
            return;
        }

        if let Err(e) = omegon_for_save.save_operator_settings(&operator_settings) {
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
                            "{ctx.vault_root().display()}"
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
                                    let secs: u64 = e.value().parse().unwrap_or(0);
                                    // Enforce minimum 30 seconds (or 0 for manual only)
                                    let secs = if secs > 0 && secs < 30 { 30 } else { secs };
                                    if let SyncConfig::Git { ref mut auto_commit_seconds, .. } = *sync_config.write() {
                                        *auto_commit_seconds = secs;
                                    }
                                },
                            }
                            span { class: "settings-hint muted", "(0 = manual only, minimum 30)" }
                        }
                    }
                }

                SettingsSection { heading: "Publication",
                    PublicationRulesEditor {
                        default_visibility: publication_default_visibility,
                        rules: publication_rules,
                    }
                }

                SettingsSection { heading: "Local runtime",
                    SettingsRow { label: "State root",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{local_state_root}",
                            placeholder: "optional absolute path",
                            oninput: move |e| *local_state_root.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Codex index DB",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{codex_index_db_path}",
                            placeholder: "optional absolute path",
                            oninput: move |e| *codex_index_db_path.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Omegon runtime root",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{omegon_runtime_root}",
                            placeholder: "optional absolute path",
                            oninput: move |e| *omegon_runtime_root.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Omegon mind DB",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{omegon_mind_db_path}",
                            placeholder: "optional absolute path",
                            oninput: move |e| *omegon_mind_db_path.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Styrene Identity",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{styrene_identity_profile}",
                            placeholder: "optional local identity profile",
                            oninput: move |e| *styrene_identity_profile.write() = e.value(),
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
                        span { class: "settings-path muted", "{omegon.project_profile_path.display()}" }
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
                        span { class: "settings-path muted", "{omegon.operator_settings_path.display()}" }
                    }
                }

                // ── Save bar ─────────────────────────────────────────────────
                div { class: "settings-save-bar",
                    button { class: "btn btn-primary", onclick: save, "Save changes" }
                    button { class: "btn btn-ghost", onclick: publish_preview, "Export local preview" }
                    if let Some((kind, msg)) = *save_msg.read() {
                        span {
                            class: if kind == "ok" { "save-msg ok" } else { "save-msg err" },
                            "{msg}"
                        }
                    }
                    if let Some((kind, msg)) = &*publish_msg.read() {
                        span {
                            class: if *kind == "ok" { "save-msg ok" } else { "save-msg err" },
                            "{msg}"
                        }
                    }
                }
            }
        }
    }
}

// ── Sub-components ────────────────────────────────────────────────────────────

fn path_from_input(raw: &str) -> Option<std::path::PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(trimmed))
    }
}

fn string_from_input(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

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
