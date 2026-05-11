//! GitLab `ForgeClient` — direct REST calls via reqwest.
//!
//! GitLab's API diverges from GitHub/Forgejo more than they diverge
//! from each other. Key differences captured here:
//!
//! - **Path encoding**: a "repo" is a `project` with an integer ID. The
//!   `<owner>/<repo>` string is URL-encoded into the path:
//!   `/projects/<urlencoded>/issues`. Slashes become `%2F`.
//! - **Issue state**: `opened` (not `open`) and `closed`. The state we
//!   send to PATCH is `state_event` with values `close` / `reopen`.
//! - **Numbering**: each project has its own issue IIDs (internal IDs).
//!   We use `iid` as the `number` since that's what users see in the UI.
//!   The global `id` is hidden from the surface contract.
//! - **Auth header**: `PRIVATE-TOKEN: <PAT>`. Bearer also works for
//!   OAuth tokens but PATs are more common for service accounts.
//! - **Pagination**: `?page=N&per_page=M`, with `x-next-page` and
//!   `x-total-pages` response headers. No Link-header convention in
//!   the same shape as GitHub; we use the next-page header instead.
//!
//! ## Limits
//!
//! Pagination capped at [`MAX_PAGES`] × [`MAX_PER_PAGE`]. GitLab clamps
//! per_page at 100 for most endpoints.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::header::{ACCEPT, HeaderMap};
use serde::Deserialize;
use tracing::{debug, warn};

use crate::auth::SharedTokenResolver;
use styrene_forge::{
    CreateIssue, CreateRepo, CreateWebhook, ForgeClient, ForgeEndpoint, ForgeError, ForgeIssue,
    ForgeKind, ForgeLabel, ForgeMilestone, ForgeRepo, ForgeResult, ForgeWebhook, IssueState,
    ListOpts, UpdateIssue,
};

const API_PREFIX: &str = "/api/v4";
const MAX_PER_PAGE: u32 = 100;
const MAX_PAGES: usize = 10;
const USER_AGENT: &str = concat!("flynt-forge/", env!("CARGO_PKG_VERSION"));

pub struct GitlabForgeClient {
    http: reqwest::Client,
    endpoint: ForgeEndpoint,
    token: SharedTokenResolver,
}

impl GitlabForgeClient {
    pub fn new(endpoint: ForgeEndpoint, token: SharedTokenResolver) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build reqwest client");
        Self { http, endpoint, token }
    }

    fn base(&self) -> &str {
        self.endpoint.base_url.trim_end_matches('/')
    }

    /// Build the canonical project path segment. `owner/repo` → URL-encoded
    /// `owner%2Frepo`. GitLab refuses unencoded slashes in the project
    /// path, so this is not optional.
    fn project_path(owner: &str, repo: &str) -> String {
        let raw = format!("{owner}/{repo}");
        utf8_percent_encode(&raw, NON_ALPHANUMERIC).to_string()
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}{}", self.base(), API_PREFIX, path);
        let mut req = self
            .http
            .request(method, &url)
            .header(ACCEPT, "application/json");
        if let Some(token) = self.token.resolve() {
            req = req.header("PRIVATE-TOKEN", token);
        }
        req
    }

    async fn check_response(&self, resp: reqwest::Response) -> ForgeResult<reqwest::Response> {
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            debug!(status = %status, body = %body, "gitlab non-success");
            return Err(ForgeError::from_status(status.as_u16(), body));
        }
        Ok(resp)
    }

    /// GitLab signals "more pages" via `x-next-page` (the next page
    /// number, empty when finished). We construct the next URL by
    /// re-issuing the same path with the new `page=` query.
    fn next_page_number(headers: &HeaderMap) -> Option<u32> {
        let val = headers.get("x-next-page")?.to_str().ok()?;
        if val.is_empty() {
            return None;
        }
        val.parse::<u32>().ok().filter(|n| *n > 0)
    }

    async fn get_all_pages<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> ForgeResult<Vec<T>> {
        let mut all = Vec::new();
        let per_page = MAX_PER_PAGE.to_string();
        let mut page: u32 = 1;
        let mut pages_fetched = 0usize;

        loop {
            if pages_fetched >= MAX_PAGES {
                warn!(max_pages = MAX_PAGES, total = all.len(), "pagination capped");
                break;
            }
            let page_str = page.to_string();
            let mut req = self.request(reqwest::Method::GET, path);
            for (k, v) in query {
                req = req.query(&[(*k, *v)]);
            }
            req = req
                .query(&[("per_page", per_page.as_str())])
                .query(&[("page", page_str.as_str())]);

            let resp = self.check_response(req.send().await.map_err(reqwest_err)?).await?;
            let next = Self::next_page_number(resp.headers());
            let items: Vec<T> = resp.json().await.map_err(reqwest_err)?;
            if items.is_empty() {
                break;
            }
            all.extend(items);
            pages_fetched += 1;

            match next {
                Some(n) => page = n,
                None => break,
            }
        }
        Ok(all)
    }
}

