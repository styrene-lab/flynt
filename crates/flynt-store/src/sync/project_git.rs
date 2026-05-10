//! Project-level git operations scoped to a sub-path within a repository.
//!
//! Unlike `GitSync` which operates on the entire project repo, `ProjectGit`
//! stages and commits only files under a project's sub-path. For ProjectRepo
//! projects, flushing tasks to disk is the main job (project-level sync handles
//! actual commits). For ExternalRepo projects, this module handles the full
//! commit cycle independently.

use anyhow::{Context, Result};
use flynt_core::models::GitBacking;
use git2::{IndexAddOption, PushOptions, Repository};
use std::path::{Path, PathBuf};

use super::util;

/// Scoped git operations for a single project's data within a repository.
pub struct ProjectGit {
    /// Absolute path to the git repo root.
    repo_root: PathBuf,
    /// Sub-path within the repo where project data lives.
    sub_path: PathBuf,
    /// Remote + branch config (only for ExternalRepo; ProjectRepo is None).
    remote_config: Option<(String, String)>,
}

/// Report of files staged during a project commit.
#[derive(Debug, Clone, Default)]
pub struct StageReport {
    pub staged: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

impl ProjectGit {
    /// Open a `ProjectGit` from a `GitBacking` configuration.
    ///
    /// For `ProjectRepo`, the `project_root` is used as the repo root.
    /// For `ExternalRepo`, the `repo_root` field provides the path.
    pub fn open(backing: &GitBacking, project_root: &Path) -> Result<Self> {
        match backing {
            GitBacking::ProjectRepo { sub_path } => {
                Ok(Self {
                    repo_root: project_root.to_owned(),
                    sub_path: sub_path.clone(),
                    remote_config: None,
                })
            }
            GitBacking::ExternalRepo { repo_root, sub_path, remote, branch } => {
                // Verify the repo exists
                let _ = util::open_repo(repo_root)?;
                Ok(Self {
                    repo_root: repo_root.clone(),
                    sub_path: sub_path.clone(),
                    remote_config: Some((remote.clone(), branch.clone())),
                })
            }
            GitBacking::ForgeRepo { local_path, sub_path, .. } => {
                // Forge-managed repos behave like external repos at the git level.
                // Scribe handles the forge API sync; codex just operates on the local clone.
                let _ = util::open_repo(local_path)?;
                Ok(Self {
                    repo_root: local_path.clone(),
                    sub_path: sub_path.clone(),
                    remote_config: None, // scribe manages remote sync
                })
            }
        }
    }

    /// Absolute path to the project data directory on disk.
    pub fn project_data_root(&self) -> PathBuf {
        self.repo_root.join(&self.sub_path)
    }

    /// Whether this is backed by an external repo (vs project repo).
    pub fn is_external(&self) -> bool {
        self.remote_config.is_some()
    }

    fn open_repo(&self) -> Result<Repository> {
        util::open_repo(&self.repo_root)
    }

    /// Stage all files under the project sub-path.
    /// Returns the list of paths that were staged.
    pub fn stage_project_files(&self) -> Result<StageReport> {
        let repo = self.open_repo()?;
        let mut index = repo.index()?;

        let sub_str = self.sub_path.to_string_lossy();
        let pattern = format!("{}/*", sub_str.trim_end_matches('/'));

        index.add_all([&pattern].iter(), IndexAddOption::DEFAULT, None)?;

        // Also handle deletions: update index to remove files that no longer exist
        index.update_all([&pattern].iter(), None)?;

        index.write()?;

        // Collect staged paths for the report
        let mut staged = Vec::new();
        for entry in index.iter() {
            let path = String::from_utf8_lossy(&entry.path).to_string();
            if path.starts_with(sub_str.as_ref()) {
                staged.push(PathBuf::from(path));
            }
        }

        Ok(StageReport { staged, removed: vec![] })
    }

