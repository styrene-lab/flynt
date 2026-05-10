//! Sync engine — diffs forge issues against local state.
//!
//! Operates on `ForgeIssue` directly. The caller (typically the agent
//! extension) translates `SyncOp::CreateLocal` / `UpdateLocal` into
//! `flynt_models::Task` writes against the project store. This crate is
//! intentionally project-agnostic so it can be used from sentry-side
//! adapters too.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

use flynt_models::engagement::RepoBinding;
use styrene_forge::{ForgeClient, ForgeIssue, ForgeResult, IssueState, ListOpts};

/// Stable content hash — title + body + state + labels.
///
/// Used for change detection so we don't mistake "remote untouched" for
/// "remote changed but happens to roundtrip the same string." Cheap and
/// deterministic.
pub fn content_hash(title: &str, body: &str, state: &str, labels: &[String]) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    title.hash(&mut h);
    body.hash(&mut h);
    state.hash(&mut h);
    // Sort labels before hashing — GitHub returns labels in arbitrary
    // order across requests, and we don't want order-only differences
    // to trigger false conflict detection. Same content + same set of
    // labels → same hash, regardless of how the API ordered them.
    let mut sorted: Vec<&String> = labels.iter().collect();
    sorted.sort();
    for label in sorted {
        label.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

fn issue_state_str(s: IssueState) -> &'static str {
    match s {
        IssueState::Open => "open",
        IssueState::Closed => "closed",
    }
}

pub fn issue_hash(issue: &ForgeIssue) -> String {
    content_hash(&issue.title, &issue.body, issue_state_str(issue.state), &issue.labels)
}

// ── IssueMap ────────────────────────────────────────────────────────────────

/// Tracks the binding between a flynt task and a forge issue.
///
/// Persisted by `SyncStore`. The forge URL is duplicated from the
/// engagement's repo binding for provenance — easier to audit a sync
/// log without joining tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueMap {
    /// flynt task id (TaskId.0).
    pub local_id: Uuid,
    /// flynt board id (BoardId.0). Helps narrow queries when one
    /// engagement spans multiple boards.
    pub board_id: Uuid,
    pub forge_org: String,
    pub forge_repo: String,
    pub forge_issue_number: u64,
    pub last_synced: DateTime<Utc>,
    /// Hash of (title, body, state, labels) at the moment of last sync.
    /// `None` means we've never sync'd, or the sync side wrote a record
    /// without a content snapshot — re-sync will populate it.
    pub last_hash: Option<String>,
    /// Forge issue HTML URL — provenance.
    pub forge_url: Option<String>,
}

// ── SyncOp ──────────────────────────────────────────────────────────────────

/// Effect the caller should apply after the engine finishes diffing.
///
/// Engine never writes to the project — it just reports what should
/// happen. Caller is responsible for materializing each op against
/// flynt's `ProjectStore` and updating the corresponding `IssueMap`.
#[derive(Debug, Clone)]
pub enum SyncOp {
    /// Forge has an issue we don't know about — create a flynt task.
    CreateLocal { issue: ForgeIssue, local_id: Uuid },
    /// Forge issue's content changed — update the local task and
    /// refresh the stored hash.
    UpdateLocal { local_id: Uuid, issue: ForgeIssue, new_hash: String },
    /// We pushed a brand-new task to the forge; remember the assigned
    /// issue number.
    CreatedRemote { local_id: Uuid, issue_number: u64 },
    /// We pushed an update to an existing forge issue; refresh the
    /// stored hash.
    UpdatedRemote { local_id: Uuid, issue_number: u64, new_hash: String },
}

// ── SyncEngine ──────────────────────────────────────────────────────────────

pub struct SyncEngine<'a> {
    client: &'a dyn ForgeClient,
}

