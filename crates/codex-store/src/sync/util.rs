//! Shared git2 utilities used by both `GitSync` (vault-level) and
//! `ProjectGit` (project-level scoped operations).

use anyhow::{Context, Result};
use git2::{Repository, Signature};
use std::path::Path;

/// Open a git repository at the given path.
pub fn open_repo(path: &Path) -> Result<Repository> {
    Repository::open(path)
        .with_context(|| format!("failed to open git repository at {}", path.display()))
}

/// Discover the git repository containing the given path.
/// Walks up the directory tree to find the repo root.
pub fn discover_repo(path: &Path) -> Result<Repository> {
    Repository::discover(path)
        .with_context(|| format!("no git repository found containing {}", path.display()))
}

/// Default commit signature for Codex-generated commits.
pub fn codex_signature() -> Result<Signature<'static>> {
    Ok(Signature::now("Codex", "codex@local")?)
}
