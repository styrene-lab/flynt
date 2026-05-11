//! Push direction — flynt task → upstream issue.
//!
//! Stateless function `push_task` does one end-to-end push. The
//! debouncer (`PushDebouncer`) is a per-task last-edit tracker that
//! callers use to coalesce rapid edits before invoking `push_task`.
//!
//! Architecture: this module knows nothing about `flynt-store::Project`
//! or `tokio` runtimes. The flynt-store hook (separate crate) owns
//! the spawned task that drains the debouncer and calls `push_task`.
//! That keeps the push logic testable without mocking the entire
//! desktop app.
//!
//! ## Status lifecycle
//!
//! ```text
//!   LocalOnly ──first edit + engagement──> PendingPush ──debounce expires──┐
//!                                                                          │
//!     ┌──────────────── conflict detected ─── Conflict <───── push_task ──┤
//!     │                                                                    │
//!     │                                                                    ↓
//!     │                                              ┌─── PushFailed ── push_task
//!     │                                              │                     │
//!     ↓                                              │                     ↓
//!   resolve via "pull theirs" or "force push" ──> Synced <── push success ─┘
//! ```
//!
//! ## Conflict semantics
//!
//! Before any push, we GET the current issue and compare its content
//! hash to `IssueMap.last_hash`. If they differ, the upstream changed
//! since our last sync — we DO NOT push and emit a `Conflict` status.
//! Resolution is left to the operator via the SyncStatusPill's
//! blanket buttons (pull theirs / force push); deeper diff is Zed's
//! job, per the locked design.

use anyhow::{Context, Result};
use chrono::Utc;
use flynt_models::task::Task;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use styrene_forge::{ForgeClient, ForgeIssue};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::mapping::{MappingConfig, TaskFieldMapper};
use crate::store::SyncStore;
use crate::sync::{content_hash, IssueMap};

/// Current sync state of one task. Surfaced via the metadata strip's
/// SyncStatusPill. Two-direction: this enum is what the UI consumes;
/// the storage layer holds enough state to reconstruct it (IssueMap
/// row + a `pending_since` timestamp tracked separately).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    /// No engagement linked, or engagement has `auto_create_issues = false`
    /// and no IssueMap exists yet. Pure local state.
    LocalOnly,
    /// IssueMap exists and `last_synced` is current. Nothing to do.
    Synced { issue_number: u64, url: Option<String> },
    /// Edit happened locally; debounce timer running.
    PendingPush { issue_number: Option<u64> },
    /// Push in flight.
    Pushing,
    /// Push attempted but failed (network, auth, validation).
    /// Contains the error message for the operator pill click-through.
    PushFailed { issue_number: Option<u64>, error: String },
    /// Upstream changed since our last sync. Push blocked until the
    /// operator resolves via pull-theirs or force-push.
    Conflict { issue_number: u64, url: Option<String> },
}

/// One push attempt. Caller passes the inputs; we never read globals.
///
/// Returns the new SyncStatus to surface in the UI. The function also
/// updates `sync_store` (writes a fresh `IssueMap` with the new hash)
/// when the push succeeds. On Conflict it does NOT update the store —
/// the operator's resolution writes the new state.
///
/// `task_to_engagement_map` extracts the IssueMap (if any) and the
/// org/repo binding from caller-side context. Two params rather than
/// one because the IssueMap may not exist yet (first push of a new
/// task) but we still need a target repo to create against.
pub struct PushInput<'a> {
    pub task: &'a Task,
    pub mapping: &'a MappingConfig,
    pub mapper: &'a dyn TaskFieldMapper,
    pub client: &'a dyn ForgeClient,
    pub sync_store: &'a SyncStore,
    /// `(org, repo)` to target when creating a new issue. Ignored if
    /// an `IssueMap` already exists for the task.
    pub target_binding: (String, String),
    /// Existing IssueMap row, if this task has been pushed before.
    pub existing_map: Option<IssueMap>,
    /// Whether to create a new upstream issue when no map exists.
    /// Comes from `Engagement.auto_create_issues`. Default false; the
    /// operator opts in per engagement.
    pub auto_create: bool,
}

