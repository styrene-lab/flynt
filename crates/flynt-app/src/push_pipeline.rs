//! Push pipeline — the runtime that wires flynt-forge's push machinery
//! into the desktop app.
//!
//! Owns:
//! - `PushDebouncer` — fed by `Project`'s SaveHook fan-out
//! - A drain task that polls `take_ready` and runs `push_task` per task
//! - A shared status map keyed by task id, broadcast to subscribers
//!   when entries change so the SyncStatusPill re-renders
//!
//! Implements `flynt_store::save_hook::SaveHook` so a single `Arc<Self>`
//! is both the on-task-saved sink and the source of truth for sync
//! status — installed via `Project::install_save_hook`.
//!
//! ## Token handling
//!
//! v1 reads tokens from the environment per-resolve. Resolver name is
//! whatever the engagement's `forge.token_secret` says (e.g.,
//! `GITHUB_TOKEN`). Anonymous resolves to no-token requests, which
//! GitHub handles for public repos and rate-limits aggressively
//! otherwise. The token-trust note in CONTRIBUTING.md covers why this
//! is acceptable: the desktop is operator-trusted, same as the agent.

use anyhow::{Context, Result};
use flynt_core::store::ProjectStore;
use flynt_forge::{
    mapping::{self, mapper_for_kind, MappingConfig},
    push::{projected_local_hash, push_task, PushDebouncer, PushInput, SyncStatus},
    store::SyncStore,
    ForgejoForgeClient, GitHubForgeClient, GitlabForgeClient,
};
use flynt_models::engagement::Engagement;
use flynt_models::task::TaskId;
use flynt_store::project::Project;
use flynt_store::save_hook::SaveHook;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use styrene_forge::{ForgeClient, ForgeKind};
use tokio::sync::broadcast;
use uuid::Uuid;

/// Broadcast on every status transition. Subscribers (the SyncStatusPill
/// per task) trigger a re-render that re-reads the status map. We
/// don't multiplex per-task; the pill filters by its own id on the
/// receive side.
#[derive(Debug, Clone)]
pub struct SyncStatusUpdate {
    pub task_id: Uuid,
    pub status: SyncStatus,
}

/// The runtime that drives auto-push for one project.
///
/// One instance per AppContext; constructed at bootstrap. Lives for
/// the lifetime of the project.
pub struct PushPipeline {
    debouncer: PushDebouncer,
    /// Per-task sync state. UI reads via `status_for`; pipeline writes
    /// after each push.
    statuses: Arc<RwLock<HashMap<Uuid, SyncStatus>>>,
    /// Broadcasts on every status change so the pill can re-render.
    events: broadcast::Sender<SyncStatusUpdate>,
    project: Arc<Project>,
    /// Where `IssueMap` rows live (per-project sqlite). Initialized
    /// once at construction.
    sync_store: Arc<SyncStore>,
    /// Mapping config — loaded once, kept stable for the session.
    mapping: MappingConfig,
    /// Set true by `shutdown()`. The drain loop checks this on each
    /// tick and exits cleanly. Without this, switching projects via
    /// AppContext::set_runtime would leak the old drain task forever
    /// because it holds an Arc<Self> internally.
    shutdown_flag: Arc<AtomicBool>,
    /// Cached projected hash from the last successful push per task.
    /// Used to short-circuit `try_push` when the task hasn't changed
    /// since we last sync'd — saves the GET-issue API call that would
    /// otherwise fire for flynt-only edits (column moves, position
    /// changes). In-memory only; on app restart we'll burn one
    /// redundant GET per task on first edit and re-populate from there.
    last_push_hashes: Arc<RwLock<HashMap<Uuid, String>>>,
}

