//! Template system — markdown templates with variable expansion.
//!
//! Templates live in `.codex/templates/` as markdown files.
//! Variables: {{title}}, {{date}}, {{time}}, {{year}}, {{month}}, {{day}}, {{weekday}}, {{vault}}.

use std::path::{Path, PathBuf};
use std::fs;
use chrono::Local;
use anyhow::Result;

/// A template definition.
#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
}

/// List all templates in the vault's .codex/templates/ directory.
pub fn list_templates(vault_root: &Path) -> Vec<Template> {
    let dir = vault_root.join(".codex/templates");
    if !dir.exists() { return vec![]; }

    let mut templates = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let name = path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if let Ok(content) = fs::read_to_string(&path) {
                    templates.push(Template { name, path, content });
                }
            }
        }
    }
    templates.sort_by(|a, b| a.name.cmp(&b.name));
    templates
}

/// Expand a template with the given title and vault name.
pub fn expand(template: &str, title: &str, vault_name: &str) -> String {
    let now = Local::now();
    template
        .replace("{{title}}", title)
        .replace("{{date}}", &now.format("%Y-%m-%d").to_string())
        .replace("{{time}}", &now.format("%H:%M").to_string())
        .replace("{{year}}", &now.format("%Y").to_string())
        .replace("{{month}}", &now.format("%m").to_string())
        .replace("{{day}}", &now.format("%d").to_string())
        .replace("{{weekday}}", &now.format("%A").to_string())
        .replace("{{vault}}", vault_name)
}

/// Create the default templates if the templates directory doesn't exist.
pub fn ensure_default_templates(vault_root: &Path) -> Result<()> {
    let dir = vault_root.join(".codex/templates");
    if dir.exists() { return Ok(()); }

    fs::create_dir_all(&dir)?;

    fs::write(dir.join("Note.md"), r#"+++
title = "{{title}}"
tags = []
+++

# {{title}}

"#)?;

    fs::write(dir.join("Daily.md"), r#"+++
title = "{{title}}"
tags = ["daily"]
date = "{{date}}"
+++

# {{title}}

## Tasks

- [ ]

## Notes

"#)?;

    fs::write(dir.join("Meeting.md"), r#"+++
title = "{{title}}"
tags = ["meeting"]
date = "{{date}}"
+++

# {{title}}

**Date:** {{date}}
**Attendees:**

## Agenda

1.

## Notes

## Action Items

- [ ]
"#)?;

    Ok(())
}
