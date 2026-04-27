use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

#[derive(Debug, Clone)]
pub enum VaultChangeEvent {
    FileModified(PathBuf),
    FileCreated(PathBuf),
    FileDeleted(PathBuf),
}

pub struct VaultWatcher {
    _watcher: RecommendedWatcher,
    pub rx: mpsc::Receiver<VaultChangeEvent>,
}

impl VaultWatcher {
    pub fn new(vault_root: &Path) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let codex_dir = vault_root.join(".codex");

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
            for path in event.paths {
                if path.starts_with(&codex_dir) { continue; }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "md" && ext != "excalidraw" && ext != "d2" { continue; }
                let evt = match event.kind {
                    notify::EventKind::Create(_) => VaultChangeEvent::FileCreated(path),
                    notify::EventKind::Modify(_) => VaultChangeEvent::FileModified(path),
                    notify::EventKind::Remove(_) => VaultChangeEvent::FileDeleted(path),
                    _ => continue,
                };
                let _ = tx.send(evt);
            }
        })?;

        watcher.watch(vault_root, RecursiveMode::Recursive)?;
        Ok(Self { _watcher: watcher, rx })
    }
}
