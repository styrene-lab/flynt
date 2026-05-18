//! Style-guide loader — two-tier merge of project-level + user-level guides.
//!
//! Project-level: `<project>/.flynt/style-guide.md` (lives with the project repo,
//! version-controlled, overrides everything below).
//! User-level: `~/.flynt/style-guide.md` (defaults across all projects).
//!
//! Output is a structured report the agent can consume directly: presence
//! flags, sizes, checksums for drift detection, the readable content for
//! prompt context, and the resolved merged content. Project wins on
//! conflict; the per-level fields are kept on the response so the agent
//! can reason about provenance when needed.

use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct StyleGuideLevel {
    pub level: &'static str,
    pub path: String,
    pub loaded: bool,
    /// Set when `loaded == false` — explains why (e.g., "file not found").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// First non-blank line of the body — used as a one-line summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StyleGuideReport {
    pub project: StyleGuideLevel,
    pub user: StyleGuideLevel,
    /// The content the agent should use as its source of truth. Project guide
    /// wins when both exist; user-level falls through when project is absent;
    /// `None` when neither is configured.
    pub merged: Option<String>,
    /// Surfaces when neither guide is loaded — gives the agent a way to tell
    /// the user how to set one up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_hint: Option<String>,
}

pub fn project_path(project_root: &Path) -> PathBuf {
    project_root.join(".flynt").join("style-guide.md")
}

pub fn user_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".flynt")
        .join("style-guide.md")
}

fn load_level(path: PathBuf, level: &'static str) -> StyleGuideLevel {
    let path_str = path.to_string_lossy().to_string();
    match std::fs::read(&path) {
        Ok(bytes) => {
            let size = bytes.len() as u64;
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let checksum = format!("sha256:{:x}", hasher.finalize());
            let content = String::from_utf8_lossy(&bytes).to_string();
            // Skip the frontmatter block (TOML +++…+++ or YAML ---…---) before
            // looking for a body headline. The first non-empty, non-heading
            // line after the frontmatter is treated as the summary.
            let body_start = if content.starts_with("+++\n") {
                content[4..]
                    .find("\n+++")
                    .map(|i| i + 4 + "\n+++".len())
                    .unwrap_or(0)
            } else if content.starts_with("---\n") {
                content[4..]
                    .find("\n---")
                    .map(|i| i + 4 + "\n---".len())
                    .unwrap_or(0)
            } else {
                0
            };
            let body = &content[body_start..];
            let headline = body
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| {
                    if l.len() > 100 {
                        format!("{}…", &l[..97])
                    } else {
                        l.to_string()
                    }
                });
            StyleGuideLevel {
                level,
                path: path_str,
                loaded: true,
                reason: None,
                size_bytes: Some(size),
                checksum: Some(checksum),
                headline,
                content: Some(content),
            }
        }
        Err(e) => StyleGuideLevel {
            level,
            path: path_str,
            loaded: false,
            reason: Some(match e.kind() {
                std::io::ErrorKind::NotFound => "file not found".into(),
                std::io::ErrorKind::PermissionDenied => "permission denied".into(),
                _ => format!("{e}"),
            }),
            size_bytes: None,
            checksum: None,
            headline: None,
            content: None,
        },
    }
}

/// Build the full report. Reads both levels, computes the merged content,
/// emits a setup hint when no guide is configured at all.
pub fn load_report(project_root: &Path) -> Result<StyleGuideReport> {
    let project = load_level(project_path(project_root), "project");
    let user = load_level(user_path(), "user");

    let merged = match (project.content.as_ref(), user.content.as_ref()) {
        (Some(p), _) => Some(p.clone()),    // project wins
        (None, Some(u)) => Some(u.clone()), // fall through to user
        (None, None) => None,
    };

    let setup_hint = if merged.is_none() {
        Some(format!(
            "No style guide configured. To add one: copy a starter to \
             '{}' (project-level, recommended) or '{}' (user-level default). \
             A template is bundled with the omegon-design extension.",
            project.path, user.path
        ))
    } else {
        None
    };

    Ok(StyleGuideReport {
        project,
        user,
        merged,
        setup_hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_report_with_neither_level_emits_setup_hint() {
        let tmp = TempDir::new().unwrap();
        // Force user_path away from real home by skipping the assertion on it.
        let report = load_report(tmp.path()).unwrap();
        assert!(!report.project.loaded);
        assert!(report.merged.is_none() || report.user.loaded);
        if report.merged.is_none() {
            assert!(report.setup_hint.is_some());
        }
    }

    #[test]
    fn load_report_with_project_level_uses_project_as_merged() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".flynt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("style-guide.md"),
            b"# Project Guide\nProject content here.",
        )
        .unwrap();

        let report = load_report(tmp.path()).unwrap();
        assert!(report.project.loaded);
        assert!(
            report
                .project
                .checksum
                .as_ref()
                .unwrap()
                .starts_with("sha256:")
        );
        let merged = report.merged.unwrap();
        assert!(merged.contains("Project Guide"));
    }

    #[test]
    fn load_report_headline_strips_frontmatter_and_headings() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".flynt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("style-guide.md"),
            b"+++\nname = \"x\"\n+++\n# Title\nWarm beige aesthetic with amber accents.\n",
        )
        .unwrap();
        let report = load_report(tmp.path()).unwrap();
        assert_eq!(
            report.project.headline.as_deref(),
            Some("Warm beige aesthetic with amber accents.")
        );
    }
}
