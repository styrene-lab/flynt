//! Vault manifest — a registry of vaults synced across devices.
//!
//! The manifest lives in its own small git repo (or a local file synced via
//! iCloud). It lists all vaults the operator has access to, with their sync
//! coordinates. On a new device, the operator clones the manifest repo and
//! Flynt discovers all their vaults from it.
//!
//! ## File format
//!
//! `vaults.toml` at the manifest repo root:
//!
//! ```toml
//! [identity]
//! name = "Chris Wilson"
//! email = "chris@example.com"
//! fingerprint = "sha256:abc123..."
//!
//! [[vaults]]
//! name = "Personal"
//! repo = "git@github.com:user/codex-personal.git"
//! branch = "main"
//! role = "owner"
//!
//! [[vaults]]
//! name = "Work"
//! repo = "https://hub.styrene.io/org/codex-work.git"
//! branch = "main"
//! role = "editor"
//! hub = "hub.styrene.io"
//! ```

use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

/// The vault manifest — serialized as `vaults.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VaultManifest {
    /// The operator's identity metadata.
    #[serde(default)]
    pub identity: ManifestIdentity,
    /// All known vaults.
    #[serde(default)]
    pub vaults: Vec<ManifestVault>,
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

/// A vault entry in the manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestVault {
    /// Human-readable vault name.
    pub name: String,
    /// Git remote URL (SSH or HTTPS).
    pub repo: String,
    /// Branch to sync.
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Operator's role in this vault.
    #[serde(default)]
    pub role: VaultRole,
    /// If hosted on a Styrene Hub, the hub hostname.
    #[serde(default)]
    pub hub: Option<String>,
    /// Local path where this vault is cloned (device-specific, not synced).
    /// Populated after cloning. Stripped when serializing to the manifest repo.
    #[serde(skip_serializing, default)]
    pub local_path: Option<PathBuf>,
    /// Auto-commit interval in seconds (0 = manual).
    #[serde(default = "default_auto_commit")]
    pub auto_commit_seconds: u64,
}

fn default_branch() -> String { "main".into() }
fn default_auto_commit() -> u64 { 60 }

/// Role in a vault — determines default mutability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VaultRole {
    /// Full control — can modify, delete, manage access.
    #[default]
    Owner,
    /// Can read and write vault content.
    Editor,
    /// Read-only access.
    Viewer,
}

