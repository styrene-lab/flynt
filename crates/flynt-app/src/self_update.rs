use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/styrene-lab/flynt/releases/latest";
const NIGHTLY_RELEASE_URL: &str =
    "https://api.github.com/repos/styrene-lab/flynt/releases?per_page=30";
const RELEASES_LATEST_URL: &str = "https://github.com/styrene-lab/flynt/releases/latest";
const RELEASES_NIGHTLY_URL: &str = "https://github.com/styrene-lab/flynt/releases";
const MANIFEST_NAME: &str = "flynt-release.json";
const MANIFEST_SIG_NAME: &str = "flynt-release.json.sig";
const RELEASE_PUBLIC_KEY_B64: Option<&str> = option_env!("FLYNT_RELEASE_VERIFY_KEY_B64");

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    Current {
        channel: UpdateChannel,
        current: String,
    },
    Available {
        channel: UpdateChannel,
        current: String,
        latest: String,
        release_commit: Option<String>,
        html_url: String,
        dmg_url: Option<String>,
        pkg_url: Option<String>,
        checksum_url: Option<String>,
        install_source: InstallSource,
        verified: bool,
        verification: String,
        selected_artifact: Option<ReleaseArtifact>,
    },
    Unknown {
        channel: UpdateChannel,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallSource {
    DirectDownload,
    Homebrew,
    Nix,
    Development,
    Unknown,
}

impl InstallSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::DirectDownload => "Update",
            Self::Homebrew => "Update via Homebrew",
            Self::Nix => "Update via Nix",
            Self::Development => "Development build",
            Self::Unknown => "Update",
        }
    }

    pub fn should_open_direct_artifact(&self) -> bool {
        matches!(self, Self::DirectDownload | Self::Unknown)
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub version: String,
    pub channel: UpdateChannel,
    pub tag: String,
    #[serde(default)]
    pub commit: Option<String>,
    pub published_at: String,
    #[serde(default)]
    pub artifacts: Vec<ReleaseArtifact>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Nightly,
}

