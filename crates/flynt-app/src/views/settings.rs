use crate::{
    bootstrap::{AppContext, OmegonRuntimeContext, PendingVaultSetup},
    components::daemon_settings::DaemonSettingsSection,
    components::identity_settings::IdentitySettingsSection,
    components::provider_settings::ProviderSettingsSection,
    state::{SettingsTab, ThemeName},
    views::{IndexingScopesEditor, PublicationRulesEditor},
};
use flynt_core::models::{
    AppearanceConfig, FlyntOperatorSettings, FontSizePreset, IndexingConfig,
    LocalRuntimeConfig, OmegonProfile, SyncConfig, VaultConfig, VisualizationConfig,
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

    // Project + sync — local form state; persisted on explicit Save.
    let mut vault_name = use_signal(|| ctx.project().config.vault_name.clone());
    let mut sync_config = use_signal(|| ctx.project().config.sync.clone());
    let mut local_state_root = use_signal(|| {
        ctx.project()
            .config
            .local_runtime
            .local_state_root
            .as_ref()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .unwrap_or_default()
    });
    let mut flynt_index_db_path = use_signal(|| {
        ctx.project()
            .config
            .local_runtime
            .flynt_index_db_path
            .as_ref()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .unwrap_or_default()
    });
    let mut omegon_runtime_root = use_signal(|| {
        ctx.project()
            .config
            .local_runtime
            .omegon_runtime_root
            .as_ref()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .unwrap_or_default()
    });
    let mut omegon_mind_db_path = use_signal(|| {
        ctx.project()
            .config
            .local_runtime
            .omegon_mind_db_path
            .as_ref()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .unwrap_or_default()
    });
    let mut omegon_channel = use_signal(|| ctx.project().config.local_runtime.omegon_channel.clone());
    let mut omegon_bin_override = use_signal(|| {
        ctx.project()
            .config
            .local_runtime
            .omegon_bin_override
            .clone()
            .unwrap_or_default()
    });
    let mut styrene_identity_profile = use_signal(|| {
        ctx.project()
            .config
            .local_runtime
            .styrene_identity_profile
            .clone()
            .unwrap_or_default()
    });

    let publication_default_visibility =
        use_signal(|| ctx.project().config.publication.default_visibility);
    let publication_rules = use_signal(|| ctx.project().config.publication.rules.clone());

    let _project_profile_state = use_context::<Signal<OmegonProfile>>();
    let _operator_settings_state = use_context::<Signal<FlyntOperatorSettings>>();

    // Indexing
    let mut write_frontmatter = use_signal(|| ctx.project().config.indexing.write_frontmatter);
    let indexing_scopes = use_signal(|| ctx.project().config.indexing.scopes.clone());

    // Raw config editor
    let mut show_raw_config = use_signal(|| false);
    let config_path = ctx.vault_root().join(".flynt/config.toml");
    let mut raw_config_text = use_signal(|| {
        std::fs::read_to_string(ctx.vault_root().join(".flynt/config.toml")).unwrap_or_default()
    });
    let mut raw_config_msg = use_signal(|| Option::<(&'static str, &'static str)>::None);

    // Visualization
    let mut excalidraw_auto_export = use_signal(|| ctx.project().config.visualization.excalidraw_auto_export);
    let mut d2_auto_render = use_signal(|| ctx.project().config.visualization.d2_auto_render);
    let mut d2_theme = use_signal(|| ctx.project().config.visualization.d2_theme.to_string());
    let mut d2_layout = use_signal(|| ctx.project().config.visualization.d2_layout.clone());
    let mut d2_bin = use_signal(|| ctx.project().config.visualization.d2_bin.clone().unwrap_or_default());

    // Daemon config — managed by DaemonSettingsSection
    let daemon_config = use_signal(|| ctx.omegon().load_operator_settings().agent_daemon.clone());

    let mut save_msg = use_signal(|| Option::<(&'static str, &'static str)>::None);
    let publish_msg = use_signal(|| Option::<(&'static str, String)>::None);

    let mut active_tab = use_context::<Signal<SettingsTab>>();

    let project = ctx.project();
    let omegon = ctx.omegon();
    let omegon_for_save = omegon.clone();
    let publish_vault = ctx.project();
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
        if let flynt_core::models::SyncConfig::Git { ref remote, ref branch, .. } = *sync_config.read() {
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
            ("Index DB path", flynt_index_db_path.read().clone()),
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
            flynt_index_db_path: path_from_input(flynt_index_db_path.read().as_str()),
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
            publication: flynt_core::models::PublicationPolicy {
                default_visibility: *publication_default_visibility.read(),
                rules: publication_rules.read().clone(),
            },
            security: ctx.project().config.security.clone(),
            indexing: IndexingConfig {
                write_frontmatter: *write_frontmatter.read(),
                scopes: indexing_scopes.read().clone(),
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

        // Check if sync backend changed — trigger project migration
        let old_sync = &project.config.sync;
        let new_sync = &config.sync;
        if old_sync != new_sync {
            let vault_name = config.vault_name.clone();
            let current_root = project.root.clone();
            let sync_for_migrate = new_sync.clone();
            match flynt_store::migrate::migrate_vault(
                &current_root, &vault_name, &sync_for_migrate, false,
            ) {
                Ok(result) => {
                    if result.new_root != current_root {
                        // Project moved — update launcher profile and switch runtime
                        let mut profile = crate::bootstrap::OmegonRuntimeContext::load_launcher_profile();
                        crate::bootstrap::OmegonRuntimeContext::register_known_vault(
                            &mut profile, &result.new_root, &vault_name,
                        );
                        let _ = crate::bootstrap::OmegonRuntimeContext::save_launcher_profile(&profile);
                        let mut migrate_ctx = ctx;
                        migrate_ctx.set_runtime(crate::bootstrap::runtime_state_for_vault_root(result.new_root));
                        *save_msg.write() = Some(("ok", "Project migrated and sync updated."));
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

        match project.save_config(&config) {
            Ok(()) => {}
            Err(e) => {
                tracing::error!("save_config: {e}");
                *save_msg.write() = Some(("err", "Save failed — check logs."));
                return;
            }
        }

        // Persist daemon config alongside project config
        let mut operator = omegon_for_save.load_operator_settings();
        operator.agent_daemon = daemon_config.read().clone();
        if let Err(e) = omegon_for_save.save_operator_settings(&operator) {
            tracing::error!("save_operator_settings: {e}");
            *save_msg.write() = Some(("err", "Operator settings save failed — check logs."));
            return;
        }

        *save_msg.write() = Some(("ok", "Settings saved."));
    };

    rsx! {
        div { class: "settings-root",
            // ── Tab bar ──────────────────────────────────────────────────
            div { class: "settings-tab-bar",
                for tab in SettingsTab::all() {
                    button {
                        class: if *active_tab.read() == *tab { "settings-tab active" } else { "settings-tab" },
                        onclick: move |_| *active_tab.write() = *tab,
                        "{tab.label()}"
                    }
                }
            }

            div { class: "settings-scroll",

                // ════════════════════════════════════════════════════════════
                // General: Appearance, Project, Sync, Identity, Providers
                // ════════════════════════════════════════════════════════════
                if *active_tab.read() == SettingsTab::General {

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
                                    class: if *font_sz.read() == preset { "font-size-btn active" } else { "font-size-btn" },
                                    onclick: move |_| *font_sz.write() = preset,
                                    "{preset.label()}"
                                }
                            }
                        }
                    }
                }

                SettingsSection { heading: "Project",
                    SettingsRow { label: "Name",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{vault_name}",
                            oninput: move |e| *vault_name.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Location",
                        span { class: "settings-path muted", "{ctx.vault_root().display()}" }
                    }
                }

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
                                    let secs = if secs > 0 && secs < 30 { 30 } else { secs };
                                    if let SyncConfig::Git { ref mut auto_commit_seconds, .. } = *sync_config.write() {
                                        *auto_commit_seconds = secs;
                                    }
                                },
                            }
                            span { class: "settings-hint muted", "(0 = manual only, minimum 30)" }
                        }
                        {
                            let provider_id = flynt_core::providers::provider_for_url(&remote);
                            let cred_status = provider_id.and_then(|pid| {
                                flynt_core::providers::PROVIDERS.iter().find(|p| p.id == pid)
                            }).map(|p| flynt_core::providers::probe_provider(p));
                            match (provider_id, cred_status) {
                                (Some(pid), Some(flynt_core::providers::CredentialStatus::Authenticated { source })) => rsx! {
                                    SettingsRow { label: "Git credentials",
                                        span { class: "provider-status authenticated" }
                                        span { class: "provider-status-text", "Authenticated ({source}) — {pid}" }
                                    }
                                },
                                (Some(pid), _) => rsx! {
                                    SettingsRow { label: "Git credentials",
                                        span { class: "provider-status missing" }
                                        span { class: "provider-status-text", "Not configured — add a token for {pid} in Providers" }
                                    }
                                },
                                _ => rsx! {
                                    SettingsRow { label: "Git credentials",
                                        span { class: "settings-hint muted", "Unknown host — credentials managed by system git" }
                                    }
                                },
                            }
                        }
                    }
                }

                IdentitySettingsSection {}
                ProviderSettingsSection {}

                } // end General

                // ════════════════════════════════════════════════════════════
                // Project: Indexing, Visualization, Publication
                // ════════════════════════════════════════════════════════════
                if *active_tab.read() == SettingsTab::Project {

                SettingsSection { heading: "Indexing",
                    SettingsRow { label: "Write frontmatter",
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: *write_frontmatter.read(),
                                onchange: move |e| *write_frontmatter.write() = e.checked(),
                            }
                            "Write stable UUIDs into file frontmatter (project-wide default)"
                        }
                        span { class: "settings-hint muted", "Disable for code repos — then use scopes below to opt in specific directories" }
                    }
                    SettingsRow { label: "Managed scopes",
                        IndexingScopesEditor { scopes: indexing_scopes }
                    }
                }

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

                SettingsSection { heading: "Publication",
                    PublicationRulesEditor {
                        default_visibility: publication_default_visibility,
                        rules: publication_rules,
                    }
                }

                } // end Project

                // ════════════════════════════════════════════════════════════
                // Omegon: Agent config, Extensions, Skills, Daemon
                // ════════════════════════════════════════════════════════════
                if *active_tab.read() == SettingsTab::Omegon {

                crate::components::omegon::OmegonSettingsSection {}
                crate::components::omegon::ExtensionManagerSection {}

                {
                    let omegon_ctx = ctx.omegon();
                    let current_skills = ctx.omegon().load_operator_settings().enabled_skills;
                    rsx! {
                        crate::components::omegon::SkillSettingsSection {
                            enabled_skills: current_skills,
                            on_change: move |updated: Vec<String>| {
                                let omegon = omegon_ctx.clone();
                                let mut settings = omegon.load_operator_settings();
                                settings.enabled_skills = updated;
                                let _ = omegon.save_operator_settings(&settings);
                            },
                            extensions_dir: ctx.omegon().extensions_dir.clone(),
                            skills_dir: ctx.omegon().home_dir.join("skills"),
                        }
                    }
                }

                DaemonSettingsSection { config: daemon_config }

                SettingsSection { heading: "Runtime",
                    SettingsRow { label: "Omegon channel",
                        div { class: "radio-group",
                            for ch in flynt_core::models::OmegonChannel::all_named() {
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
                }

                } // end Omegon

                // ════════════════════════════════════════════════════════════
                // Advanced: Local paths, Config file editor
                // ════════════════════════════════════════════════════════════
                if *active_tab.read() == SettingsTab::Advanced {

                SettingsSection { heading: "Local paths",
                    SettingsRow { label: "State root",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{local_state_root}",
                            placeholder: "optional absolute path",
                            oninput: move |e| *local_state_root.write() = e.value(),
                        }
                    }
                    SettingsRow { label: "Flynt index DB",
                        input {
                            class: "input settings-input",
                            r#type: "text",
                            value: "{flynt_index_db_path}",
                            placeholder: "optional absolute path",
                            oninput: move |e| *flynt_index_db_path.write() = e.value(),
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

                SettingsSection { heading: "Config file",
                    div { class: "settings-row",
                        span { class: "settings-label", "config.toml" }
                        div { class: "settings-control",
                            button {
                                class: "btn btn-ghost",
                                onclick: {
                                    let cp = config_path.clone();
                                    move |_| {
                                        let v = *show_raw_config.read();
                                        if !v {
                                            *raw_config_text.write() = std::fs::read_to_string(&cp).unwrap_or_default();
                                        }
                                        *show_raw_config.write() = !v;
                                    }
                                },
                                if *show_raw_config.read() { "Close editor" } else { "Edit config.toml" }
                            }
                            span { class: "settings-hint muted", "Power user: edit the project config directly" }
                        }
                    }
                    if *show_raw_config.read() {
                        div { class: "raw-config-editor",
                            textarea {
                                class: "input raw-config-textarea",
                                value: "{raw_config_text}",
                                rows: "20",
                                spellcheck: "false",
                                oninput: move |e| *raw_config_text.write() = e.value(),
                            }
                            div { class: "raw-config-actions",
                                button {
                                    class: "btn btn-primary",
                                    onclick: {
                                        let cp = config_path.clone();
                                        move |_| {
                                            let text = raw_config_text.read().clone();
                                            match toml::from_str::<VaultConfig>(&text) {
                                                Ok(_) => {
                                                    if let Err(e) = std::fs::write(&cp, &text) {
                                                        *raw_config_msg.write() = Some(("err", "Write failed — check permissions."));
                                                        tracing::error!("raw config write: {e}");
                                                    } else {
                                                        *raw_config_msg.write() = Some(("ok", "Config saved. Restart or re-open project to apply."));
                                                    }
                                                }
                                                Err(_) => {
                                                    *raw_config_msg.write() = Some(("err", "Invalid TOML — fix syntax before saving."));
                                                }
                                            }
                                        }
                                    },
                                    "Save config.toml"
                                }
                                button {
                                    class: "btn btn-ghost",
                                    onclick: {
                                        let cp = config_path.clone();
                                        move |_| {
                                            *raw_config_text.write() = std::fs::read_to_string(&cp).unwrap_or_default();
                                            *raw_config_msg.write() = None;
                                        }
                                    },
                                    "Revert"
                                }
                                if let Some((kind, msg)) = *raw_config_msg.read() {
                                    span {
                                        class: if kind == "ok" { "save-msg ok" } else { "save-msg err" },
                                        "{msg}"
                                    }
                                }
                            }
                        }
                    }
                }

                } // end Advanced

                // ── Save bar ─────────────────────────────────────────────────
                div { class: "settings-save-bar",
                    button { class: "btn btn-primary", onclick: save, "Save changes" }
                    if *active_tab.read() == SettingsTab::Project {
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