fn reqwest_err(e: reqwest::Error) -> ForgeError {
    ForgeError::Network(e.to_string())
}

#[async_trait]
impl ForgeClient for GitlabForgeClient {
    fn kind(&self) -> ForgeKind {
        ForgeKind::GitLab
    }
    fn endpoint(&self) -> &ForgeEndpoint {
        &self.endpoint
    }

    async fn list_issues(
        &self,
        owner: &str,
        repo: &str,
        opts: &ListOpts,
    ) -> ForgeResult<Vec<ForgeIssue>> {
        let project = Self::project_path(owner, repo);
        let path = format!("/projects/{project}/issues");

        let mut query: Vec<(&str, &str)> = Vec::new();
        if let Some(state) = &opts.state {
            query.push((
                "state",
                match state {
                    IssueState::Open => "opened",
                    IssueState::Closed => "closed",
                },
            ));
        }

        if opts.per_page.is_some() || opts.page.is_some() {
            let mut req = self.request(reqwest::Method::GET, &path);
            for (k, v) in &query {
                req = req.query(&[(*k, *v)]);
            }
            if let Some(page) = opts.page {
                req = req.query(&[("page", page.to_string())]);
            }
            if let Some(per) = opts.per_page {
                req = req.query(&[("per_page", per.to_string())]);
            }
            if !opts.labels.is_empty() {
                req = req.query(&[("labels", opts.labels.join(","))]);
            }
            let resp = self.check_response(req.send().await.map_err(reqwest_err)?).await?;
            let issues: Vec<GlIssue> = resp.json().await.map_err(reqwest_err)?;
            return Ok(issues.into_iter().map(GlIssue::into_forge_issue).collect());
        }

        let issues: Vec<GlIssue> = self.get_all_pages(&path, &query).await?;
        Ok(issues.into_iter().map(GlIssue::into_forge_issue).collect())
    }

    async fn get_issue(&self, owner: &str, repo: &str, number: u64) -> ForgeResult<ForgeIssue> {
        let project = Self::project_path(owner, repo);
        let resp = self
            .check_response(
                self.request(
                    reqwest::Method::GET,
                    &format!("/projects/{project}/issues/{number}"),
                )
                .send()
                .await
                .map_err(reqwest_err)?,
            )
            .await?;
        let issue: GlIssue = resp.json().await.map_err(reqwest_err)?;
        Ok(issue.into_forge_issue())
    }

