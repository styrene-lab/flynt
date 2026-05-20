//! Skill settings — checkbox list for enabled skills.
//!
//! Discovers available skills by scanning extension manifests for
//! declared skill names. Renders as a checkbox list backed by
//! `FlyntOperatorSettings.enabled_skills`.

use dioxus::prelude::*;
use std::path::Path;

/// A skill entry with metadata for display.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillEntry {
    pub name: String,
    pub source: String, // which extension provides it
}

/// Discover available skills from extension manifests and the global
/// skills directory.
pub fn discover_skills(extensions_dir: &Path, skills_dir: &Path) -> Vec<SkillEntry> {
    let mut skills = Vec::new();

    // Scan extension manifests for declared skills
    if let Ok(entries) = std::fs::read_dir(extensions_dir) {
        for entry in entries.flatten() {
            let dir = entry.path();
            let manifest = dir.join("manifest.toml");
            if !manifest.exists() {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let ext_name = dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            // Look for [skills] section or skill-related entries
            // Extension manifests don't always declare skills explicitly;
            // for now, each extension is implicitly a "skill source"
            if raw.contains("[skills]") || raw.contains("[[skills]]") {
                // Parse skill names from the manifest
                for line in raw.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("name") {
                        if let Some(rest) = rest.trim().strip_prefix('=') {
                            let name = rest.trim().trim_matches('"').to_string();
                            if !name.is_empty() {
                                skills.push(SkillEntry {
                                    name,
                                    source: ext_name.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Scan global skills directory
    if let Ok(entries) = std::fs::read_dir(skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !skills.iter().any(|s| s.name == name) {
                    skills.push(SkillEntry {
                        name,
                        source: "global".into(),
                    });
                }
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Skill settings component — checkboxes for each discovered skill.
#[component]
pub fn SkillSettingsSection(
    enabled_skills: Vec<String>,
    on_change: EventHandler<Vec<String>>,
    extensions_dir: std::path::PathBuf,
    skills_dir: std::path::PathBuf,
) -> Element {
    let available = use_resource(move || {
        let ext_dir = extensions_dir.clone();
        let sk_dir = skills_dir.clone();
        async move {
            tokio::task::spawn_blocking(move || discover_skills(&ext_dir, &sk_dir))
                .await
                .unwrap_or_default()
        }
    });

    if available
        .read()
        .as_ref()
        .map(|v| v.is_empty())
        .unwrap_or(true)
    {
        return rsx! {
            section { class: "settings-section",
                h2 { class: "settings-heading", "Skills" }
                div { class: "settings-rows",
                    span { class: "settings-hint muted", "No skills discovered" }
                }
            }
        };
    }

    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "Skills" }
            div { class: "settings-rows",
                for skill in available.read().as_ref().unwrap_or(&vec![]).iter() {
                    div { class: "settings-row",
                        label { class: "checkbox-label",
                            input {
                                r#type: "checkbox",
                                checked: enabled_skills.contains(&skill.name),
                                onchange: {
                                    let skill_name = skill.name.clone();
                                    let current = enabled_skills.clone();
                                    move |e: Event<FormData>| {
                                        let mut updated = current.clone();
                                        if e.checked() {
                                            if !updated.contains(&skill_name) {
                                                updated.push(skill_name.clone());
                                            }
                                        } else {
                                            updated.retain(|s| s != &skill_name);
                                        }
                                        on_change.call(updated);
                                    }
                                },
                            }
                            span { "{skill.name}" }
                            span { class: "settings-hint muted", " ({skill.source})" }
                        }
                    }
                }
            }
        }
    }
}
