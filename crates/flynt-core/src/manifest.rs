//! Project manifest — a registry of projects synced across devices.
//!
//! The manifest lives in its own small git repo (or a local file synced via
//! iCloud). It lists all projects the operator has access to, with their sync
//! coordinates. On a new device, the operator clones the manifest repo and
//! Flynt discovers all their projects from it.
//!
//! ## File format
//!
//! `projects.toml` at the manifest repo root:
//!
//! ```toml
//! [identity]
//! name = "Chris Wilson"
//! email = "chris@example.com"
//! fingerprint = "sha256:abc123..."
//!
//! [[projects]]
//! name = "Personal"
//! repo = "git@github.com:user/codex-personal.git"
//! branch = "main"
//! role = "owner"
//!
//! [[projects]]
//! name = "Work"
//! repo = "https://hub.styrene.io/org/codex-work.git"
//! branch = "main"
//! role = "editor"
//! hub = "hub.styrene.io"
//! ```

use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

/// The project manifest — serialized as `projects.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectManifest {
    /// The operator's identity metadata.
    #[serde(default)]
    pub identity: ManifestIdentity,
    /// All known projects.
    #[serde(default)]
    pub projects: Vec<ManifestProject>,
}

/// Identity section — ties the manifest to a Styrene identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ManifestIdentity {
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Email (used for git commits).
    #[serde(default)]
    pub email: String,
    /// StyreneIdentity fingerprint (if identity is provisioned).
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// A project entry in the manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestProject {
    /// Human-readable project name.
    pub name: String,
    /// Git remote URL (SSH or HTTPS).
    pub repo: String,
    /// Branch to sync.
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Operator's role in this project.
    #[serde(default)]
    pub role: ProjectRole,
    /// If hosted on a Styrene Hub, the hub hostname.
    #[serde(default)]
    pub hub: Option<String>,
    /// Local path where this project is cloned (device-specific, not synced).
    /// Populated after cloning. Stripped when serializing to the manifest repo.
    #[serde(skip_serializing, default)]
    pub local_path: Option<PathBuf>,
    /// Auto-commit interval in seconds (0 = manual).
    #[serde(default = "default_auto_commit")]
    pub auto_commit_seconds: u64,
}

fn default_branch() -> String { "main".into() }
fn default_auto_commit() -> u64 { 60 }

/// Role in a project — determines default mutability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    /// Full control — can modify, delete, manage access.
    #[default]
    Owner,
    /// Can read and write project content.
    Editor,
    /// Read-only access.
    Viewer,
}

impl ProjectRole {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Owner => "Owner",
            Self::Editor => "Editor",
            Self::Viewer => "Viewer",
        }
    }

    pub fn can_write(&self) -> bool {
        matches!(self, Self::Owner | Self::Editor)
    }
}

// ── Manifest repo operations ────────────────────────────────────────────────

/// Standard manifest filename.
pub const MANIFEST_FILENAME: &str = "projects.toml";
/// Legacy filename — read for back-compat, never written.
const LEGACY_MANIFEST_FILENAME: &str = "vaults.toml";

/// Load a manifest from a directory (the manifest repo root). Reads
/// `projects.toml` first, falls back to the legacy `vaults.toml` so
/// pre-rename installs keep working.
pub fn load_manifest(manifest_dir: &Path) -> anyhow::Result<ProjectManifest> {
    let path = manifest_dir.join(MANIFEST_FILENAME);
    let path = if path.exists() { path } else { manifest_dir.join(LEGACY_MANIFEST_FILENAME) };
    let content = fs::read_to_string(&path)?;
    let manifest: ProjectManifest = toml::from_str(&content)?;
    Ok(manifest)
}

/// Save a manifest to a directory.
pub fn save_manifest(manifest_dir: &Path, manifest: &ProjectManifest) -> anyhow::Result<()> {
    let path = manifest_dir.join(MANIFEST_FILENAME);
    let content = toml::to_string_pretty(manifest)?;
    fs::write(&path, content)?;
    Ok(())
}