impl PushPipeline {
    pub fn new(project: Arc<Project>) -> Result<Arc<Self>> {
        // SyncStore — colocated with the project, separate sqlite file
        // from the document index. Matches what flynt-agent's
        // `sync_store_for` does for consistency.
        let sync_db_path = project.root.join(".flynt").join("forge-sync.db");
        if let Some(parent) = sync_db_path.parent() {
            std::fs::create_dir_all(parent).context("create .flynt dir")?;
        }
        let sync_store = Arc::new(
            SyncStore::open(&sync_db_path).context("open forge-sync.db")?,
        );

        let mapping = mapping::load(&project.root).unwrap_or_else(|err| {
            tracing::warn!(error = %err, "forge-mapping.toml unreadable; using defaults");
            MappingConfig::default()
        });

        let (events, _rx) = broadcast::channel(256);

        Ok(Arc::new(Self {
            debouncer: PushDebouncer::new(),
            statuses: Arc::new(RwLock::new(HashMap::new())),
            events,
            project,
            sync_store,
            mapping,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            last_push_hashes: Arc::new(RwLock::new(HashMap::new())),
        }))
    }

    /// Signal the drain loop to exit on its next tick. Called by
    /// `AppContext::set_runtime` before installing a fresh runtime
    /// so the old pipeline doesn't leak.
    ///
    /// Idempotent. Safe to call multiple times. Doesn't block — the
    /// drain loop exits up to ~1s later (on the next tick boundary).
    pub fn shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
    }

    /// Read the current sync status for a task. Returns `LocalOnly`
    /// for tasks the pipeline hasn't seen yet — that's the correct
    /// default since absence of a status mirrors absence of an
    /// IssueMap.
    pub fn status_for(&self, task_id: Uuid) -> SyncStatus {
        self.statuses
            .read()
            .map(|m| m.get(&task_id).cloned().unwrap_or(SyncStatus::LocalOnly))
            .unwrap_or(SyncStatus::LocalOnly)
    }

    /// Subscribe to status changes. Caller (a Dioxus use_effect)
    /// loops on `recv` and bumps a local refresh signal when an
    /// update arrives.
    pub fn subscribe(&self) -> broadcast::Receiver<SyncStatusUpdate> {
        self.events.subscribe()
    }

    /// Spawn the drain loop. Runs forever (scope-tied to the spawning
    /// runtime). Polls the debouncer every second; processes each
    /// ready task through `push_task`; updates status map + broadcasts.
    ///
    /// Returns the spawned task's handle in case the caller wants to
    /// `.abort()` on shutdown — flynt-app doesn't today (tokio runtime
    /// outlives the project, so leaking the task on app close is fine).
    pub fn spawn_drain_loop(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // 1s tick. Decoupled from the debouncer's 5s window — the
            // tick is "how often we check"; the window is "how long
            // we wait after an edit before pushing."
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tick.tick().await;

                if self.shutdown_flag.load(Ordering::SeqCst) {
                    tracing::debug!("PushPipeline drain loop exiting (shutdown requested)");
                    break;
                }

                let ready = self.debouncer.take_ready();
                if ready.is_empty() {
                    continue;
                }

                for task_id in ready {
                    // Re-check shutdown between tasks — a burst of
                    // ready tasks after shutdown was requested should
                    // exit promptly, not push everything queued first.
                    if self.shutdown_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    self.process_one(task_id).await;
                }
            }
        })
    }

    async fn process_one(&self, task_id: Uuid) {
        // First, mark as Pushing so the pill flashes.
        //
        // If `try_push` panics, the status stays "Pushing" forever in
        // the shared map — tokio catches the panic at the task
        // boundary but doesn't unwind our state. Self-healing: any
        // subsequent edit fires `on_task_saved` which overwrites the
        // stale Pushing with PendingPush. So a panic-stuck task heals
        // on the operator's next edit; no zombie state across sessions
        // (the map is in-memory only).
        self.set_status(task_id, SyncStatus::Pushing);

        let result = self.try_push(task_id).await;
        let status = match result {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, task = %task_id, "push pipeline error");
                SyncStatus::PushFailed {
                    issue_number: self
                        .existing_map_for(task_id)
                        .map(|m| m.forge_issue_number),
                    error: e.to_string(),
                }
            }
        };
        self.set_status(task_id, status);
    }

    async fn try_push(&self, task_id: Uuid) -> Result<SyncStatus> {
        // Load the task + engagement + (optionally) existing IssueMap.
        let task = self
            .project
            .store
            .get_task(&TaskId(task_id))?
            .ok_or_else(|| anyhow::anyhow!("task {task_id} not found"))?;
        let Some(engagement_id) = task.engagement_id.clone() else {
            return Ok(SyncStatus::LocalOnly);
        };
        let engagement = self
            .project
            .store
            .get_engagement(&engagement_id)?
            .ok_or_else(|| {
                anyhow::anyhow!("task {task_id} references engagement {engagement_id:?} which doesn't exist")
            })?;

        let existing_map = self.existing_map_for(task_id);

        // Decide which repo binding to target. For an existing map we
        // use whatever it's pointing at (the operator could have moved
        // tasks across repos, but the IssueMap is the source of truth
        // for "where this lives upstream"). For a fresh task with
        // multiple repo bindings: first one. v2 could let the operator
        // pick.
        let (org, repo) = match &existing_map {
            Some(m) => (m.forge_org.clone(), m.forge_repo.clone()),
            None => {
                let Some(b) = engagement.repos.first() else {
                    return Ok(SyncStatus::LocalOnly);
                };
                (b.forge_org.clone(), b.forge_repo.clone())
            }
        };

        // Mapper from the factory — same field translation regardless
        // of forge kind in v1; per-provider mappers diverge as quirks
        // surface (GitLab scoped labels, Forgejo extensions, etc.).
        let mapper = mapper_for_kind(engagement.forge.kind);

        // Short-circuit: if there's an existing map AND we already
        // pushed this exact task content earlier in this process,
        // there's nothing to push. Skips the GET that update_with_
        // conflict_check would otherwise issue — saves an API call
        // per flynt-only edit (column move, etc.). Process-local
        // cache; on restart we'll do one redundant GET and repopulate.
        let projected = projected_local_hash(&task, &self.mapping, &*mapper);
        if let Some(ref map) = existing_map {
            let cached = self
                .last_push_hashes
                .read()
                .ok()
                .and_then(|m| m.get(&task_id).cloned());
            if cached.as_deref() == Some(projected.as_str()) {
                tracing::debug!(task = %task_id, "push: local unchanged since last sync, skipping GET");
                return Ok(SyncStatus::Synced {
                    issue_number: map.forge_issue_number,
                    url: map.forge_url.clone(),
                });
            }
        }

        // Client construction is still GitHub-only at the network
        // layer (GitlabForgeClient / ForgejoForgeClient haven't been
        // ported into flynt-forge yet). Surface a clean error until
        // they land — better than a crash, gives the operator a
        // signal to fix.
        let client = match build_client(&engagement) {
            Ok(c) => c,
            Err(e) => {
                return Ok(SyncStatus::PushFailed {
                    issue_number: existing_map.as_ref().map(|m| m.forge_issue_number),
                    error: e.to_string(),
                });
            }
        };

        let input = PushInput {
            task: &task,
            mapping: &self.mapping,
            mapper: &*mapper,
            client: &*client,
            sync_store: &self.sync_store,
            target_binding: (org, repo),
            existing_map,
            auto_create: engagement.auto_create_issues,
        };
        let status = push_task(input).await;

        // Cache the projected hash on success so the next try_push for
        // this task can short-circuit if nothing changed. We only
        // cache on Synced — PushFailed/Conflict leave the cache as-is
        // so the next attempt does the full path.
        if matches!(status, SyncStatus::Synced { .. }) {
            if let Ok(mut m) = self.last_push_hashes.write() {
                m.insert(task_id, projected);
            }
        }
        Ok(status)
    }

    /// Conflict resolution: fetch upstream and overwrite the local
    /// task with whatever the forge says. Updates `IssueMap.last_hash`
    /// to the freshly-fetched upstream hash and clears the
    /// pipeline-cached projected hash. After this the next save-hook
    /// fire will project a fresh hash and only push if local changes
    /// going forward.
    ///
    /// The local write uses [`Project::update_any_task`] which doesn't
    /// fire the save hook — without that, the pulled state would
    /// immediately turn around and try to re-push.
    pub async fn resolve_pull_theirs(&self, task_id: Uuid) -> Result<()> {
        let task = self
            .project
            .store
            .get_task(&TaskId(task_id))?
            .ok_or_else(|| anyhow::anyhow!("task {task_id} not found"))?;
        let Some(engagement_id) = task.engagement_id.clone() else {
            anyhow::bail!("task has no engagement; nothing to pull from");
        };
        let engagement = self
            .project
            .store
            .get_engagement(&engagement_id)?
            .ok_or_else(|| anyhow::anyhow!("engagement {engagement_id:?} not found"))?;
        let map = self
            .existing_map_for(task_id)
            .ok_or_else(|| anyhow::anyhow!("no IssueMap for task — nothing to pull"))?;
        let mapper = mapper_for_kind(engagement.forge.kind);
        let client = build_client(&engagement).context("build forge client for pull")?;

        let issue = client
            .get_issue(&map.forge_org, &map.forge_repo, map.forge_issue_number)
            .await
            .context("get_issue")?;
        let patch = mapper.issue_to_task_patch(&issue, &self.mapping);

        // Apply the patch silently — update_any_task doesn't fire the
        // save hook, so the pulled state won't immediately re-queue
        // through the push pipeline.
        self.project
            .update_any_task(&TaskId(task_id), &patch)
            .context("apply pull patch")?;

        // Record the new last_hash so subsequent pushes can detect
        // future divergence relative to "where we are now."
        let new_hash = flynt_forge::issue_hash(&issue);
        let mut updated = map.clone();
        updated.last_synced = chrono::Utc::now();
        updated.last_hash = Some(new_hash);
        updated.forge_url = Some(issue.url.clone());
        self.sync_store.upsert(&updated).context("sync_store.upsert")?;

        // Clear the cached projected hash so the next save (or the
        // next try_push) compares against fresh data.
        if let Ok(mut h) = self.last_push_hashes.write() {
            h.remove(&task_id);
        }

        // Surface the new Synced state immediately so the pill flips.
        self.set_status(
            task_id,
            SyncStatus::Synced {
                issue_number: map.forge_issue_number,
                url: Some(issue.url),
            },
        );
        Ok(())
    }

    /// Conflict resolution: align `IssueMap.last_hash` to whatever
    /// upstream looks like NOW, then re-queue for a normal push. The
    /// next push will see no conflict (hash matches stored), diff the
    /// task against current upstream, and overwrite — effectively a
    /// "we won, take our version" force.
    ///
    /// Doesn't touch the local task; preserves whatever's there as
    /// the authoritative version.
    pub async fn resolve_force_push(&self, task_id: Uuid) -> Result<()> {
        let task = self
            .project
            .store
            .get_task(&TaskId(task_id))?
            .ok_or_else(|| anyhow::anyhow!("task {task_id} not found"))?;
        let Some(engagement_id) = task.engagement_id.clone() else {
            anyhow::bail!("task has no engagement; nothing to push to");
        };
        let engagement = self
            .project
            .store
            .get_engagement(&engagement_id)?
            .ok_or_else(|| anyhow::anyhow!("engagement {engagement_id:?} not found"))?;
        let map = self
            .existing_map_for(task_id)
            .ok_or_else(|| anyhow::anyhow!("no IssueMap for task — nothing to push over"))?;
        let client = build_client(&engagement).context("build forge client for force push")?;

        // Realign last_hash to current upstream. After this the regular
        // push path's conflict check passes (hash matches GET result),
        // and the diff against current is whatever local says.
        let current = client
            .get_issue(&map.forge_org, &map.forge_repo, map.forge_issue_number)
            .await
            .context("get_issue")?;
        let realigned = flynt_forge::issue_hash(&current);
        let mut updated = map.clone();
        updated.last_synced = chrono::Utc::now();
        updated.last_hash = Some(realigned);
        updated.forge_url = Some(current.url.clone());
        self.sync_store.upsert(&updated).context("sync_store.upsert")?;

        // Clear projection cache so the upcoming push doesn't
        // short-circuit thinking local hasn't changed.
        if let Ok(mut h) = self.last_push_hashes.write() {
            h.remove(&task_id);
        }

        // Surface PendingPush + queue. The drain loop will pick it up
        // on the next tick — same shape as a regular save.
        self.debouncer.note_edit(task_id);
        self.set_status(
            task_id,
            SyncStatus::PendingPush {
                issue_number: Some(map.forge_issue_number),
            },
        );
        Ok(())
    }

    fn existing_map_for(&self, task_id: Uuid) -> Option<flynt_forge::IssueMap> {
        self.sync_store
            .list_by_local(&task_id)
            .ok()
            .and_then(|v| v.into_iter().next())
    }

    fn set_status(&self, task_id: Uuid, status: SyncStatus) {
        if let Ok(mut map) = self.statuses.write() {
            map.insert(task_id, status.clone());
        }
        // Broadcast on every transition. Failure to send (no receivers
        // currently subscribed) is benign — pill components subscribe
        // lazily as they mount.
        let _ = self.events.send(SyncStatusUpdate { task_id, status });
    }
}

