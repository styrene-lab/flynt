//! Forge + engagement tool handlers.
//!
//! Lives in its own module to keep `extension.rs` from sprawling.
//! All handlers are free functions that take what they need from
//! `FlyntExtension` (vault store + secret bag) and return omegon
//! `Result<Value>`.
//!
//! ## Tool surface (Phase 3 — scribe absorption)
//!
//! - `bootstrap_secrets` — receive secrets from omegon at startup.
//!   Falls back to `FLYNT_GITHUB_TOKEN` env var if omegon hasn't been
//!   wired to push them yet (relevant for the ACP/Zed launch path).
//! - `engagement_create` / `engagement_list` / `engagement_status`
//!   — CRUD on the engagements table (added in Phase 1.5 / migration v7).
//! - `forge_status` — connectivity check (do we have a token? does
//!   the API respond?).
//! - `forge_list_issues` — pull issues for one repo binding.
//! - `forge_sync_issues` — bidirectional sync via flynt-forge SyncEngine,
//!   materialising CreateLocal/UpdateLocal as flynt tasks.
//! - `forge_create_issue` — create on the forge + create a linked
//!   flynt task + remember the binding in SyncStore.
//! - `log_work` / `timeline` — work-log jsonl per engagement at
//!   `<vault>/.flynt/work-logs/<engagement-id>.jsonl`.

