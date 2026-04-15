use anyhow::{Context, Result, bail};
use codex_core::sync::{SyncBackend, SyncResult, SyncStatus};
use git2::{IndexAddOption, ObjectType, Repository, ResetType, Signature};
use std::path::PathBuf;

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
        Repository::open(&self.vault_root)
            .context("failed to open git repository — is this vault a git repo?")
    }

    fn sig() -> Result<Signature<'static>> {
        Ok(Signature::now("Codex", "codex@local")?)
    }
}

impl SyncBackend for GitSync {
    fn name(&self) -> &str { "git" }

    fn status(&self) -> Result<SyncStatus> {
        let repo = self.open_repo()?;
        let statuses = repo.statuses(None)?;
        let dirty = statuses.iter().any(|s| {
            !s.status().is_empty()
                && !s.status().contains(git2::Status::IGNORED)
                && !s.status().contains(git2::Status::CURRENT)
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
        remote.fetch(&[&self.branch], None, None)?;

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
        remote.push(&[&refspec], None)?;
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
        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
        index.write()?;
        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let sig = Self::sig()?;

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
