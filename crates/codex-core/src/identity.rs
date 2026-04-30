//! Styrene Identity integration — uses the upstream 0.3.0 APIs for
//! discovery, key derivation, formatting, and git signing.

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

/// Probe the machine for an existing identity (no passphrase needed).
pub fn probe_identity() -> IdentityStatus {
    match styrene_identity::discover::discover() {
        Some(found) => IdentityStatus {
            available: true,
            tier: format!("{:?}", found.tier),
            label: found.label,
            path: found.path.display().to_string(),
        },
        None => IdentityStatus {
            available: false,
            tier: "None".into(),
            label: "No identity configured".into(),
            path: styrene_identity::file_signer::FileSigner::default_path()
                .display().to_string(),
        },
    }
}

pub fn has_identity() -> bool {
    styrene_identity::discover::discover().is_some()
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
    /// Canonical 32-char hex identity hash.
    pub identity_hash: String,
    /// SSH public key for git remote auth (OpenSSH format).
    pub ssh_auth_pubkey: String,
    /// SSH fingerprint (SHA256:... format).
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

    // Use upstream APIs for canonical derivation + formatting
    let identity_hash = styrene_identity::identity_hash(&root_secret);

    let deriver = KeyDeriver::new(root_secret.as_bytes());

    // Git/identity signing key (unified Signing purpose in 0.3.0)
    let git_seed = deriver.derive(KeyPurpose::Signing);
    let git_signing_pubkey = styrene_identity::format::ssh_pubkey(&git_seed, "codyx-signing");

    // SSH auth key for git remotes (user key, not host)
    let auth_seed = deriver.derive_ssh_user_key("git-auth")
        .map_err(|e| anyhow::anyhow!("SSH key derivation failed: {e}"))?;
    let ssh_auth_pubkey = styrene_identity::format::ssh_pubkey(&auth_seed, "codyx@styrene");
    let ssh_fingerprint = styrene_identity::format::ssh_pubkey_fingerprint(&auth_seed);

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
    fn git_signing_config_requires_git_repo() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(configure_git_signing(tmp.path(), "ssh-ed25519 AAAA test", None, None).is_err());
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
