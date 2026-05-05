//! Git sync integration tests.
//!
//! Tests all sync paths using local bare repos as "remotes" — no network required.
//! Covers: status, auto_commit, pull (fast-forward, merge, conflict),
//! push, clone_repo, and the full sync cycle.

use flynt_core::sync::{SyncBackend, SyncStatus};
use flynt_store::sync::git::GitSync;
use git2::{IndexAddOption, Repository, Signature};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────────────

fn sig() -> Signature<'static> {
    Signature::now("Test", "test@test.com").unwrap()
}

/// Create a bare "remote" repo and a working "local" clone.
/// Returns (tmp_dir, local_path, remote_path).
fn setup_local_remote() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::Builder::new()
        .prefix("codex-git-test-")
        .tempdir()
        .unwrap();
    let remote_path = tmp.path().join("remote.git");
    let local_path = tmp.path().join("local");

    // Create bare remote
    Repository::init_bare(&remote_path).unwrap();

    // Clone it to local
    let repo = Repository::clone(remote_path.to_str().unwrap(), &local_path).unwrap();

    // Seed with an initial commit so we have a HEAD
    let file = local_path.join("init.md");
    fs::write(&file, "# Init\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    repo.commit(Some("HEAD"), &sig(), &sig(), "initial", &tree, &[]).unwrap();

    // Push to remote
    let mut remote = repo.find_remote("origin").unwrap();
    remote
        .push(&["refs/heads/main:refs/heads/main"], None)
        .unwrap();

    (tmp, local_path, remote_path)
}

/// Create a second clone of the same remote (simulates another device).
fn clone_second(tmp: &TempDir, remote_path: &Path) -> std::path::PathBuf {
    let second_path = tmp.path().join("second");
    Repository::clone(remote_path.to_str().unwrap(), &second_path).unwrap();
    second_path
}

fn git_sync(local_path: &Path) -> GitSync {
    GitSync::new(local_path.to_path_buf(), "origin", "main")
}

fn commit_file(repo_path: &Path, filename: &str, content: &str, message: &str) {
    fs::write(repo_path.join(filename), content).unwrap();
    let repo = Repository::open(repo_path).unwrap();
    let mut index = repo.index().unwrap();
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    repo.commit(Some("HEAD"), &sig(), &sig(), message, &tree, &[&parent]).unwrap();
}

// ── Status ──────────────────────────────────────────────────────────────────

#[test]
fn status_clean_repo_is_idle() {
    let (_tmp, local, _remote) = setup_local_remote();
    let sync = git_sync(&local);
    assert_eq!(sync.status().unwrap(), SyncStatus::Idle);
}

#[test]
fn status_dirty_file_is_syncing() {
    let (_tmp, local, _remote) = setup_local_remote();
    // Modify a tracked file (untracked files might not count as "dirty" in all configs)
    fs::write(local.join("init.md"), "# Modified\n").unwrap();
    let sync = git_sync(&local);
    assert_eq!(sync.status().unwrap(), SyncStatus::Syncing);
}

#[test]
fn status_unpushed_commit_is_syncing() {
    let (_tmp, local, _remote) = setup_local_remote();
    commit_file(&local, "new.md", "# New\n", "local commit");
    let sync = git_sync(&local);
    assert_eq!(sync.status().unwrap(), SyncStatus::Syncing);
}

// ── Auto-commit ─────────────────────────────────────────────────────────────

#[test]
fn auto_commit_stages_and_commits() {
    let (_tmp, local, _remote) = setup_local_remote();
    let sync = git_sync(&local);

    fs::write(local.join("note.md"), "# Note\n").unwrap();
    sync.auto_commit("test commit").unwrap();

    // Should be clean now (committed but not pushed)
    let repo = Repository::open(&local).unwrap();
    let statuses = repo.statuses(None).unwrap();
    let dirty = statuses.iter().any(|s| {
        !s.status().is_empty()
            && !s.status().contains(git2::Status::IGNORED)
            && !s.status().contains(git2::Status::CURRENT)
    });
    assert!(!dirty, "working tree should be clean after auto_commit");
}

#[test]
fn auto_commit_noop_when_clean() {
    let (_tmp, local, _remote) = setup_local_remote();
    let sync = git_sync(&local);

    let repo = Repository::open(&local).unwrap();
    let before = repo.head().unwrap().peel_to_commit().unwrap().id();

    // No changes → should be a no-op
    sync.auto_commit("should not create commit").unwrap();

    let after = repo.head().unwrap().peel_to_commit().unwrap().id();
    assert_eq!(before, after, "no commit should be created when tree is clean");
}

#[test]
fn auto_commit_empty_repo() {
    let tmp = tempfile::Builder::new()
        .prefix("codex-git-test-")
        .tempdir()
        .unwrap();
    let path = tmp.path().join("empty");
    Repository::init(&path).unwrap();

    let sync = GitSync::new(path.clone(), "origin", "main");
    fs::write(path.join("first.md"), "# First\n").unwrap();
    sync.auto_commit("first commit").unwrap();

    let repo = Repository::open(&path).unwrap();
    assert!(repo.head().is_ok(), "HEAD should exist after first commit");
}