/// Load a manifest with device-local path overrides from a sidecar file.
/// The sidecar (`projects.local.toml`) stores local_path mappings that aren't
/// committed to the manifest repo.
pub fn load_manifest_with_local(manifest_dir: &Path) -> anyhow::Result<ProjectManifest> {
    let mut manifest = load_manifest(manifest_dir)?;

    let local_path = manifest_dir.join("projects.local.toml");
    let local_path = if local_path.exists() { local_path } else { manifest_dir.join("vaults.local.toml") };
    if local_path.exists() {
        let content = fs::read_to_string(&local_path)?;
        let local: LocalManifest = toml::from_str(&content)?;
        for entry in &local.projects {
            if let Some(project) = manifest.projects.iter_mut().find(|v| v.name == entry.name) {
                project.local_path = Some(entry.local_path.clone());
            }
        }
    }

    Ok(manifest)
}

/// Save the device-local sidecar (not committed to git).
pub fn save_local_manifest(manifest_dir: &Path, manifest: &ProjectManifest) -> anyhow::Result<()> {
    let entries: Vec<LocalProjectEntry> = manifest.projects.iter()
        .filter_map(|v| {
            v.local_path.as_ref().map(|p| LocalProjectEntry {
                name: v.name.clone(),
                local_path: p.clone(),
            })
        })
        .collect();

    if entries.is_empty() {
        return Ok(());
    }

    let local = LocalManifest { projects: entries };
    let path = manifest_dir.join("projects.local.toml");
    fs::write(&path, toml::to_string_pretty(&local)?)?;

    // Ensure the sidecar is gitignored
    let gitignore = manifest_dir.join(".gitignore");
    let existing = fs::read_to_string(&gitignore).unwrap_or_default();
    if !existing.contains("projects.local.toml") {
        let mut content = existing;
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str("projects.local.toml\n");
        fs::write(&gitignore, content)?;
    }

    Ok(())
}

/// Add a project to the manifest. Returns error if name already exists.
pub fn add_project(manifest: &mut ProjectManifest, project: ManifestProject) -> anyhow::Result<()> {
    if manifest.projects.iter().any(|v| v.name == project.name) {
        anyhow::bail!("Project '{}' already exists in the manifest", project.name);
    }
    manifest.projects.push(project);
    Ok(())
}

/// Remove a project from the manifest by name.
pub fn remove_project(manifest: &mut ProjectManifest, name: &str) -> anyhow::Result<()> {
    let before = manifest.projects.len();
    manifest.projects.retain(|v| v.name != name);
    if manifest.projects.len() == before {
        anyhow::bail!("Project '{}' not found in the manifest", name);
    }
    Ok(())
}

