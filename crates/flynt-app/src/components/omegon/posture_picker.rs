//! Posture picker — built-in presets + custom .pkl posture discovery.
//!
//! Scans `.omegon/postures/` (project-level) and `~/.omegon/postures/`
//! (user-level) for custom .pkl posture files. Displays name + description
//! without requiring the pkl CLI (light parse for display only).

use dioxus::prelude::*;
use std::path::{Path, PathBuf};

/// A posture entry for display in the picker.
#[derive(Debug, Clone, PartialEq)]
pub struct PostureEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_builtin: bool,
}

/// Built-in posture presets.
pub fn builtin_postures() -> Vec<PostureEntry> {
    vec![
        PostureEntry {
            id: "fabricator".into(),
            name: "Fabricator".into(),
            description: "Balanced coding — direct execution, delegates larger tasks".into(),
            is_builtin: true,
        },
        PostureEntry {
            id: "architect".into(),
            name: "Architect".into(),
            description: "Orchestrator — plans, delegates to local models, reviews".into(),
            is_builtin: true,
        },
        PostureEntry {
            id: "explorator".into(),
            name: "Explorator".into(),
            description: "Read-only exploration — lean, no file mutations".into(),
            is_builtin: true,
        },
        PostureEntry {
            id: "devastator".into(),
            name: "Devastator".into(),
            description: "Maximum force — deep reasoning, large context".into(),
            is_builtin: true,
        },
    ]
}

/// Discover custom postures from .pkl files. Light parse — extracts name
/// and description without invoking the pkl CLI.
pub fn discover_custom_postures(project_root: &Path) -> Vec<PostureEntry> {
    let mut entries = Vec::new();
    let dirs = [
        project_root.join(".omegon/postures"),
        dirs::home_dir()
            .unwrap_or_default()
            .join(".omegon/postures"),
    ];

    let mut seen = std::collections::HashSet::new();
    for dir in &dirs {
        let Ok(read) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("pkl") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let name = extract_pkl_field(&content, "name")
                .unwrap_or_else(|| {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                });
            if seen.contains(&name) {
                continue; // project-level shadows user-level
            }
            let description =
                extract_pkl_field(&content, "description").unwrap_or_default();
            let id = name.clone();
            seen.insert(name.clone());
            entries.push(PostureEntry {
                id,
                name,
                description,
                is_builtin: false,
            });
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Light extraction of a string field from pkl content.
/// Matches `name = "value"` inside a `posture { ... }` block.
fn extract_pkl_field(content: &str, field: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(field) {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim().trim_matches('"');
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }
        }
    }
    None
}

/// Posture picker component. Shows built-in presets + discovered custom
/// postures as radio buttons.
#[component]
pub fn PosturePicker(
    current: String,
    on_change: EventHandler<String>,
    vault_root: PathBuf,
) -> Element {
    let all_postures = use_resource(move || {
        let root = vault_root.clone();
        async move {
            let mut all = builtin_postures();
            let custom = tokio::task::spawn_blocking(move || discover_custom_postures(&root))
                .await
                .unwrap_or_default();
            all.extend(custom);
            all
        }
    });

    rsx! {
        div { class: "posture-picker",
            for posture in all_postures.read().as_ref().unwrap_or(&vec![]).iter() {
                label { class: "posture-option",
                    input {
                        r#type: "radio",
                        name: "posture",
                        value: "{posture.id}",
                        checked: current == posture.id,
                        onchange: {
                            let id = posture.id.clone();
                            move |_| on_change.call(id.clone())
                        },
                    }
                    div { class: "posture-info",
                        span { class: "posture-name",
                            "{posture.name}"
                            if !posture.is_builtin {
                                span { class: "posture-badge", " (custom)" }
                            }
                        }
                        if !posture.description.is_empty() {
                            span { class: "posture-desc muted", "{posture.description}" }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_name_from_pkl() {
        let content = r#"
posture {
  name = "reviewer"
  description = "Code review mode"
  base = "architect"
}
"#;
        assert_eq!(extract_pkl_field(content, "name"), Some("reviewer".into()));
        assert_eq!(
            extract_pkl_field(content, "description"),
            Some("Code review mode".into())
        );
        assert_eq!(extract_pkl_field(content, "base"), Some("architect".into()));
    }

    #[test]
    fn extract_missing_field() {
        assert_eq!(extract_pkl_field("posture { }", "name"), None);
    }

    #[test]
    fn builtins_have_four_presets() {
        assert_eq!(builtin_postures().len(), 4);
    }
}