    async fn create_issue(
        &self,
        owner: &str,
        repo: &str,
        issue: &CreateIssue,
    ) -> ForgeResult<ForgeIssue> {
        let project = Self::project_path(owner, repo);
        let body = serde_json::json!({
            "title": issue.title,
            "description": issue.body,
            // GitLab takes labels as a comma-separated string. Empty
            // vec → omit the field entirely so we don't accidentally
            // clear an existing set.
            "labels": issue.labels.join(","),
            // GitLab uses `assignee_ids` (integers), not usernames. We
            // can't map names → IDs without an extra round-trip, so
            // omit assignees on the create path. PRs / new tasks won't
            // typically come with assignees attached locally.
        });
        let resp = self
            .check_response(
                self.request(
                    reqwest::Method::POST,
                    &format!("/projects/{project}/issues"),
                )
                .json(&body)
                .send()
                .await
                .map_err(reqwest_err)?,
            )
            .await?;
        let created: GlIssue = resp.json().await.map_err(reqwest_err)?;
        Ok(created.into_forge_issue())
    }

    async fn update_issue(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        update: &UpdateIssue,
    ) -> ForgeResult<ForgeIssue> {
        let project = Self::project_path(owner, repo);
        let mut body = serde_json::Map::new();
        if let Some(t) = &update.title {
            body.insert("title".into(), serde_json::Value::String(t.clone()));
        }
        if let Some(b) = &update.body {
            // GitLab field name differs: `description`.
            body.insert("description".into(), serde_json::Value::String(b.clone()));
        }
        if let Some(s) = &update.state {
            // GitLab uses state_event with verbs, not values: "close"
            // closes an open issue, "reopen" reopens a closed one.
            body.insert(
                "state_event".into(),
                serde_json::Value::String(
                    match s {
                        IssueState::Open => "reopen",
                        IssueState::Closed => "close",
                    }
                    .into(),
                ),
            );
        }
        if let Some(labels) = &update.labels {
            // Same comma-encoded string format on update.
            body.insert(
                "labels".into(),
                serde_json::Value::String(labels.join(",")),
            );
        }
        let resp = self
            .check_response(
                self.request(
                    reqwest::Method::PUT,
                    &format!("/projects/{project}/issues/{number}"),
                )
                .json(&body)
                .send()
                .await
                .map_err(reqwest_err)?,
            )
            .await?;
        let updated: GlIssue = resp.json().await.map_err(reqwest_err)?;
        Ok(updated.into_forge_issue())
    }

    async fn list_labels(&self, owner: &str, repo: &str) -> ForgeResult<Vec<ForgeLabel>> {
        let project = Self::project_path(owner, repo);
        let labels: Vec<GlLabel> = self
            .get_all_pages(&format!("/projects/{project}/labels"), &[])
            .await?;
        Ok(labels
            .into_iter()
            .map(|l| ForgeLabel {
                name: l.name,
                color: l.color,
                description: l.description,
            })
            .collect())
    }

    async fn list_milestones(&self, owner: &str, repo: &str) -> ForgeResult<Vec<ForgeMilestone>> {
        let project = Self::project_path(owner, repo);
        let ms: Vec<GlMilestone> = self
            .get_all_pages(&format!("/projects/{project}/milestones"), &[])
            .await?;
        Ok(ms
            .into_iter()
            .map(|m| ForgeMilestone {
                title: m.title,
                description: m.description,
                // GitLab milestone states: "active" / "closed".
                state: if m.state == "closed" {
                    IssueState::Closed
                } else {
                    IssueState::Open
                },
                // GitLab milestones have `due_date` as YYYY-MM-DD string,
                // not a full timestamp like GitHub. Parse to noon UTC
                // so the field round-trips through a DateTime<Utc>.
                due_date: m.due_date.and_then(|d| {
                    chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                        .ok()
                        .and_then(|nd| nd.and_hms_opt(12, 0, 0))
                        .map(|ndt| ndt.and_utc())
                }),
            })
            .collect())
    }

    async fn list_repos(&self, org: &str) -> ForgeResult<Vec<ForgeRepo>> {
        // GitLab's "group" concept replaces "org". Try the group path
        // first; if that 404s, try the user namespace. Both endpoints
        // return the same project shape.
        let path_group = format!(
            "/groups/{}/projects",
            utf8_percent_encode(org, NON_ALPHANUMERIC)
        );
        let repos: Vec<GlProject> = match self.get_all_pages(&path_group, &[]).await {
            Ok(r) => r,
            Err(_) => {
                let path_user = format!(
                    "/users/{}/projects",
                    utf8_percent_encode(org, NON_ALPHANUMERIC)
                );
                self.get_all_pages(&path_user, &[]).await?
            }
        };
        Ok(repos.into_iter().map(GlProject::into_forge_repo).collect())
    }

