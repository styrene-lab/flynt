use anyhow::Result;
use flynt_core::sync::{SyncBackend, SyncResult, SyncStatus};
use git2::{Cred, FetchOptions, IndexAddOption, PushOptions, RemoteCallbacks, Repository};
use std::path::PathBuf;
use tracing::debug;

use super::util;

pub struct GitSync {
    pub vault_root: PathBuf,
    pub remote: String,
    pub branch: String,
}

impl GitSync {
    pub fn new(vault_root: PathBuf, remote: impl Into<String>, branch: impl Into<String>) -> Self {
        Self { vault_root, remote: remote.into(), branch: branch.into() }
    }

    fn open_repo(&self) -> Result<Repository> {
        util::open_repo(&self.vault_root)
    }

    /// Build credential callbacks that handle SSH agent, SSH key files, and HTTPS.
    pub fn credential_callbacks() -> RemoteCallbacks<'static> {
        let mut cb = RemoteCallbacks::new();
        let mut attempt: u32 = 0;
        cb.credentials(move |url, username_from_url, allowed_types| {
            attempt += 1;
            debug!("git auth attempt {attempt} for {url}: username={username_from_url:?}, types={allowed_types:?}");

            // Bail after a few attempts to avoid infinite loops
            if attempt > 4 {
                return Err(git2::Error::from_str(
                    "Authentication failed. Make sure your SSH key is loaded in ssh-agent \
                     (run: ssh-add), or configure a Git credential helper for HTTPS."
                ));
            }

            // SSH agent first (most common for GitHub users)
            if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                let user = username_from_url.unwrap_or("git");

                // Attempt 1: SSH agent
                if attempt == 1 {
                    if let Ok(cred) = Cred::ssh_key_from_agent(user) {
                        return Ok(cred);
                    }
                }

                // Attempt 2+: try key files (without passphrase — works for unencrypted keys
                // and keys whose passphrase is cached by the agent)
                if let Some(home) = dirs::home_dir() {
                    let ssh_dir = home.join(".ssh");
                    let key_idx = (attempt as usize).saturating_sub(1);
                    let key_names = ["id_ed25519", "id_rsa", "id_ecdsa"];
                    if let Some(key_name) = key_names.get(key_idx) {
                        let key_path = ssh_dir.join(key_name);
                        let pub_path = ssh_dir.join(format!("{key_name}.pub"));
                        if key_path.exists() {
                            let pub_file = if pub_path.exists() { Some(pub_path.as_path()) } else { None };
                            if let Ok(cred) = Cred::ssh_key(user, pub_file, &key_path, None) {
                                return Ok(cred);
                            }
                        }
                    }
                }

                return Err(git2::Error::from_str(
                    "SSH authentication failed. If your key has a passphrase, \
                     load it first with: ssh-add ~/.ssh/id_ed25519"
                ));
            }

            // HTTPS: check auth.json for stored PAT/OAuth token, then credential helper
            if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
                if attempt == 1 {
                    if let Some(token) = flynt_core::providers::token_for_url(url) {
                        return Cred::userpass_plaintext(&token, "x-oauth-basic");
                    }
                }
                return Cred::credential_helper(
                    &git2::Config::open_default().unwrap_or_else(|_| git2::Config::new().unwrap()),
                    url,
                    username_from_url,
                );
            }

            // Username query (often precedes SSH_KEY)
            if allowed_types.contains(git2::CredentialType::USERNAME) {
                return Cred::username(username_from_url.unwrap_or("git"));
            }

            Err(git2::Error::from_str("unsupported credential type"))
        });
        cb
    }

    fn fetch_options() -> FetchOptions<'static> {
        let mut opts = FetchOptions::new();
        opts.remote_callbacks(Self::credential_callbacks());
        opts
    }

    fn push_options() -> PushOptions<'static> {
        let mut opts = PushOptions::new();
        opts.remote_callbacks(Self::credential_callbacks());
        opts
    }
}