use chrono::{DateTime, Utc};
use flynt_core::{
    models::{BoardId, Task, TaskId},
    store::{TaskFilter, VaultStore},
};
use flynt_forge::{
    GitHubForgeClient, IssueMap, ListOpts, StaticToken, SyncEngine, SyncOp, SyncStore,
    TokenResolver, issue_hash,
};
use flynt_models::engagement::{Engagement, EngagementId, RepoBinding};
use flynt_store::vault::Vault;
use omegon_extension::{Error as ExtError, Result as ExtResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use styrene_forge::{
    CreateIssue as ForgeCreateIssue, ForgeClient, ForgeEndpoint, ForgeKind, IssueState,
};
use uuid::Uuid;

// ── Secret bag ──────────────────────────────────────────────────────────────

/// In-process secret bag. Populated by `bootstrap_secrets` (omegon push)
/// or by the env-var fallback. Used to construct flynt-forge
/// `TokenResolver`s on demand.
///
/// Wrapped in `Arc<RwLock<…>>` so the same bag is shared between the
/// extension RPC handlers and the closures we hand to GitHubForgeClient.
#[derive(Debug, Clone, Default)]
pub struct SecretBag {
    inner: Arc<RwLock<HashMap<String, String>>>,
}

impl SecretBag {
    pub fn new() -> Self { Self::default() }

    /// Seed from environment. Currently honours `FLYNT_GITHUB_TOKEN`
    /// → key `GITHUB_TOKEN` so the github resolver can find it. Called
    /// once at extension construction; safe to call again later
    /// (overwrites existing values).
    pub fn seed_from_env(&self) {
        if let Ok(t) = std::env::var("FLYNT_GITHUB_TOKEN") {
            self.set("GITHUB_TOKEN", &t);
        }
    }

    pub fn set(&self, name: &str, value: &str) {
        self.inner.write().unwrap().insert(name.to_string(), value.to_string());
    }

    pub fn merge(&self, kv: HashMap<String, String>) {
        let mut g = self.inner.write().unwrap();
        for (k, v) in kv { g.insert(k, v); }
    }

    pub fn get(&self, name: &str) -> Option<String> {
        self.inner.read().unwrap().get(name).cloned()
    }

    /// Build a sync TokenResolver bound to one secret name. Resolves
    /// per-request so rotation via subsequent `set` is observed.
    pub fn resolver(&self, secret_name: &str) -> Arc<dyn TokenResolver> {
        let bag = self.inner.clone();
        let name = secret_name.to_string();
        Arc::new(move || bag.read().ok().and_then(|m| m.get(&name).cloned()))
    }
}

// ── Tool definitions ────────────────────────────────────────────────────────

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "engagement_create",
            "label": "Create Engagement",
            "description": "Create a multi-repo engagement (project / sprint / contract). Required: name + forge.kind + forge.base_url. Optional: description, partnership_id, repos[{forge_org, forge_repo}].",
            "parameters": {
                "type": "object",
                "required": ["name", "forge"],
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "partnership_id": { "type": "string", "description": "Optional partnership UUID." },
                    "forge": {
                        "type": "object",
                        "required": ["kind", "base_url"],
                        "properties": {
                            "id": { "type": "string", "description": "Forge endpoint id (default: kind name)." },
                            "kind": { "type": "string", "enum": ["github", "forgejo", "gitlab"] },
                            "base_url": { "type": "string" },
                            "token_secret": { "type": "string", "description": "Name of the secret holding the token (e.g. GITHUB_TOKEN)." }
                        }
                    },
                    "repos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["forge_org", "forge_repo"],
                            "properties": {
                                "forge_org": { "type": "string" },
                                "forge_repo": { "type": "string" },
                                "sync_issues": { "type": "boolean", "default": true },
                                "sync_prs": { "type": "boolean", "default": false }
                            }
                        }
                    }
                }
            }
        }),
        json!({
            "name": "engagement_list",
            "label": "List Engagements",
            "description": "List all engagements with id, name, status, repo count.",
            "parameters": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "engagement_status",
            "label": "Engagement Status",
            "description": "Detail for one engagement: repos, forge endpoint, recent sync timestamps, and task counts. Required: engagement_id.",
            "parameters": {
                "type": "object",
                "required": ["engagement_id"],
                "properties": { "engagement_id": { "type": "string" } }
            }
        }),
        json!({
            "name": "forge_status",
            "label": "Forge Status",
            "description": "Connectivity probe for an engagement's forge: do we have a token, does the API respond. Required: engagement_id.",
            "parameters": {
                "type": "object",
                "required": ["engagement_id"],
                "properties": { "engagement_id": { "type": "string" } }
            }
        }),
        json!({
            "name": "forge_list_issues",
            "label": "List Forge Issues",
            "description": "Pull issues for one repo binding. Required: engagement_id, repo (org/name). Optional: state (open|closed), labels[].",
            "parameters": {
                "type": "object",
                "required": ["engagement_id", "repo"],
                "properties": {
                    "engagement_id": { "type": "string" },
                    "repo": { "type": "string", "description": "org/name format" },
                    "state": { "type": "string", "enum": ["open", "closed"] },
                    "labels": { "type": "array", "items": { "type": "string" } }
                }
            }
        }),
        json!({
            "name": "forge_sync_issues",
            "label": "Sync Forge Issues",
            "description": "Pull all issues for one repo binding and materialise new/updated ones as flynt tasks on the given board. Returns counts and the resulting SyncOps. Required: engagement_id, repo, board_id.",
            "parameters": {
                "type": "object",
                "required": ["engagement_id", "repo", "board_id"],
                "properties": {
                    "engagement_id": { "type": "string" },
                    "repo": { "type": "string", "description": "org/name format" },
                    "board_id": { "type": "string", "description": "Board to land created tasks on." },
                    "column": { "type": "string", "description": "Column for new tasks (default: Backlog)." }
                }
            }
        }),
        json!({
            "name": "forge_create_issue",
            "label": "Create Forge Issue",
            "description": "Create an issue on the forge AND a linked flynt task. Returns the issue + task ids. Required: engagement_id, repo, title, board_id.",
            "parameters": {
                "type": "object",
                "required": ["engagement_id", "repo", "title", "board_id"],
                "properties": {
                    "engagement_id": { "type": "string" },
                    "repo":          { "type": "string" },
                    "title":         { "type": "string" },
                    "body":          { "type": "string" },
                    "labels":        { "type": "array", "items": { "type": "string" } },
                    "board_id":      { "type": "string" },
                    "column":        { "type": "string", "description": "Column to land the task on (default: Backlog)." }
                }
            }
        }),
        json!({
            "name": "log_work",
            "label": "Log Work",
            "description": "Append a work log entry to one engagement. Persisted as JSONL at <vault>/.flynt/work-logs/<engagement-id>.jsonl. Required: engagement_id, content.",
            "parameters": {
                "type": "object",
                "required": ["engagement_id", "content"],
                "properties": {
                    "engagement_id": { "type": "string" },
                    "content": { "type": "string" },
                    "category": {
                        "type": "string",
                        "enum": ["development", "architecture", "review", "deployment", "meeting", "investigation"],
                        "default": "development"
                    },
                    "task_id": { "type": "string", "description": "Optional task this entry relates to." }
                }
            }
        }),
        json!({
            "name": "timeline",
            "label": "Timeline",
            "description": "Aggregated timeline for one engagement: work logs + issue mappings, ts-sorted, newest first. Required: engagement_id. Optional: limit (default 50).",
            "parameters": {
                "type": "object",
                "required": ["engagement_id"],
                "properties": {
                    "engagement_id": { "type": "string" },
                    "limit": { "type": "integer", "default": 50 }
                }
            }
        }),
    ]
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn parse_eid(params: &Value) -> ExtResult<EngagementId> {
    let s = params.get("engagement_id").and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("engagement_id is required"))?;
    let u = Uuid::parse_str(s)
        .map_err(|_| ExtError::invalid_params("engagement_id: not a UUID"))?;
    Ok(EngagementId(u))
}

