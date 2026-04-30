//! Styrene Identity integration.
//!
//! Currently uses styrene-identity 0.1.x. When 0.3.0 is published,
//! this module should switch to the upstream `discover`, `format`,
//! `AllPublicKeys`, and `identity_hash` APIs.

use anyhow::Result;
use std::path::{Path, PathBuf};
use styrene_identity::derive::{KeyDeriver, KeyPurpose};

// ── Discovery ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IdentityStatus {
    pub available: bool,
    pub tier: String,
    pub label: String,
    pub path: String,
}

pub fn probe_identity() -> IdentityStatus {
    let path = styrene_identity::file_signer::FileSigner::default_path();
    if path.exists() {
        IdentityStatus {
            available: true,
            tier: "Tier D (encrypted file)".into(),
            label: "Styrene Identity".into(),
            path: path.display().to_string(),
        }
    } else {
        IdentityStatus {
            available: false,
            tier: "None".into(),
            label: "No identity configured".into(),
            path: path.display().to_string(),
        }
    }
}

pub fn has_identity() -> bool {
    styrene_identity::file_signer::FileSigner::default_path().exists()
}

// ── Creation ────────────────────────────────────────────────────────────────

pub fn create_identity(passphrase: &str) -> Result<PathBuf> {
    let path = styrene_identity::file_signer::FileSigner::default_path();
    if path.exists() {
        anyhow::bail!("Identity already exists at {}", path.display());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    use styrene_identity::file_signer::{ClosurePassphraseProvider, FileSigner};
    let pp = passphrase.as_bytes().to_vec();
    let provider = ClosurePassphraseProvider::new(move || Ok(pp.clone()));
    let signer = FileSigner::new(&path, Box::new(provider));
    signer.generate(passphrase.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to create identity: {e}"))?;

    Ok(path)
}

// ── Unlock ──────────────────────────────────────────────────────────────────

pub struct UnlockedIdentity {
    /// Canonical identity hash (hex). TODO: use identity_hash() from 0.3.0.
    pub identity_hash: String,
    /// SSH public key for git remote auth.
    pub ssh_auth_pubkey: String,
    /// SSH fingerprint.
    pub ssh_fingerprint: String,
    /// SSH public key for git commit signing.
    pub git_signing_pubkey: String,
}

pub fn unlock_identity(passphrase: &str) -> Result<UnlockedIdentity> {
    let path = styrene_identity::file_signer::FileSigner::default_path();
    if !path.exists() {
        anyhow::bail!("No identity file at {}", path.display());
    }

    use styrene_identity::file_signer::{ClosurePassphraseProvider, FileSigner};
    let pp = passphrase.as_bytes().to_vec();
    let provider = ClosurePassphraseProvider::new(move || Ok(pp.clone()));
    let signer = FileSigner::new(&path, Box::new(provider));
    let root_secret = match signer.load(passphrase.as_bytes()) {
        Ok(s) => s,
        Err(_) => {
            std::thread::sleep(std::time::Duration::from_secs(2));
            return Err(anyhow::anyhow!("Failed to unlock identity — wrong passphrase?"));
        }
    };

    let deriver = KeyDeriver::new(root_secret.as_bytes());

    // Git signing key
    let git_seed = deriver.derive(KeyPurpose::GitSigning);
    let git_signing_pubkey = format_ssh_pubkey(&git_seed, "codyx-signing");

    // SSH auth key (user key, not host key)
    let auth_seed = deriver.derive_ssh_user_key("git-auth")
        .map_err(|e| anyhow::anyhow!("SSH key derivation failed: {e}"))?;
    let ssh_auth_pubkey = format_ssh_pubkey(&auth_seed, "codyx@styrene");

    // Identity hash: SHA-256 of git signing verifying key, truncated to 16 bytes hex
    let identity_hash = {
        use ed25519_dalek::SigningKey;
        use sha2::{Sha256, Digest};
        let vk = SigningKey::from_bytes(&git_seed).verifying_key();
        let digest = Sha256::digest(vk.as_bytes());
        hex::encode(&digest[..16])
    };

    // SSH fingerprint
    let ssh_fingerprint = ssh_pubkey_fingerprint(&auth_seed);

    Ok(UnlockedIdentity {
        identity_hash,
        ssh_auth_pubkey,
        ssh_fingerprint,
        git_signing_pubkey,
    })
}

// ── Git signing config ──────────────────────────────────────────────────────

pub fn configure_git_signing(
    vault_root: &Path,
    ssh_pubkey: &str,
    name: Option<&str>,
    email: Option<&str>,
) -> Result<()> {
    let git_dir = vault_root.join(".git");
    if !git_dir.exists() {
        anyhow::bail!("Not a git repository: {}", vault_root.display());
    }

    let key_path = git_dir.join("styrene-signing-key.pub");
    std::fs::write(&key_path, ssh_pubkey)?;

    let repo = git2::Repository::open(vault_root)
        .map_err(|e| anyhow::anyhow!("Failed to open git repo: {e}"))?;
    let mut config = repo.config()
        .map_err(|e| anyhow::anyhow!("Failed to open git config: {e}"))?;

    config.set_str("commit.gpgsign", "true")?;
    config.set_str("gpg.format", "ssh")?;
    config.set_str("user.signingkey", &key_path.display().to_string())?;
    config.set_str("user.name", name.unwrap_or("Codyx"))?;
    config.set_str("user.email", email.unwrap_or("codyx@local"))?;

    Ok(())
}

// ── Formatting helpers (replaced by styrene_identity::format in 0.3.0) ─────

fn format_ssh_pubkey(seed: &[u8; 32], comment: &str) -> String {
    use ed25519_dalek::SigningKey;
    let vk = SigningKey::from_bytes(seed).verifying_key();
    let pubkey_bytes = vk.as_bytes();

    let key_type = b"ssh-ed25519";
    let mut wire = Vec::with_capacity(4 + key_type.len() + 4 + pubkey_bytes.len());
    wire.extend_from_slice(&(key_type.len() as u32).to_be_bytes());
    wire.extend_from_slice(key_type);
    wire.extend_from_slice(&(pubkey_bytes.len() as u32).to_be_bytes());
    wire.extend_from_slice(pubkey_bytes);

    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &wire);
    format!("ssh-ed25519 {b64} {comment}")
}

fn ssh_pubkey_fingerprint(seed: &[u8; 32]) -> String {
    use ed25519_dalek::SigningKey;
    use sha2::{Sha256, Digest};
    let vk = SigningKey::from_bytes(seed).verifying_key();
    let pubkey_bytes = vk.as_bytes();

    let key_type = b"ssh-ed25519";
    let mut wire = Vec::with_capacity(4 + key_type.len() + 4 + pubkey_bytes.len());
    wire.extend_from_slice(&(key_type.len() as u32).to_be_bytes());
    wire.extend_from_slice(key_type);
    wire.extend_from_slice(&(pubkey_bytes.len() as u32).to_be_bytes());
    wire.extend_from_slice(pubkey_bytes);

    let hash = Sha256::digest(&wire);
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD_NO_PAD, &hash);
    format!("SHA256:{b64}")
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_without_identity() {
        let status = probe_identity();
        assert!(!status.tier.is_empty());
    }

    #[test]
    fn ssh_pubkey_format() {
        let key = [42u8; 32];
        let result = format_ssh_pubkey(&key, "test");
        assert!(result.starts_with("ssh-ed25519 "));
        assert!(result.ends_with(" test"));
    }

    #[test]
    fn ssh_fingerprint_format() {
        let key = [42u8; 32];
        let fp = ssh_pubkey_fingerprint(&key);
        assert!(fp.starts_with("SHA256:"));
    }

    #[test]
    fn identity_hash_format() {
        // Verify hash is 32 hex chars
        use ed25519_dalek::SigningKey;
        use sha2::{Sha256, Digest};
        let seed = [42u8; 32];
        let vk = SigningKey::from_bytes(&seed).verifying_key();
        let digest = Sha256::digest(vk.as_bytes());
        let hash = hex::encode(&digest[..16]);
        assert_eq!(hash.len(), 32);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn git_signing_config_requires_git_repo() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = configure_git_signing(tmp.path(), "ssh-ed25519 AAAA test", None, None);
        assert!(result.is_err());
    }

    #[test]
    fn git_signing_config_writes_to_git_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        git2::Repository::init(tmp.path()).unwrap();

        configure_git_signing(
            tmp.path(), "ssh-ed25519 AAAA test@host",
            Some("Test User"), Some("test@example.com"),
        ).unwrap();

        let config = std::fs::read_to_string(tmp.path().join(".git/config")).unwrap();
        assert!(config.contains("gpgsign"));
        assert!(config.contains("ssh"));
        assert!(config.contains("signingkey"));
        assert!(config.contains("Test User"));
        assert!(config.contains("test@example.com"));
        assert!(tmp.path().join(".git/styrene-signing-key.pub").exists());
    }
}