impl GitSync {
    /// Build credential callbacks that use a personal access token for HTTPS auth.
    /// The token is passed as the password with an empty username (GitHub convention).
    pub fn credential_callbacks_with_token(token: String) -> RemoteCallbacks<'static> {
        let mut cb = RemoteCallbacks::new();
        cb.credentials(move |_url, _username_from_url, _allowed_types| {
            Cred::userpass_plaintext(&token, "x-oauth-basic")
        });
        cb
    }

    /// Clone a remote repository using a personal access token or OAuth token.
    /// On failure, cleans up the destination directory if it was created by the clone.
    pub fn clone_repo_with_token(
        url: &str,
        branch: &str,
        dest: &std::path::Path,
        token: &str,
    ) -> Result<Repository> {
        use git2::build::RepoBuilder;

        let dest_existed = dest.exists();

        let mut fetch_opts = FetchOptions::new();
        fetch_opts.remote_callbacks(Self::credential_callbacks_with_token(token.to_string()));

        let result = RepoBuilder::new()
            .branch(branch)
            .fetch_options(fetch_opts)
            .clone(url, dest);

        match result {
            Ok(repo) => Ok(repo),
            Err(e) => {
                if !dest_existed && dest.exists() {
                    let _ = std::fs::remove_dir_all(dest);
                }
                Err(e.into())
            }
        }
    }

    /// Clone a remote repository into `dest`. Returns the cloned repo.
    /// On failure, cleans up the destination directory if it was created by the clone.
    pub fn clone_repo(url: &str, branch: &str, dest: &std::path::Path) -> Result<Repository> {
        use git2::build::RepoBuilder;

        let dest_existed = dest.exists();

        let mut fetch_opts = FetchOptions::new();
        fetch_opts.remote_callbacks(Self::credential_callbacks());

        let result = RepoBuilder::new()
            .branch(branch)
            .fetch_options(fetch_opts)
            .clone(url, dest);

        match result {
            Ok(repo) => Ok(repo),
            Err(e) => {
                // Clean up partially-cloned directory if we created it
                if !dest_existed && dest.exists() {
                    let _ = std::fs::remove_dir_all(dest);
                }
                Err(e.into())
            }
        }
    }
}

// ── Tagging ─────────────────────────────────────────────────────────────────

/// A project tag (git tag wrapper).
#[derive(Debug, Clone)]
pub struct VaultTag {
    pub name: String,
    pub message: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl GitSync {
    /// Create a lightweight tag at HEAD.
    pub fn create_tag(&self, name: &str, message: Option<&str>) -> Result<()> {
        let repo = self.open_repo()?;
        let head = repo.head()?.peel_to_commit()?;
        if let Some(msg) = message {
            let sig = util::repo_signature(&repo)?;
            repo.tag(name, head.as_object(), &sig, msg, false)?;
        } else {
            repo.tag_lightweight(name, head.as_object(), false)?;
        }
        Ok(())
    }

    /// List all tags with timestamps.
    pub fn list_tags(&self) -> Result<Vec<VaultTag>> {
        let repo = self.open_repo()?;
        let mut tags = Vec::new();
        repo.tag_foreach(|oid, raw_name| {
            let name = String::from_utf8_lossy(raw_name)
                .trim_start_matches("refs/tags/")
                .to_string();
            let timestamp = repo.find_commit(oid)
                .map(|c| {
                    chrono::DateTime::from_timestamp(c.time().seconds(), 0)
                        .unwrap_or_default()
                })
                .or_else(|_| {
                    // Annotated tag — dereference to commit
                    repo.find_tag(oid).and_then(|tag| {
                        tag.target().and_then(|t| t.peel_to_commit()).map(|c| {
                            chrono::DateTime::from_timestamp(c.time().seconds(), 0)
                                .unwrap_or_default()
                        })
                    })
                })
                .unwrap_or_default();
            let message = repo.find_tag(oid).ok().map(|t| t.message().unwrap_or("").to_string());
            tags.push(VaultTag { name, message, timestamp });
            true
        })?;
        tags.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(tags)
    }

    /// Delete a tag.
    pub fn delete_tag(&self, name: &str) -> Result<()> {
        let repo = self.open_repo()?;
        repo.tag_delete(name)?;
        Ok(())
    }

    /// Push tags to remote.
    pub fn push_tags(&self) -> Result<()> {
        let repo = self.open_repo()?;
        let mut remote = repo.find_remote(&self.remote)?;
        let mut opts = Self::push_options();
        remote.push(&["refs/tags/*:refs/tags/*"], Some(&mut opts))?;
        Ok(())
    }
}

impl SyncBackend for GitSync {
    fn name(&self) -> &str { "git" }

    fn status(&self) -> Result<SyncStatus> {
        let repo = self.open_repo()?;
        let statuses = repo.statuses(None)?;
        let dirty = statuses.iter().any(|s| {
            let st = s.status();
            !st.is_empty() && !st.contains(git2::Status::IGNORED)
        });
        if dirty {
            return Ok(SyncStatus::Syncing); // uncommitted changes
        }

        // Check unpushed commits: HEAD vs remote branch
        let head = match repo.head() {
            Ok(h) => h,
            Err(_) => return Ok(SyncStatus::Idle), // no commits yet
        };
        let remote_ref = format!("refs/remotes/{}/{}", self.remote, self.branch);
        if let (Ok(local), Ok(remote)) = (
            head.peel_to_commit(),
            repo.find_reference(&remote_ref).and_then(|r| r.peel_to_commit()),
        ) {
            if local.id() != remote.id() {
                return Ok(SyncStatus::Syncing); // ahead of remote
            }
        }
        Ok(SyncStatus::Idle)
    }