impl<'a> SyncEngine<'a> {
    pub fn new(client: &'a dyn ForgeClient) -> Self { Self { client } }

    /// Pull issues from the forge and report changes.
    ///
    /// **Truncation caveat**: this delegates to
    /// [`ForgeClient::list_issues`] with no pagination args, so the
    /// underlying client may silently cap at its `MAX_PAGES` (1000
    /// items for the GitHub client). For repos that overflow that
    /// window, missing issues will not appear as `CreateLocal` ops
    /// — they'll just be absent. See `clients::github` module doc.
    pub async fn pull_issues(
        &self,
        binding: &RepoBinding,
        existing: &[IssueMap],
    ) -> ForgeResult<Vec<SyncOp>> {
        let issues = self
            .client
            .list_issues(&binding.forge_org, &binding.forge_repo, &ListOpts::default())
            .await?;

        info!(forge = %binding.full_name(), count = issues.len(), "pulled issues from forge");

        let mut ops = Vec::new();
        for issue in issues {
            let hash = issue_hash(&issue);
            match existing.iter().find(|m| m.forge_issue_number == issue.number) {
                Some(map) => {
                    if map.last_hash.as_deref() != Some(&hash) {
                        debug!(issue = issue.number, "issue changed since last sync");
                        ops.push(SyncOp::UpdateLocal {
                            local_id: map.local_id,
                            issue,
                            new_hash: hash,
                        });
                    }
                }
                None => {
                    ops.push(SyncOp::CreateLocal { issue, local_id: Uuid::new_v4() });
                }
            }
        }
        Ok(ops)
    }

    /// Push local changes to the forge. `items` carries the minimum
    /// shape the engine needs — caller flattens flynt Tasks into this
    /// tuple shape.
    pub async fn push_changes(
        &self,
        binding: &RepoBinding,
        existing: &[IssueMap],
        items: &[(Uuid, String, String, IssueState, Vec<String>)],
    ) -> ForgeResult<Vec<SyncOp>> {
        let mut ops = Vec::new();
        for (local_id, title, body, state, labels) in items {
            match existing.iter().find(|m| m.local_id == *local_id) {
                Some(map) => {
                    let update = styrene_forge::UpdateIssue {
                        title: Some(title.clone()),
                        body: Some(body.clone()),
                        state: Some(*state),
                        labels: Some(labels.clone()),
                        ..Default::default()
                    };
                    match self
                        .client
                        .update_issue(&binding.forge_org, &binding.forge_repo, map.forge_issue_number, &update)
                        .await
                    {
                        Ok(issue) => {
                            ops.push(SyncOp::UpdatedRemote {
                                local_id: *local_id,
                                issue_number: issue.number,
                                new_hash: issue_hash(&issue),
                            });
                        }
                        Err(e) => warn!(local_id = %local_id, error = %e, "push update failed"),
                    }
                }
                None => {
                    let create = styrene_forge::CreateIssue {
                        title: title.clone(),
                        body: body.clone(),
                        labels: labels.clone(),
                        milestone: None,
                        assignees: Vec::new(),
                    };
                    match self
                        .client
                        .create_issue(&binding.forge_org, &binding.forge_repo, &create)
                        .await
                    {
                        Ok(issue) => {
                            ops.push(SyncOp::CreatedRemote {
                                local_id: *local_id,
                                issue_number: issue.number,
                            });
                        }
                        Err(e) => warn!(local_id = %local_id, error = %e, "create failed"),
                    }
                }
            }
        }
        Ok(ops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use styrene_forge::{
        CreateIssue, CreateRepo, CreateWebhook, ForgeEndpoint, ForgeError, ForgeKind, ForgeLabel,
        ForgeMilestone, ForgeRepo, ForgeWebhook, UpdateIssue,
    };

    struct MockClient {
        endpoint: ForgeEndpoint,
        issues: Vec<ForgeIssue>,
        created: Mutex<Vec<CreateIssue>>,
    }

    impl MockClient {
        fn new(issues: Vec<ForgeIssue>) -> Self {
            Self {
                endpoint: ForgeEndpoint {
                    id: "mock".into(),
                    kind: ForgeKind::GitHub,
                    base_url: "http://mock".into(),
                    token_secret: None,
                },
                issues,
                created: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ForgeClient for MockClient {
        fn kind(&self) -> ForgeKind { ForgeKind::GitHub }
        fn endpoint(&self) -> &ForgeEndpoint { &self.endpoint }

        async fn list_issues(&self, _: &str, _: &str, _: &ListOpts) -> ForgeResult<Vec<ForgeIssue>> {
            Ok(self.issues.clone())
        }
        async fn get_issue(&self, _: &str, _: &str, n: u64) -> ForgeResult<ForgeIssue> {
            self.issues.iter().find(|i| i.number == n).cloned()
                .ok_or_else(|| ForgeError::NotFound(format!("issue {n}")))
        }
        async fn create_issue(&self, _: &str, _: &str, c: &CreateIssue) -> ForgeResult<ForgeIssue> {
            self.created.lock().unwrap().push(c.clone());
            Ok(ForgeIssue {
                number: 99, title: c.title.clone(), body: c.body.clone(),
                state: IssueState::Open, labels: c.labels.clone(), milestone: None,
                assignees: Vec::new(), created_at: Utc::now(), updated_at: Utc::now(),
                closed_at: None, url: "http://mock/99".into(),
            })
        }
        async fn update_issue(&self, _: &str, _: &str, n: u64, u: &UpdateIssue) -> ForgeResult<ForgeIssue> {
            Ok(ForgeIssue {
                number: n,
                title: u.title.clone().unwrap_or_default(),
                body: u.body.clone().unwrap_or_default(),
                state: u.state.unwrap_or(IssueState::Open),
                labels: u.labels.clone().unwrap_or_default(),
                milestone: None, assignees: Vec::new(),
                created_at: Utc::now(), updated_at: Utc::now(),
                closed_at: None, url: format!("http://mock/{n}"),
            })
        }
        async fn list_labels(&self, _: &str, _: &str) -> ForgeResult<Vec<ForgeLabel>> { Ok(vec![]) }
        async fn list_milestones(&self, _: &str, _: &str) -> ForgeResult<Vec<ForgeMilestone>> { Ok(vec![]) }
        async fn list_repos(&self, _: &str) -> ForgeResult<Vec<ForgeRepo>> { Ok(vec![]) }
        async fn create_repo(&self, _: &str, _: &CreateRepo) -> ForgeResult<ForgeRepo> {
            Err(ForgeError::Api { status: 501, message: "not impl".into() })
        }
        async fn create_webhook(&self, _: &str, _: &str, _: &CreateWebhook) -> ForgeResult<ForgeWebhook> {
            Err(ForgeError::Api { status: 501, message: "not impl".into() })
        }
    }

    fn issue(number: u64, title: &str) -> ForgeIssue {
        ForgeIssue {
            number, title: title.into(), body: "".into(), state: IssueState::Open,
            labels: vec![], milestone: None, assignees: vec![],
            created_at: Utc::now(), updated_at: Utc::now(), closed_at: None,
            url: format!("http://mock/{number}"),
        }
    }

    fn binding() -> RepoBinding {
        RepoBinding::new("anthropics", "test")
    }

    #[tokio::test]
    async fn pull_create_for_unknown_issues() {
        let client = MockClient::new(vec![issue(1, "A"), issue(2, "B")]);
        let engine = SyncEngine::new(&client);
        let ops = engine.pull_issues(&binding(), &[]).await.unwrap();
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], SyncOp::CreateLocal { .. }));
    }

    #[tokio::test]
    async fn pull_skips_unchanged_known_issues() {
        let i1 = issue(1, "A");
        let h1 = issue_hash(&i1);
        let client = MockClient::new(vec![i1.clone()]);
        let engine = SyncEngine::new(&client);
        let map = IssueMap {
            local_id: Uuid::new_v4(),
            board_id: Uuid::new_v4(),
            forge_org: "anthropics".into(),
            forge_repo: "test".into(),
            forge_issue_number: 1,
            last_synced: Utc::now(),
            last_hash: Some(h1),
            forge_url: Some(i1.url.clone()),
        };
        let ops = engine.pull_issues(&binding(), &[map]).await.unwrap();
        assert!(ops.is_empty(), "unchanged issue should produce no ops");
    }

    #[tokio::test]
    async fn pull_emits_update_when_hash_differs() {
        // Map says old hash; remote shows fresh issue → UpdateLocal.
        let client = MockClient::new(vec![issue(1, "A v2")]);
        let engine = SyncEngine::new(&client);
        let map = IssueMap {
            local_id: Uuid::new_v4(),
            board_id: Uuid::new_v4(),
            forge_org: "anthropics".into(),
            forge_repo: "test".into(),
            forge_issue_number: 1,
            last_synced: Utc::now(),
            last_hash: Some("stale".into()),
            forge_url: None,
        };
        let ops = engine.pull_issues(&binding(), &[map]).await.unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], SyncOp::UpdateLocal { .. }));
    }