    /// Create a scoped commit containing only project files.
    ///
    /// This stages all files under the sub-path and creates a commit.
    /// Returns the commit OID, or None if there was nothing to commit.
    pub fn commit(&self, message: &str) -> Result<Option<git2::Oid>> {
        let repo = self.open_repo()?;
        let mut index = repo.index()?;

        let sub_str = self.sub_path.to_string_lossy();
        let pattern = format!("{}/*", sub_str.trim_end_matches('/'));

        index.add_all([&pattern].iter(), IndexAddOption::DEFAULT, None)?;
        index.update_all([&pattern].iter(), None)?;
        index.write()?;

        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let sig = util::repo_signature(&repo)?;

        let is_empty = repo.head().is_err();
        if !is_empty {
            let parent = repo.head()?.peel_to_commit()?;
            if parent.tree_id() == tree_oid {
                return Ok(None); // nothing changed
            }
            let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?;
            Ok(Some(oid))
        } else {
            let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])?;
            Ok(Some(oid))
        }
    }

    /// List working-tree changes under the project sub-path.
    pub fn list_dirty_files(&self) -> Result<Vec<PathBuf>> {
        let repo = self.open_repo()?;
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true);
        let statuses = repo.statuses(Some(&mut opts))?;
        let sub_str = self.sub_path.to_string_lossy();

        let dirty: Vec<PathBuf> = statuses
            .iter()
            .filter(|s| {
                let st = s.status();
                !st.is_empty() && !st.contains(git2::Status::IGNORED)
            })
            .filter_map(|s| s.path().map(String::from))
            .filter(|p| p.starts_with(sub_str.as_ref()))
            .map(PathBuf::from)
            .collect();

        Ok(dirty)
    }

    /// Push to remote (only meaningful for ExternalRepo).
    pub fn push(&self) -> Result<()> {
        let (remote_name, branch) = self.remote_config.as_ref()
            .context("push called on ProjectRepo project — project-level sync handles this")?;
        let repo = self.open_repo()?;
        let mut remote = repo.find_remote(remote_name)?;
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        let mut push_opts = PushOptions::new();
        push_opts.remote_callbacks(super::git::GitSync::credential_callbacks());
        remote.push(&[&refspec], Some(&mut push_opts))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo(dir: &Path) {
        let repo = Repository::init(dir).unwrap();
        // Initial commit so HEAD exists
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let mut index = repo.index().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).unwrap();
    }

    #[test]
    fn open_project_repo_project() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path();
        init_repo(project_root);

        let backing = GitBacking::ProjectRepo {
            sub_path: PathBuf::from(".flynt/projects/test"),
        };
        let pg = ProjectGit::open(&backing, project_root).unwrap();
        assert!(!pg.is_external());
        assert_eq!(
            pg.project_data_root(),
            project_root.join(".flynt/projects/test")
        );
    }

    #[test]
    fn open_external_repo_project() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("ext-repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        init_repo(&repo_root);

        let backing = GitBacking::ExternalRepo {
            repo_root: repo_root.clone(),
            sub_path: PathBuf::from("data"),
            remote: "origin".into(),
            branch: "main".into(),
        };
        let pg = ProjectGit::open(&backing, tmp.path()).unwrap();
        assert!(pg.is_external());
        assert_eq!(pg.project_data_root(), repo_root.join("data"));
    }

    #[test]
    fn list_dirty_files_scoped_to_subpath() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        init_repo(root);

        let sub = root.join("projects/test/tasks");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("task1.md"), "# Task 1").unwrap();
        // Also create a file outside the sub-path
        std::fs::write(root.join("outside.md"), "# Outside").unwrap();

        let backing = GitBacking::ProjectRepo {
            sub_path: PathBuf::from("projects/test"),
        };
        let pg = ProjectGit::open(&backing, root).unwrap();
        let dirty = pg.list_dirty_files().unwrap();

        // Should include the task file but not outside.md
        assert!(dirty.iter().any(|p| p.to_string_lossy().contains("task1.md")),
            "dirty files: {:?}", dirty);
        assert!(!dirty.iter().any(|p| p.to_string_lossy().contains("outside.md")));
    }

    #[test]
    fn commit_scoped_project_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        init_repo(root);

        let sub = root.join(".flynt/projects/test/tasks");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("task1.md"), "# Task 1\n").unwrap();

        let backing = GitBacking::ProjectRepo {
            sub_path: PathBuf::from(".flynt/projects/test"),
        };
        let pg = ProjectGit::open(&backing, root).unwrap();

        let oid = pg.commit("[codex:test] flush tasks").unwrap();
        assert!(oid.is_some(), "should have created a commit");

        // Second commit with no changes should return None
        let oid2 = pg.commit("[codex:test] no-op").unwrap();
        assert!(oid2.is_none(), "no changes should mean no commit");
    }

    #[test]
    fn push_errors_on_project_repo() {
        let tmp = TempDir::new().unwrap();
        init_repo(tmp.path());

        let backing = GitBacking::ProjectRepo {
            sub_path: PathBuf::from("data"),
        };
        let pg = ProjectGit::open(&backing, tmp.path()).unwrap();
        assert!(pg.push().is_err());
    }
}
