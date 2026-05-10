//! iCloud Drive sync for Flynt vaults.
//!
//! The simplest sync path: the project folder lives inside iCloud Drive.
//! Apple handles all file sync transparently. No server, no tokens, no git.
//!
//! On macOS: ~/Library/Mobile Documents/com~apple~CloudDrive/Flynt/
//! On iOS: app's iCloud container (requires entitlement)
//!
//! Flynt just needs to:
//! 1. Create/open the project in the right location
//! 2. Handle .icloud placeholder files (not-yet-downloaded)
//! 3. Detect and resolve conflicts (.icloud suffix files)

use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::info;

/// Get the iCloud Drive root on macOS.
pub fn icloud_drive_root() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("Library/Mobile Documents/com~apple~CloudDrive"))
}

/// Get the default Flynt project path inside iCloud Drive.
pub fn icloud_vault_path(vault_name: &str) -> Option<PathBuf> {
    icloud_drive_root().map(|root| root.join(vault_name))
}

/// Check if a path is inside iCloud Drive.
pub fn is_in_icloud(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    path_str.contains("Mobile Documents") || path_str.contains("iCloud")
}

/// Check if iCloud Drive is available (the directory exists and is accessible).
pub fn is_icloud_available() -> bool {
    icloud_drive_root()
        .map(|root| root.exists())
        .unwrap_or(false)
}

/// Download any .icloud placeholder files in the project.
/// On macOS, files in iCloud Drive may be evicted (replaced with .icloud stubs).
/// This triggers download of all markdown files.
pub fn ensure_downloaded(vault_root: &Path) -> Result<usize> {
    let mut count = 0;

    for entry in walkdir::WalkDir::new(vault_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // .icloud files are placeholders: ".filename.icloud"
        if name.starts_with('.') && name.ends_with(".icloud") {
            // Trigger download by reading the original filename
            let original_name = &name[1..name.len() - 7]; // strip leading . and trailing .icloud
            let original_path = path.parent().unwrap_or(path).join(original_name);

            if !original_path.exists() {
                // On macOS, opening the file triggers iCloud download
                // We can use brctl to request download
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("brctl")
                        .arg("download")
                        .arg(&original_path)
                        .output();
                    info!("Requested iCloud download: {}", original_path.display());
                    count += 1;
                }
            }
        }
    }

    if count > 0 {
        info!("Requested download of {count} iCloud placeholder(s)");
    }
    Ok(count)
}

/// Detect iCloud conflict files and list them.
/// iCloud creates files like "Note 2.md" when conflicts occur.
/// Returns pairs of (original_path, conflict_path).
pub fn detect_conflicts(vault_root: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut conflicts = Vec::new();

    for entry in walkdir::WalkDir::new(vault_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();

        // iCloud conflict pattern: "filename 2.md", "filename 3.md"
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if stem.ends_with(" 2") || stem.ends_with(" 3") || stem.ends_with(" 4") {
                let base = stem.rsplit_once(' ').map(|(b, _)| b).unwrap_or(stem);
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let original = path.parent().unwrap_or(path).join(format!("{base}.{ext}"));
                if original.exists() && original != path {
                    conflicts.push((original, path.to_path_buf()));
                }
            }
        }
    }

    conflicts
}

/// Create a new project in iCloud Drive.
pub fn create_icloud_vault(vault_name: &str) -> Result<PathBuf> {
    let root = icloud_vault_path(vault_name)
        .ok_or_else(|| anyhow::anyhow!("iCloud Drive not available"))?;

    if root.exists() {
        anyhow::bail!("Project '{}' already exists in iCloud Drive", vault_name);
    }

    std::fs::create_dir_all(&root)?;
    std::fs::create_dir_all(root.join(".flynt"))?;

    // Write config with iCloud sync
    let config = format!(
        r#"vault_name = "{vault_name}"

[sync]
backend = "icloud"

[appearance]
theme = "alpharius"
font_size = "medium"

[local_runtime]

[publication]
default_visibility = "private"
"#
    );
    std::fs::write(root.join(".flynt/config.toml"), config)?;

    info!("Created iCloud project at {}", root.display());
    Ok(root)
}