pub async fn push_task(input: PushInput<'_>) -> SyncStatus {
    match push_task_inner(input).await {
        Ok(status) => status,
        Err(e) => {
            warn!(error = %e, "push_task failed");
            SyncStatus::PushFailed {
                issue_number: None,
                error: e.to_string(),
            }
        }
    }
}

async fn push_task_inner(input: PushInput<'_>) -> Result<SyncStatus> {
    if let Some(map) = input.existing_map.clone() {
        // Update path. Read current issue, conflict-check, push if clean.
        update_with_conflict_check(input, map).await
    } else if !input.auto_create {
        // No map and no auto-create — stay local.
        Ok(SyncStatus::LocalOnly)
    } else {
        // No map, auto-create on: mirror upstream.
        create_and_record(input).await
    }
}

async fn create_and_record(input: PushInput<'_>) -> Result<SyncStatus> {
    let (org, repo) = &input.target_binding;
    let payload = input.mapper.task_to_issue_create(input.task, input.mapping);
    let issue = input
        .client
        .create_issue(org, repo, &payload)
        .await
        .context("create_issue")?;

    let hash = content_hash(&issue.title, &issue.body, state_str(&issue), &issue.labels);
    let map = IssueMap {
        local_id: input.task.id.0,
        board_id: input.task.board_id.0,
        forge_org: org.clone(),
        forge_repo: repo.clone(),
        forge_issue_number: issue.number,
        last_synced: Utc::now(),
        last_hash: Some(hash),
        forge_url: Some(issue.url.clone()),
    };
    input.sync_store.upsert(&map).context("sync_store.upsert")?;

    Ok(SyncStatus::Synced {
        issue_number: issue.number,
        url: Some(issue.url),
    })
}

async fn update_with_conflict_check(
    input: PushInput<'_>,
    map: IssueMap,
) -> Result<SyncStatus> {
    // GET upstream. Treat 404 specially — the issue was deleted on
    // the forge side. Returning generic PushFailed would surface as
    // a transient error and retry next edit; the operator needs to
    // see "your link is dead, decide what to do" instead. We don't
    // auto-recreate (operator might have deleted intentionally),
    // don't auto-delete the IssueMap (operator might want the link
    // back). Just stop pushing and surface clearly.
    let current = match input
        .client
        .get_issue(&map.forge_org, &map.forge_repo, map.forge_issue_number)
        .await
    {
        Ok(c) => c,
        Err(styrene_forge::ForgeError::NotFound(_)) => {
            return Ok(SyncStatus::PushFailed {
                issue_number: Some(map.forge_issue_number),
                error: format!(
                    "Upstream issue #{} no longer exists. Clear the link manually or recreate.",
                    map.forge_issue_number
                ),
            });
        }
        Err(e) => return Err(anyhow::anyhow!(e).context("get_issue")),
    };

    let current_hash = content_hash(
        &current.title,
        &current.body,
        state_str(&current),
        &current.labels,
    );

    // Conflict detection: someone else changed the issue since our
    // last sync. We never trust an empty stored hash as "clean" — if
    // we don't know what state we last saw, we can't tell if upstream
    // diverged, so treat as conflict and force a resolution.
    let stored = match map.last_hash.as_deref() {
        Some(h) => h,
        None => {
            return Ok(SyncStatus::Conflict {
                issue_number: map.forge_issue_number,
                url: map.forge_url.clone(),
            })
        }
    };
    if stored != current_hash {
        return Ok(SyncStatus::Conflict {
            issue_number: map.forge_issue_number,
            url: map.forge_url.clone(),
        });
    }

    // Clean: build the update from current + task, push, record.
    let update = input.mapper.task_to_issue_update(input.task, &current, input.mapping);

    // If the update has no fields set, there's literally nothing to
    // push — every flynt field already matches upstream. Common after
    // a successful sync that gets re-triggered by a non-content edit
    // (column move, etc.).
    if update.title.is_none()
        && update.body.is_none()
        && update.state.is_none()
        && update.labels.is_none()
        && update.milestone.is_none()
        && update.assignees.is_none()
    {
        debug!(task = %input.task.id.0, "push: no diff, skipping");
        return Ok(SyncStatus::Synced {
            issue_number: map.forge_issue_number,
            url: map.forge_url.clone(),
        });
    }

    let new_issue = input
        .client
        .update_issue(&map.forge_org, &map.forge_repo, map.forge_issue_number, &update)
        .await
        .context("update_issue")?;

    let new_hash = content_hash(
        &new_issue.title,
        &new_issue.body,
        state_str(&new_issue),
        &new_issue.labels,
    );
    let mut updated = map.clone();
    updated.last_synced = Utc::now();
    updated.last_hash = Some(new_hash);
    updated.forge_url = Some(new_issue.url.clone());
    input.sync_store.upsert(&updated).context("sync_store.upsert")?;

    Ok(SyncStatus::Synced {
        issue_number: new_issue.number,
        url: Some(new_issue.url),
    })
}

