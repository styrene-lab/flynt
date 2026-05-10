//! Styrene Identity integration — uses the upstream 0.3.x APIs for
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

// ── Keychain (Tier B) ──────────────────────────────────────────────────────

/// Check if the platform supports Keychain biometric identity.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn keychain_available() -> bool {
    true
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub fn keychain_available() -> bool {
    false
}

/// Check if a biometric identity already exists in the Keychain.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn has_keychain_identity() -> bool {
    styrene_identity::keychain_signer::KeychainSigner::default().exists()
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub fn has_keychain_identity() -> bool {
    false
}

/// Create a new biometric-protected identity in the Keychain.
/// Triggers a Face ID / Touch ID prompt immediately.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn create_keychain_identity() -> Result<()> {
    use styrene_identity::keychain_signer::KeychainSigner;
    let signer = KeychainSigner::default();
    signer.create()
        .map_err(|e| anyhow::anyhow!("Keychain identity creation failed: {e}"))
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub fn create_keychain_identity() -> Result<()> {
    anyhow::bail!("Keychain identity is only available on macOS and iOS")
}

/// Unlock a Keychain identity via biometrics and return derived keys.
/// Triggers Face ID / Touch ID.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn unlock_keychain_identity() -> Result<UnlockedIdentity> {
    use security_framework::passwords::{generic_password, PasswordOptions};
    use styrene_identity::keychain_signer;

    let data = generic_password(
        PasswordOptions::new_generic_password(
            keychain_signer::SERVICE,
            keychain_signer::ACCOUNT,
        ),
    ).map_err(|e| {
        let code = e.code();
        if code == -25293 || code == -128 {
            anyhow::anyhow!("Biometric authentication cancelled")
        } else if code == -25308 {
            anyhow::anyhow!("Biometric authentication required but not available in this context")
        } else if code == -25300 {
            anyhow::anyhow!("No identity in Keychain")
        } else {
            anyhow::anyhow!("Keychain read failed (OSStatus {code})")
        }
    })?;

    if data.len() != 32 {
        anyhow::bail!("Invalid root secret length: {} (expected 32)", data.len());
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&data);
    let root_secret = styrene_identity::signer::RootSecret::new(bytes);

    derive_unlocked_identity(&root_secret)
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub fn unlock_keychain_identity() -> Result<UnlockedIdentity> {
    anyhow::bail!("Keychain identity is only available on macOS and iOS")
}

/// Delete the Keychain identity.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn delete_keychain_identity() -> Result<()> {
    use styrene_identity::keychain_signer::KeychainSigner;
    KeychainSigner::default().delete()
        .map_err(|e| anyhow::anyhow!("Keychain identity deletion failed: {e}"))
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub fn delete_keychain_identity() -> Result<()> {
    anyhow::bail!("Keychain identity is only available on macOS and iOS")
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

/// Derive all public keys from a root secret.
fn derive_unlocked_identity(root_secret: &styrene_identity::signer::RootSecret) -> Result<UnlockedIdentity> {
    let identity_hash = styrene_identity::identity_hash(root_secret);
    let deriver = KeyDeriver::new(root_secret.as_bytes());

    let git_seed = deriver.derive(KeyPurpose::Signing);
    let git_signing_pubkey = styrene_identity::format::ssh_pubkey(&git_seed, "flynt-signing");

    let auth_seed = deriver.derive_ssh_user_key("git-auth")
        .map_err(|e| anyhow::anyhow!("SSH key derivation failed: {e}"))?;
    let ssh_auth_pubkey = styrene_identity::format::ssh_pubkey(&auth_seed, "flynt@styrene");
    let ssh_fingerprint = styrene_identity::format::ssh_pubkey_fingerprint(&auth_seed);

    Ok(UnlockedIdentity {
        identity_hash,
        ssh_auth_pubkey,
        ssh_fingerprint,
        git_signing_pubkey,
    })
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

    derive_unlocked_identity(&root_secret)
}

// ── Git signing config ──────────────────────────────────────────────────────

pub fn configure_git_signing(
    project_root: &Path,
    ssh_pubkey: &str,
    name: Option<&str>,
    email: Option<&str>,
) -> Result<()> {
    let git_dir = project_root.join(".git");
    if !git_dir.exists() {
        anyhow::bail!("Not a git repository: {}", project_root.display());
    }

    let key_path = git_dir.join("styrene-signing-key.pub");
    std::fs::write(&key_path, ssh_pubkey)?;

    let repo = git2::Repository::open(project_root)
        .map_err(|e| anyhow::anyhow!("Failed to open git repo: {e}"))?;
    let mut config = repo.config()
        .map_err(|e| anyhow::anyhow!("Failed to open git config: {e}"))?;

    config.set_str("commit.gpgsign", "true")?;
    config.set_str("gpg.format", "ssh")?;
    config.set_str("user.signingkey", &key_path.display().to_string())?;
    config.set_str("user.name", name.unwrap_or("Flynt"))?;
    config.set_str("user.email", email.unwrap_or("flynt@local"))?;

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

    #[test]
    fn keychain_available_returns_expected_value() {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        assert!(keychain_available());
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        assert!(!keychain_available());
    }

    #[test]
    fn non_apple_keychain_stubs_return_error() {
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            assert!(create_keychain_identity().is_err());
            assert!(unlock_keychain_identity().is_err());
            assert!(delete_keychain_identity().is_err());
        }
    }
}