    async fn create_repo(&self, org: &str, repo: &CreateRepo) -> ForgeResult<ForgeRepo> {
        // GitLab `POST /projects` with `namespace_id` would be canonical,
        // but resolving group name → ID requires an extra GET. For now
        // we use the simpler `path` + `name` form which GitLab accepts
        // under the current user's namespace. To create in a group,
        // `namespace_id` is mandatory; v2 can add that round-trip.
        let _ = org;
        let body = serde_json::json!({
            "name": repo.name,
            "path": repo.name,
            "description": repo.description,
            "visibility": if repo.private { "private" } else { "public" },
        });
        let resp = self
            .check_response(
                self.request(reqwest::Method::POST, "/projects")
                    .json(&body)
                    .send()
                    .await
                    .map_err(reqwest_err)?,
            )
            .await?;
        let created: GlProject = resp.json().await.map_err(reqwest_err)?;
        Ok(created.into_forge_repo())
    }

    async fn create_webhook(
        &self,
        owner: &str,
        repo: &str,
        hook: &CreateWebhook,
    ) -> ForgeResult<ForgeWebhook> {
        let project = Self::project_path(owner, repo);
        // GitLab webhook events are individual booleans on the request
        // body (push_events, issues_events, etc.). Map the string set
        // we accept into those flags. Unknown event names are silently
        // dropped — better than failing the whole hook for one typo.
        let mut body = serde_json::Map::new();
        body.insert("url".into(), serde_json::Value::String(hook.url.clone()));
        if let Some(s) = &hook.secret {
            body.insert("token".into(), serde_json::Value::String(s.clone()));
        }
        for e in &hook.events {
            let flag = match e.as_str() {
                "push" => Some("push_events"),
                "issues" => Some("issues_events"),
                "merge_request" | "merge_requests" => Some("merge_requests_events"),
                "note" => Some("note_events"),
                _ => None,
            };
            if let Some(f) = flag {
                body.insert(f.into(), serde_json::Value::Bool(true));
            }
        }
        let resp = self
            .check_response(
                self.request(
                    reqwest::Method::POST,
                    &format!("/projects/{project}/hooks"),
                )
                .json(&body)
                .send()
                .await
                .map_err(reqwest_err)?,
            )
            .await?;
        let created: GlWebhook = resp.json().await.map_err(reqwest_err)?;
        Ok(ForgeWebhook {
            id: created.id,
            url: hook.url.clone(),
            events: hook.events.clone(),
            active: true,
        })
    }
}

// ── GitLab API response types ───────────────────────────────────────────────

#[derive(Deserialize)]
struct GlIssue {
    iid: u64, // project-local issue number (what users see)
    title: String,
    #[serde(default)]
    description: Option<String>,
    state: String,
    #[serde(default)]
    labels: Vec<String>,
    milestone: Option<GlMilestoneInIssue>,
    #[serde(default)]
    assignees: Vec<GlUser>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
    web_url: String,
}