fn state_str(issue: &ForgeIssue) -> &'static str {
    match issue.state {
        styrene_forge::IssueState::Open => "open",
        styrene_forge::IssueState::Closed => "closed",
    }
}

// ── Debouncer ───────────────────────────────────────────────────────────────

/// Per-task last-edit tracker. Callers (the flynt-store save hook)
/// call `note_edit` after each save; a separate drain loop calls
/// `take_ready` periodically to find tasks that have been quiet long
/// enough to push.
///
/// Stateless from the network's perspective — just timestamps in a
/// HashMap. Mutex-wrapped so the save path and the drain loop don't
/// race; both calls are O(1) amortized.
pub struct PushDebouncer {
    inner: Mutex<HashMap<Uuid, Instant>>,
    debounce: Duration,
}

impl PushDebouncer {
    /// Default 5-second window — longer than the editor's 2s autosave
    /// so a single edit doesn't push twice (once mid-typing, once
    /// after the editor settled).
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            debounce: Duration::from_secs(5),
        }
    }

    pub fn with_debounce(debounce: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            debounce,
        }
    }

    /// Record an edit for a task. Resets the timer.
    pub fn note_edit(&self, task_id: Uuid) {
        let mut guard = self.inner.lock().unwrap();
        guard.insert(task_id, Instant::now());
    }

    /// Drop a task from the queue (e.g., it was just pushed
    /// successfully, or the operator deleted it).
    pub fn forget(&self, task_id: Uuid) {
        let mut guard = self.inner.lock().unwrap();
        guard.remove(&task_id);
    }

    /// Return task ids whose last edit was at least `debounce` ago,
    /// removing them from the queue (caller is expected to push
    /// immediately).
    pub fn take_ready(&self) -> Vec<Uuid> {
        let mut guard = self.inner.lock().unwrap();
        let now = Instant::now();
        let ready: Vec<Uuid> = guard
            .iter()
            .filter(|(_, t)| now.duration_since(**t) >= self.debounce)
            .map(|(id, _)| *id)
            .collect();
        for id in &ready {
            guard.remove(id);
        }
        ready
    }

    /// Test-only: count of tasks currently in the queue.
    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