// ── Pull: fast-forward ──────────────────────────────────────────────────────

#[test]
fn pull_fast_forward() {
    let (tmp, local, remote) = setup_local_remote();
    let second = clone_second(&tmp, &remote);
    let sync = git_sync(&local);

    // Commit + push from second clone
    commit_file(&second, "from-second.md", "# Second\n", "from second");
    let repo2 = Repository::open(&second).unwrap();
    let mut remote2 = repo2.find_remote("origin").unwrap();
    remote2.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

    // Pull into local — should fast-forward
    let result = sync.pull().unwrap();
    assert_eq!(result.files_pulled, 1);
    assert!(result.conflicts.is_empty());
    assert!(local.join("from-second.md").exists(), "pulled file should exist");
}

#[test]
fn pull_up_to_date() {
    let (_tmp, local, _remote) = setup_local_remote();
    let sync = git_sync(&local);

    let result = sync.pull().unwrap();
    assert_eq!(result.files_pulled, 0);
    assert!(result.conflicts.is_empty());
}

// ── Pull: merge conflict ────────────────────────────────────────────────────

#[test]
fn pull_detects_merge_conflict() {
    let (tmp, local, remote) = setup_local_remote();
    let second = clone_second(&tmp, &remote);

    // Both sides modify the same file differently
    commit_file(&local, "init.md", "# Local change\n", "local edit");
    commit_file(&second, "init.md", "# Remote change\n", "remote edit");

    // Push from second
    let repo2 = Repository::open(&second).unwrap();
    let mut remote2 = repo2.find_remote("origin").unwrap();
    remote2.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

    // Pull into local — should detect conflict
    let sync = git_sync(&local);
    let result = sync.pull().unwrap();
    assert!(!result.conflicts.is_empty(), "should detect conflict on init.md");
    assert!(result.conflicts.iter().any(|c| c.contains("init.md")));
}

// ── Pull: non-conflicting merge ─────────────────────────────────────────────

#[test]
fn pull_non_conflicting_merge() {
    let (tmp, local, remote) = setup_local_remote();
    let second = clone_second(&tmp, &remote);

    // Local adds a different file
    commit_file(&local, "local-only.md", "# Local\n", "local add");
    // Remote adds a different file
    commit_file(&second, "remote-only.md", "# Remote\n", "remote add");

    // Push from second
    let repo2 = Repository::open(&second).unwrap();
    let mut remote2 = repo2.find_remote("origin").unwrap();
    remote2.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

    // Pull into local — should merge cleanly
    let sync = git_sync(&local);
    let result = sync.pull().unwrap();
    assert!(result.conflicts.is_empty(), "no conflicts expected");
    assert!(local.join("remote-only.md").exists(), "merged file should exist");
}

// ── Push ────────────────────────────────────────────────────────────────────

#[test]
fn push_sends_commits_to_remote() {
    let (tmp, local, remote) = setup_local_remote();
    let sync = git_sync(&local);

    commit_file(&local, "pushed.md", "# Pushed\n", "push test");
    sync.push().unwrap();

    // Verify the file appears in a fresh clone
    let verify = clone_second(&tmp, &remote);
    assert!(verify.join("pushed.md").exists(), "pushed file should be in remote");
}

#[test]
fn push_fails_when_behind_remote() {
    let (tmp, local, remote) = setup_local_remote();

    // Push a commit from second clone first
    let second = clone_second(&tmp, &remote);
    commit_file(&second, "ahead.md", "# Ahead\n", "ahead");
    let repo2 = Repository::open(&second).unwrap();
    let mut remote2 = repo2.find_remote("origin").unwrap();
    remote2.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

    // Now local is behind — push should fail (non-fast-forward)
    commit_file(&local, "behind.md", "# Behind\n", "behind");
    let sync = git_sync(&local);
    let result = sync.push();
    assert!(result.is_err(), "push should fail when behind remote");
}

// ── Full sync cycle ─────────────────────────────────────────────────────────

#[test]
fn sync_commits_pulls_pushes() {
    let (tmp, local, remote) = setup_local_remote();
    let sync = git_sync(&local);

    // Create a local change
    fs::write(local.join("synced.md"), "# Synced\n").unwrap();
    sync.auto_commit("sync commit").unwrap();

    // Full sync
    let result = sync.sync().unwrap();
    assert!(result.conflicts.is_empty());
    assert_eq!(result.files_pushed, 1);

    // Verify in fresh clone
    let verify = clone_second(&tmp, &remote);
    assert!(verify.join("synced.md").exists());
}

