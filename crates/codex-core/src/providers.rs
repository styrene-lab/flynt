//! Provider authentication — reads/writes Omegon's auth.json and probes
//! environment variables to determine provider status.
//!
//! This module lets the Codex settings UI manage API keys and trigger OAuth
//! flows without requiring the user to use the terminal.

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
};

// ── Provider catalogue ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub auth_method: AuthMethod,
    pub env_vars: &'static [&'static str],
    /// OAuth authorize URL template (only for OAuth providers).
    pub oauth_url: Option<&'static str>,
    /// Local callback port for OAuth flow.
    pub oauth_port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthMethod {
    ApiKey,
    OAuth,
}

/// All known providers. Matches Omegon's provider registry.
pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        id: "anthropic",
        label: "Anthropic",
        auth_method: AuthMethod::OAuth,
        env_vars: &["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"],
        oauth_url: Some("https://claude.ai/oauth/authorize"),
        oauth_port: Some(53692),
    },
    ProviderInfo {
        id: "openai",
        label: "OpenAI API",
        auth_method: AuthMethod::ApiKey,
        env_vars: &["OPENAI_API_KEY"],
        oauth_url: None,
        oauth_port: None,
    },
    ProviderInfo {
        id: "openai-codex",
        label: "OpenAI (ChatGPT)",
        auth_method: AuthMethod::OAuth,
        env_vars: &["OPENAI_CODEX_TOKEN"],
        oauth_url: Some("https://auth.openai.com/oauth/authorize"),
        oauth_port: Some(1455),
    },
    ProviderInfo {
        id: "openrouter",
        label: "OpenRouter",
        auth_method: AuthMethod::ApiKey,
        env_vars: &["OPENROUTER_API_KEY"],
        oauth_url: None,
        oauth_port: None,
    },
    ProviderInfo {
        id: "groq",
        label: "Groq",
        auth_method: AuthMethod::ApiKey,
        env_vars: &["GROQ_API_KEY"],
        oauth_url: None,
        oauth_port: None,
    },
    ProviderInfo {
        id: "xai",
        label: "xAI (Grok)",
        auth_method: AuthMethod::ApiKey,
        env_vars: &["XAI_API_KEY"],
        oauth_url: None,
        oauth_port: None,
    },
    ProviderInfo {
        id: "mistral",
        label: "Mistral",
        auth_method: AuthMethod::ApiKey,
        env_vars: &["MISTRAL_API_KEY"],
        oauth_url: None,
        oauth_port: None,
    },
    ProviderInfo {
        id: "google",
        label: "Google Gemini",
        auth_method: AuthMethod::ApiKey,
        env_vars: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        oauth_url: None,
        oauth_port: None,
    },
    ProviderInfo {
        id: "github",
        label: "GitHub",
        auth_method: AuthMethod::ApiKey,
        env_vars: &["GITHUB_TOKEN"],
        oauth_url: None,
        oauth_port: None,
    },
];

// ── Credential status ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum CredentialStatus {
    /// API key or valid OAuth token available.
    Authenticated { source: String },
    /// OAuth token exists but is expired.
    Expired,
    /// No credentials found.
    Missing,
}

// ── auth.json format ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthStore {
    #[serde(flatten)]
    pub providers: HashMap<String, StoredCredential>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StoredCredential {
    ApiKey { key: String },
    Oauth {
        access: String,
        #[serde(default)]
        refresh: Option<String>,
        #[serde(default)]
        expires: Option<u64>,
    },
}

// ── Path resolution ─────────────────────────────────────────────────────────

/// Default auth.json path — matches Omegon's location.
pub fn auth_json_path() -> PathBuf {
    if let Ok(p) = std::env::var("OMEGON_AUTH_JSON") {
        return PathBuf::from(p);
    }
    // XDG config dir fallback chain
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    config_dir.join("omegon").join("auth.json")
}

// ── Read/write ──────────────────────────────────────────────────────────────

pub fn load_auth_store() -> AuthStore {
    let path = auth_json_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_auth_store(store: &AuthStore) -> anyhow::Result<()> {
    let path = auth_json_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    with_auth_json_lock(&path, || {
        atomic_write_auth_json(&path, store)
    })
}

/// Atomic write: serialize to .tmp, set perms, rename over original.
fn atomic_write_auth_json(path: &std::path::Path, store: &AuthStore) -> anyhow::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(store)?;
    fs::write(&tmp, &json)?;
    set_auth_file_permissions(&tmp)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Lock via create_new — same pattern as Omegon's auth.rs.
fn with_auth_json_lock<T>(
    path: &std::path::Path,
    f: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    let lock_path = std::path::PathBuf::from(lock_path);

    for _ in 0..200 {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_) => {
                let result = f();
                let _ = fs::remove_file(&lock_path);
                return result;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("Timed out waiting for auth.json lock: {}", lock_path.display())
}

fn set_auth_file_permissions(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

// ── Provider operations ─────────────────────────────────────────────────────

/// Check the status of a provider: env var → auth.json → missing.
pub fn probe_provider(provider: &ProviderInfo) -> CredentialStatus {
    // Check environment variables first
    for var in provider.env_vars {
        if let Ok(val) = std::env::var(var) {
            if !val.is_empty() {
                return CredentialStatus::Authenticated {
                    source: format!("env:{var}"),
                };
            }
        }
    }

    // Check auth.json
    let store = load_auth_store();
    if let Some(cred) = store.providers.get(provider.id) {
        match cred {
            StoredCredential::ApiKey { key } if !key.is_empty() => {
                return CredentialStatus::Authenticated {
                    source: "auth.json".into(),
                };
            }
            StoredCredential::Oauth { expires, .. } => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if expires.map(|e| now_ms >= e).unwrap_or(false) {
                    return CredentialStatus::Expired;
                }
                return CredentialStatus::Authenticated {
                    source: "auth.json (oauth)".into(),
                };
            }
            _ => {}
        }
    }

    CredentialStatus::Missing
}

/// Probe all known providers and return their statuses.
pub fn probe_all() -> Vec<(&'static ProviderInfo, CredentialStatus)> {
    PROVIDERS.iter().map(|p| (p, probe_provider(p))).collect()
}

/// Save an API key for a provider.
pub fn save_api_key(provider_id: &str, key: &str) -> anyhow::Result<()> {
    let mut store = load_auth_store();
    store.providers.insert(
        provider_id.to_string(),
        StoredCredential::ApiKey { key: key.to_string() },
    );
    save_auth_store(&store)
}

/// Remove credentials for a provider.
pub fn remove_credential(provider_id: &str) -> anyhow::Result<()> {
    let mut store = load_auth_store();
    store.providers.remove(provider_id);
    save_auth_store(&store)
}

/// Build the command to start an OAuth login flow.
/// Uses the centralized binary resolver from LocalRuntimeConfig.
pub fn oauth_login_command(config: &crate::models::LocalRuntimeConfig, provider_id: &str) -> (String, Vec<String>) {
    let binary = crate::models::resolve_omegon_binary(config);
    (binary.to_string_lossy().into_owned(), vec!["auth".into(), "login".into(), provider_id.into()])
}
