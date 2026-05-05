//! Template system — markdown templates with variable expansion.
//!
//! Templates live in `.flynt/templates/` as markdown files.
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

/// List all templates in the vault's .flynt/templates/ directory.
pub fn list_templates(vault_root: &Path) -> Vec<Template> {
    let dir = vault_root.join(".flynt/templates");
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
    let dir = vault_root.join(".flynt/templates");
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn list_templates_empty_vault() {
        let tmp = TempDir::new().unwrap();
        let templates = list_templates(tmp.path());
        assert!(templates.is_empty());
    }

    #[test]
    fn list_templates_finds_md_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".flynt/templates");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Note.md"), "note content").unwrap();
        fs::write(dir.join("Daily.md"), "daily content").unwrap();
        fs::write(dir.join("ignore.txt"), "not a template").unwrap();

        let templates = list_templates(tmp.path());
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].name, "Daily");
        assert_eq!(templates[1].name, "Note");
    }

    #[test]
    fn expand_replaces_title_and_vault() {
        let result = expand("Hello {{title}} in {{vault}}", "My Note", "My Vault");
        assert!(result.contains("My Note"));
        assert!(result.contains("My Vault"));
    }

    #[test]
    fn expand_replaces_date_vars() {
        let result = expand("{{year}}-{{month}}-{{day}}", "t", "v");
        // Should contain current year
        let year = chrono::Local::now().format("%Y").to_string();
        assert!(result.contains(&year));
    }

    #[test]
    fn ensure_default_templates_creates_three() {
        let tmp = TempDir::new().unwrap();
        ensure_default_templates(tmp.path()).unwrap();

        let templates = list_templates(tmp.path());
        assert_eq!(templates.len(), 3);
        let names: Vec<&str> = templates.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"Note"));
        assert!(names.contains(&"Daily"));
        assert!(names.contains(&"Meeting"));
    }

    #[test]
    fn ensure_default_templates_idempotent() {
        let tmp = TempDir::new().unwrap();
        ensure_default_templates(tmp.path()).unwrap();
        // Second call should be a no-op (dir exists)
        ensure_default_templates(tmp.path()).unwrap();
        assert_eq!(list_templates(tmp.path()).len(), 3);
    }

    #[test]
    fn ensure_default_templates_skips_existing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".flynt/templates");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Custom.md"), "custom").unwrap();

        // Should NOT overwrite — dir already exists
        ensure_default_templates(tmp.path()).unwrap();
        let templates = list_templates(tmp.path());
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, "Custom");
    }
}