    #[test]
    fn content_hash_is_label_order_invariant() {
        // Forge APIs return labels in arbitrary order. content_hash
        // sorts before hashing so the same set of labels in different
        // order produces the same hash — otherwise every other GET
        // would falsely flag a conflict.
        let a = content_hash("t", "b", "open", &["bug".into(), "infra".into()]);
        let b = content_hash("t", "b", "open", &["infra".into(), "bug".into()]);
        assert_eq!(a, b, "label order shouldn't change the hash");
    }

    #[test]
    fn content_hash_still_differs_on_content_change() {
        let a = content_hash("title", "body", "open", &["a".into()]);
        let b = content_hash("title-2", "body", "open", &["a".into()]);
        assert_ne!(a, b);
    }

    #[test]
    fn content_hash_label_set_difference_changes_hash() {
        // Sort doesn't dedupe — a label being added/removed still
        // changes the hash.
        let a = content_hash("t", "b", "open", &["a".into()]);
        let b = content_hash("t", "b", "open", &["a".into(), "b".into()]);
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn push_creates_when_no_mapping() {
        let client = MockClient::new(vec![]);
        let engine = SyncEngine::new(&client);
        let local = Uuid::new_v4();
        let ops = engine
            .push_changes(
                &binding(),
                &[],
                &[(local, "T".into(), "B".into(), IssueState::Open, vec!["bug".into()])],
            )
            .await
            .unwrap();
        assert!(matches!(ops[0], SyncOp::CreatedRemote { .. }));
        assert_eq!(client.created.lock().unwrap().len(), 1);
    }
}
