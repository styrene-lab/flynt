//! Cloud sync provider detection — finds locally-installed sync folders.
//!
//! Detects iCloud Drive, Google Drive, Dropbox, and OneDrive by checking
//! for their local sync directories. No API keys or authentication needed —
//! the provider's desktop client handles sync transparently.

use std::path::PathBuf;

/// A detected cloud sync provider.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudProvider {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// The local sync root directory.
    pub sync_root: PathBuf,
}

/// Detect all available cloud sync providers on this machine.
pub fn detect_providers() -> Vec<CloudProvider> {
    let mut providers = Vec::new();

    if let Some(p) = detect_icloud() { providers.push(p); }
    if let Some(p) = detect_google_drive() { providers.push(p); }
    if let Some(p) = detect_dropbox() { providers.push(p); }
    if let Some(p) = detect_onedrive() { providers.push(p); }

    providers
}

/// Get the project path within a cloud provider's sync directory.
pub fn vault_path_for_provider(provider: &CloudProvider, vault_name: &str) -> PathBuf {
    provider.sync_root.join(vault_name)
}

/// Create a project inside a cloud provider's sync directory.
pub fn create_cloud_vault(
    provider: &CloudProvider,
    vault_name: &str,
) -> anyhow::Result<PathBuf> {
    let vault_root = vault_path_for_provider(provider, vault_name);
    if vault_root.exists() {
        anyhow::bail!("Project '{}' already exists in {}", vault_name, provider.label);
    }
    std::fs::create_dir_all(vault_root.join(".flynt"))?;

    // Determine the sync config based on provider type
    let sync_backend = match provider.id {
        "icloud" => "icloud",
        _ => "none", // filesystem sync providers don't need a backend — they just sync the folder
    };

    let config = format!(
        r#"vault_name = "{vault_name}"

[sync]
backend = "{sync_backend}"

[appearance]
theme = "alpharius"
"#
    );
    std::fs::write(vault_root.join(".flynt/config.toml"), config)?;

    Ok(vault_root)
}

// ── Provider detection ──────────────────────────────────────────────────────

fn detect_icloud() -> Option<CloudProvider> {
    #[cfg(target_os = "macos")]
    {
        let path = dirs::home_dir()?.join("Library/Mobile Documents/com~apple~CloudDocs");
        if path.is_dir() {
            return Some(CloudProvider {
                id: "icloud",
                label: "iCloud Drive",
                description: "Syncs automatically between Apple devices",
                sync_root: path,
            });
        }
    }
    None
}

fn detect_google_drive() -> Option<CloudProvider> {
    let home = dirs::home_dir()?;

    // macOS: Google Drive for Desktop
    #[cfg(target_os = "macos")]
    {
        let cloud_storage = home.join("Library/CloudStorage");
        if cloud_storage.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&cloud_storage) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("GoogleDrive") {
                        let my_drive = entry.path().join("My Drive");
                        if my_drive.is_dir() {
                            return Some(CloudProvider {
                                id: "google-drive",
                                label: "Google Drive",
                                description: "15 GB free, syncs across all platforms",
                                sync_root: my_drive,
                            });
                        }
                    }
                }
            }
        }
    }

    // Linux: common mount points
    #[cfg(target_os = "linux")]
    {
        for candidate in [
            home.join("Google Drive"),
            home.join("google-drive"),
            PathBuf::from("/mnt/gdrive"),
        ] {
            if candidate.is_dir() {
                return Some(CloudProvider {
                    id: "google-drive",
                    label: "Google Drive",
                    description: "15 GB free, syncs across all platforms",
                    sync_root: candidate,
                });
            }
        }
    }

    None
}

fn detect_dropbox() -> Option<CloudProvider> {
    let home = dirs::home_dir()?;

    // Check common Dropbox locations
    for candidate in [
        home.join("Dropbox"),
        #[cfg(target_os = "macos")]
        home.join("Library/CloudStorage/Dropbox"),
    ] {
        if candidate.is_dir() {
            return Some(CloudProvider {
                id: "dropbox",
                label: "Dropbox",
                description: "2 GB free, widely supported",
                sync_root: candidate,
            });
        }
    }

    None
}

fn detect_onedrive() -> Option<CloudProvider> {
    let home = dirs::home_dir()?;

    // macOS: OneDrive desktop client
    #[cfg(target_os = "macos")]
    {
        let cloud_storage = home.join("Library/CloudStorage");
        if cloud_storage.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&cloud_storage) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("OneDrive") {
                        return Some(CloudProvider {
                            id: "onedrive",
                            label: "OneDrive",
                            description: "5 GB free, included with Microsoft 365",
                            sync_root: entry.path(),
                        });
                    }
                }
            }
        }
    }

    // Linux / fallback
    for candidate in [
        home.join("OneDrive"),
        home.join("onedrive"),
    ] {
        if candidate.is_dir() {
            return Some(CloudProvider {
                id: "onedrive",
                label: "OneDrive",
                description: "5 GB free, included with Microsoft 365",
                sync_root: candidate,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_path_for_provider_appends_name() {
        let provider = CloudProvider {
            id: "test",
            label: "Test",
            description: "test provider",
            sync_root: PathBuf::from("/cloud/sync"),
        };
        let path = vault_path_for_provider(&provider, "MyVault");
        assert_eq!(path, PathBuf::from("/cloud/sync/MyVault"));
    }

    #[test]
    fn create_cloud_vault_creates_directory_and_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        let provider = CloudProvider {
            id: "test",
            label: "Test Cloud",
            description: "test",
            sync_root: tmp.path().to_path_buf(),
        };

        let result = create_cloud_vault(&provider, "TestVault").unwrap();
        assert!(result.exists());
        assert!(result.join(".flynt/config.toml").exists());

        let config = std::fs::read_to_string(result.join(".flynt/config.toml")).unwrap();
        assert!(config.contains("TestVault"));
    }

    #[test]
    fn create_cloud_vault_rejects_existing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let provider = CloudProvider {
            id: "test",
            label: "Test",
            description: "test",
            sync_root: tmp.path().to_path_buf(),
        };

        create_cloud_vault(&provider, "Existing").unwrap();
        let result = create_cloud_vault(&provider, "Existing");
        assert!(result.is_err());
    }

    #[test]
    fn detect_providers_does_not_panic() {
        let providers = detect_providers();
        for p in &providers {
            assert!(!p.id.is_empty());
            assert!(!p.label.is_empty());
        }
    }
}
