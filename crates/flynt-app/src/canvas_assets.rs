//! Canvas asset bootstrap — copies bundled tweakcn presets and shadcn
//! primitives into the project's `.flynt-local/flynt/assets/` directory so
//! `flynt-agent` (a separate binary) can read them via the `canvas_*`
//! tools (phase 5).
//!
//! Why a project-side copy? `flynt-app` and `flynt-agent` are two binaries
//! installed into different locations. The app has the bundled assets
//! via `include_str!`; the agent needs to find them without depending on
//! the app's install path. Putting a copy under `.flynt-local/flynt/` is
//! the same handoff pattern `ui_state.rs` uses.
//!
//! The copy is idempotent and content-aware: we only write when the file
//! is missing or its bytes differ from the bundled version. New Flynt
//! releases that ship updated presets/primitives propagate to the project
//! on next launch with no migration step.

use std::path::Path;

const TWEAKCN_PRESETS: &[u8] = include_bytes!("../assets/vendor/tweakcn-presets.json");
const SHADCN_PRIMITIVES: &[u8] = include_bytes!("../assets/vendor/shadcn-primitives.json");

/// Copy bundled canvas assets into `<project>/.flynt-local/flynt/assets/`
/// if they're missing or stale. Errors are logged but not propagated —
/// the canvas still renders without the project-side copy; the only
/// surface that requires it is the `canvas_*` agent tool family.
pub fn bootstrap(project_root: &Path) {
    let dir = project_root
        .join(".flynt-local")
        .join("flynt")
        .join("assets");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("canvas asset dir create failed: {e}");
        return;
    }

    write_if_changed(&dir.join("tweakcn-presets.json"), TWEAKCN_PRESETS);
    write_if_changed(&dir.join("shadcn-primitives.json"), SHADCN_PRIMITIVES);
}

fn write_if_changed(path: &Path, bundled: &[u8]) {
    let current = std::fs::read(path).ok();
    if current.as_deref() == Some(bundled) {
        return;
    }
    if let Err(e) = std::fs::write(path, bundled) {
        tracing::warn!("canvas asset write {} failed: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn bootstrap_creates_both_files() {
        let tmp = TempDir::new().unwrap();
        bootstrap(tmp.path());

        let presets = tmp
            .path()
            .join(".flynt-local/flynt/assets/tweakcn-presets.json");
        let primitives = tmp
            .path()
            .join(".flynt-local/flynt/assets/shadcn-primitives.json");
        assert!(presets.exists(), "presets file should be written");
        assert!(primitives.exists(), "primitives file should be written");
    }

    #[test]
    fn bootstrap_files_are_valid_json() {
        let tmp = TempDir::new().unwrap();
        bootstrap(tmp.path());

        let presets = tmp
            .path()
            .join(".flynt-local/flynt/assets/tweakcn-presets.json");
        let parsed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&presets).unwrap()).unwrap();
        assert!(
            parsed.get("default").is_some(),
            "default theme should be present"
        );

        let primitives = tmp
            .path()
            .join(".flynt-local/flynt/assets/shadcn-primitives.json");
        let parsed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&primitives).unwrap()).unwrap();
        assert_eq!(parsed["version"], 1);
        assert!(parsed["primitives"].as_array().unwrap().len() >= 5);
    }

    #[test]
    fn bootstrap_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        bootstrap(tmp.path());
        let path = tmp
            .path()
            .join(".flynt-local/flynt/assets/tweakcn-presets.json");
        let mtime1 = std::fs::metadata(&path).unwrap().modified().unwrap();

        // Second call shouldn't rewrite an unchanged file.
        std::thread::sleep(std::time::Duration::from_millis(20));
        bootstrap(tmp.path());
        let mtime2 = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(mtime1, mtime2, "unchanged file must not be rewritten");
    }

    #[test]
    fn bootstrap_repairs_corrupted_file() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".flynt-local/flynt/assets");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tweakcn-presets.json");
        std::fs::write(&path, b"corrupted").unwrap();

        bootstrap(tmp.path());
        let after = std::fs::read(&path).unwrap();
        assert_ne!(after, b"corrupted", "corrupted file must be repaired");
        assert_eq!(after, TWEAKCN_PRESETS);
    }
}
