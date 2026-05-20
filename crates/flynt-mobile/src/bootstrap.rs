use anyhow::Result;
use flynt_core::models::SyncConfig;
use flynt_store::{project::Project, sync::AutoSyncHandle};
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use tracing::{info, warn};

/// Project root on mobile — uses the app's Documents directory.
pub fn project_root() -> PathBuf {
    dirs::document_dir()
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("Flynt")
}

/// Alias for use from onboarding view.
pub fn default_project_root() -> PathBuf {
    project_root()
}

/// Check if a project has been initialized (has at least a .flynt directory or any .md files).
pub fn has_project() -> bool {
    let root = project_root();
    root.join(".flynt").exists() || root.join(".git").exists()
}

/// Mobile runtime state — simpler than desktop (no watcher, no omegon).
#[derive(Clone)]
pub struct MobileRuntime {
    pub project_root: PathBuf,
    pub project: Arc<Project>,
    pub _sync_handle: Option<Arc<AutoSyncHandle>>,
}

/// Bootstrap the mobile project — open, index, start sync.
pub fn bootstrap() -> Result<MobileRuntime> {
    let root = project_root();
    std::fs::create_dir_all(&root)?;

    let project = Arc::new(Project::open(&root)?);

    match project.reindex() {
        Ok((n, errs)) => {
            info!("Project indexed: {n} files");
            for e in &errs {
                warn!("Index error: {e}");
            }
            // Create a welcome note for fresh projects
            if n == 0 {
                let welcome = std::path::PathBuf::from("Welcome.md");
                let content = "+++\ntitle = \"Welcome\"\ntags = []\n+++\n\n# Welcome to Flynt\n\nThis is your first note. Start writing, or explore the app.\n\n- **Notes** — write and organize your thoughts\n- **Board** — track tasks with kanban boards\n- **Graph** — see how your notes connect\n";
                let _ = project.save_document_content(&welcome, content);
                let _ = project.reindex();
            }
        }
        Err(e) => warn!("Reindex failed: {e}"),
    }

    // Drain share-extension inbox
    match drain_inbox(&project) {
        Ok(0) => {}
        Ok(n) => info!("Imported {n} notes from share inbox"),
        Err(e) => warn!("Inbox drain error: {e}"),
    }

    // Start auto-sync if configured
    let sync_handle = match &project.config.sync {
        SyncConfig::Git {
            remote,
            branch,
            auto_commit_seconds,
        } if *auto_commit_seconds > 0 => {
            let interval = Duration::from_secs((*auto_commit_seconds).max(30));
            let project_for_reindex = Arc::clone(&project);
            let reindex_cb: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Err(e) = project_for_reindex.reindex() {
                    warn!("Post-pull reindex failed: {e}");
                }
            });
            let (handle, _status_rx) = flynt_store::sync::start_auto_sync(
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
        project_root: root,
        project,
        _sync_handle: sync_handle,
    })
}

// ─── Share Extension Inbox ────────────────────────────────────

/// Drain the share-extension inbox: move .md files (and assets/) from the
/// App Group container into the project, then index each one.
pub fn drain_inbox(project: &Project) -> Result<usize> {
    let Some(inbox) = app_group_inbox_dir() else {
        return Ok(0);
    };
    if !inbox.exists() {
        return Ok(0);
    }

    let mut count = 0;

    // Move shared assets first (images etc.)
    let inbox_assets = inbox.join("assets");
    if inbox_assets.is_dir() {
        let project_assets = project.root.join("assets");
        fs::create_dir_all(&project_assets)?;
        for entry in fs::read_dir(&inbox_assets)? {
            let entry = entry?;
            let dest = project_assets.join(entry.file_name());
            fs::rename(entry.path(), &dest)?;
        }
        let _ = fs::remove_dir(&inbox_assets);
    }

    // Move .md files into project root and index
    for entry in fs::read_dir(&inbox)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "md") {
            let dest = project.root.join(path.file_name().unwrap());
            fs::rename(&path, &dest)?;
            if let Err(e) = project.index_file(&dest) {
                warn!("Failed to index shared note {}: {e}", dest.display());
            }
            count += 1;
        }
    }

    Ok(count)
}

/// Resolve the flynt-inbox directory inside the App Group shared container.
fn app_group_inbox_dir() -> Option<PathBuf> {
    app_group_container().map(|c| c.join("flynt-inbox"))
}

/// Get the App Group container path via NSFileManager.
#[cfg(target_os = "ios")]
fn app_group_container() -> Option<PathBuf> {
    use objc2::rc::Retained;
    use objc2_foundation::{NSFileManager, NSString};

    let fm = NSFileManager::defaultManager();
    let group = NSString::from_str("group.io.styrene.codex");
    let url: Option<Retained<objc2_foundation::NSURL>> =
        fm.containerURLForSecurityApplicationGroupIdentifier(&group);
    url.and_then(|u| u.path().map(|p| PathBuf::from(p.to_string())))
}

/// Stub for non-iOS builds (macOS dev, tests).
#[cfg(not(target_os = "ios"))]
fn app_group_container() -> Option<PathBuf> {
    // On macOS, use a local dev path for testing the inbox flow
    dirs::data_dir().map(|d| d.join("io.styrene.codex.group"))
}
