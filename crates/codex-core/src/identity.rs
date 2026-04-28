//! Styrene Identity integration — key derivation, SSH key management, git signing.
//!
//! Uses the `styrene-identity` crate to derive deterministic keys from a single
//! root secret. The operator creates an identity once (with a passphrase),
//! and all protocol keys are derived automatically:
//!
//! - SSH keys for git authentication (per-remote)
//! - Git commit signing key
//! - Vault manifest fingerprint

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use styrene_identity::derive::{KeyDeriver, KeyPurpose};

/// Default identity file location.
pub fn identity_path() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".styrene/identity.key"))
        .unwrap_or_else(|_| PathBuf::from(".styrene/identity.key"))
}

/// Check if an identity file exists.
pub fn has_identity() -> bool {
    identity_path().exists()
}

/// Identity status for the UI.
#[derive(Debug, Clone)]
pub struct IdentityStatus {
    pub available: bool,
    pub tier: String,
    pub label: String,
    pub path: String,
}

/// Probe the current identity status.
pub fn probe_identity() -> IdentityStatus {
    let path = identity_path();
    if !path.exists() {
        return IdentityStatus {
            available: false,
            tier: "None".into(),
            label: "No identity configured".into(),
            path: path.display().to_string(),
        };
    }

    IdentityStatus {
        available: true,
        tier: "Tier D (encrypted file)".into(),
        label: "Styrene Identity".into(),
        path: path.display().to_string(),
    }
}

/// Create a new identity with a passphrase.
pub fn create_identity(passphrase: &str) -> Result<PathBuf> {
    let path = identity_path();
    if path.exists() {
        anyhow::bail!("Identity already exists at {}", path.display());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    use styrene_identity::file_signer::{FileSigner, ClosurePassphraseProvider};
    let pp = passphrase.as_bytes().to_vec();
    let provider = ClosurePassphraseProvider::new(move || Ok(pp.clone()));
    let signer = FileSigner::new(&path, Box::new(provider));
    signer.generate(passphrase.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to create identity: {e}"))?;

    Ok(path)
}

/// Unlock an existing identity and derive keys.
pub fn unlock_identity(passphrase: &str) -> Result<UnlockedIdentity> {
    let path = identity_path();
    if !path.exists() {
        anyhow::bail!("No identity file at {}", path.display());
    }

    use styrene_identity::file_signer::{FileSigner, ClosurePassphraseProvider};
    let pp = passphrase.as_bytes().to_vec();
    let provider = ClosurePassphraseProvider::new(move || Ok(pp.clone()));
    let signer = FileSigner::new(&path, Box::new(provider));
    let root_secret = signer.load(passphrase.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to unlock identity: {e}"))?;

    let deriver = KeyDeriver::new(root_secret.as_bytes());

    // Derive git signing key
    let git_signing_bytes = deriver.derive(KeyPurpose::GitSigning);

    // Fingerprint: hex of first 16 bytes of git signing key
    let fingerprint = hex_fingerprint(&git_signing_bytes);

    // SSH public key for display
    let ssh_pubkey = format_ssh_ed25519_pubkey(&git_signing_bytes, "codyx@styrene");

    Ok(UnlockedIdentity {
        fingerprint,
        ssh_pubkey,
        git_signing_key: git_signing_bytes,
        deriver,
    })
}

/// An unlocked identity with derived keys.
pub struct UnlockedIdentity {
    pub fingerprint: String,
    pub ssh_pubkey: String,
    pub git_signing_key: [u8; 32],
    deriver: KeyDeriver,
}

impl UnlockedIdentity {
    /// Derive an SSH public key for a specific remote host.
    pub fn ssh_key_for_remote(&self, remote_host: &str) -> Result<String> {
        let key_bytes = self.deriver.derive_ssh_user_key(remote_host)
            .map_err(|e| anyhow::anyhow!("Key derivation failed: {e}"))?;
        Ok(format_ssh_ed25519_pubkey(&key_bytes, &format!("codyx@{remote_host}")))
    }
}

/// Configure git commit signing for a vault.
pub fn configure_git_signing(vault_root: &Path, ssh_pubkey: &str) -> Result<()> {
    let git_dir = vault_root.join(".git");
    if !git_dir.exists() {
        anyhow::bail!("Not a git repository: {}", vault_root.display());
    }

    // Write the public key
    let key_path = git_dir.join("styrene-signing-key.pub");
    std::fs::write(&key_path, ssh_pubkey)?;

    // Append git config for SSH signing
    let config_path = git_dir.join("config");
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();

    let mut additions = String::new();
    if !existing.contains("[commit]") {
        additions.push_str("[commit]\n\tgpgsign = true\n");
    }
    if !existing.contains("gpg.format") {
        additions.push_str("[gpg]\n\tformat = ssh\n");
    }
    if !existing.contains("user.signingkey") {
        additions.push_str(&format!("[user]\n\tsigningkey = {}\n", key_path.display()));
    }

    if !additions.is_empty() {
        let mut config = existing;
        config.push('\n');
        config.push_str(&additions);
        std::fs::write(&config_path, config)?;
    }

    Ok(())
}

/// Format 32 bytes as an SSH Ed25519 public key string.
fn format_ssh_ed25519_pubkey(seed_bytes: &[u8; 32], comment: &str) -> String {
    // Derive the actual Ed25519 public key from the seed
    use ed25519_dalek::SigningKey;
    let signing_key = SigningKey::from_bytes(seed_bytes);
    let verifying_key = signing_key.verifying_key();
    let pubkey_bytes = verifying_key.as_bytes();

    // SSH wire format
    let mut wire = Vec::new();
    let key_type = b"ssh-ed25519";
    wire.extend_from_slice(&(key_type.len() as u32).to_be_bytes());
    wire.extend_from_slice(key_type);
    wire.extend_from_slice(&(pubkey_bytes.len() as u32).to_be_bytes());
    wire.extend_from_slice(pubkey_bytes);

    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &wire,
    );
    format!("ssh-ed25519 {encoded} {comment}")
}

/// Short hex fingerprint for display.
fn hex_fingerprint(key_bytes: &[u8; 32]) -> String {
    key_bytes[..8].iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_pubkey_format() {
        let key = [42u8; 32];
        let result = format_ssh_ed25519_pubkey(&key, "test");
        assert!(result.starts_with("ssh-ed25519 "));
        assert!(result.ends_with(" test"));
    }

    #[test]
    fn fingerprint_format() {
        let key = [0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(hex_fingerprint(&key), "ab:cd:ef:01:23:45:67:89");
    }

    #[test]
    fn probe_without_identity() {
        let status = probe_identity();
        assert!(!status.tier.is_empty());
    }

    #[test]
    fn git_signing_config_requires_git_repo() {
        let tmp = tempfile::TempDir::new().unwrap();
        // No .git directory
        let result = configure_git_signing(tmp.path(), "ssh-ed25519 AAAA test");
        assert!(result.is_err());
    }

    #[test]
    fn git_signing_config_writes_to_git_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        let git_dir = tmp.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("config"), "").unwrap();

        configure_git_signing(tmp.path(), "ssh-ed25519 AAAA test@host").unwrap();

        let config = std::fs::read_to_string(git_dir.join("config")).unwrap();
        assert!(config.contains("gpgsign = true"));
        assert!(config.contains("format = ssh"));
        assert!(config.contains("signingkey"));

        // Key file should exist
        assert!(git_dir.join("styrene-signing-key.pub").exists());
    }
}
