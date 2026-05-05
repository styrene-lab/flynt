//! Vault migration — move a vault between sync locations.
//!
//! Handles: Local ↔ iCloud, Local → Git, iCloud → Git, any → any.
//! The migration copies vault content (all files except `.flynt-local/`),
//! writes a new config at the destination, and returns the new root path.

use anyhow::{Context, Result};
use flynt_core::models::{SyncConfig, VaultConfig};
use std::path::{Path, PathBuf};

/// Result of a successful migration.
#[derive(Debug)]
pub struct MigrationResult {
    /// The new vault root directory.
    pub new_root: PathBuf,
    /// Number of files copied.
    pub files_copied: usize,
    /// Whether the old location was removed.
    pub old_removed: bool,
}

/// Migrate a vault from its current location to a new sync target.
///
/// - `current_root`: the existing vault directory
/// - `new_sync`: the desired sync configuration
/// - `remove_old`: whether to delete the old vault after migration
///
/// For iCloud: destination is `~/Library/Mobile Documents/com~apple~CloudDocs/<vault_name>/`
/// For Git: destination stays the same (just init a repo + set remote)
/// For Local from iCloud: destination is `~/Documents/<vault_name>/`
pub fn migrate_vault(
    current_root: &Path,
    vault_name: &str,
    new_sync: &SyncConfig,
    remove_old: bool,
) -> Result<MigrationResult> {
    let new_root = destination_for_sync(vault_name, new_sync, current_root)?;

    if new_root == current_root {
        // Same location — just update the config in place
        update_config_sync(current_root, new_sync)?;
        if let SyncConfig::Git { remote, branch, .. } = new_sync {
            init_git_if_needed(current_root, remote, branch)?;
        }
        return Ok(MigrationResult {
            new_root: current_root.to_path_buf(),
            files_copied: 0,
            old_removed: false,
        });
    }

    // Copy vault contents to new location
    let files_copied = copy_vault(current_root, &new_root)?;

    // Update config at destination
    update_config_sync(&new_root, new_sync)?;

    // Init git repo if moving to git sync
    if let SyncConfig::Git { remote, branch, .. } = new_sync {
        init_git_if_needed(&new_root, remote, branch)?;
    }

    // Remove old location if requested
    let old_removed = if remove_old && current_root != new_root {
        std::fs::remove_dir_all(current_root).is_ok()
    } else {
        false
    };

    Ok(MigrationResult {
        new_root,
        files_copied,
        old_removed,
    })
}

/// Determine the destination path for a sync target.
fn destination_for_sync(
    vault_name: &str,
    sync: &SyncConfig,
    current_root: &Path,
) -> Result<PathBuf> {
    match sync {
        SyncConfig::ICloud => {
            super::sync::icloud::icloud_vault_path(vault_name)
                .ok_or_else(|| anyhow::anyhow!(
                    "iCloud Drive is not available. Enable it in System Settings > Apple ID > iCloud > iCloud Drive."
                ))
        }
        SyncConfig::None => {
            // Move to ~/Documents/<vault_name>
            let docs = dirs::document_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));
            Ok(docs.join(vault_name))
        }
        SyncConfig::Git { .. } | SyncConfig::S3 { .. } | SyncConfig::Forge { .. } => {
            // Git/S3/Forge: stay in current location, just change config
            Ok(current_root.to_path_buf())
        }
    }
}

/// Copy all vault files to a new directory, excluding `.flynt-local/` and `.git/`.
fn copy_vault(src: &Path, dst: &Path) -> Result<usize> {
    if dst.exists() {
        // Check if destination is non-empty
        let has_content = std::fs::read_dir(dst)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);
        if has_content {
            anyhow::bail!(
                "Destination already exists and is not empty: {}",
                dst.display()
            );
        }
    }

    std::fs::create_dir_all(dst)?;
    let mut count = 0;

    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip local state and git internals
            name != ".flynt-local" && name != ".git" && name != ".DS_Store"
        })
    {
        let entry = entry?;
        let relative = entry.path().strip_prefix(src)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let dest_path = dst.join(relative);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &dest_path)?;
            count += 1;
        }
    }

    Ok(count)
}

