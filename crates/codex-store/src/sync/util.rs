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

/// Commit signature resolution order:
/// 1. Git repo config (user.name + user.email) — set by `configure_git_signing`
/// 2. Vault manifest identity (name + email from vaults.toml)
/// 3. Default: "Codyx <codyx@local>"
pub fn codex_signature() -> Result<Signature<'static>> {
    // Try git global/local config first (includes StyreneIdentity-configured signing)
    if let Ok(config) = git2::Config::open_default() {
        let name = config.get_string("user.name");
        let email = config.get_string("user.email");
        if let (Ok(name), Ok(email)) = (name, email) {
            if !name.is_empty() && !email.is_empty() {
                return Ok(Signature::now(&name, &email)?);
            }
        }
    }

    // Fallback
    Ok(Signature::now("Codyx", "codyx@local")?)
}

/// Signature for a specific repository (checks repo-local config first).
pub fn repo_signature(repo: &Repository) -> Result<Signature<'static>> {
    // Try repo-local config
    if let Ok(config) = repo.config() {
        let name = config.get_string("user.name");
        let email = config.get_string("user.email");
        if let (Ok(name), Ok(email)) = (name, email) {
            if !name.is_empty() && !email.is_empty() {
                return Ok(Signature::now(&name, &email)?);
            }
        }
    }

    // Fall back to global resolution
    codex_signature()
}
