use codex_store::{vault::Vault, watcher::{VaultChangeEvent, VaultWatcher}};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::broadcast;
use tracing::{info, warn};

/// Top-level runtime context injected into the Dioxus app.
#[derive(Clone)]
pub struct AppContext {
    pub vault: Arc<Vault>,
    pub vault_events: broadcast::Sender<VaultChangeEvent>,
}

/// Build AppContext at launch. Reads CODEX_VAULT env var or defaults to ~/Documents/Codex.
pub fn bootstrap_from_env() -> AppContext {
    let vault_root = std::env::var("CODEX_VAULT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("Codex")
        });

    std::fs::create_dir_all(&vault_root).expect("cannot create vault directory");

    let vault = Arc::new(Vault::open(&vault_root).expect("failed to open vault"));

    match vault.reindex() {
        Ok((n, errs)) => {
            info!("Vault indexed: {n} files");
            for e in &errs {
                warn!("Index error: {e}");
            }
        }
        Err(e) => warn!("Reindex failed: {e}"),
    }

    let (tx, _rx) = broadcast::channel::<VaultChangeEvent>(256);
    let tx_clone = tx.clone();
    let vault_root_clone = vault_root.clone();
    let vault_clone = Arc::clone(&vault);

    tokio::spawn(async move {
        let watcher = match VaultWatcher::new(&vault_root_clone) {
            Ok(w) => w,
            Err(e) => { warn!("VaultWatcher failed to start: {e}"); return; }
        };
        loop {
            match watcher.rx.recv() {
                Ok(evt) => {
                    // Re-index the changed file
                    let path = match &evt {
                        VaultChangeEvent::FileModified(p) | VaultChangeEvent::FileCreated(p) => {
                            Some(p.clone())
                        }
                        VaultChangeEvent::FileDeleted(_) => None,
                    };
                    if let Some(p) = path {
                        if let Err(e) = vault_clone.index_file(&p) {
                            warn!("Re-index failed for {}: {e}", p.display());
                        }
                    }
                    let _ = tx_clone.send(evt);
                }
                Err(_) => break,
            }
        }
    });

    AppContext { vault, vault_events: tx }
}