fn parse_repo(params: &Value) -> ExtResult<(String, String)> {
    let raw = params.get("repo").and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("repo is required (org/name)"))?;
    let (org, name) = raw.split_once('/')
        .ok_or_else(|| ExtError::invalid_params("repo must be 'org/name'"))?;
    if org.is_empty() || name.is_empty() {
        return Err(ExtError::invalid_params("repo must be 'org/name'"));
    }
    Ok((org.to_string(), name.to_string()))
}

fn load_engagement(vault: &Vault, eid: &EngagementId) -> ExtResult<Engagement> {
    vault.store.get_engagement(eid)
        .map_err(|e| ExtError::internal_error(e.to_string()))?
        .ok_or_else(|| ExtError::invalid_params(format!("engagement {} not found", eid.0)))
}

fn load_binding<'a>(eng: &'a Engagement, org: &str, repo: &str) -> ExtResult<&'a RepoBinding> {
    eng.repos.iter()
        .find(|b| b.forge_org == org && b.forge_repo == repo)
        .ok_or_else(|| ExtError::invalid_params(format!("no binding for {org}/{repo} on this engagement")))
}

/// Build a forge client appropriate for the engagement. Today: GitHub
/// only. Other forges (Forgejo, GitLab) return invalid_params until
/// their flynt-forge clients land.
fn build_client(eng: &Engagement, secrets: &SecretBag) -> ExtResult<Box<dyn ForgeClient>> {
    match eng.forge.kind {
        ForgeKind::GitHub => {
            let secret_name = eng.forge.token_secret.as_deref().unwrap_or("GITHUB_TOKEN");
            let token = match secrets.get(secret_name) {
                Some(t) => Arc::new(StaticToken::new(t)) as Arc<dyn TokenResolver>,
                None => secrets.resolver(secret_name),
            };
            Ok(Box::new(GitHubForgeClient::new(eng.forge.clone(), token)))
        }
        other => Err(ExtError::invalid_params(format!(
            "{other:?} forge client not yet ported into flynt-forge — only GitHub for now"
        ))),
    }
}

fn sync_store_for(vault: &Vault) -> ExtResult<SyncStore> {
    let path = vault.root.join(".flynt").join("forge-sync.db");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ExtError::internal_error(format!("create .flynt dir: {e}")))?;
    }
    SyncStore::open(&path).map_err(|e| ExtError::internal_error(e.to_string()))
}

// ── Work log ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkLogEntry {
    pub ts: DateTime<Utc>,
    pub category: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
}

fn work_log_path(vault: &Vault, eid: &EngagementId) -> PathBuf {
    vault.root.join(".flynt").join("work-logs").join(format!("{}.jsonl", eid.0))
}

