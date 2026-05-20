//! Settings prerequisite evaluators — sync backends today, agent
//! daemon and beyond next. Module name is `sync_prereq` for now but
//! the pattern generalizes.
//!
//!
//! Settings UI uses this to decide whether each backend (None / iCloud /
//! Git) is selectable, blocked, or selectable-with-warning. The
//! evaluation runs at render time so the UI can surface "you can't
//! pick this yet, here's why" before the operator runs into a
//! mid-operation crash.
//!
//! Each backend gets its own `evaluate_*` function returning a
//! [`SyncBackendStatus`] with a human-readable explanation when it
//! isn't fully available.
//!
//! The runtime guard (pre-flight check on activation) is separate —
//! these functions are declarative, fast, and synchronous so they're
//! safe to call on every render of the settings panel.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum SyncBackendStatus {
    /// Pick it freely.
    Available,
    /// Selectable, but show a caution explaining what could go wrong.
    Warning(String),
    /// Disabled — operator must satisfy the prerequisite before this
    /// option becomes selectable.
    Blocked(String),
}

impl SyncBackendStatus {
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked(_))
    }

    pub fn explanation(&self) -> Option<&str> {
        match self {
            Self::Available => None,
            Self::Warning(s) | Self::Blocked(s) => Some(s.as_str()),
        }
    }
}

/// "None" sync is always available — it just means flynt won't push
/// anywhere. Returned as a function for symmetry with the others.
pub fn evaluate_none() -> SyncBackendStatus {
    SyncBackendStatus::Available
}

/// iCloud sync requires the project root to live inside iCloud Drive.
/// macOS exposes iCloud Drive at `~/Library/Mobile Documents/com~apple~CloudDocs/`;
/// folders outside that won't be synced even if iCloud is configured
/// in System Settings. The operator's best move when blocked is to
/// move the folder (or pick a new project in iCloud Drive).
///
/// Non-macOS hosts always block iCloud — flynt doesn't ship iCloud
/// support on Linux.
pub fn evaluate_icloud(project_root: &Path) -> SyncBackendStatus {
    if !cfg!(target_os = "macos") {
        return SyncBackendStatus::Blocked(
            "iCloud sync is macOS-only. Use Git sync on Linux.".to_string(),
        );
    }
    let Ok(home) = std::env::var("HOME") else {
        return SyncBackendStatus::Blocked("Can't resolve $HOME to find iCloud Drive.".to_string());
    };
    let icloud_root =
        std::path::PathBuf::from(&home).join("Library/Mobile Documents/com~apple~CloudDocs");
    if project_root.starts_with(&icloud_root) {
        SyncBackendStatus::Available
    } else {
        SyncBackendStatus::Blocked(format!(
            "Project must live inside iCloud Drive ({}). Move it there or create a new project under iCloud Drive to enable.",
            icloud_root.display(),
        ))
    }
}

/// Git sync needs (a) a working git binary on PATH, and (b) either an
/// existing repo at the project root OR credentials configured for a
/// remote. The remote URL is part of the sync config — operators
/// typically fill that in after selecting Git, so we don't block on it.
///
/// The credentials check is permissive: if ANY git provider has a
/// stored token, we treat the operator as "git-capable." Per-remote
/// auth gets validated by the runtime pre-flight check, not here.
pub fn evaluate_git() -> SyncBackendStatus {
    // git binary presence.
    let git_available = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !git_available {
        return SyncBackendStatus::Blocked(
            "git binary not found on PATH. Install git, then return.".to_string(),
        );
    }

    // Any provider credentials? If none, allow selection but warn —
    // operator can complete setup inside the Git config rows below.
    let any_provider_ready = flynt_core::providers::PROVIDERS.iter().any(|p| {
        matches!(
            flynt_core::providers::probe_provider(p),
            flynt_core::providers::CredentialStatus::Authenticated { .. }
        )
    });
    if !any_provider_ready {
        return SyncBackendStatus::Warning(
            "No git provider credentials are configured yet. You can still pick this and set up credentials below, but pushes will fail until a token is in place.".to_string(),
        );
    }

    SyncBackendStatus::Available
}

/// Agent daemon needs the omegon binary to be resolvable — the daemon
/// is just `omegon serve` invoked as a background process. Without
/// the binary on disk (no installed version, no override, no PATH
/// match) the daemon will fail to start with a confusing
/// "no such file or directory" error.
///
/// Pass the resolved binary path from `LocalRuntimeConfig::resolve_omegon_binary`
/// — this helper just checks file existence so it stays fast and
/// synchronous for use in a render path.
pub fn evaluate_daemon(omegon_binary: &Path) -> SyncBackendStatus {
    if omegon_binary.exists() {
        SyncBackendStatus::Available
    } else {
        SyncBackendStatus::Blocked(format!(
            "Omegon binary not found at {}. Install a version (or set the channel/binary in Runtime settings) before enabling the daemon.",
            omegon_binary.display(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn none_always_available() {
        assert_eq!(evaluate_none(), SyncBackendStatus::Available);
    }

    #[test]
    fn icloud_blocked_outside_icloud_drive() {
        let path = PathBuf::from("/tmp/not-icloud");
        let status = evaluate_icloud(&path);
        assert!(status.is_blocked());
        assert!(
            status.explanation().unwrap().contains("iCloud Drive")
                || status.explanation().unwrap().contains("macOS-only"),
            "explanation should mention iCloud Drive or macOS-only, got: {:?}",
            status.explanation(),
        );
    }

    #[test]
    fn daemon_blocked_when_binary_missing() {
        let missing = PathBuf::from("/definitely/does/not/exist/omegon-binary");
        let status = evaluate_daemon(&missing);
        assert!(status.is_blocked());
        assert!(
            status
                .explanation()
                .unwrap()
                .contains("Omegon binary not found")
        );
    }

    #[test]
    fn status_explanation_is_some_when_not_available() {
        assert!(
            SyncBackendStatus::Warning("x".into())
                .explanation()
                .is_some()
        );
        assert!(
            SyncBackendStatus::Blocked("y".into())
                .explanation()
                .is_some()
        );
        assert!(SyncBackendStatus::Available.explanation().is_none());
    }
}