    fn pull(&self) -> Result<SyncResult> {
        let repo = self.open_repo()?;
        let mut remote = repo.find_remote(&self.remote)?;
        remote.fetch(&[&self.branch], Some(&mut Self::fetch_options()), None)?;

        let remote_ref = format!("refs/remotes/{}/{}", self.remote, self.branch);
        let fetch_commit = repo
            .find_reference(&remote_ref)?
            .peel_to_commit()?;
        let fetch_annotated = repo.find_annotated_commit(fetch_commit.id())?;

        let (analysis, _) = repo.merge_analysis(&[&fetch_annotated])?;

        if analysis.is_up_to_date() {
            return Ok(SyncResult { files_pulled: 0, files_pushed: 0, conflicts: vec![] });
        }

        if analysis.is_fast_forward() {
            let refname = format!("refs/heads/{}", self.branch);
            let mut reference = repo.find_reference(&refname)?;
            reference.set_target(fetch_commit.id(), "fast-forward")?;
            repo.set_head(&refname)?;
            repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
            return Ok(SyncResult { files_pulled: 1, files_pushed: 0, conflicts: vec![] });
        }

        // Non-fast-forward: attempt merge, detect conflicts
        repo.merge(&[&fetch_annotated], None, None)?;
        let index = repo.index()?;
        if index.has_conflicts() {
            let conflicts: Vec<String> = index
                .conflicts()?
                .filter_map(|c| c.ok())
                .filter_map(|c| c.our.or(c.their))
                .filter_map(|e| String::from_utf8(e.path).ok())
                .collect();
            repo.cleanup_state()?;
            return Ok(SyncResult { files_pulled: 0, files_pushed: 0, conflicts });
        }
        repo.cleanup_state()?;
        Ok(SyncResult { files_pulled: 1, files_pushed: 0, conflicts: vec![] })
    }

    fn push(&self) -> Result<SyncResult> {
        let repo = self.open_repo()?;
        let mut remote = repo.find_remote(&self.remote)?;
        let refspec = format!("refs/heads/{}:refs/heads/{}", self.branch, self.branch);
        remote.push(&[&refspec], Some(&mut Self::push_options()))?;
        Ok(SyncResult { files_pulled: 0, files_pushed: 1, conflicts: vec![] })
    }

    fn sync(&self) -> Result<SyncResult> {
        let pulled = self.pull()?;
        if !pulled.conflicts.is_empty() {
            return Ok(pulled);
        }
        let pushed = self.push()?;
        Ok(SyncResult {
            files_pulled: pulled.files_pulled,
            files_pushed: pushed.files_pushed,
            conflicts: vec![],
        })
    }
}

impl GitSync {
    /// Stage all changes and commit. Safe to call even if working tree is clean.
    pub fn auto_commit(&self, message: &str) -> Result<()> {
        let repo = self.open_repo()?;
        let mut index = repo.index()?;
        // Stage all changes. IndexAddOption::DEFAULT respects .gitignore.
        // The .gitignore (created by Project::open) excludes .flynt-local/.
        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
        index.write()?;
        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let sig = util::repo_signature(&repo)?;

        // Check for empty commit (nothing staged)
        let is_empty = repo.head().is_err(); // no commits yet
        if !is_empty {
            let parent = repo.head()?.peel_to_commit()?;
            if parent.tree_id() == tree_oid {
                return Ok(()); // nothing changed
            }
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?;
        } else {
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tag_tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_repo() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        let repo = git2::Repository::init(&repo_path).unwrap();

        // Create initial commit
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        (tmp, repo_path)
    }

    #[test]
    fn create_and_list_tags() {
        let (_tmp, repo_path) = setup_repo();
        let git = GitSync::new(repo_path, "origin", "main");

        git.create_tag("v1.0.0", Some("release")).unwrap();
        git.create_tag("v1.0.1", None).unwrap();

        let tags = git.list_tags().unwrap();
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"v1.0.0"));
        assert!(names.contains(&"v1.0.1"));
    }

    #[test]
    fn delete_tag() {
        let (_tmp, repo_path) = setup_repo();
        let git = GitSync::new(repo_path, "origin", "main");

        git.create_tag("deleteme", None).unwrap();
        assert!(git.list_tags().unwrap().iter().any(|t| t.name == "deleteme"));

        git.delete_tag("deleteme").unwrap();
        assert!(!git.list_tags().unwrap().iter().any(|t| t.name == "deleteme"));
    }

    #[test]
    fn duplicate_tag_fails() {
        let (_tmp, repo_path) = setup_repo();
        let git = GitSync::new(repo_path, "origin", "main");

        git.create_tag("unique", None).unwrap();
        assert!(git.create_tag("unique", None).is_err());
    }
}