#[test]
fn sync_aborts_on_conflict() {
    let (tmp, local, remote) = setup_local_remote();
    let second = clone_second(&tmp, &remote);

    // Create conflicting changes
    commit_file(&local, "init.md", "# Local\n", "local");
    commit_file(&second, "init.md", "# Remote\n", "remote");

    let repo2 = Repository::open(&second).unwrap();
    let mut remote2 = repo2.find_remote("origin").unwrap();
    remote2.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

    let sync = git_sync(&local);
    let result = sync.sync().unwrap();
    assert!(!result.conflicts.is_empty(), "sync should report conflicts");
    assert_eq!(result.files_pushed, 0, "should not push when conflicts exist");
}

// ── Clone ───────────────────────────────────────────────────────────────────

#[test]
fn clone_repo_into_new_dir() {
    let (_tmp, _local, remote) = setup_local_remote();

    let clone_tmp = tempfile::Builder::new()
        .prefix("codex-clone-test-")
        .tempdir()
        .unwrap();
    let dest = clone_tmp.path().join("cloned");

    let repo = GitSync::clone_repo(remote.to_str().unwrap(), "main", &dest).unwrap();
    assert!(dest.join("init.md").exists(), "cloned file should exist");
    assert!(repo.head().is_ok());
}

#[test]
fn clone_repo_bad_url_fails() {
    let tmp = tempfile::Builder::new()
        .prefix("codex-clone-test-")
        .tempdir()
        .unwrap();
    let dest = tmp.path().join("bad-clone");

    let result = GitSync::clone_repo("/nonexistent/path.git", "main", &dest);
    assert!(result.is_err(), "clone from nonexistent path should fail");
}

#[test]
fn clone_repo_cleans_up_on_failure() {
    let tmp = tempfile::Builder::new()
        .prefix("codex-clone-test-")
        .tempdir()
        .unwrap();
    let dest = tmp.path().join("cleanup-clone");

    assert!(!dest.exists());
    let _ = GitSync::clone_repo("/nonexistent/path.git", "main", &dest);
    assert!(!dest.exists(), "failed clone should clean up dest directory");
}

#[test]
fn clone_repo_bad_branch_fails() {
    let (_tmp, _local, remote) = setup_local_remote();

    let clone_tmp = tempfile::Builder::new()
        .prefix("codex-clone-test-")
        .tempdir()
        .unwrap();
    let dest = clone_tmp.path().join("bad-branch");

    let result = GitSync::clone_repo(remote.to_str().unwrap(), "nonexistent-branch", &dest);
    assert!(result.is_err(), "clone with bad branch should fail");
}

// ── No remote configured ────────────────────────────────────────────────────

#[test]
fn open_repo_fails_on_non_git_dir() {
    let tmp = tempfile::Builder::new()
        .prefix("codex-git-test-")
        .tempdir()
        .unwrap();
    let path = tmp.path().join("not-a-repo");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("note.md"), "# Note\n").unwrap();

    let sync = GitSync::new(path, "origin", "main");
    let result = sync.status();
    assert!(result.is_err(), "status should fail on non-git directory");
}

#[test]
fn pull_fails_when_remote_missing() {
    let tmp = tempfile::Builder::new()
        .prefix("codex-git-test-")
        .tempdir()
        .unwrap();
    let path = tmp.path().join("no-remote");
    Repository::init(&path).unwrap();

    // Commit something so HEAD exists
    fs::write(path.join("file.md"), "# File\n").unwrap();
    let repo = Repository::open(&path).unwrap();
    let mut index = repo.index().unwrap();
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    repo.commit(Some("HEAD"), &sig(), &sig(), "init", &tree, &[]).unwrap();

    let sync = GitSync::new(path, "origin", "main");
    let result = sync.pull();
    assert!(result.is_err(), "pull should fail when remote 'origin' doesn't exist");
}

#[test]
fn push_fails_when_remote_missing() {
    let tmp = tempfile::Builder::new()
        .prefix("codex-git-test-")
        .tempdir()
        .unwrap();
    let path = tmp.path().join("no-remote-push");
    Repository::init(&path).unwrap();

    fs::write(path.join("file.md"), "# File\n").unwrap();
    let repo = Repository::open(&path).unwrap();
    let mut index = repo.index().unwrap();
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    repo.commit(Some("HEAD"), &sig(), &sig(), "init", &tree, &[]).unwrap();

    let sync = GitSync::new(path, "origin", "main");
    let result = sync.push();
    assert!(result.is_err(), "push should fail when remote 'origin' doesn't exist");
}

// ── Status edge: empty repo (no commits) ────────────────────────────────────

#[test]
fn status_empty_repo_is_idle() {
    let tmp = tempfile::Builder::new()
        .prefix("codex-git-test-")
        .tempdir()
        .unwrap();
    let path = tmp.path().join("empty-status");
    Repository::init(&path).unwrap();

    let sync = GitSync::new(path, "origin", "main");
    let status = sync.status().unwrap();
    assert_eq!(status, SyncStatus::Idle, "empty repo with no HEAD should be idle");
}