// SaveHook impl — the bridge from flynt-store save paths to our debouncer.
impl SaveHook for PushPipeline {
    fn on_task_saved(&self, task_id: Uuid) {
        self.debouncer.note_edit(task_id);
        // Surface a PendingPush state immediately so the UI can show
        // "Push pending…" without waiting for the drain loop's first
        // tick. The pill subscriber re-renders on this broadcast.
        let issue_number = self.existing_map_for(task_id).map(|m| m.forge_issue_number);
        self.set_status(task_id, SyncStatus::PendingPush { issue_number });
    }

    fn on_task_deleted(&self, task_id: Uuid) {
        // Drop all per-task state so the deleted task doesn't keep
        // ticking through the drain loop. The IssueMap row is kept
        // — the operator may have deleted the local task while
        // intending to leave the upstream issue alone; cleaning up
        // the link would require a deliberate "stop syncing" action
        // (v2 surface). For now, we just stop pushing.
        self.debouncer.forget(task_id);
        if let Ok(mut m) = self.statuses.write() {
            m.remove(&task_id);
        }
        if let Ok(mut h) = self.last_push_hashes.write() {
            h.remove(&task_id);
        }
    }
}

/// Build a ForgeClient for the engagement's forge endpoint.
///
/// Dispatches on `forge.kind`. Token resolution is uniform across
/// providers: read the env var named by `forge.token_secret` (defaults
/// to the provider's canonical name — `GITHUB_TOKEN`, `FORGEJO_TOKEN`,
/// `GITLAB_TOKEN`). The token-trust model in CONTRIBUTING.md says the
/// desktop client is trusted with the same tokens as the agent.
fn build_client(eng: &Engagement) -> Result<Box<dyn ForgeClient>> {
    let secret_name = eng
        .forge
        .token_secret
        .clone()
        .unwrap_or_else(|| default_secret_name(eng.forge.kind).to_string());
    let resolver: Arc<dyn flynt_forge::TokenResolver> =
        Arc::new(move || std::env::var(&secret_name).ok());

    match eng.forge.kind {
        ForgeKind::GitHub => Ok(Box::new(GitHubForgeClient::new(eng.forge.clone(), resolver))),
        ForgeKind::Forgejo => Ok(Box::new(ForgejoForgeClient::new(eng.forge.clone(), resolver))),
        ForgeKind::GitLab => Ok(Box::new(GitlabForgeClient::new(eng.forge.clone(), resolver))),
    }
}

