use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

#[derive(Debug, Clone)]
pub enum ProjectChangeEvent {
    FileModified(PathBuf),
    FileCreated(PathBuf),
    FileDeleted(PathBuf),
}

pub struct ProjectWatcher {
    _watcher: RecommendedWatcher,
    pub rx: mpsc::Receiver<ProjectChangeEvent>,
}

impl ProjectWatcher {
    pub fn new(project_root: &Path) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        // Canonicalize the project path before deriving the skip prefix and
        // before handing it to notify. macOS FSEvents emits paths against
        // the underlying filesystem (symlinks resolved), so a starts_with
        // filter built from a symlinked input would silently miss every
        // event. Same hardening pass omegon shipped in 0.19.4's triggers.rs;
        // we have the same notify backend and the same exposure.
        let canonical_root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
        let flynt_dir = canonical_root.join(".flynt");

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
            for path in event.paths {
                if path.starts_with(&flynt_dir) { continue; }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "md" && ext != "excalidraw" && ext != "d2" && ext != "canvas" && ext != "flow" { continue; }
                let evt = match event.kind {
                    notify::EventKind::Create(_) => ProjectChangeEvent::FileCreated(path),
                    notify::EventKind::Modify(_) => ProjectChangeEvent::FileModified(path),
                    notify::EventKind::Remove(_) => ProjectChangeEvent::FileDeleted(path),
                    _ => continue,
                };
                let _ = tx.send(evt);
            }
        })?;

        watcher.watch(&canonical_root, RecursiveMode::Recursive)?;
        Ok(Self { _watcher: watcher, rx })
    }
}