impl VaultRole {
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
pub const MANIFEST_FILENAME: &str = "vaults.toml";

/// Load a manifest from a directory (the manifest repo root).
pub fn load_manifest(manifest_dir: &Path) -> anyhow::Result<VaultManifest> {
    let path = manifest_dir.join(MANIFEST_FILENAME);
    let content = fs::read_to_string(&path)?;
    let manifest: VaultManifest = toml::from_str(&content)?;
    Ok(manifest)
}

/// Save a manifest to a directory.
pub fn save_manifest(manifest_dir: &Path, manifest: &VaultManifest) -> anyhow::Result<()> {
    let path = manifest_dir.join(MANIFEST_FILENAME);
    let content = toml::to_string_pretty(manifest)?;
    fs::write(&path, content)?;
    Ok(())
}

/// Load a manifest with device-local path overrides from a sidecar file.
/// The sidecar (`vaults.local.toml`) stores local_path mappings that aren't
/// committed to the manifest repo.
pub fn load_manifest_with_local(manifest_dir: &Path) -> anyhow::Result<VaultManifest> {
    let mut manifest = load_manifest(manifest_dir)?;

    let local_path = manifest_dir.join("vaults.local.toml");
    if local_path.exists() {
        let content = fs::read_to_string(&local_path)?;
        let local: LocalManifest = toml::from_str(&content)?;
        for entry in &local.vaults {
            if let Some(vault) = manifest.vaults.iter_mut().find(|v| v.name == entry.name) {
                vault.local_path = Some(entry.local_path.clone());
            }
        }
    }

    Ok(manifest)
}

/// Save the device-local sidecar (not committed to git).
pub fn save_local_manifest(manifest_dir: &Path, manifest: &VaultManifest) -> anyhow::Result<()> {
    let entries: Vec<LocalVaultEntry> = manifest.vaults.iter()
        .filter_map(|v| {
            v.local_path.as_ref().map(|p| LocalVaultEntry {
                name: v.name.clone(),
                local_path: p.clone(),
            })
        })
        .collect();

    if entries.is_empty() {
        return Ok(());
    }

    let local = LocalManifest { vaults: entries };
    let path = manifest_dir.join("vaults.local.toml");
    fs::write(&path, toml::to_string_pretty(&local)?)?;

    // Ensure the sidecar is gitignored
    let gitignore = manifest_dir.join(".gitignore");
    let existing = fs::read_to_string(&gitignore).unwrap_or_default();
    if !existing.contains("vaults.local.toml") {
        let mut content = existing;
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str("vaults.local.toml\n");
        fs::write(&gitignore, content)?;
    }

    Ok(())
}

/// Add a vault to the manifest. Returns error if name already exists.
pub fn add_vault(manifest: &mut VaultManifest, vault: ManifestVault) -> anyhow::Result<()> {
    if manifest.vaults.iter().any(|v| v.name == vault.name) {
        anyhow::bail!("Vault '{}' already exists in the manifest", vault.name);
    }
    manifest.vaults.push(vault);
    Ok(())
}

/// Remove a vault from the manifest by name.
pub fn remove_vault(manifest: &mut VaultManifest, name: &str) -> anyhow::Result<()> {
    let before = manifest.vaults.len();
    manifest.vaults.retain(|v| v.name != name);
    if manifest.vaults.len() == before {
        anyhow::bail!("Vault '{}' not found in the manifest", name);
    }
    Ok(())
}

// ── Local sidecar format ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalManifest {
    #[serde(default)]
    vaults: Vec<LocalVaultEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalVaultEntry {
    name: String,
    local_path: PathBuf,
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_manifest() {
        let manifest = VaultManifest {
            identity: ManifestIdentity {
                name: "Chris".into(),
                email: "chris@example.com".into(),
                fingerprint: Some("sha256:abc123".into()),
            },
            vaults: vec![
                ManifestVault {
                    name: "Personal".into(),
                    repo: "git@github.com:user/vault.git".into(),
                    branch: "main".into(),
                    role: VaultRole::Owner,
                    hub: None,
                    local_path: None,
                    auto_commit_seconds: 60,
                },
                ManifestVault {
                    name: "Work".into(),
                    repo: "https://hub.styrene.io/org/work.git".into(),
                    branch: "main".into(),
                    role: VaultRole::Editor,
                    hub: Some("hub.styrene.io".into()),
                    local_path: None,
                    auto_commit_seconds: 30,
                },
            ],
        };

        let toml = toml::to_string_pretty(&manifest).unwrap();
        let parsed: VaultManifest = toml::from_str(&toml).unwrap();
        assert_eq!(manifest, parsed);
    }

    #[test]
    fn save_and_load_with_local_paths() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();

        let mut manifest = VaultManifest {
            identity: ManifestIdentity::default(),
            vaults: vec![ManifestVault {
                name: "Test".into(),
                repo: "git@example.com:test.git".into(),
                branch: "main".into(),
                role: VaultRole::Owner,
                hub: None,
                local_path: Some(PathBuf::from("/Users/me/vaults/test")),
                auto_commit_seconds: 60,
            }],
        };

        save_manifest(dir, &manifest).unwrap();
        save_local_manifest(dir, &manifest).unwrap();

        // Verify main manifest doesn't contain local_path
        let raw = fs::read_to_string(dir.join(MANIFEST_FILENAME)).unwrap();
        assert!(!raw.contains("local_path"), "local_path should not be in the synced manifest");

        // Verify sidecar has it
        let raw_local = fs::read_to_string(dir.join("vaults.local.toml")).unwrap();
        assert!(raw_local.contains("/Users/me/vaults/test"));

        // Verify gitignore
        let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains("vaults.local.toml"));

        // Round-trip with local paths
        let loaded = load_manifest_with_local(dir).unwrap();
        assert_eq!(loaded.vaults[0].local_path, Some(PathBuf::from("/Users/me/vaults/test")));
    }

    #[test]
    fn add_and_remove_vault() {
        let mut manifest = VaultManifest {
            identity: ManifestIdentity::default(),
            vaults: vec![],
        };

        add_vault(&mut manifest, ManifestVault {
            name: "First".into(),
            repo: "git@example.com:first.git".into(),
            branch: "main".into(),
            role: VaultRole::Owner,
            hub: None,
            local_path: None,
            auto_commit_seconds: 60,
        }).unwrap();
        assert_eq!(manifest.vaults.len(), 1);

        // Duplicate name should fail
        assert!(add_vault(&mut manifest, ManifestVault {
            name: "First".into(),
            repo: "git@example.com:other.git".into(),
            branch: "main".into(),
            role: VaultRole::Owner,
            hub: None,
            local_path: None,
            auto_commit_seconds: 60,
        }).is_err());

        remove_vault(&mut manifest, "First").unwrap();
        assert!(manifest.vaults.is_empty());

        // Remove non-existent should fail
        assert!(remove_vault(&mut manifest, "Nope").is_err());
    }

    #[test]
    fn role_permissions() {
        assert!(VaultRole::Owner.can_write());
        assert!(VaultRole::Editor.can_write());
        assert!(!VaultRole::Viewer.can_write());
    }
}
