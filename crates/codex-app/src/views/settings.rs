use crate::{
    bootstrap::{AppContext, OmegonRuntimeContext, PendingVaultSetup},
    components::daemon_settings::DaemonSettingsSection,
    components::identity_settings::IdentitySettingsSection,
    components::provider_settings::ProviderSettingsSection,
    state::ThemeName,
    views::PublicationRulesEditor,
};
use codex_core::models::{
    AppearanceConfig, CodexOperatorSettings, FontSizePreset, LocalRuntimeConfig,
    OmegonProfile, OmegonProfileModel, SyncConfig, VaultConfig, VisualizationConfig,
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
    let mut omegon_channel = use_signal(|| ctx.vault().config.local_runtime.omegon_channel.clone());
    let mut omegon_bin_override = use_signal(|| {
        ctx.vault()
            .config
            .local_runtime
            .omegon_bin_override
            .clone()
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

    // Indexing
    let mut write_frontmatter = use_signal(|| ctx.vault().config.indexing.write_frontmatter);

    // Visualization
    let mut excalidraw_auto_export = use_signal(|| ctx.vault().config.visualization.excalidraw_auto_export);
    let mut d2_auto_render = use_signal(|| ctx.vault().config.visualization.d2_auto_render);
    let mut d2_theme = use_signal(|| ctx.vault().config.visualization.d2_theme.to_string());
    let mut d2_layout = use_signal(|| ctx.vault().config.visualization.d2_layout.clone());
    let mut d2_bin = use_signal(|| ctx.vault().config.visualization.d2_bin.clone().unwrap_or_default());

    // Daemon config — managed by DaemonSettingsSection
    let daemon_config = use_signal(|| initial_operator.agent_daemon.clone());

    let mut save_msg = use_signal(|| Option::<(&'static str, &'static str)>::None);
    let mut show_advanced = use_signal(|| false);
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
            omegon_channel: omegon_channel.read().clone(),
            omegon_bin_override: string_from_input(omegon_bin_override.read().as_str()),
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
            indexing: codex_core::models::IndexingConfig {
                write_frontmatter: *write_frontmatter.read(),
            },
            visualization: VisualizationConfig {
                excalidraw_auto_export: *excalidraw_auto_export.read(),
                d2_auto_render: *d2_auto_render.read(),
                d2_theme: d2_theme.read().parse::<u32>().unwrap_or(200),
                d2_layout: d2_layout.read().clone(),
                d2_bin: {
                    let bin = d2_bin.read().trim().to_string();
                    if bin.is_empty() { None } else { Some(bin) }
                },
            },
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
            agent_daemon: daemon_config.read().clone(),
        };

        // Check if sync backend changed — trigger vault migration
        let old_sync = &vault.config.sync;
        let new_sync = &config.sync;
        if old_sync != new_sync {
            let vault_name = config.vault_name.clone();
            let current_root = vault.root.clone();
            let sync_for_migrate = new_sync.clone();
            match codex_store::migrate::migrate_vault(
                &current_root, &vault_name, &sync_for_migrate, false,
            ) {
                Ok(result) => {
                    if result.new_root != current_root {
                        // Vault moved — update launcher profile and switch runtime
                        let mut profile = crate::bootstrap::OmegonRuntimeContext::load_launcher_profile();
                        crate::bootstrap::OmegonRuntimeContext::register_known_vault(
                            &mut profile, &result.new_root, &vault_name,
                        );
                        let _ = crate::bootstrap::OmegonRuntimeContext::save_launcher_profile(&profile);
                        let mut migrate_ctx = ctx;
                        migrate_ctx.set_runtime(crate::bootstrap::runtime_state_for_vault_root(result.new_root));
                        *save_msg.write() = Some(("ok", "Vault migrated and sync updated."));
                        return; // config already written by migrate
                    }
                    // Same location — migration updated config in place, continue to save other settings
                }
                Err(e) => {
                    tracing::error!("Migration failed: {e}");
                    *save_msg.write() = Some(("err", "Migration failed — check logs."));
                    return;
                }
            }
        }

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

                // ── Advanced toggle ──────────────────────────────────────────
                div { class: "settings-advanced-toggle",
                    button {
                        class: "settings-toggle-btn",
                        onclick: move |_| {
                            let v = *show_advanced.read();
                            *show_advanced.write() = !v;
                        },
                        if *show_advanced.read() {
                            "Hide advanced settings \u{25B4}"
                        } else {
                            "Show advanced settings \u{25BE}"
                        }
                    }
                }

                if *show_advanced.read() {

                // ── Visualization ────────────────────────────────────────────
                SettingsSection { heading: "Visualization",
                    SettingsRow { label: "Excalidraw auto-export",
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: *excalidraw_auto_export.read(),
                                onchange: move |e| *excalidraw_auto_export.write() = e.checked(),
                            }
                            "Auto-export SVG when drawings are saved"
                        }
                    }
                    SettingsRow { label: "D2 auto-render",
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: *d2_auto_render.read(),
                                onchange: move |e| *d2_auto_render.write() = e.checked(),
                            }
                            "Auto-render D2 diagrams to SVG"
                        }
                    }
                    SettingsRow { label: "D2 theme",
                        input {
                            class: "input settings-input settings-input-sm",
                            r#type: "number",
                            value: "{d2_theme}",
                            placeholder: "200",
                            oninput: move |e| *d2_theme.write() = e.value(),
                        }
                        span { class: "settings-hint muted", "(200 = dark, 0 = default)" }
                    }
                    SettingsRow { label: "D2 layout",
                        div { class: "radio-group",
                            for (value, label) in [("elk", "ELK"), ("dagre", "Dagre"), ("tala", "TALA")] {
                                button {
                                    class: if d2_layout.read().as_str() == value { "radio-btn active" } else { "radio-btn" },
                                    onclick: move |_| *d2_layout.write() = value.to_string(),
                                    "{label}"
                                }
                            }
                        }
                    }
                    SettingsRow { label: "D2 binary",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{d2_bin}",
                            placeholder: "d2 (on PATH)",
                            oninput: move |e| *d2_bin.write() = e.value(),
                        }
                    }
                }

                // ── Indexing ────────────────────────────────────────────────────
                SettingsSection { heading: "Indexing",
                    SettingsRow { label: "Write frontmatter",
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: *write_frontmatter.read(),
                                onchange: move |e| *write_frontmatter.write() = e.checked(),
                            }
                            "Write stable UUIDs into file frontmatter"
                        }
                        span { class: "settings-hint muted", "Disable for shared repos where files shouldn't be modified" }
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
                    SettingsRow { label: "Omegon channel",
                        div { class: "radio-group",
                            for ch in codex_core::models::OmegonChannel::all_named() {
                                {
                                    let ch_clone = ch.clone();
                                    let lbl = ch.label().to_string();
                                    rsx! {
                                        button {
                                            class: if *omegon_channel.read() == *ch { "radio-btn active" } else { "radio-btn" },
                                            onclick: move |_| *omegon_channel.write() = ch_clone.clone(),
                                            "{lbl}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    SettingsRow { label: "Omegon binary",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{omegon_bin_override}",
                            placeholder: "Auto-detect from channel",
                            oninput: move |e| *omegon_bin_override.write() = e.value(),
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
                        select {
                            class: "input settings-input",
                            value: "{thinking_level}",
                            onchange: move |e| *thinking_level.write() = e.value(),
                            option { value: "", "None" }
                            option { value: "low", "Low" }
                            option { value: "medium", "Medium" }
                            option { value: "high", "High" }
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
                // ── Identity ─────────────────────────────────────────────────
                IdentitySettingsSection {}

                // ── Providers ────────────────────────────────────────────────
                ProviderSettingsSection {}

                // ── Operator ─────────────────────────────────────────────────
                SettingsSection { heading: "Operator",
                    SettingsRow { label: "Active persona",
                        select {
                            class: "input settings-input",
                            value: "{active_persona}",
                            onchange: move |e| *active_persona.write() = e.value(),
                            option { value: "off", "Off" }
                            option { value: "scribe", "Scribe" }
                            option { value: "omegon", "Omegon" }
                        }
                    }
                    SettingsRow { label: "Rail extension",
                        select {
                            class: "input settings-input",
                            value: "{rail_extension}",
                            onchange: move |e| *rail_extension.write() = e.value(),
                            option { value: "", "None" }
                            option { value: "vox", "Vox" }
                            option { value: "codex", "Codyx" }
                        }
                    }
                    SettingsRow { label: "Vox enabled",
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: *vox_enabled.read(),
                                onchange: move |e| *vox_enabled.write() = e.checked(),
                            }
                            "Enable Vox communication"
                        }
                    }
                    SettingsRow { label: "Vox TTS",
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: *vox_tts_enabled.read(),
                                onchange: move |e| *vox_tts_enabled.write() = e.checked(),
                            }
                            "Enable text-to-speech"
                        }
                    }
                    SettingsRow { label: "Vox voice",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{vox_voice}",
                            placeholder: "System default",
                            oninput: move |e| *vox_voice.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Operator settings",
                        span { class: "settings-path muted", "{omegon.operator_settings_path.display()}" }
                    }
                }

                // ── Agent Daemon ────────────────────────────────────────────
                DaemonSettingsSection {
                    config: daemon_config,
                }

                } // end if show_advanced

                // ── Save bar ─────────────────────────────────────────────────
                div { class: "settings-save-bar",
                    button { class: "btn btn-primary", onclick: save, "Save changes" }
                    if *show_advanced.read() {
                        button { class: "btn btn-ghost", onclick: publish_preview, "Export local preview" }
                    }
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