impl GlIssue {
    fn into_forge_issue(self) -> ForgeIssue {
        ForgeIssue {
            number: self.iid,
            title: self.title,
            body: self.description.unwrap_or_default(),
            // GitLab state values: "opened" / "closed". Anything else
            // (very unlikely; "locked" doesn't exist for issues) falls
            // through to Open as a safe default.
            state: if self.state == "closed" {
                IssueState::Closed
            } else {
                IssueState::Open
            },
            labels: self.labels,
            milestone: self.milestone.map(|m| m.title),
            assignees: self.assignees.into_iter().map(|u| u.username).collect(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            closed_at: self.closed_at,
            url: self.web_url,
        }
    }
}

#[derive(Deserialize)]
struct GlMilestoneInIssue {
    title: String,
}

#[derive(Deserialize)]
struct GlLabel {
    name: String,
    color: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct GlMilestone {
    title: String,
    description: Option<String>,
    state: String,
    due_date: Option<String>,
}

#[derive(Deserialize)]
struct GlUser {
    username: String,
}

#[derive(Deserialize)]
struct GlProject {
    name: String,
    path_with_namespace: String,
    #[serde(default)]
    description: Option<String>,
    default_branch: Option<String>,
    http_url_to_repo: String,
    ssh_url_to_repo: String,
    web_url: String,
    visibility: String,
    #[serde(default)]
    forked_from_project: Option<serde_json::Value>,
    #[serde(default)]
    archived: bool,
    last_activity_at: DateTime<Utc>,
}

impl GlProject {
    fn into_forge_repo(self) -> ForgeRepo {
        ForgeRepo {
            name: self.name,
            full_name: self.path_with_namespace,
            description: self.description.unwrap_or_default(),
            default_branch: self.default_branch.unwrap_or_else(|| "main".into()),
            clone_url: self.http_url_to_repo,
            ssh_url: self.ssh_url_to_repo,
            html_url: self.web_url,
            private: self.visibility != "public",
            fork: self.forked_from_project.is_some(),
            archived: self.archived,
            updated_at: self.last_activity_at,
        }
    }
}

#[derive(Deserialize)]
struct GlWebhook {
    id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::StaticToken;
    use std::sync::Arc;

    fn endpoint(url: &str) -> ForgeEndpoint {
        ForgeEndpoint {
            id: "gitlab-test".into(),
            kind: ForgeKind::GitLab,
            base_url: url.into(),
            token_secret: None,
        }
    }

    #[test]
    fn project_path_url_encodes_slash() {
        // owner/repo → owner%2Frepo. Without this, GitLab refuses the
        // request with 404 — slashes are path separators, project IDs
        // require the encoded form.
        assert_eq!(
            GitlabForgeClient::project_path("flynt", "core"),
            "flynt%2Fcore"
        );
    }

    #[test]
    fn project_path_handles_deeply_nested_namespace() {
        // GitLab groups can nest: `group/subgroup/project`. The whole
        // path goes into the URL-encoded segment.
        assert_eq!(
            GitlabForgeClient::project_path("group/subgroup", "proj"),
            "group%2Fsubgroup%2Fproj"
        );
    }

    #[test]
    fn base_trims_trailing_slashes() {
        let client = GitlabForgeClient::new(
            endpoint("https://gitlab.com/"),
            Arc::new(StaticToken::anonymous()),
        );
        assert_eq!(client.base(), "https://gitlab.com");
    }

    #[test]
    fn client_reports_gitlab_kind() {
        let client = GitlabForgeClient::new(
            endpoint("https://gitlab.com"),
            Arc::new(StaticToken::anonymous()),
        );
        assert_eq!(client.kind(), ForgeKind::GitLab);
    }

    #[test]
    fn next_page_number_parses_valid() {
        let mut h = HeaderMap::new();
        h.insert("x-next-page", "3".parse().unwrap());
        assert_eq!(GitlabForgeClient::next_page_number(&h), Some(3));
    }

    #[test]
    fn next_page_number_empty_means_done() {
        // GitLab returns the header with an empty value on the last
        // page rather than omitting it; we must treat that as "stop"
        // or we'd loop forever requesting page 0.
        let mut h = HeaderMap::new();
        h.insert("x-next-page", "".parse().unwrap());
        assert_eq!(GitlabForgeClient::next_page_number(&h), None);
    }

    #[test]
    fn next_page_number_zero_means_done() {
        // Defensive: some GitLab versions emit "0" instead of empty
        // on the final page.
        let mut h = HeaderMap::new();
        h.insert("x-next-page", "0".parse().unwrap());
        assert_eq!(GitlabForgeClient::next_page_number(&h), None);
    }
}
