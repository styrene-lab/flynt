# Tier B: Secure Enclave Identity — Implementation Spec

## Summary

Use the `security-framework` crate (v3.7.0) to store the 32-byte root
secret in the macOS/iOS Keychain with biometric protection. Face ID or
Touch ID replaces the passphrase. The HKDF derivation is identical to
Tier D — only the unlock mechanism changes.

## API Verification

The `security-framework` crate provides everything needed:

```rust
use security_framework::passwords::{
    set_generic_password_options, generic_password, PasswordOptions,
};
use security_framework::passwords_options::AccessControlOptions;

// Store root secret with biometric protection
let mut opts = PasswordOptions::new_generic_password(
    "io.styrene.identity",  // service
    "root-secret",          // account
);
opts.set_access_control_options(AccessControlOptions::BIOMETRY_CURRENT_SET);
set_generic_password_options(&root_secret_bytes, opts)?;

// Retrieve — OS automatically shows biometric prompt
let data = generic_password(
    PasswordOptions::new_generic_password("io.styrene.identity", "root-secret")
)?;
// data is the 32-byte root secret
```

### Flags verified in `security-framework-sys`:

| Flag | Constant | Purpose |
|------|----------|---------|
| `BIOMETRY_CURRENT_SET` | `1 << 3` | Require current enrolled biometrics |
| `BIOMETRY_ANY` | `1 << 1` | Any enrolled biometric (survives re-enrollment) |
| `USER_PRESENCE` | `1 << 0` | Biometric or device passcode fallback |
| `DEVICE_PASSCODE` | `1 << 4` | Passcode only (no biometric) |

## Implementation Location

**In styrene-identity crate** (not in Flynt):

```
crates/libs/styrene-identity/
  src/
    keychain_signer.rs  ← new file
```

### `KeychainSigner` struct

```rust
pub struct KeychainSigner {
    service: String,
    account: String,
}

impl Default for KeychainSigner {
    fn default() -> Self {
        Self {
            service: "io.styrene.identity".into(),
            account: "root-secret".into(),
        }
    }
}

impl KeychainSigner {
    /// Check if a biometric-protected identity exists.
    pub fn exists(&self) -> bool {
        // Query without retrieving data — no biometric prompt
        // Use SecItemCopyMatching with kSecReturnRef only
    }

    /// Create a new identity protected by biometrics.
    pub fn create(&self) -> Result<(), SignerError> {
        // 1. Generate 32 random bytes via OsRng
        // 2. Store with BIOMETRY_CURRENT_SET access control
        // 3. Keychain entry is device-only, non-synchronizable
    }

    /// Delete the identity.
    pub fn delete(&self) -> Result<(), SignerError> {
        delete_generic_password(service, account)
    }
}

#[async_trait]
impl IdentitySigner for KeychainSigner {
    fn tier(&self) -> SignerTier { SignerTier::DeviceHsm }
    fn label(&self) -> &str { "Keychain (biometric)" }

    fn is_available(&self) -> bool {
        self.exists()
    }

    async fn root_secret(&self) -> Result<RootSecret, SignerError> {
        // generic_password() triggers the biometric prompt
        let data = generic_password(
            PasswordOptions::new_generic_password(&self.service, &self.account)
        ).map_err(|e| SignerError::AuthRequired(e.to_string()))?;

        if data.len() != 32 {
            return Err(SignerError::DecryptionFailed("invalid key length".into()));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data);
        Ok(RootSecret::new(bytes))
    }

    async fn sign(&self, data: &[u8]) -> Result<Vec<u8>, SignerError> {
        let root = self.root_secret().await?;
        let deriver = KeyDeriver::new(root.as_bytes());
        let seed = Zeroizing::new(deriver.derive(KeyPurpose::Signing));
        Ok(sign_with_seed(&seed, data))
    }
}
```

### Feature gate

```toml
[features]
keychain = ["security-framework"]  # macOS + iOS only
```

### Discovery integration

Update `discover::discover()` to check Keychain before file:

```rust
pub fn discover() -> Option<DiscoveredIdentity> {
    // 1. Check Keychain (Tier B) — no biometric prompt, just existence check
    #[cfg(feature = "keychain")]
    if KeychainSigner::default().exists() {
        return Some(DiscoveredIdentity {
            path: PathBuf::from("(Keychain)"),
            tier: SignerTier::DeviceHsm,
            label: "Keychain (biometric)".into(),
        });
    }

    // 2. Check file (Tier D) — existing logic
    let path = FileSigner::default_path();
    if path.exists() { ... }

    // 3. Check env vars
    ...
}
```

### SignerChain default

```rust
SignerChain::new_sorted(vec![
    Box::new(KeychainSigner::default()),  // Tier B — tried first
    Box::new(FileSigner::new(...)),        // Tier D — fallback
])
```

## Flynt Changes

### Identity Settings UI

```
if biometrics_available:
    "Create identity" → "Protect with Face ID" button
    "Unlock" → "Authenticate" button (triggers biometric)
else:
    Current passphrase flow (unchanged)
```

### Detection

```rust
// Check for biometric capability
fn biometrics_available() -> bool {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        // LAContext.canEvaluatePolicy — requires LocalAuthentication framework
        // OR: just try KeychainSigner::default().exists() — if the OS
        // supports biometric Keychain, it works
        true  // All modern Apple devices support it
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    false
}
```

## Migration Path

| From | To | How |
|------|-----|-----|
| No identity | Keychain | `KeychainSigner::create()` — one biometric prompt |
| File (Tier D) | Keychain (Tier B) | Unlock with passphrase → store root secret in Keychain → delete file |
| Keychain (Tier B) | File (Tier D) | Authenticate → read root secret → write encrypted file |

## Security Properties

| Property | Tier D (file) | Tier B (Keychain) |
|----------|--------------|-------------------|
| Unlock | Passphrase (argon2id) | Face ID / Touch ID |
| Storage | `~/.config/styrene/identity.key` | macOS/iOS Keychain |
| At rest | ChaCha20Poly1305 | Secure Enclave encryption |
| Exportable | Yes (file copy) | No (device-only) |
| Survives reset | Yes | No (Keychain wiped) |
| Cross-device | Yes (copy file) | No (device-bound) |
| Brute force | argon2id (200ms/attempt) | SE lockout (escalating delay) |

## Non-Goals

- Tier B does NOT use the Secure Enclave for HKDF or Ed25519. The SE
  only supports P-256. The SE protects *access* to the root secret.
- Tier B is NOT cross-device. Each device has its own Keychain entry.
  The root secret can be the same (migrated from Tier D) or different.
- Tier B does NOT replace Tier D. Both coexist via `SignerChain`.