/// Update the sync field in the vault's config.toml, preserving all other fields.
fn update_config_sync(vault_root: &Path, sync: &SyncConfig) -> Result<()> {
    let config_path = vault_root.join(".flynt/config.toml");
    std::fs::create_dir_all(vault_root.join(".flynt"))?;

    if config_path.exists() {
        // Preserve existing config — only replace [sync] section
        let existing = std::fs::read_to_string(&config_path)?;
        let mut doc: toml_edit::DocumentMut = existing.parse()
            .context("Failed to parse config.toml")?;

        // Serialize just the sync value and merge it in
        let sync_toml = toml::to_string(sync)?;
        let sync_value: toml_edit::DocumentMut = sync_toml.parse()?;
        doc["sync"] = sync_value.as_item().clone();

        std::fs::write(&config_path, doc.to_string())?;
    } else {
        // No existing config — create from scratch
        let config = VaultConfig {
            vault_name: vault_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Flynt")
                .to_string(),
            sync: sync.clone(),
            ..Default::default()
        };
        std::fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    }
    Ok(())
}

/// Initialize a git repo at the vault root if one doesn't exist.
fn init_git_if_needed(vault_root: &Path, remote: &str, _branch: &str) -> Result<()> {
    if vault_root.join(".git").exists() {
        return Ok(());
    }

    let repo = git2::Repository::init(vault_root)?;

    // Add remote
    repo.remote("origin", remote)?;

    // Create .gitignore if it doesn't exist
    let gitignore = vault_root.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(
            &gitignore,
            ".flynt-local/\n.DS_Store\n*.swp\n*~\n",
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn migrate_to_git_stays_in_place() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        std::fs::create_dir_all(root.join(".flynt")).unwrap();
        std::fs::write(
            root.join(".flynt/config.toml"),
            "vault_name = \"test\"\n\n[sync]\nbackend = \"none\"\n",
        ).unwrap();
        std::fs::write(root.join("note.md"), "# Hello").unwrap();

        let sync = SyncConfig::Git {
            remote: "git@github.com:user/vault.git".into(),
            branch: "main".into(),
            auto_commit_seconds: 60,
        };
        // Git migration stays in same directory — just inits repo + updates config
        let result = migrate_vault(&root, "test", &sync, false).unwrap();
        assert_eq!(result.new_root, root);
        assert_eq!(result.files_copied, 0);
        assert!(root.join(".git").exists());
    }

    #[test]
    fn migrate_copies_files_excludes_local_state() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src-vault");
        let dst = tmp.path().join("dst-vault");

        std::fs::create_dir_all(src.join(".flynt")).unwrap();
        std::fs::create_dir_all(src.join(".flynt-local/db")).unwrap();
        std::fs::create_dir_all(src.join(".git/objects")).unwrap();
        std::fs::write(src.join("note.md"), "# Note").unwrap();
        std::fs::write(src.join(".flynt/config.toml"), "vault_name = \"test\"").unwrap();
        std::fs::write(src.join(".flynt-local/db/index.db"), "binary").unwrap();
        std::fs::write(src.join(".git/objects/abc"), "git obj").unwrap();

        let count = copy_vault(&src, &dst).unwrap();
        assert_eq!(count, 2); // note.md + config.toml
        assert!(dst.join("note.md").exists());
        assert!(dst.join(".flynt/config.toml").exists());
        assert!(!dst.join(".flynt-local").exists());
        assert!(!dst.join(".git").exists());
    }

    #[test]
    fn migrate_to_git_inits_repo() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        std::fs::create_dir_all(root.join(".flynt")).unwrap();
        std::fs::write(
            root.join(".flynt/config.toml"),
            "vault_name = \"test\"\n\n[sync]\nbackend = \"none\"\n",
        ).unwrap();

        let sync = SyncConfig::Git {
            remote: "git@github.com:user/vault.git".into(),
            branch: "main".into(),
            auto_commit_seconds: 60,
        };
        let result = migrate_vault(&root, "test", &sync, false).unwrap();
        assert!(root.join(".git").exists());
        assert!(root.join(".gitignore").exists());
        assert_eq!(result.new_root, root);
    }
}