impl UpdateChannel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Nightly => "Nightly",
        }
    }

    pub fn all_named() -> &'static [Self] {
        &[Self::Stable, Self::Nightly]
    }

    fn release_api_url(self) -> &'static str {
        match self {
            Self::Stable => LATEST_RELEASE_URL,
            Self::Nightly => NIGHTLY_RELEASE_URL,
        }
    }

    fn release_page_url(self) -> &'static str {
        match self {
            Self::Stable => RELEASES_LATEST_URL,
            Self::Nightly => RELEASES_NIGHTLY_URL,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseArtifact {
    pub name: String,
    pub kind: String,
    pub platform: String,
    pub arch: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub apple_team_id: Option<String>,
    #[serde(default)]
    pub apple_notarized: bool,
}

pub async fn check_latest_release() -> UpdateState {
    check_channel(configured_channel()).await
}

pub async fn check_channel(channel: UpdateChannel) -> UpdateState {
    let current = env!("CARGO_PKG_VERSION").to_string();
    match check_channel_inner(channel, &current).await {
        Ok(state) => state,
        Err(err) => UpdateState::Unknown {
            channel,
            message: err.to_string(),
        },
    }
}

async fn check_channel_inner(channel: UpdateChannel, current: &str) -> anyhow::Result<UpdateState> {
    let release = fetch_release(channel).await?;
    let tag_version = normalize_version(&release.tag_name);

    let manifest_url = asset_url_exact(&release.assets, MANIFEST_NAME);
    let manifest_sig_url = asset_url_exact(&release.assets, MANIFEST_SIG_NAME);
    let mut verified = false;
    let mut verification = "Signed update manifest is not available for this release.".to_string();
    let mut manifest = None;

    if let (Some(manifest_url), Some(sig_url)) = (manifest_url, manifest_sig_url) {
        let client = release_client()?;
        let manifest_bytes = fetch_bytes(&client, &manifest_url).await?;
        let signature_bytes = fetch_bytes(&client, &sig_url).await?;
        match verify_manifest(&manifest_bytes, &signature_bytes, channel) {
            Ok(parsed) => {
                if parsed.tag != release.tag_name {
                    verification = format!(
                        "Manifest verification failed: tag mismatch, expected {}, got {}",
                        release.tag_name, parsed.tag
                    );
                } else if channel == UpdateChannel::Stable
                    && normalize_version(&parsed.version) != tag_version
                {
                    verification = format!(
                        "Manifest verification failed: version mismatch, expected {}, got {}",
                        tag_version, parsed.version
                    );
                } else {
                    verified = true;
                    verification = "Signed manifest verified.".into();
                    manifest = Some(parsed);
                }
            }
            Err(err) => {
                verification = format!("Manifest verification failed: {err}");
            }
        }
    }

    if channel == UpdateChannel::Nightly && !verified {
        anyhow::bail!("{verification}");
    }

    let latest = manifest
        .as_ref()
        .map(|manifest| normalize_version(&manifest.version))
        .unwrap_or(tag_version);
    let release_commit = manifest
        .as_ref()
        .and_then(|manifest| manifest.commit.clone());

    if !candidate_is_update(channel, &latest, release_commit.as_deref(), current) {
        return Ok(UpdateState::Current {
            channel,
            current: current.into(),
        });
    }

    let selected_artifact = manifest.as_ref().and_then(|manifest| {
        if cfg!(target_os = "macos") {
            manifest
                .artifact("pkg", "macos")
                .or_else(|| manifest.artifact("dmg", "macos"))
                .cloned()
        } else {
            None
        }
    });

    let (pkg_url, dmg_url, checksum_url) = if let Some(ref manifest) = manifest {
        (
            manifest.artifact_url("pkg", "macos"),
            manifest.artifact_url("dmg", "macos"),
            None,
        )
    } else {
        (
            asset_url(&release.assets, ".pkg"),
            asset_url(&release.assets, ".dmg"),
            asset_url(&release.assets, ".sha256")
                .or_else(|| asset_url(&release.assets, "SHA256SUMS"))
                .or_else(|| asset_url(&release.assets, "checksums.txt")),
        )
    };

    Ok(UpdateState::Available {
        channel,
        current: current.into(),
        latest,
        release_commit,
        html_url: release.html_url,
        dmg_url,
        pkg_url,
        checksum_url,
        install_source: detect_install_source(),
        verified,
        verification,
        selected_artifact,
    })
}

pub fn latest_releases_url() -> &'static str {
    RELEASES_LATEST_URL
}

pub fn release_page_url(channel: UpdateChannel) -> &'static str {
    channel.release_page_url()
}

pub fn configured_channel() -> UpdateChannel {
    crate::bootstrap::OmegonRuntimeContext::load_launcher_profile().flynt_update_channel
}

async fn fetch_release(channel: UpdateChannel) -> anyhow::Result<GitHubRelease> {
    let client = release_client()?;
    let response = client.get(channel.release_api_url()).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("GitHub release check failed: {}", response.status());
    }
    match channel {
        UpdateChannel::Stable => Ok(response.json::<GitHubRelease>().await?),
        UpdateChannel::Nightly => response
            .json::<Vec<GitHubRelease>>()
            .await?
            .into_iter()
            .filter(|release| {
                !release.draft
                    && release.prerelease
                    && release.tag_name.starts_with("nightly-")
                    && asset_url_exact(&release.assets, MANIFEST_NAME).is_some()
                    && asset_url_exact(&release.assets, MANIFEST_SIG_NAME).is_some()
            })
            .max_by(|left, right| left.tag_name.cmp(&right.tag_name))
            .ok_or_else(|| anyhow::anyhow!("no signed nightly release found")),
    }
}

fn release_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(format!("flynt/{}", env!("CARGO_PKG_VERSION")))
        .build()?)
}

async fn fetch_bytes(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("download failed for {url}: {}", response.status());
    }
    Ok(response.bytes().await?.to_vec())
}

