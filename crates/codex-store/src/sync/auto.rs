//! Background auto-sync loop for git-backed vaults.
//!
//! Periodically commits local changes and syncs with the remote.
//! Designed to keep phone and desktop vaults in sync via a shared git repo.

use super::git::GitSync;
use anyhow::Result;
use codex_core::sync::SyncBackend;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{info, warn};

/// Status reported by the sync loop.
#[derive(Debug, Clone, PartialEq)]
pub enum AutoSyncStatus {
    Idle,
    Committing,
    Pulling,
    Pushing,
    Conflict(Vec<String>),
    Error(String),
}

/// Handle to a running background sync loop. Drop to stop.
pub struct AutoSyncHandle {
    _cancel: watch::Sender<bool>,
}

/// Start a background auto-sync loop for a vault.
///
/// - Commits any dirty files every `interval`
/// - Pulls from remote (fast-forward or merge)
/// - Pushes local commits to remote
/// - Reports status via the returned watch receiver
///
/// The loop runs until the handle is dropped.
pub fn start_auto_sync(
    vault_root: PathBuf,
    remote: String,
    branch: String,
    interval: Duration,
    reindex: Option<Arc<dyn Fn() + Send + Sync>>,
) -> (AutoSyncHandle, watch::Receiver<AutoSyncStatus>) {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (status_tx, status_rx) = watch::channel(AutoSyncStatus::Idle);

    tokio::spawn(async move {
        let git = GitSync::new(vault_root, &remote, &branch);
        let mut cancel = cancel_rx;

        loop {
            // Wait for the interval or cancellation
            tokio::select! {
                _ = tokio::time::sleep(interval) => {},
                _ = cancel.changed() => {
                    if *cancel.borrow() { break; }
                }
            }

            // Auto-commit
            let _ = status_tx.send(AutoSyncStatus::Committing);
            if let Err(e) = git.auto_commit("[codex] auto-sync") {
                warn!("auto-commit failed: {e}");
                let _ = status_tx.send(AutoSyncStatus::Error(format!("commit: {e}")));
                continue;
            }

            // Pull
            let _ = status_tx.send(AutoSyncStatus::Pulling);
            match git.pull() {
                Ok(result) if !result.conflicts.is_empty() => {
                    warn!("sync conflicts: {:?}", result.conflicts);
                    let _ = status_tx.send(AutoSyncStatus::Conflict(result.conflicts));
                    continue;
                }
                Ok(result) if result.files_pulled > 0 => {
                    info!("pulled {} file(s)", result.files_pulled);
                    // Trigger reindex after pull
                    if let Some(ref cb) = reindex {
                        cb();
                    }
                }
                Ok(_) => {} // up to date
                Err(e) => {
                    warn!("pull failed: {e}");
                    let _ = status_tx.send(AutoSyncStatus::Error(format!("pull: {e}")));
                    continue;
                }
            }

            // Push
            let _ = status_tx.send(AutoSyncStatus::Pushing);
            match git.push() {
                Ok(_) => {
                    let _ = status_tx.send(AutoSyncStatus::Idle);
                }
                Err(e) => {
                    warn!("push failed: {e}");
                    let _ = status_tx.send(AutoSyncStatus::Error(format!("push: {e}")));
                }
            }
        }

        info!("auto-sync loop stopped");
    });

    (AutoSyncHandle { _cancel: cancel_tx }, status_rx)
}
