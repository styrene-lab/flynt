//! Vault sealing — encryption at rest for notes and vaults.
//!
//! Two modes:
//! - **Sealed vault**: all files encrypted, decrypted in memory while unlocked
//! - **Selective seal**: individual notes marked `sealed = true`, body encrypted
//!
//! Key derivation is handled by StyreneIdentity. This module provides the
//! encrypt/decrypt primitives and the sealed body format.

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ── Configuration ───────────────────────────────────────────────────────────

/// How the vault handles encryption at rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SealMode {
    /// No encryption — plain markdown files.
    #[default]
    Open,
    /// Entire vault encrypted. All files stored as `.sealed` on disk.
    Sealed,
    /// Per-note encryption. Notes with `sealed = true` have encrypted bodies.
    Selective,
}

/// Which symmetric cipher to use.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SealAlgorithm {
    #[default]
    ChaCha20Poly1305,
    Aes256Gcm,
}

/// Security configuration stored in `.codex/config.toml` under `[security]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SealConfig {
    #[serde(default)]
    pub mode: SealMode,
    #[serde(default)]
    pub algorithm: SealAlgorithm,
    /// Auto-lock after N minutes of inactivity. 0 = never.
    #[serde(default = "default_auto_lock")]
    pub auto_lock_minutes: u32,
}

fn default_auto_lock() -> u32 {
    15
}

impl Default for SealConfig {
    fn default() -> Self {
        Self {
            mode: SealMode::Open,
            algorithm: SealAlgorithm::default(),
            auto_lock_minutes: default_auto_lock(),
        }
    }
}

// ── Sealed body format ──────────────────────────────────────────────────────

/// The magic prefix for sealed file format.
pub const SEALED_MAGIC: &[u8; 4] = b"CDXS";

/// Version byte for the sealed format.
pub const SEALED_VERSION: u8 = 0x01;

/// A sealed body — the encrypted payload of a note or file.
#[derive(Debug, Clone)]
pub struct SealedBody {
    pub version: u8,
    pub algorithm: SealAlgorithm,
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
    pub tag: [u8; 16],
}

impl SealedBody {
    /// Encode as a single-line string for embedding in markdown files.
    /// Format: `CDXS:v1:chacha20:NONCE_B64:CIPHERTEXT_B64:TAG_B64`
    pub fn encode_inline(&self) -> String {
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;
        let algo = match self.algorithm {
            SealAlgorithm::ChaCha20Poly1305 => "chacha20",
            SealAlgorithm::Aes256Gcm => "aes256gcm",
        };
        format!(
            "CDXS:v{ver}:{algo}:{nonce}:{ct}:{tag}",
            ver = self.version,
            nonce = engine.encode(self.nonce),
            ct = engine.encode(&self.ciphertext),
            tag = engine.encode(self.tag),
        )
    }

    /// Decode from the inline string format.
    pub fn decode_inline(line: &str) -> Result<Self> {
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;

        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() != 6 || parts[0] != "CDXS" {
            anyhow::bail!("invalid sealed body format");
        }

        let version = parts[1]
            .strip_prefix('v')
            .and_then(|v| v.parse::<u8>().ok())
            .ok_or_else(|| anyhow::anyhow!("invalid version"))?;

        let algorithm = match parts[2] {
            "chacha20" => SealAlgorithm::ChaCha20Poly1305,
            "aes256gcm" => SealAlgorithm::Aes256Gcm,
            other => anyhow::bail!("unknown algorithm: {other}"),
        };

        let nonce_bytes = engine.decode(parts[3])?;
        let ciphertext = engine.decode(parts[4])?;
        let tag_bytes = engine.decode(parts[5])?;

        let mut nonce = [0u8; 12];
        let mut tag = [0u8; 16];
        if nonce_bytes.len() != 12 {
            anyhow::bail!("nonce must be 12 bytes");
        }
        if tag_bytes.len() != 16 {
            anyhow::bail!("tag must be 16 bytes");
        }
        nonce.copy_from_slice(&nonce_bytes);
        tag.copy_from_slice(&tag_bytes);

        Ok(SealedBody {
            version,
            algorithm,
            nonce,
            ciphertext,
            tag,
        })
    }

    /// Encode as binary for `.sealed` files (full vault mode).
    pub fn encode_binary(&self) -> Vec<u8> {
        let algo_byte = match self.algorithm {
            SealAlgorithm::ChaCha20Poly1305 => 0x01,
            SealAlgorithm::Aes256Gcm => 0x02,
        };
        let mut out = Vec::with_capacity(4 + 1 + 1 + 12 + self.ciphertext.len() + 16);
        out.extend_from_slice(SEALED_MAGIC);
        out.push(self.version);
        out.push(algo_byte);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        out.extend_from_slice(&self.tag);
        out
    }

    /// Decode from binary `.sealed` file format.
    pub fn decode_binary(data: &[u8]) -> Result<Self> {
        if data.len() < 34 {
            anyhow::bail!("sealed file too short");
        }
        if &data[0..4] != SEALED_MAGIC {
            anyhow::bail!("not a sealed file (bad magic)");
        }
        let version = data[4];
        let algorithm = match data[5] {
            0x01 => SealAlgorithm::ChaCha20Poly1305,
            0x02 => SealAlgorithm::Aes256Gcm,
            other => anyhow::bail!("unknown algorithm byte: {other:#x}"),
        };

        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&data[6..18]);