fn append_work_log(vault: &Vault, eid: &EngagementId, entry: &WorkLogEntry) -> ExtResult<()> {
    let path = work_log_path(vault, eid);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ExtError::internal_error(format!("create work-logs dir: {e}")))?;
    }
    use std::fs::OpenOptions;
    use std::io::Write;
    let mut f = OpenOptions::new().create(true).append(true).open(&path)
        .map_err(|e| ExtError::internal_error(format!("open work log: {e}")))?;
    let line = serde_json::to_string(entry)
        .map_err(|e| ExtError::internal_error(format!("serialize entry: {e}")))?;
    writeln!(f, "{line}").map_err(|e| ExtError::internal_error(format!("write entry: {e}")))?;
    Ok(())
}

fn read_work_log(vault: &Vault, eid: &EngagementId) -> Vec<WorkLogEntry> {
    let path = work_log_path(vault, eid);
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    text.lines()
        .filter_map(|l| serde_json::from_str::<WorkLogEntry>(l).ok())
        .collect()
}

// ── bootstrap_secrets ───────────────────────────────────────────────────────

pub fn bootstrap_secrets(secrets: &SecretBag, params: Value) -> ExtResult<Value> {
    let kv: HashMap<String, String> = serde_json::from_value(params)
        .map_err(|e| ExtError::invalid_params(format!("expected object of name→value: {e}")))?;
    let n = kv.len();
    secrets.merge(kv);
    Ok(json!({ "acknowledged": true, "count": n }))
}

// ── engagement_* ────────────────────────────────────────────────────────────

pub fn engagement_create(vault: &Vault, params: Value) -> ExtResult<Value> {
    let name = params.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("name is required"))?
        .to_string();

    let forge = params.get("forge")
        .ok_or_else(|| ExtError::invalid_params("forge is required"))?;
    let kind_str = forge.get("kind").and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("forge.kind is required"))?;
    let kind: ForgeKind = serde_json::from_value(json!(kind_str))
        .map_err(|e| ExtError::invalid_params(format!("forge.kind: {e}")))?;
    let base_url = forge.get("base_url").and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("forge.base_url is required"))?
        .to_string();
    let endpoint = ForgeEndpoint {
        id: forge.get("id").and_then(|v| v.as_str()).unwrap_or(kind_str).to_string(),
        kind,
        base_url,
        token_secret: forge.get("token_secret").and_then(|v| v.as_str()).map(String::from),
    };

    let mut e = Engagement::new(name, endpoint);
    e.description = params.get("description").and_then(|v| v.as_str()).map(String::from);
    if let Some(p) = params.get("partnership_id").and_then(|v| v.as_str()) {
        let u = Uuid::parse_str(p)
            .map_err(|_| ExtError::invalid_params("partnership_id: not a UUID"))?;
        e.partnership_id = Some(flynt_models::engagement::PartnershipId(u));
    }
    if let Some(arr) = params.get("repos").and_then(|v| v.as_array()) {
        for r in arr {
            let org = r.get("forge_org").and_then(|v| v.as_str())
                .ok_or_else(|| ExtError::invalid_params("repos[].forge_org is required"))?;
            let name = r.get("forge_repo").and_then(|v| v.as_str())
                .ok_or_else(|| ExtError::invalid_params("repos[].forge_repo is required"))?;
            let mut b = RepoBinding::new(org, name);
            if let Some(v) = r.get("sync_issues").and_then(|v| v.as_bool()) { b.sync_issues = v; }
            if let Some(v) = r.get("sync_prs").and_then(|v| v.as_bool()) { b.sync_prs = v; }
            e.repos.push(b);
        }
    }

    vault.store.save_engagement(&e)
        .map_err(|err| ExtError::internal_error(err.to_string()))?;
    Ok(serde_json::to_value(&e).unwrap_or(json!({})))
}

pub fn engagement_list(vault: &Vault, _params: Value) -> ExtResult<Value> {
    let list = vault.store.list_engagements()
        .map_err(|e| ExtError::internal_error(e.to_string()))?;
    let summary: Vec<Value> = list.iter().map(|e| json!({
        "id": e.id.0.to_string(),
        "name": e.name,
        "status": e.status,
        "forge_kind": format!("{:?}", e.forge.kind).to_lowercase(),
        "repo_count": e.repos.len(),
    })).collect();
    Ok(json!(summary))
}