impl Default for PushDebouncer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::GitHubMapper;
    use async_trait::async_trait;
    use chrono::Utc;
    use flynt_models::task::{BoardId, Priority, Task, TaskId, TaskStatus};
    use std::sync::Arc;
    use styrene_forge::{
        CreateIssue, ForgeClient, ForgeIssue, ForgeKind, ForgeLabel, ForgeMilestone,
        ForgeRepo, ForgeResult, IssueState, ListOpts, UpdateIssue,
    };

    // ── Mock forge client ────────────────────────────────────────────

    struct MockClient {
        endpoint: styrene_forge::ForgeEndpoint,
        issues: Mutex<HashMap<u64, ForgeIssue>>,
        next_number: Mutex<u64>,
    }

    impl MockClient {
        fn new() -> Self {
            Self {
                endpoint: styrene_forge::ForgeEndpoint {
                    id: "mock".into(),
                    kind: ForgeKind::GitHub,
                    base_url: "https://mock".into(),
                    token_secret: None,
                },
                issues: Mutex::new(HashMap::new()),
                next_number: Mutex::new(1),
            }
        }
        fn put(&self, issue: ForgeIssue) {
            self.issues.lock().unwrap().insert(issue.number, issue);
        }
    }

    #[async_trait]
    impl ForgeClient for MockClient {
        fn kind(&self) -> ForgeKind { ForgeKind::GitHub }
        fn endpoint(&self) -> &styrene_forge::ForgeEndpoint { &self.endpoint }

        async fn list_issues(&self, _o: &str, _r: &str, _o2: &ListOpts) -> ForgeResult<Vec<ForgeIssue>> {
            Ok(self.issues.lock().unwrap().values().cloned().collect())
        }

        async fn get_issue(&self, _o: &str, _r: &str, n: u64) -> ForgeResult<ForgeIssue> {
            self.issues.lock().unwrap()
                .get(&n).cloned()
                .ok_or_else(|| styrene_forge::ForgeError::NotFound("issue".into()))
        }

        async fn create_issue(&self, _o: &str, _r: &str, c: &CreateIssue) -> ForgeResult<ForgeIssue> {
            let mut n = self.next_number.lock().unwrap();
            let number = *n;
            *n += 1;
            drop(n);
            let issue = ForgeIssue {
                number,
                title: c.title.clone(),
                body: c.body.clone(),
                state: IssueState::Open,
                labels: c.labels.clone(),
                milestone: c.milestone.clone(),
                assignees: c.assignees.clone(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                closed_at: None,
                url: format!("https://mock/{number}"),
            };
            self.issues.lock().unwrap().insert(number, issue.clone());
            Ok(issue)
        }

        async fn update_issue(&self, _o: &str, _r: &str, n: u64, u: &UpdateIssue) -> ForgeResult<ForgeIssue> {
            let mut issues = self.issues.lock().unwrap();
            let issue = issues.get_mut(&n)
                .ok_or_else(|| styrene_forge::ForgeError::NotFound("issue".into()))?;
            if let Some(t) = &u.title { issue.title = t.clone(); }
            if let Some(b) = &u.body { issue.body = b.clone(); }
            if let Some(s) = u.state { issue.state = s; }
            if let Some(l) = &u.labels { issue.labels = l.clone(); }
            if let Some(m) = &u.milestone { issue.milestone = Some(m.clone()); }
            if let Some(a) = &u.assignees { issue.assignees = a.clone(); }
            issue.updated_at = Utc::now();
            Ok(issue.clone())
        }

        async fn list_labels(&self, _: &str, _: &str) -> ForgeResult<Vec<ForgeLabel>> { Ok(vec![]) }
        async fn list_milestones(&self, _: &str, _: &str) -> ForgeResult<Vec<ForgeMilestone>> { Ok(vec![]) }
        async fn list_repos(&self, _: &str) -> ForgeResult<Vec<ForgeRepo>> { Ok(vec![]) }
        async fn create_repo(&self, _: &str, _: &styrene_forge::CreateRepo) -> ForgeResult<ForgeRepo> {
            Err(styrene_forge::ForgeError::NotFound("not implemented".into()))
        }
        async fn create_webhook(&self, _: &str, _: &str, _: &styrene_forge::CreateWebhook) -> ForgeResult<styrene_forge::ForgeWebhook> {
            Err(styrene_forge::ForgeError::NotFound("not implemented".into()))
        }
    }

    fn make_task() -> Task {
        Task {
            id: TaskId(Uuid::new_v4()),
            board_id: BoardId(Uuid::new_v4()),
            column: "Active".into(),
            title: "Fix the indexer".into(),
            description: "Body".into(),
            priority: Priority::High,
            status: TaskStatus::Todo,
            tags: vec!["infra".into()],
            document_refs: vec![],
            external_refs: vec![],
            due_date: None,
            position: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            decay: flynt_models::task::DecayRate::Natural,
            last_touched_at: None,
            design_node_id: None,
            execution: None,
            openspec_change: None,
            engagement_id: None,
        }
    }

    fn fresh_store() -> SyncStore {
        SyncStore::in_memory().unwrap()
    }

    // ── push_task ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn push_with_no_map_and_no_auto_create_returns_local_only() {
        let task = make_task();
        let client = Arc::new(MockClient::new());
        let store = fresh_store();
        let mapper = GitHubMapper;
        let cfg = MappingConfig::default();

        let status = push_task(PushInput {
            task: &task,
            mapping: &cfg,
            mapper: &mapper,
            client: &*client,
            sync_store: &store,
            target_binding: ("org".into(), "repo".into()),
            existing_map: None,
            auto_create: false,
        }).await;

        assert_eq!(status, SyncStatus::LocalOnly);
        assert!(client.issues.lock().unwrap().is_empty(),
                "didn't create when auto_create = false");
    }

    #[tokio::test]
    async fn push_with_no_map_and_auto_create_creates_issue() {
        let task = make_task();
        let client = Arc::new(MockClient::new());
        let store = fresh_store();
        let mapper = GitHubMapper;
        let cfg = MappingConfig::default();

        let status = push_task(PushInput {
            task: &task,
            mapping: &cfg,
            mapper: &mapper,
            client: &*client,
            sync_store: &store,
            target_binding: ("org".into(), "repo".into()),
            existing_map: None,
            auto_create: true,
        }).await;

        match status {
            SyncStatus::Synced { issue_number, .. } => assert_eq!(issue_number, 1),
            other => panic!("expected Synced, got {other:?}"),
        }
        // Issue created upstream.
        assert_eq!(client.issues.lock().unwrap().len(), 1);
        // IssueMap recorded.
        let maps = store.list_by_local(&task.id.0).unwrap();
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].forge_issue_number, 1);
        assert!(maps[0].last_hash.is_some());
    }

    #[tokio::test]
    async fn push_with_existing_map_updates_on_diff() {
        let mut task = make_task();
        task.title = "Updated locally".into();
        let client = Arc::new(MockClient::new());

        // Pre-seed the upstream issue at the matching hash.
        let issue = ForgeIssue {
            number: 42,
            title: "Original".into(),
            body: "Body".into(),
            state: IssueState::Open,
            labels: vec!["priority:high".into(), "infra".into()],
            milestone: None,
            assignees: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            url: "https://mock/42".into(),
        };
        client.put(issue.clone());

        let store = fresh_store();
        let map = IssueMap {
            local_id: task.id.0,
            board_id: task.board_id.0,
            forge_org: "org".into(),
            forge_repo: "repo".into(),
            forge_issue_number: 42,
            last_synced: Utc::now(),
            last_hash: Some(content_hash(&issue.title, &issue.body, "open", &issue.labels)),
            forge_url: Some(issue.url.clone()),
        };
        store.upsert(&map).unwrap();

        let mapper = GitHubMapper;
        let cfg = MappingConfig::default();
        let status = push_task(PushInput {
            task: &task,
            mapping: &cfg,
            mapper: &mapper,
            client: &*client,
            sync_store: &store,
            target_binding: ("org".into(), "repo".into()),
            existing_map: Some(map),
            auto_create: false,
        }).await;

        match status {
            SyncStatus::Synced { issue_number, .. } => assert_eq!(issue_number, 42),
            other => panic!("expected Synced, got {other:?}"),
        }
        let upstream = client.issues.lock().unwrap().get(&42).cloned().unwrap();
        assert_eq!(upstream.title, "Updated locally");
    }

    #[tokio::test]
    async fn push_detects_conflict_when_upstream_changed() {
        let task = make_task();
        let client = Arc::new(MockClient::new());

        // Map says we last saw "old" content; upstream now has "newer"
        // content (simulates someone editing on GitHub since our last
        // sync).
        let issue_now = ForgeIssue {
            number: 42,
            title: "Someone changed it".into(),
            body: "On GitHub directly".into(),
            state: IssueState::Open,
            labels: vec![],
            milestone: None,
            assignees: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            url: "https://mock/42".into(),
        };
        client.put(issue_now);

        let store = fresh_store();
        let map = IssueMap {
            local_id: task.id.0,
            board_id: task.board_id.0,
            forge_org: "org".into(),
            forge_repo: "repo".into(),
            forge_issue_number: 42,
            last_synced: Utc::now() - chrono::Duration::hours(1),
            last_hash: Some("old-hash-from-last-sync".into()),
            forge_url: Some("https://mock/42".into()),
        };
        store.upsert(&map).unwrap();

        let status = push_task(PushInput {
            task: &task,
            mapping: &MappingConfig::default(),
            mapper: &GitHubMapper,
            client: &*client,
            sync_store: &store,
            target_binding: ("org".into(), "repo".into()),
            existing_map: Some(map),
            auto_create: false,
        }).await;

        match status {
            SyncStatus::Conflict { issue_number, .. } => assert_eq!(issue_number, 42),
            other => panic!("expected Conflict, got {other:?}"),
        }
        // Did NOT push — upstream unchanged.
        let upstream = client.issues.lock().unwrap().get(&42).cloned().unwrap();
        assert_eq!(upstream.title, "Someone changed it");
    }

    #[tokio::test]
    async fn push_surfaces_404_as_dead_link_not_generic_failure() {
        // Issue deleted upstream — get_issue returns NotFound. The
        // operator needs to see "your link is dead, decide what to do"
        // rather than a generic "push failed (will retry)" pattern.
        let task = make_task();
        let client = Arc::new(MockClient::new()); // no issues in mock
        let store = fresh_store();
        let map = IssueMap {
            local_id: task.id.0,
            board_id: task.board_id.0,
            forge_org: "org".into(),
            forge_repo: "repo".into(),
            forge_issue_number: 42,
            last_synced: Utc::now(),
            last_hash: Some("anything".into()),
            forge_url: Some("https://mock/42".into()),
        };

        let status = push_task(PushInput {
            task: &task,
            mapping: &MappingConfig::default(),
            mapper: &GitHubMapper,
            client: &*client,
            sync_store: &store,
            target_binding: ("org".into(), "repo".into()),
            existing_map: Some(map),
            auto_create: false,
        }).await;

        match status {
            SyncStatus::PushFailed { issue_number, error } => {
                assert_eq!(issue_number, Some(42));
                assert!(error.contains("no longer exists"),
                        "informative error mentions deletion: {error}");
            }
            other => panic!("expected PushFailed with dead-link message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn push_treats_missing_last_hash_as_conflict() {
        // A map without a last_hash means we don't know what we last
        // saw. Better to flag conflict than to risk an unsafe overwrite.
        let task = make_task();
        let client = Arc::new(MockClient::new());
        let issue = ForgeIssue {
            number: 42,
            title: "Anything".into(),
            body: String::new(),
            state: IssueState::Open,
            labels: vec![],
            milestone: None,
            assignees: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            url: String::new(),
        };
        client.put(issue);

        let store = fresh_store();
        let map = IssueMap {
            local_id: task.id.0,
            board_id: task.board_id.0,
            forge_org: "org".into(),
            forge_repo: "repo".into(),
            forge_issue_number: 42,
            last_synced: Utc::now(),
            last_hash: None,
            forge_url: None,
        };

        let status = push_task(PushInput {
            task: &task,
            mapping: &MappingConfig::default(),
            mapper: &GitHubMapper,
            client: &*client,
            sync_store: &store,
            target_binding: ("org".into(), "repo".into()),
            existing_map: Some(map),
            auto_create: false,
        }).await;

        assert!(matches!(status, SyncStatus::Conflict { .. }));
    }

    #[tokio::test]
    async fn push_no_diff_does_not_call_update() {
        // Task and upstream already in sync — push is a no-op, status
        // stays Synced, no API call to update_issue.
        let task = make_task();
        let client = Arc::new(MockClient::new());

        // Set upstream to match the task exactly.
        let mapper = GitHubMapper;
        let cfg = MappingConfig::default();
        let create = mapper.task_to_issue_create(&task, &cfg);
        let issue = ForgeIssue {
            number: 1,
            title: create.title,
            body: create.body,
            state: IssueState::Open,
            labels: create.labels,
            milestone: create.milestone,
            assignees: create.assignees,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            url: "https://mock/1".into(),
        };
        let before_updated = issue.updated_at;
        client.put(issue.clone());

        let store = fresh_store();
        let map = IssueMap {
            local_id: task.id.0,
            board_id: task.board_id.0,
            forge_org: "org".into(),
            forge_repo: "repo".into(),
            forge_issue_number: 1,
            last_synced: Utc::now(),
            last_hash: Some(content_hash(&issue.title, &issue.body, "open", &issue.labels)),
            forge_url: Some(issue.url.clone()),
        };

        let status = push_task(PushInput {
            task: &task,
            mapping: &cfg,
            mapper: &mapper,
            client: &*client,
            sync_store: &store,
            target_binding: ("org".into(), "repo".into()),
            existing_map: Some(map),
            auto_create: false,
        }).await;

        assert!(matches!(status, SyncStatus::Synced { .. }));
        let after = client.issues.lock().unwrap().get(&1).cloned().unwrap();
        assert_eq!(after.updated_at, before_updated,
                   "no update_issue call — upstream timestamp unchanged");
    }

    // ── PushDebouncer ────────────────────────────────────────────────────

    #[test]
    fn debouncer_take_ready_empty_initially() {
        let d = PushDebouncer::new();
        assert!(d.take_ready().is_empty());
    }

    #[test]
    fn debouncer_note_edit_then_short_wait_not_ready() {
        let d = PushDebouncer::with_debounce(Duration::from_millis(200));
        let id = Uuid::new_v4();
        d.note_edit(id);
        assert!(d.take_ready().is_empty(), "not yet expired");
        assert_eq!(d.pending_count(), 1);
    }

    #[test]
    fn debouncer_note_edit_then_long_wait_returns_id() {
        let d = PushDebouncer::with_debounce(Duration::from_millis(20));
        let id = Uuid::new_v4();
        d.note_edit(id);
        std::thread::sleep(Duration::from_millis(40));
        let ready = d.take_ready();
        assert_eq!(ready, vec![id]);
        assert_eq!(d.pending_count(), 0, "take_ready removes the entry");
    }

    #[test]
    fn debouncer_repeated_edit_resets_timer() {
        let d = PushDebouncer::with_debounce(Duration::from_millis(40));
        let id = Uuid::new_v4();
        d.note_edit(id);
        std::thread::sleep(Duration::from_millis(25));
        d.note_edit(id); // reset
        std::thread::sleep(Duration::from_millis(25));
        // Only 25ms since the second edit — not ready.
        assert!(d.take_ready().is_empty());
        std::thread::sleep(Duration::from_millis(25));
        // Now 50ms since the second edit — ready.
        assert_eq!(d.take_ready(), vec![id]);
    }

    #[test]
    fn debouncer_forget_removes_id() {
        let d = PushDebouncer::with_debounce(Duration::from_millis(20));
        let id = Uuid::new_v4();
        d.note_edit(id);
        d.forget(id);
        std::thread::sleep(Duration::from_millis(40));
        assert!(d.take_ready().is_empty());
    }
}