fn verify_manifest(
    manifest_bytes: &[u8],
    signature_bytes: &[u8],
    expected_channel: UpdateChannel,
) -> anyhow::Result<ReleaseManifest> {
    let key_b64 = RELEASE_PUBLIC_KEY_B64
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("release verification key is not embedded"))?;
    let key_bytes = base64::engine::general_purpose::STANDARD.decode(key_b64.trim())?;
    let key_array: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("release verification key must be 32 bytes"))?;
    let verifying_key = VerifyingKey::from_bytes(&key_array)?;

    let sig_bytes = decode_signature(signature_bytes)?;
    let sig_array: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("release manifest signature must be 64 bytes"))?;
    let signature = Signature::from_bytes(&sig_array);
    verifying_key.verify(manifest_bytes, &signature)?;

    let manifest: ReleaseManifest = serde_json::from_slice(manifest_bytes)?;
    if manifest.channel != expected_channel {
        anyhow::bail!(
            "manifest channel mismatch: expected {:?}, got {:?}",
            expected_channel,
            manifest.channel
        );
    }
    verify_manifest_hashes(&manifest)?;
    Ok(manifest)
}

fn decode_signature(signature_bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    if let Ok(raw) = std::str::from_utf8(signature_bytes) {
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(raw.trim()) {
            return Ok(decoded);
        }
    }
    Ok(signature_bytes.to_vec())
}

fn verify_manifest_hashes(manifest: &ReleaseManifest) -> anyhow::Result<()> {
    for artifact in &manifest.artifacts {
        if artifact.sha256.len() != 64 || !artifact.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("invalid sha256 for {}", artifact.name);
        }
    }
    Ok(())
}

pub fn verify_artifact_bytes(artifact: &ReleaseArtifact, bytes: &[u8]) -> anyhow::Result<()> {
    if artifact.size != 0 && bytes.len() as u64 != artifact.size {
        anyhow::bail!(
            "size mismatch for {}: expected {}, got {}",
            artifact.name,
            artifact.size,
            bytes.len()
        );
    }
    let digest = Sha256::digest(bytes);
    let actual = hex::encode(digest);
    if actual != artifact.sha256.to_lowercase() {
        anyhow::bail!("sha256 mismatch for {}", artifact.name);
    }
    Ok(())
}

pub async fn download_verified_artifact(artifact: ReleaseArtifact) -> anyhow::Result<PathBuf> {
    let client = release_client()?;
    let bytes = fetch_bytes(&client, &artifact.url).await?;
    verify_artifact_bytes(&artifact, &bytes)?;

    let file_name = Path::new(&artifact.name)
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("invalid artifact name {}", artifact.name))?;
    let destination_dir = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
    tokio::fs::create_dir_all(&destination_dir).await?;
    let destination = destination_dir.join(file_name);
    tokio::fs::write(&destination, bytes).await?;
    Ok(destination)
}

fn asset_url(assets: &[GitHubAsset], suffix: &str) -> Option<String> {
    assets
        .iter()
        .find(|asset| asset.name.ends_with(suffix))
        .map(|asset| asset.browser_download_url.clone())
}

fn asset_url_exact(assets: &[GitHubAsset], name: &str) -> Option<String> {
    assets
        .iter()
        .find(|asset| asset.name == name)
        .map(|asset| asset.browser_download_url.clone())
}

impl ReleaseManifest {
    fn artifact(&self, kind: &str, platform: &str) -> Option<&ReleaseArtifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.kind == kind && artifact.platform == platform)
    }

    fn artifact_url(&self, kind: &str, platform: &str) -> Option<String> {
        self.artifact(kind, platform)
            .map(|artifact| artifact.url.clone())
    }
}