// ── Local sidecar format ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalManifest {
    #[serde(default)]
    projects: Vec<LocalProjectEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalProjectEntry {
    name: String,
    local_path: PathBuf,
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_manifest() {
        let manifest = ProjectManifest {
            identity: ManifestIdentity {
                name: "Chris".into(),
                email: "chris@example.com".into(),
                fingerprint: Some("sha256:abc123".into()),
            },
            projects: vec![
                ManifestProject {
                    name: "Personal".into(),
                    repo: "git@github.com:user/project.git".into(),
                    branch: "main".into(),
                    role: ProjectRole::Owner,
                    hub: None,
                    local_path: None,
                    auto_commit_seconds: 60,
                },
                ManifestProject {
                    name: "Work".into(),
                    repo: "https://hub.styrene.io/org/work.git".into(),
                    branch: "main".into(),
                    role: ProjectRole::Editor,
                    hub: Some("hub.styrene.io".into()),
                    local_path: None,
                    auto_commit_seconds: 30,
                },
            ],
        };

        let toml = toml::to_string_pretty(&manifest).unwrap();
        let parsed: ProjectManifest = toml::from_str(&toml).unwrap();
        assert_eq!(manifest, parsed);
    }

    #[test]
    fn save_and_load_with_local_paths() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();

        let mut manifest = ProjectManifest {
            identity: ManifestIdentity::default(),
            projects: vec![ManifestProject {
                name: "Test".into(),
                repo: "git@example.com:test.git".into(),
                branch: "main".into(),
                role: ProjectRole::Owner,
                hub: None,
                local_path: Some(PathBuf::from("/Users/me/projects/test")),
                auto_commit_seconds: 60,
            }],
        };

        save_manifest(dir, &manifest).unwrap();
        save_local_manifest(dir, &manifest).unwrap();

        // Verify main manifest doesn't contain local_path
        let raw = fs::read_to_string(dir.join(MANIFEST_FILENAME)).unwrap();
        assert!(!raw.contains("local_path"), "local_path should not be in the synced manifest");

        // Verify sidecar has it
        let raw_local = fs::read_to_string(dir.join("projects.local.toml")).unwrap();
        assert!(raw_local.contains("/Users/me/projects/test"));

        // Verify gitignore
        let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains("projects.local.toml"));

        // Round-trip with local paths
        let loaded = load_manifest_with_local(dir).unwrap();
        assert_eq!(loaded.projects[0].local_path, Some(PathBuf::from("/Users/me/projects/test")));
    }

    #[test]
    fn add_and_remove_project() {
        let mut manifest = ProjectManifest {
            identity: ManifestIdentity::default(),
            projects: vec![],
        };

        add_project(&mut manifest, ManifestProject {
            name: "First".into(),
            repo: "git@example.com:first.git".into(),
            branch: "main".into(),
            role: ProjectRole::Owner,
            hub: None,
            local_path: None,
            auto_commit_seconds: 60,
        }).unwrap();
        assert_eq!(manifest.projects.len(), 1);

        // Duplicate name should fail
        assert!(add_project(&mut manifest, ManifestProject {
            name: "First".into(),
            repo: "git@example.com:other.git".into(),
            branch: "main".into(),
            role: ProjectRole::Owner,
            hub: None,
            local_path: None,
            auto_commit_seconds: 60,
        }).is_err());

        remove_project(&mut manifest, "First").unwrap();
        assert!(manifest.projects.is_empty());

        // Remove non-existent should fail
        assert!(remove_project(&mut manifest, "Nope").is_err());
    }

    #[test]
    fn role_permissions() {
        assert!(ProjectRole::Owner.can_write());
        assert!(ProjectRole::Editor.can_write());
        assert!(!ProjectRole::Viewer.can_write());
    }

    // ── Vault → Project rename: legacy filename fallbacks ──────────────────────

    fn empty_manifest_toml() -> String {
        let m = ProjectManifest {
            identity: ManifestIdentity::default(),
            projects: vec![],
        };
        toml::to_string_pretty(&m).unwrap()
    }

    #[test]
    fn load_manifest_falls_back_to_legacy_vaults_toml() {
        // Pre-rename install: only `vaults.toml` exists.
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(tmp.path().join("vaults.toml"), empty_manifest_toml()).unwrap();
        // No projects.toml; loader should still succeed.
        let loaded = load_manifest(tmp.path()).unwrap();
        assert!(loaded.projects.is_empty());
    }

    #[test]
    fn load_manifest_prefers_new_filename_when_both_present() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Legacy file with one entry, new file empty.
        let with_entry = ProjectManifest {
            identity: ManifestIdentity::default(),
            projects: vec![ManifestProject {
                name: "Legacy".into(),
                repo: "git@example.com:legacy.git".into(),
                branch: "main".into(),
                role: ProjectRole::Owner,
                hub: None,
                local_path: None,
                auto_commit_seconds: 60,
            }],
        };
        fs::write(tmp.path().join("vaults.toml"), toml::to_string_pretty(&with_entry).unwrap()).unwrap();
        fs::write(tmp.path().join(MANIFEST_FILENAME), empty_manifest_toml()).unwrap();
        // The new file (empty) wins, proving we don't silently read the legacy
        // copy when both are present.
        let loaded = load_manifest(tmp.path()).unwrap();
        assert!(loaded.projects.is_empty());
    }

    #[test]
    fn load_local_manifest_falls_back_to_legacy_vaults_local_toml() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Both the new and legacy main file are absent of a local_path entry,
        // but a legacy `vaults.local.toml` carries one. The loader should
        // pick it up.
        fs::write(tmp.path().join(MANIFEST_FILENAME), toml::to_string_pretty(&ProjectManifest {
            identity: ManifestIdentity::default(),
            projects: vec![ManifestProject {
                name: "Test".into(),
                repo: "git@example.com:test.git".into(),
                branch: "main".into(),
                role: ProjectRole::Owner,
                hub: None,
                local_path: None,
                auto_commit_seconds: 60,
            }],
        }).unwrap()).unwrap();

        let local = LocalManifest {
            projects: vec![LocalProjectEntry {
                name: "Test".into(),
                local_path: PathBuf::from("/legacy/path"),
            }],
        };
        fs::write(tmp.path().join("vaults.local.toml"), toml::to_string_pretty(&local).unwrap()).unwrap();

        let loaded = load_manifest_with_local(tmp.path()).unwrap();
        assert_eq!(loaded.projects[0].local_path, Some(PathBuf::from("/legacy/path")));
    }
}
