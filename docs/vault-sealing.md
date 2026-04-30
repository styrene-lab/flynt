# Vault Sealing (Encryption at Rest)

Design document for Codyx vault encryption. Two modes: full vault seal and selective per-note seal.

## Key Derivation

All encryption keys derive from StyreneIdentity via HKDF:

```
RootSecret (from IdentityVault / SignerChain)
  └─ KeyDeriver::derive_vault_key(vault_id: &str)
       ├─ vault_seal_key    — AES-256-GCM or ChaCha20-Poly1305 for full vault mode
       └─ note_seal_key(note_id: &str) — per-note key for selective mode
```

The vault ID is the `id` field from `.codex/config.toml` (a UUID assigned at vault creation). Per-note keys are derived from `vault_seal_key + note_id` so each note has a unique key but all keys trace back to the same root secret.

### No Passphrase Stored

The vault never stores the passphrase or root key. On unlock:
1. User provides passphrase (or biometric via StyreneIdentity Tier B/C)
2. StyreneIdentity derives the root secret
3. HKDF derives the vault key
4. Vault unlocks in memory
5. Key zeroed on lock

If the user loses their passphrase and has no StyreneIdentity backup, sealed content is irrecoverable. This is by design.

## Mode 1: Sealed Vault

### UX

- Settings > Security > "Seal this vault" toggle
- First time: prompts for passphrase (stored via StyreneIdentity, not in vault)
- On launch: vault is locked, shows unlock prompt
- Cmd+L: lock vault (clears decrypted content from memory)
- Auto-lock after configurable idle timeout (default: 15 minutes)

### How It Works

**Encryption format:** each file becomes `{filename}.sealed`

```
Original: notes/my-note.md
Sealed:   notes/my-note.md.sealed
```

The `.sealed` file format:

```
[4 bytes]  magic: "CDXS"
[1 byte]   version: 0x01
[1 byte]   algorithm: 0x01 (ChaCha20-Poly1305)
[12 bytes] nonce
[N bytes]  ciphertext (encrypted original content)
[16 bytes] authentication tag
```

**On unlock:**
- Walk vault, decrypt all `.sealed` files to memory
- SQLite index rebuilt from decrypted content
- File watcher monitors the sealed files

**On save:**
- Content encrypted and written as `.sealed` file
- Original plaintext never touches disk

**On lock:**
- Clear all decrypted content from memory
- Drop SQLite index
- Show lock screen

### Git Sync

- Git tracks `.sealed` files (binary blobs)
- No meaningful diffs — commits show binary changes
- Merge conflicts: last-write-wins (acceptable for single-user)
- `.codex/config.toml` remains unencrypted (contains vault name, sync config, seal mode flag — no sensitive data)

### Config

```toml
# .codex/config.toml
[security]
mode = "sealed"          # "open" | "sealed" | "selective"
algorithm = "chacha20"   # "chacha20" | "aes256gcm"
auto_lock_minutes = 15   # 0 = never auto-lock
vault_id = "a1b2c3..."   # UUID for key derivation
```

## Mode 2: Selective Seal

### UX

- Right-click note > "Seal this note"
- Or frontmatter: `sealed = true`
- Sealed notes show a lock icon in the sidebar
- Opening a sealed note prompts for unlock if vault is locked
- Unsealed notes remain plain markdown — fully diffable and syncable

### How It Works

**Frontmatter stays cleartext:**

```markdown
+++
title = "API Keys"
tags = ["credentials", "sealed"]
sealed = true
+++

CDXS:v1:chacha20:NONCE_BASE64:CIPHERTEXT_BASE64:TAG_BASE64
```

The body below `+++` is replaced with a single line containing the encrypted payload. Frontmatter (title, tags, metadata) remains searchable and visible in the graph.

**On open:**
- Parser detects `sealed = true` in frontmatter
- Body is decrypted with `note_seal_key(note_id)`
- Decrypted content shown in editor
- On save: re-encrypt body, write file

**On seal:**
- Read current body
- Derive `note_seal_key(note_id)` from vault key
- Encrypt body
- Rewrite file with `sealed = true` in frontmatter + encrypted body
- Reindex (title/tags still visible)

**On unseal:**
- Decrypt body
- Remove `sealed = true` from frontmatter
- Write plaintext file
- Reindex

### Git Sync

