//! Shared git2 utilities used by both `GitSync` (project-level) and
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

/// Default commit signature. Used when no repo-local identity is configured.
fn default_signature() -> Result<Signature<'static>> {
    Ok(Signature::now("Flynt", "flynt@local")?)
}

/// Signature for a specific repository.
///
/// Resolution order (intentional — never leak global gitconfig into project commits):
///   1. Repo-local .git/config (set by StyreneIdentity configure_git_signing)
///   2. Flynt default ("Flynt <flynt@local>")
///
/// Global ~/.gitconfig is NOT consulted. The operator's personal git
/// identity for other repos should not bleed into project auto-commits.
pub fn repo_signature(repo: &Repository) -> Result<Signature<'static>> {
    if let Ok(config) = repo.config() {
        // Only read repo-local level, not global
        if let Ok(local) = config.open_level(git2::ConfigLevel::Local) {
            let name = local.get_string("user.name");
            let email = local.get_string("user.email");
            if let (Ok(name), Ok(email)) = (name, email) {
                if !name.is_empty() && !email.is_empty() {
                    return Ok(Signature::now(&name, &email)?);
                }
            }
        }
    }

    default_signature()
}

/// Backward-compat alias — prefer repo_signature() when a repo is available.
pub fn flynt_signature() -> Result<Signature<'static>> {
    default_signature()
}
