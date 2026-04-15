use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

pub enum VaultEvent {
    Modified(PathBuf),
    Created(PathBuf),
    Deleted(PathBuf),
}

pub struct VaultWatcher {
    _watcher: RecommendedWatcher,
    pub rx: mpsc::Receiver<VaultEvent>,
}

impl VaultWatcher {
    pub fn new(vault_root: &Path) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let codex_dir = vault_root.join(".codex");

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
            for path in event.paths {
                // Skip .codex internals and non-markdown files
                if path.starts_with(&codex_dir) {
                    continue;
                }
                let is_md = path.extension().map(|e| e == "md").unwrap_or(false);
                if !is_md {
                    continue;
                }
                let evt = match event.kind {
                    notify::EventKind::Create(_) => VaultEvent::Created(path),
                    notify::EventKind::Modify(_) => VaultEvent::Modified(path),
                    notify::EventKind::Remove(_) => VaultEvent::Deleted(path),
                    _ => continue,
                };
                let _ = tx.send(evt);
            }
        })?;

        watcher.watch(vault_root, RecursiveMode::Recursive)?;
        Ok(Self { _watcher: watcher, rx })
    }
}