- Frontmatter diffs work (tag changes, title renames)
- Body changes show as base64 blob changes (not human-readable but git tracks them)
- Merge conflicts on body: take-theirs or take-ours (can't merge encrypted content)
- Most notes stay plaintext — selective seal minimizes sync friction

### Config

```toml
# .codex/config.toml
[security]
mode = "selective"
algorithm = "chacha20"
auto_lock_minutes = 15
vault_id = "a1b2c3..."
```

## Implementation Plan

### Phase 1: Core (codex-core)

New module: `codex-core/src/seal.rs`

```rust
pub enum SealMode {
    Open,
    Sealed,
    Selective,
}

pub struct VaultSealConfig {
    pub mode: SealMode,
    pub algorithm: SealAlgorithm,
    pub auto_lock_minutes: u32,
    pub vault_id: String,
}

pub enum SealAlgorithm {
    ChaCha20Poly1305,
    Aes256Gcm,
}

/// Seal/unseal operations — stateless, key provided by caller
pub trait Sealer: Send + Sync {
    fn seal(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>>;
    fn unseal(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>>;
}

/// Per-note seal header in the file body
pub struct SealedBody {
    pub version: u8,
    pub algorithm: SealAlgorithm,
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
    pub tag: [u8; 16],
}

impl SealedBody {
    pub fn encode(&self) -> String; // → "CDXS:v1:chacha20:BASE64..."
    pub fn decode(line: &str) -> Result<Self>;
}
```

### Phase 2: Store (codex-store)

- `Vault::open()` checks `security.mode` in config
- `Vault::seal_note()` / `Vault::unseal_note()` — per-note operations
- `Vault::lock()` / `Vault::unlock(key)` — full vault operations
- `index_file()` skips body indexing for sealed notes (indexes frontmatter only)
- `save_document_content()` encrypts body if note is sealed

### Phase 3: UI (codex-app)

- Lock screen component (shown when vault is locked)
- Unlock dialog (passphrase input or biometric prompt)
- Sidebar: lock icon on sealed notes
- Context menu: "Seal this note" / "Unseal this note"
- Settings > Security panel
- Cmd+L: lock vault
- Status bar: lock/unlock indicator

### Phase 4: StyreneIdentity Integration

- Key derivation via `KeyDeriver::derive_vault_key()`
- Biometric unlock via `SecureEnclaveSigner` (Tier B)
- Credential manager unlock via `CredentialManagerSigner` (Tier C)
- Passphrase fallback via `FileSigner` (Tier D)
- `SignerChain` handles tier selection automatically

## Dependencies

```toml
# codex-core/Cargo.toml
age = "0.10"     # audited file encryption (X25519 + ChaCha20-Poly1305 internally)
base64 = "0.22"  # for inline encoding
zeroize = "1"    # for key memory cleanup
```

**NOT** adding raw `chacha20poly1305` or `aes-gcm` — the `age` crate handles
cipher selection internally and is audited. StyreneIdentity already owns the
Argon2id + ChaCha20-Poly1305 pipeline for identity file encryption.

## StyreneIdentity Integration (What We DON'T Reinvent)

StyreneIdentity already provides:

| Capability | StyreneIdentity API | Codyx usage |
|-----------|-------------------|-------------|
| Key derivation | `KeyDeriver::age_secret()` → 32-byte X25519 | Vault encryption key |
| Passphrase stretching | `FileSigner` (Argon2id, 64 MiB, 3 iterations) | Identity unlock |
| Tiered auth | `SignerChain` A→B→C→D fallback | Vault unlock |
| Biometric | `SecureEnclaveSigner` (Tier B, planned) | Touch ID unlock |
| Credential manager | `CredentialManagerSigner` (Tier C, planned) | 1Password/Bitwarden |
| Memory safety | `zeroize` on all key material | Key cleanup on lock |
| Backup | `IdentityVault::backup()` | Before key rotation |

**Codyx does NOT:**
- Implement its own HKDF hierarchy
- Store passphrases or keys
- Implement its own AEAD cipher
- Manage signer tiers

**Codyx DOES:**
- Call `IdentityVault::unlock()` → `KeyDeriver::age_secret()`
- Pass the age secret to the `age` crate for file encryption
- Manage which files are sealed vs open
- Handle the git sync implications

## Alternative: age-based file format (preferred over CDXS)

Instead of the custom CDXS binary format, use `age` armored encryption:

**Selective seal (per-note):**
```markdown
+++
title = "API Keys"
tags = ["credentials"]
sealed = true
+++

-----BEGIN AGE ENCRYPTED FILE-----
YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSBTZWFsZWQgbm90ZSBi
b2R5IGVuY3J5cHRlZCB3aXRoIGFnZSB1c2luZyBTdHlyZW5lSWRlbnRpdHkg
ZGVyaXZlZCBrZXk=
-----END AGE ENCRYPTED FILE-----
```

**Sealed vault (per-file):**
```
notes/my-note.md.age   ← age-encrypted .md file
```

Benefits over CDXS:
- Audited, well-known format
- Streaming encryption (handles large files)
- Built-in key ID / recipient management
- Compatible with the `age` CLI tool for emergency recovery
- No custom parser to maintain

## Security Properties

- **At rest:** sealed content is encrypted on disk
- **In memory:** decrypted content exists only while vault is unlocked
- **Key management:** delegated to StyreneIdentity (not stored in vault)
- **Forward secrecy:** not provided (same key until rotated)
- **Key rotation:** re-encrypt all sealed content with new key (manual operation)
- **Metadata protection:** selective mode leaks title/tags; sealed vault mode protects everything except config.toml
- **No backdoor:** lost passphrase + lost StyreneIdentity = irrecoverable

## What This Does NOT Cover

- In-transit encryption (handled by SSH/HTTPS for git sync)
- Shared vault encryption (requires key exchange — future StyreneIdentity feature)
- Plausible deniability / hidden volumes
- Hardware key enforcement (YubiKey required — future via SignerChain Tier A)