pub fn engagement_status(vault: &Vault, params: Value) -> ExtResult<Value> {
    let eid = parse_eid(&params)?;
    let eng = load_engagement(vault, &eid)?;

    // Task counts under this engagement.
    let tasks = vault.store.list_tasks(&TaskFilter {
        engagement_id: Some(eid.clone()),
        ..Default::default()
    }).map_err(|e| ExtError::internal_error(e.to_string()))?;

    // Per-repo last_synced timestamps (max across mappings).
    let store = sync_store_for(vault).ok();
    let per_repo: Vec<Value> = eng.repos.iter().map(|b| {
        let last_synced = store.as_ref()
            .and_then(|s| s.list_by_repo(&b.forge_org, &b.forge_repo).ok())
            .and_then(|maps| maps.iter().map(|m| m.last_synced).max())
            .map(|t| t.to_rfc3339());
        json!({
            "forge_org": b.forge_org,
            "forge_repo": b.forge_repo,
            "sync_issues": b.sync_issues,
            "sync_prs": b.sync_prs,
            "last_synced": last_synced,
        })
    }).collect();

    Ok(json!({
        "engagement": eng,
        "task_count": tasks.len(),
        "repos": per_repo,
    }))
}

// ── forge_* ─────────────────────────────────────────────────────────────────

pub fn forge_status(vault: &Vault, secrets: &SecretBag, params: Value) -> ExtResult<Value> {
    let eid = parse_eid(&params)?;
    let eng = load_engagement(vault, &eid)?;
    let secret_name = eng.forge.token_secret.as_deref().unwrap_or("GITHUB_TOKEN");
    let has_token = secrets.get(secret_name).is_some();
    Ok(json!({
        "engagement_id": eid.0.to_string(),
        "forge_kind": format!("{:?}", eng.forge.kind).to_lowercase(),
        "base_url": eng.forge.base_url,
        "token_secret": secret_name,
        "has_token": has_token,
        "client_supported": matches!(eng.forge.kind, ForgeKind::GitHub),
    }))
}

pub async fn forge_list_issues(
    vault: &Vault,
    secrets: &SecretBag,
    params: Value,
) -> ExtResult<Value> {
    let eid = parse_eid(&params)?;
    let eng = load_engagement(vault, &eid)?;
    let (org, repo) = parse_repo(&params)?;
    load_binding(&eng, &org, &repo)?;
    let client = build_client(&eng, secrets)?;
    let opts = ListOpts {
        state: params.get("state").and_then(|v| v.as_str()).and_then(|s| match s {
            "open" => Some(IssueState::Open),
            "closed" => Some(IssueState::Closed),
            _ => None,
        }),
        labels: params.get("labels").and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        ..Default::default()
    };
    let issues = client.list_issues(&org, &repo, &opts).await
        .map_err(|e| ExtError::internal_error(format!("forge: {e}")))?;
    Ok(serde_json::to_value(&issues).unwrap_or(json!([])))
}

