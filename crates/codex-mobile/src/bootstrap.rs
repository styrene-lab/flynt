use anyhow::Result;
use codex_core::models::SyncConfig;
use codex_store::{sync::AutoSyncHandle, vault::Vault};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tracing::{info, warn};

/// Vault root on mobile — uses the app's Documents directory.
pub fn vault_root() -> PathBuf {
    dirs::document_dir()
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("Codex")
}

/// Mobile runtime state — simpler than desktop (no watcher, no omegon).
#[derive(Clone)]
pub struct MobileRuntime {
    pub vault_root: PathBuf,
    pub vault: Arc<Vault>,
    pub _sync_handle: Option<Arc<AutoSyncHandle>>,
}

/// Bootstrap the mobile vault — open, index, start sync.
pub fn bootstrap() -> Result<MobileRuntime> {
    let root = vault_root();
    std::fs::create_dir_all(&root)?;

    let vault = Arc::new(Vault::open(&root)?);

    match vault.reindex() {
        Ok((n, errs)) => {
            info!("Vault indexed: {n} files");
            for e in &errs {
                warn!("Index error: {e}");
            }
        }
        Err(e) => warn!("Reindex failed: {e}"),
    }

    // Start auto-sync if configured
    let sync_handle = match &vault.config.sync {
        SyncConfig::Git {
            remote,
            branch,
            auto_commit_seconds,
        } if *auto_commit_seconds > 0 => {
            let interval = Duration::from_secs((*auto_commit_seconds).max(30));
            let vault_for_reindex = Arc::clone(&vault);
            let reindex_cb: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Err(e) = vault_for_reindex.reindex() {
                    warn!("Post-pull reindex failed: {e}");
                }
            });
            let (handle, _status_rx) = codex_store::sync::start_auto_sync(
                root.clone(),
                remote.clone(),
                branch.clone(),
                interval,
                Some(reindex_cb),
            );
            info!(
                "Auto-sync started: every {}s to {remote}/{branch}",
                auto_commit_seconds
            );
            Some(Arc::new(handle))
        }
        _ => None,
    };

    Ok(MobileRuntime {
        vault_root: root,
        vault,
        _sync_handle: sync_handle,
    })
}