        let ct_len = data.len() - 18 - 16;
        let ciphertext = data[18..18 + ct_len].to_vec();

        let mut tag = [0u8; 16];
        tag.copy_from_slice(&data[18 + ct_len..]);

        Ok(SealedBody {
            version,
            algorithm,
            nonce,
            ciphertext,
            tag,
        })
    }
}

// ── Sealer trait ─────────────────────────────────────────────────────────────

/// Stateless encryption/decryption. Key provided by caller (from StyreneIdentity).
pub trait Sealer: Send + Sync {
    /// Encrypt plaintext with the given key. Returns a SealedBody.
    fn seal(&self, key: &[u8], plaintext: &[u8]) -> Result<SealedBody>;

    /// Decrypt a SealedBody with the given key. Returns plaintext.
    fn unseal(&self, key: &[u8], sealed: &SealedBody) -> Result<Vec<u8>>;
}

// ── Detection helpers ───────────────────────────────────────────────────────

/// Check if a file's content starts with the sealed magic bytes.
pub fn is_sealed_file(data: &[u8]) -> bool {
    data.len() >= 4 && &data[0..4] == SEALED_MAGIC
}

/// Check if a note body line is a sealed inline payload.
pub fn is_sealed_inline(line: &str) -> bool {
    line.starts_with("CDXS:v")
}

/// Check if a document's frontmatter indicates it's sealed.
pub fn is_note_sealed(frontmatter: &crate::models::Frontmatter) -> bool {
    frontmatter
        .metadata
        .get("sealed")
        .map(|v| matches!(v, crate::models::MetadataValue::Bool(true)))
        .unwrap_or(false)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sealed_body() -> SealedBody {
        SealedBody {
            version: 1,
            algorithm: SealAlgorithm::ChaCha20Poly1305,
            nonce: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            ciphertext: b"encrypted content here".to_vec(),
            tag: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
        }
    }

    #[test]
    fn inline_roundtrip() {
        let body = make_sealed_body();
        let encoded = body.encode_inline();
        assert!(encoded.starts_with("CDXS:v1:chacha20:"));

        let decoded = SealedBody::decode_inline(&encoded).unwrap();
        assert_eq!(decoded.version, 1);
        assert_eq!(decoded.algorithm, SealAlgorithm::ChaCha20Poly1305);
        assert_eq!(decoded.nonce, body.nonce);
        assert_eq!(decoded.ciphertext, body.ciphertext);
        assert_eq!(decoded.tag, body.tag);
    }

    #[test]
    fn binary_roundtrip() {
        let body = make_sealed_body();
        let binary = body.encode_binary();
        assert_eq!(&binary[0..4], b"CDXS");

        let decoded = SealedBody::decode_binary(&binary).unwrap();
        assert_eq!(decoded.version, 1);
        assert_eq!(decoded.algorithm, SealAlgorithm::ChaCha20Poly1305);
        assert_eq!(decoded.nonce, body.nonce);
        assert_eq!(decoded.ciphertext, body.ciphertext);
        assert_eq!(decoded.tag, body.tag);
    }

    #[test]
    fn inline_invalid_format() {
        assert!(SealedBody::decode_inline("not a sealed body").is_err());
        assert!(SealedBody::decode_inline("CDXS:v1").is_err());
        assert!(SealedBody::decode_inline("").is_err());
    }

    #[test]
    fn binary_too_short() {
        assert!(SealedBody::decode_binary(b"CDX").is_err());
        assert!(SealedBody::decode_binary(b"CDXS").is_err());
    }

    #[test]
    fn binary_bad_magic() {
        let mut data = make_sealed_body().encode_binary();
        data[0] = b'X';
        assert!(SealedBody::decode_binary(&data).is_err());
    }

    #[test]
    fn is_sealed_file_detection() {
        assert!(is_sealed_file(b"CDXS\x01\x01rest"));
        assert!(!is_sealed_file(b"not sealed"));
        assert!(!is_sealed_file(b"CDX")); // too short
    }

    #[test]
    fn is_sealed_inline_detection() {
        assert!(is_sealed_inline("CDXS:v1:chacha20:abc:def:ghi"));
        assert!(!is_sealed_inline("regular text"));
        assert!(!is_sealed_inline(""));
    }

    #[test]
    fn aes256gcm_algorithm_roundtrip() {
        let body = SealedBody {
            version: 1,
            algorithm: SealAlgorithm::Aes256Gcm,
            nonce: [0; 12],
            ciphertext: b"test".to_vec(),
            tag: [0; 16],
        };

        // Inline
        let encoded = body.encode_inline();
        assert!(encoded.contains("aes256gcm"));
        let decoded = SealedBody::decode_inline(&encoded).unwrap();
        assert_eq!(decoded.algorithm, SealAlgorithm::Aes256Gcm);

        // Binary
        let binary = body.encode_binary();
        assert_eq!(binary[5], 0x02); // AES byte
        let decoded = SealedBody::decode_binary(&binary).unwrap();
        assert_eq!(decoded.algorithm, SealAlgorithm::Aes256Gcm);
    }

    #[test]
    fn seal_config_defaults() {
        let config = SealConfig::default();
        assert_eq!(config.mode, SealMode::Open);
        assert_eq!(config.algorithm, SealAlgorithm::ChaCha20Poly1305);
        assert_eq!(config.auto_lock_minutes, 15);
    }
}