pub async fn forge_sync_issues(
    vault: &Vault,
    secrets: &SecretBag,
    params: Value,
) -> ExtResult<Value> {
    let eid = parse_eid(&params)?;
    let eng = load_engagement(vault, &eid)?;
    let (org, repo) = parse_repo(&params)?;
    let binding = load_binding(&eng, &org, &repo)?.clone();
    let board_id = parse_board_id(&params)?;
    let column = params.get("column").and_then(|v| v.as_str()).unwrap_or("Backlog").to_string();
    let client = build_client(&eng, secrets)?;
    let store = sync_store_for(vault)?;

    let existing = store.list_by_repo(&org, &repo)
        .map_err(|e| ExtError::internal_error(e.to_string()))?;
    let engine = SyncEngine::new(client.as_ref());
    let ops = engine.pull_issues(&binding, &existing).await
        .map_err(|e| ExtError::internal_error(format!("sync: {e}")))?;

    let mut created = 0_usize;
    let mut updated = 0_usize;
    for op in &ops {
        match op {
            SyncOp::CreateLocal { issue, local_id } => {
                let mut t = Task::new(board_id.clone(), &column, issue.title.clone());
                t.id = TaskId(*local_id);
                t.description = issue.body.clone();
                t.tags = issue.labels.clone();
                t.external_refs = vec![issue.url.clone()];
                t.engagement_id = Some(eid.clone());
                vault.store.save_task(&t)
                    .map_err(|e| ExtError::internal_error(e.to_string()))?;
                store.upsert(&IssueMap {
                    local_id: *local_id,
                    board_id: board_id.0,
                    forge_org: org.clone(),
                    forge_repo: repo.clone(),
                    forge_issue_number: issue.number,
                    last_synced: Utc::now(),
                    last_hash: Some(issue_hash(issue)),
                    forge_url: Some(issue.url.clone()),
                }).map_err(|e| ExtError::internal_error(e.to_string()))?;
                created += 1;
            }
            SyncOp::UpdateLocal { local_id, issue, new_hash } => {
                let mut patch = flynt_models::TaskPatch::default();
                patch.title = Some(issue.title.clone());
                patch.description = Some(issue.body.clone());
                patch.tags = Some(issue.labels.clone());
                vault.store.update_task(&TaskId(*local_id), &patch)
                    .map_err(|e| ExtError::internal_error(e.to_string()))?;
                if let Some(mut m) = store.get_by_issue(&org, &repo, issue.number)
                    .map_err(|e| ExtError::internal_error(e.to_string()))? {
                    m.last_synced = Utc::now();
                    m.last_hash = Some(new_hash.clone());
                    store.upsert(&m).map_err(|e| ExtError::internal_error(e.to_string()))?;
                }
                updated += 1;
            }
            // CreatedRemote / UpdatedRemote come from push paths — not
            // emitted by pull_issues. Defensive arm.
            _ => {}
        }
    }
    Ok(json!({
        "engagement_id": eid.0.to_string(),
        "repo": format!("{org}/{repo}"),
        "created": created,
        "updated": updated,
        "ops": ops.len(),
    }))
}

pub async fn forge_create_issue(
    vault: &Vault,
    secrets: &SecretBag,
    params: Value,
) -> ExtResult<Value> {
    let eid = parse_eid(&params)?;
    let eng = load_engagement(vault, &eid)?;
    let (org, repo) = parse_repo(&params)?;
    load_binding(&eng, &org, &repo)?;
    let title = params.get("title").and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("title is required"))?
        .to_string();
    let body = params.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let labels: Vec<String> = params.get("labels").and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let board_id = parse_board_id(&params)?;
    let column = params.get("column").and_then(|v| v.as_str()).unwrap_or("Backlog").to_string();

    let client = build_client(&eng, secrets)?;
    let issue = client.create_issue(&org, &repo, &ForgeCreateIssue {
        title: title.clone(),
        body: body.clone(),
        labels: labels.clone(),
        milestone: None,
        assignees: Vec::new(),
    }).await.map_err(|e| ExtError::internal_error(format!("forge: {e}")))?;

    let mut t = Task::new(board_id.clone(), &column, title);
    t.description = body;
    t.tags = labels;
    t.external_refs = vec![issue.url.clone()];
    t.engagement_id = Some(eid.clone());
    vault.store.save_task(&t)
        .map_err(|e| ExtError::internal_error(e.to_string()))?;

    let store = sync_store_for(vault)?;
    store.upsert(&IssueMap {
        local_id: t.id.0,
        board_id: board_id.0,
        forge_org: org.clone(),
        forge_repo: repo.clone(),
        forge_issue_number: issue.number,
        last_synced: Utc::now(),
        last_hash: Some(issue_hash(&issue)),
        forge_url: Some(issue.url.clone()),
    }).map_err(|e| ExtError::internal_error(e.to_string()))?;

    Ok(json!({
        "task_id": t.id.0.to_string(),
        "issue_number": issue.number,
        "issue_url": issue.url,
    }))
}

// ── log_work / timeline ─────────────────────────────────────────────────────