pub fn detect_install_source() -> InstallSource {
    let Ok(exe) = std::env::current_exe() else {
        return InstallSource::Unknown;
    };
    let path = exe.to_string_lossy();
    if path.contains("/target/") {
        InstallSource::Development
    } else if path.contains("/nix/store/") {
        InstallSource::Nix
    } else if path.contains("/Cellar/flynt/") || path.contains("/homebrew/Cellar/flynt/") {
        InstallSource::Homebrew
    } else if path.starts_with("/Applications/") && path.contains(".app/Contents/MacOS/") {
        InstallSource::DirectDownload
    } else {
        InstallSource::Unknown
    }
}

fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches('v').to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VersionKey {
    major: u32,
    minor: u32,
    patch: u32,
    prerelease: Option<String>,
}

impl Ord for VersionKey {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(right),
            })
    }
}

impl PartialOrd for VersionKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_version_key(version: &str) -> VersionKey {
    let normalized = normalize_version(version);
    let (base, pre) = normalized
        .split_once('-')
        .map(|(base, pre)| (base, pre.to_string()))
        .unwrap_or((normalized.as_str(), String::new()));
    let parts: Vec<u32> = base
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect();
    VersionKey {
        major: parts.first().copied().unwrap_or(0),
        minor: parts.get(1).copied().unwrap_or(0),
        patch: parts.get(2).copied().unwrap_or(0),
        prerelease: if pre.is_empty() { None } else { Some(pre) },
    }
}

fn version_is_newer(candidate: &str, current: &str) -> bool {
    parse_version_key(candidate) > parse_version_key(current)
}

fn candidate_is_update(
    channel: UpdateChannel,
    candidate: &str,
    release_commit: Option<&str>,
    current: &str,
) -> bool {
    match channel {
        UpdateChannel::Stable => version_is_newer(candidate, current),
        UpdateChannel::Nightly => {
            let build_hash = env!("FLYNT_BUILD_HASH");
            if let Some(commit) = release_commit {
                return !commit.starts_with(build_hash) && !build_hash.starts_with(commit);
            }
            candidate != current
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ReleaseArtifact, UpdateChannel, VersionKey, candidate_is_update, normalize_version,
        parse_version_key, verify_artifact_bytes, version_is_newer,
    };

    #[test]
    fn normalizes_v_prefix() {
        assert_eq!(normalize_version("v0.10.0"), "0.10.0");
    }

    #[test]
    fn compares_basic_versions() {
        assert!(version_is_newer("0.10.1", "0.10.0"));
        assert!(version_is_newer("0.11.0", "0.10.9"));
        assert!(!version_is_newer("0.10.0", "0.10.0"));
    }

    #[test]
    fn parses_prerelease_suffix() {
        assert_eq!(
            parse_version_key("v0.10.0-rc.1"),
            VersionKey {
                major: 0,
                minor: 10,
                patch: 0,
                prerelease: Some("rc.1".into())
            }
        );
    }

    #[test]
    fn stable_release_beats_prerelease() {
        assert!(version_is_newer("0.10.0", "0.10.0-rc.1"));
        assert!(!version_is_newer("0.10.0-rc.1", "0.10.0"));
    }

    #[test]
    fn nightly_uses_commit_identity_when_present() {
        let current = env!("CARGO_PKG_VERSION");
        let build = env!("FLYNT_BUILD_HASH");
        assert!(!candidate_is_update(
            UpdateChannel::Nightly,
            "0.10.0-nightly.20260514",
            Some(build),
            current
        ));
        assert!(candidate_is_update(
            UpdateChannel::Nightly,
            "0.10.0-nightly.20260514",
            Some("different-build"),
            current
        ));
    }

    #[test]
    fn verifies_artifact_hash() {
        let artifact = ReleaseArtifact {
            name: "test.txt".into(),
            kind: "txt".into(),
            platform: "test".into(),
            arch: "any".into(),
            url: "https://example.invalid/test.txt".into(),
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
            size: 5,
            apple_team_id: None,
            apple_notarized: false,
        };
        verify_artifact_bytes(&artifact, b"hello").unwrap();
        assert!(verify_artifact_bytes(&artifact, b"tampered").is_err());
    }
}
