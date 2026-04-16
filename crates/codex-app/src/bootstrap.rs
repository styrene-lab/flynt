use codex_store::{vault::Vault, watcher::{VaultChangeEvent, VaultWatcher}};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::broadcast;
use tracing::{info, warn};

#[derive(Clone)]
pub struct OmegonRuntimeContext {
    pub home_dir: PathBuf,
    pub project_profile_path: PathBuf,
    pub global_profile_path: PathBuf,
    pub extensions_dir: PathBuf,
    pub vox_manifest_path: PathBuf,
}

impl OmegonRuntimeContext {
    fn discover(vault_root: &std::path::Path) -> Self {
        let home_dir = std::env::var("OMEGON_HOME")
            .map(PathBuf::from)
            .ok()
            .filter(|path| path.is_absolute())
            .or_else(|| dirs::home_dir().map(|home| home.join(".omegon")))
            .unwrap_or_else(|| vault_root.join(".omegon-runtime"));

        Self {
            project_profile_path: vault_root.join(".omegon/profile.json"),
            global_profile_path: home_dir.join("profile.json"),
            extensions_dir: home_dir.join("extensions"),
            vox_manifest_path: home_dir.join("extensions/vox/manifest.toml"),
            home_dir,
        }
    }
}

/// Top-level runtime context injected into the Dioxus app.
#[derive(Clone)]
pub struct AppContext {
    pub vault: Arc<Vault>,
    pub vault_events: broadcast::Sender<VaultChangeEvent>,
    pub omegon: OmegonRuntimeContext,
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

    let omegon = OmegonRuntimeContext::discover(&vault_root);

    AppContext { vault, vault_events: tx, omegon }
}