fn default_secret_name(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::GitHub => "GITHUB_TOKEN",
        ForgeKind::Forgejo => "FORGEJO_TOKEN",
        ForgeKind::GitLab => "GITLAB_TOKEN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn status_for_unknown_task_is_local_only() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = Arc::new(Project::open(tmp.path()).unwrap());
        let pipeline = PushPipeline::new(project).unwrap();
        let unknown = Uuid::new_v4();
        assert!(matches!(pipeline.status_for(unknown), SyncStatus::LocalOnly));
    }

    #[tokio::test]
    async fn save_hook_marks_task_pending() {
        // on_task_saved transitions to PendingPush immediately so the
        // pill flashes without waiting for the drain loop.
        let tmp = tempfile::TempDir::new().unwrap();
        let project = Arc::new(Project::open(tmp.path()).unwrap());
        let pipeline = PushPipeline::new(project).unwrap();
        let mut rx = pipeline.subscribe();

        let task_id = Uuid::new_v4();
        pipeline.on_task_saved(task_id);

        match pipeline.status_for(task_id) {
            SyncStatus::PendingPush { .. } => {}
            other => panic!("expected PendingPush, got {other:?}"),
        }
        // Broadcast fired.
        let update = rx.try_recv().expect("event delivered");
        assert_eq!(update.task_id, task_id);
    }

    #[tokio::test]
    async fn save_hook_feeds_debouncer() {
        // The other side of on_task_saved — id ends up in the
        // debouncer with the current timestamp. take_ready won't
        // pick it up until the debounce window elapses.
        let tmp = tempfile::TempDir::new().unwrap();
        let project = Arc::new(Project::open(tmp.path()).unwrap());
        let pipeline = PushPipeline::new(project).unwrap();
        let task_id = Uuid::new_v4();
        pipeline.on_task_saved(task_id);
        // Not ready (default 5s window).
        assert!(pipeline.debouncer.take_ready().is_empty());
    }

    #[tokio::test]
    async fn on_task_deleted_clears_per_task_state() {
        // Verifies the cleanup contract: after on_task_deleted fires,
        // the debouncer, status map, and hash cache no longer hold an
        // entry for that task. Without this, a deleted task would keep
        // ticking through the drain loop forever.
        let tmp = tempfile::TempDir::new().unwrap();
        let project = Arc::new(Project::open(tmp.path()).unwrap());
        let pipeline = PushPipeline::new(project).unwrap();

        let task_id = Uuid::new_v4();
        // Seed state: simulate an edit + cached hash.
        pipeline.on_task_saved(task_id);
        if let Ok(mut h) = pipeline.last_push_hashes.write() {
            h.insert(task_id, "fake-hash".into());
        }
        assert_eq!(pipeline.debouncer.pending_count(), 1);
        assert!(pipeline.last_push_hashes.read().unwrap().contains_key(&task_id));
        assert!(!matches!(pipeline.status_for(task_id), SyncStatus::LocalOnly));

        pipeline.on_task_deleted(task_id);

        assert_eq!(pipeline.debouncer.pending_count(), 0, "debouncer cleared");
        assert!(!pipeline.last_push_hashes.read().unwrap().contains_key(&task_id),
                "hash cache cleared");
        // Status reverts to LocalOnly because the map entry is gone.
        assert!(matches!(pipeline.status_for(task_id), SyncStatus::LocalOnly));
    }

    #[tokio::test]
    async fn resolve_pull_theirs_errors_when_task_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = Arc::new(Project::open(tmp.path()).unwrap());
        let pipeline = PushPipeline::new(project).unwrap();
        // No task in the store — pull should surface a clean error
        // rather than panic.
        let err = pipeline.resolve_pull_theirs(Uuid::new_v4()).await.unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn resolve_force_push_errors_when_task_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = Arc::new(Project::open(tmp.path()).unwrap());
        let pipeline = PushPipeline::new(project).unwrap();
        let err = pipeline.resolve_force_push(Uuid::new_v4()).await.unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn shutdown_breaks_drain_loop_on_next_tick() {
        // The fix for the leak across set_runtime: shutdown() flips
        // the flag; the spawned drain loop checks on each tick and
        // exits. We can verify the JoinHandle completes after we
        // signal shutdown.
        let tmp = tempfile::TempDir::new().unwrap();
        let project = Arc::new(Project::open(tmp.path()).unwrap());
        let pipeline = PushPipeline::new(project).unwrap();
        let handle = pipeline.clone().spawn_drain_loop();
        // Give the loop a moment to start ticking.
        tokio::time::sleep(Duration::from_millis(50)).await;
        pipeline.shutdown();
        // Wait up to 3s for the loop to exit (it polls every ~1s).
        let result = tokio::time::timeout(Duration::from_secs(3), handle).await;
        assert!(result.is_ok(), "drain loop should exit after shutdown()");
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = Arc::new(Project::open(tmp.path()).unwrap());
        let pipeline = PushPipeline::new(project).unwrap();
        pipeline.shutdown();
        pipeline.shutdown(); // second call doesn't panic
        assert!(pipeline.shutdown_flag.load(Ordering::SeqCst));
    }

}