pub fn log_work(vault: &Vault, params: Value) -> ExtResult<Value> {
    let eid = parse_eid(&params)?;
    // Validate engagement exists so we don't silently scribble logs for
    // a non-existent record.
    let _ = load_engagement(vault, &eid)?;
    let content = params.get("content").and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("content is required"))?
        .to_string();
    let category = params.get("category").and_then(|v| v.as_str())
        .unwrap_or("development").to_string();
    let task_id = params.get("task_id").and_then(|v| v.as_str())
        .map(|s| Uuid::parse_str(s).map_err(|_| ExtError::invalid_params("task_id: not a UUID")))
        .transpose()?;
    let entry = WorkLogEntry {
        ts: Utc::now(),
        category,
        content,
        task_id,
    };
    append_work_log(vault, &eid, &entry)?;
    Ok(json!({ "logged": true, "ts": entry.ts.to_rfc3339() }))
}

pub fn timeline(vault: &Vault, params: Value) -> ExtResult<Value> {
    let eid = parse_eid(&params)?;
    let _ = load_engagement(vault, &eid)?;
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

    let mut events: Vec<Value> = Vec::new();

    // Work log entries
    for e in read_work_log(vault, &eid) {
        events.push(json!({
            "kind": "work_log",
            "ts": e.ts.to_rfc3339(),
            "category": e.category,
            "content": e.content,
            "task_id": e.task_id.map(|u| u.to_string()),
        }));
    }

    // Issue mappings — show as forge events at last_synced time.
    if let Ok(store) = sync_store_for(vault) {
        let eng = load_engagement(vault, &eid)?;
        for binding in &eng.repos {
            if let Ok(maps) = store.list_by_repo(&binding.forge_org, &binding.forge_repo) {
                for m in maps {
                    events.push(json!({
                        "kind": "issue_map",
                        "ts": m.last_synced.to_rfc3339(),
                        "repo": format!("{}/{}", m.forge_org, m.forge_repo),
                        "issue_number": m.forge_issue_number,
                        "url": m.forge_url,
                        "task_id": m.local_id.to_string(),
                    }));
                }
            }
        }
    }

    // Newest-first sort by ts string (ISO-8601 sorts lexicographically).
    events.sort_by(|a, b| {
        let ta = a.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    events.truncate(limit);

    Ok(json!({ "engagement_id": eid.0.to_string(), "events": events }))
}

// ── Utility: shared with extension.rs (board parsing) ───────────────────────

fn parse_board_id(params: &Value) -> ExtResult<BoardId> {
    let s = params.get("board_id").and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("board_id is required"))?;
    let u = Uuid::parse_str(s).map_err(|_| ExtError::invalid_params("board_id: not a UUID"))?;
    Ok(BoardId(u))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use flynt_store::vault::Vault;
    use tempfile::TempDir;

    fn fresh_vault() -> (TempDir, Arc<Vault>) {
        let tmp = TempDir::new().unwrap();
        let vault = Arc::new(Vault::open(tmp.path()).unwrap());
        (tmp, vault)
    }

    fn engagement_create_params() -> Value {
        json!({
            "name": "Test Engagement",
            "description": "test",
            "forge": {
                "kind": "github",
                "base_url": "https://api.github.com",
                "token_secret": "GITHUB_TOKEN"
            },
            "repos": [
                { "forge_org": "anthropics", "forge_repo": "claude-code" }
            ]
        })
    }

    #[test]
    fn engagement_create_then_list_then_status() {
        let (_tmp, vault) = fresh_vault();
        let created = engagement_create(&vault, engagement_create_params()).unwrap();
        let eid = created.get("id").and_then(|v| v.as_str()).unwrap().to_string();

        let listed = engagement_list(&vault, json!({})).unwrap();
        let arr = listed.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["repo_count"], 1);

        let status = engagement_status(&vault, json!({ "engagement_id": eid })).unwrap();
        assert_eq!(status["task_count"], 0);
        assert_eq!(status["repos"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn engagement_create_rejects_missing_forge() {
        let (_tmp, vault) = fresh_vault();
        let bad = json!({ "name": "x" });
        let err = engagement_create(&vault, bad).unwrap_err();
        // Error::invalid_params messages are conventionally fine to assert against
        // by category — we just want to ensure we got an Err, not a panic.
        let _ = format!("{err:?}");
    }

    #[test]
    fn forge_status_reflects_token_presence() {
        let (_tmp, vault) = fresh_vault();
        let secrets = SecretBag::new();
        let created = engagement_create(&vault, engagement_create_params()).unwrap();
        let eid = created.get("id").and_then(|v| v.as_str()).unwrap();

        let s = forge_status(&vault, &secrets, json!({ "engagement_id": eid })).unwrap();
        assert_eq!(s["has_token"], false);
        assert_eq!(s["client_supported"], true);

        secrets.set("GITHUB_TOKEN", "ghp_xyz");
        let s = forge_status(&vault, &secrets, json!({ "engagement_id": eid })).unwrap();
        assert_eq!(s["has_token"], true);
    }

    #[test]
    fn bootstrap_secrets_merges_into_bag() {
        let secrets = SecretBag::new();
        let _ = bootstrap_secrets(&secrets, json!({
            "GITHUB_TOKEN": "ghp_xyz",
            "OTHER": "v"
        })).unwrap();
        assert_eq!(secrets.get("GITHUB_TOKEN").as_deref(), Some("ghp_xyz"));
        assert_eq!(secrets.get("OTHER").as_deref(), Some("v"));
    }

    #[test]
    fn seed_from_env_reads_flynt_github_token() {
        // SAFETY: test process controls this env var and does not iterate env concurrently.
        unsafe { std::env::set_var("FLYNT_GITHUB_TOKEN", "ghp_envseed"); }
        let secrets = SecretBag::new();
        secrets.seed_from_env();
        assert_eq!(secrets.get("GITHUB_TOKEN").as_deref(), Some("ghp_envseed"));
        unsafe { std::env::remove_var("FLYNT_GITHUB_TOKEN"); }
    }

    #[test]
    fn log_work_then_timeline_returns_entries() {
        let (_tmp, vault) = fresh_vault();
        let created = engagement_create(&vault, engagement_create_params()).unwrap();
        let eid = created.get("id").and_then(|v| v.as_str()).unwrap();

        log_work(&vault, json!({
            "engagement_id": eid,
            "content": "Wrote forge tools",
            "category": "development"
        })).unwrap();
        log_work(&vault, json!({
            "engagement_id": eid,
            "content": "Reviewed PR",
            "category": "review"
        })).unwrap();

        let tl = timeline(&vault, json!({ "engagement_id": eid })).unwrap();
        let events = tl["events"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        // Newest-first: "Reviewed PR" was last logged.
        assert_eq!(events[0]["content"], "Reviewed PR");
        assert_eq!(events[0]["kind"], "work_log");
    }

    #[test]
    fn log_work_rejects_missing_engagement() {
        let (_tmp, vault) = fresh_vault();
        let phantom = Uuid::new_v4().to_string();
        let err = log_work(&vault, json!({
            "engagement_id": phantom,
            "content": "x"
        })).unwrap_err();
        let _ = format!("{err:?}");
    }

    #[test]
    fn parse_repo_requires_org_slash_name() {
        let ok = parse_repo(&json!({ "repo": "anthropics/claude-code" })).unwrap();
        assert_eq!(ok, ("anthropics".to_string(), "claude-code".to_string()));
        assert!(parse_repo(&json!({ "repo": "missing-slash" })).is_err());
        assert!(parse_repo(&json!({ "repo": "/missing-org" })).is_err());
        assert!(parse_repo(&json!({ "repo": "missing-name/" })).is_err());
    }

    // SecretBag & resolver: rotation visibility
    #[test]
    fn secret_bag_resolver_observes_rotation() {
        let bag = SecretBag::new();
        let r = bag.resolver("GITHUB_TOKEN");
        assert!(r.resolve().is_none());
        bag.set("GITHUB_TOKEN", "v1");
        assert_eq!(r.resolve().as_deref(), Some("v1"));
        bag.set("GITHUB_TOKEN", "v2");
        assert_eq!(r.resolve().as_deref(), Some("v2"));
    }
}

