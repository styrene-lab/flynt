//! GitHub `ForgeClient` — direct REST calls via reqwest.
//!
//! Ported from scribe with two changes:
//! - Token comes from a [`SharedTokenResolver`], resolved per-request.
//!   This lets the agent extension reuse omegon's `SecretsManager` and
//!   pick up rotated PATs without rebuilding the client.
//! - User-agent and version strings are flynt's. No scribe dependency
//!   leaked through.
//!
//! ## Pagination contract
//!
//! `list_*` methods cap pagination at [`MAX_PAGES`] × [`MAX_PER_PAGE`]
//! (currently 10 × 100 = 1000 items). When the cap is hit a
//! `tracing::warn!` is emitted but the truncated `Vec` is still
//! returned successfully — there is no in-band signal that the result
//! is partial.
//!
//! Implication: for sync against repos with more than ~1k issues, the
//! local mirror will diverge from the forge silently. Either narrow the
//! query (`ListOpts::state = Closed/Open` filters help), use
//! `page` / `per_page` for explicit cursoring, or treat this client
//! as best-effort. A future revision can lift the cap or surface a
//! `truncated: bool` flag — for now it preserves scribe's behavior to
//! keep the absorption diff small.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap};
use serde::Deserialize;
use tracing::{debug, warn};

use crate::auth::SharedTokenResolver;
use styrene_forge::{
    CreateIssue, CreateRepo, CreateWebhook, ForgeClient, ForgeEndpoint, ForgeError, ForgeIssue,
    ForgeKind, ForgeLabel, ForgeMilestone, ForgeRepo, ForgeResult, ForgeWebhook, IssueState,
    ListOpts, UpdateIssue,
};

const GITHUB_API: &str = "https://api.github.com";
const GITHUB_API_VERSION: &str = "2022-11-28";
const MAX_PER_PAGE: u32 = 100;
/// Cap pagination so a misconfigured query can't OOM us. 10 × 100 = 1k items.
const MAX_PAGES: usize = 10;
const USER_AGENT: &str = concat!("flynt-forge/", env!("CARGO_PKG_VERSION"));

pub struct GitHubForgeClient {
    http: reqwest::Client,
    endpoint: ForgeEndpoint,
    token: SharedTokenResolver,
}

impl GitHubForgeClient {
    pub fn new(endpoint: ForgeEndpoint, token: SharedTokenResolver) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build reqwest client");
        Self {
            http,
            endpoint,
            token,
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{GITHUB_API}{path}");
        let mut req = self
            .http
            .request(method, &url)
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION);
        if let Some(token) = self.token.resolve() {
            req = req.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        req
    }

    async fn check_response(&self, resp: reqwest::Response) -> ForgeResult<reqwest::Response> {
        if let Some(remaining) = resp.headers().get("x-ratelimit-remaining")
            && let Ok(n) = remaining.to_str().unwrap_or("?").parse::<u32>()
        {
            if n < 100 {
                warn!(remaining = n, "GitHub rate limit running low");
            } else {
                debug!(remaining = n, "GitHub rate limit");
            }
        }
        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN
            && resp
                .headers()
                .get("x-ratelimit-remaining")
                .and_then(|v| v.to_str().ok())
                == Some("0")
        {
            let reset = resp
                .headers()
                .get("x-ratelimit-reset")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string();
            return Err(ForgeError::RateLimited { reset_at: reset });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ForgeError::from_status(status.as_u16(), body));
        }
        Ok(resp)
    }

    /// Parse the `Link` header to find the next page URL.
    /// Validates host equality so a compromised server can't redirect
    /// pagination to an attacker-controlled host (which would leak the
    /// auth header via the next `request()` call).
    fn next_page_url(headers: &HeaderMap, expected_host: &str) -> Option<String> {
        let link = headers.get("link")?.to_str().ok()?;
        for part in link.split(',') {
            let part = part.trim();
            if part.contains("rel=\"next\"") {
                let start = part.find('<')? + 1;
                let end = part.find('>')?;
                let url = &part[start..end];
                if let Some(next_host) = url
                    .strip_prefix("https://")
                    .or_else(|| url.strip_prefix("http://"))
                    .and_then(|s| s.split('/').next())
                    && next_host != expected_host
                {
                    tracing::warn!(
                        expected = expected_host,
                        got = next_host,
                        "pagination Link header points to different host — refusing to follow"
                    );
                    return None;
                }
                return Some(url.to_string());
            }
        }
        None
    }

    async fn get_all_pages<T: for<'de> Deserialize<'de>>(
        &self,
        initial_path: &str,
        query: &[(&str, &str)],
    ) -> ForgeResult<Vec<T>> {
        let mut all = Vec::new();
        let per_page = MAX_PER_PAGE.to_string();
        let expected_host = "api.github.com";

        let mut req = self.request(reqwest::Method::GET, initial_path);
        for (k, v) in query {
            req = req.query(&[(*k, *v)]);
        }
        req = req.query(&[("per_page", per_page.as_str())]);

        let resp = self
            .check_response(req.send().await.map_err(reqwest_err)?)
            .await?;
        let mut next = Self::next_page_url(resp.headers(), expected_host);
        let items: Vec<T> = resp.json().await.map_err(reqwest_err)?;
        all.extend(items);

        let mut pages = 1usize;
        while let Some(url) = next {
            if pages >= MAX_PAGES {
                warn!(
                    max_pages = MAX_PAGES,
                    total = all.len(),
                    "pagination capped"
                );
                break;
            }
            let mut req = self
                .http
                .get(&url)
                .header(ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", GITHUB_API_VERSION);
            if let Some(token) = self.token.resolve() {
                req = req.header(AUTHORIZATION, format!("Bearer {token}"));
            }
            let resp = self
                .check_response(req.send().await.map_err(reqwest_err)?)
                .await?;
            next = Self::next_page_url(resp.headers(), expected_host);
            let items: Vec<T> = resp.json().await.map_err(reqwest_err)?;
            if items.is_empty() {
                break;
            }
            all.extend(items);
            pages += 1;
        }
        Ok(all)
    }
}

fn reqwest_err(e: reqwest::Error) -> ForgeError {
    ForgeError::Network(e.to_string())
}

#[async_trait]
impl ForgeClient for GitHubForgeClient {
    fn kind(&self) -> ForgeKind {
        ForgeKind::GitHub
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
        let mut query: Vec<(&str, &str)> = Vec::new();
        if let Some(state) = &opts.state {
            query.push((
                "state",
                match state {
                    IssueState::Open => "open",
                    IssueState::Closed => "closed",
                },
            ));
        }

        // Single-page mode if caller pinned page or per_page; otherwise
        // paginate to MAX_PAGES.
        if opts.per_page.is_some() || opts.page.is_some() {
            let mut req = self.request(
                reqwest::Method::GET,
                &format!("/repos/{owner}/{repo}/issues"),
            );
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
            let resp = self
                .check_response(req.send().await.map_err(reqwest_err)?)
                .await?;
            let issues: Vec<GhIssue> = resp.json().await.map_err(reqwest_err)?;
            return Ok(issues
                .into_iter()
                .filter(|i| i.pull_request.is_none())
                .map(GhIssue::into_forge_issue)
                .collect());
        }

        let issues: Vec<GhIssue> = self
            .get_all_pages(&format!("/repos/{owner}/{repo}/issues"), &query)
            .await?;
        Ok(issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(GhIssue::into_forge_issue)
            .collect())
    }

    async fn get_issue(&self, owner: &str, repo: &str, number: u64) -> ForgeResult<ForgeIssue> {
        let resp = self
            .check_response(
                self.request(
                    reqwest::Method::GET,
                    &format!("/repos/{owner}/{repo}/issues/{number}"),
                )
                .send()
                .await
                .map_err(reqwest_err)?,
            )
            .await?;
        let issue: GhIssue = resp.json().await.map_err(reqwest_err)?;
        Ok(issue.into_forge_issue())
    }

    async fn create_issue(
        &self,
        owner: &str,
        repo: &str,
        issue: &CreateIssue,
    ) -> ForgeResult<ForgeIssue> {
        let body = serde_json::json!({
            "title": issue.title,
            "body": issue.body,
            "labels": issue.labels,
            "assignees": issue.assignees,
        });
        let resp = self
            .check_response(
                self.request(
                    reqwest::Method::POST,
                    &format!("/repos/{owner}/{repo}/issues"),
                )
                .json(&body)
                .send()
                .await
                .map_err(reqwest_err)?,
            )
            .await?;
        let created: GhIssue = resp.json().await.map_err(reqwest_err)?;
        Ok(created.into_forge_issue())
    }

    async fn update_issue(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        update: &UpdateIssue,
    ) -> ForgeResult<ForgeIssue> {
        let mut body = serde_json::Map::new();
        if let Some(t) = &update.title {
            body.insert("title".into(), serde_json::Value::String(t.clone()));
        }
        if let Some(b) = &update.body {
            body.insert("body".into(), serde_json::Value::String(b.clone()));
        }
        if let Some(s) = &update.state {
            body.insert(
                "state".into(),
                serde_json::Value::String(
                    match s {
                        IssueState::Open => "open",
                        IssueState::Closed => "closed",
                    }
                    .into(),
                ),
            );
        }
        if let Some(labels) = &update.labels {
            body.insert(
                "labels".into(),
                serde_json::Value::Array(
                    labels
                        .iter()
                        .map(|l| serde_json::Value::String(l.clone()))
                        .collect(),
                ),
            );
        }
        let resp = self
            .check_response(
                self.request(
                    reqwest::Method::PATCH,
                    &format!("/repos/{owner}/{repo}/issues/{number}"),
                )
                .json(&body)
                .send()
                .await
                .map_err(reqwest_err)?,
            )
            .await?;
        let updated: GhIssue = resp.json().await.map_err(reqwest_err)?;
        Ok(updated.into_forge_issue())
    }

    async fn list_labels(&self, owner: &str, repo: &str) -> ForgeResult<Vec<ForgeLabel>> {
        let labels: Vec<GhLabel> = self
            .get_all_pages(&format!("/repos/{owner}/{repo}/labels"), &[])
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
        let ms: Vec<GhMilestone> = self
            .get_all_pages(&format!("/repos/{owner}/{repo}/milestones"), &[])
            .await?;
        Ok(ms
            .into_iter()
            .map(|m| ForgeMilestone {
                title: m.title,
                description: m.description,
                state: if m.state == "closed" {
                    IssueState::Closed
                } else {
                    IssueState::Open
                },
                due_date: m.due_on,
            })
            .collect())
    }

    async fn list_repos(&self, org: &str) -> ForgeResult<Vec<ForgeRepo>> {
        // Try org first, fall back to user (covers both cases without a
        // pre-flight check).
        let repos: Vec<GhRepo> = match self.get_all_pages(&format!("/orgs/{org}/repos"), &[]).await
        {
            Ok(r) => r,
            Err(_) => {
                self.get_all_pages(&format!("/users/{org}/repos"), &[])
                    .await?
            }
        };
        Ok(repos.into_iter().map(GhRepo::into_forge_repo).collect())
    }

    async fn create_repo(&self, org: &str, repo: &CreateRepo) -> ForgeResult<ForgeRepo> {
        let body = serde_json::json!({
            "name": repo.name,
            "description": repo.description,
            "private": repo.private,
        });
        let resp = self
            .check_response(
                self.request(reqwest::Method::POST, &format!("/orgs/{org}/repos"))
                    .json(&body)
                    .send()
                    .await
                    .map_err(reqwest_err)?,
            )
            .await?;
        let created: GhRepo = resp.json().await.map_err(reqwest_err)?;
        Ok(created.into_forge_repo())
    }

    async fn create_webhook(
        &self,
        owner: &str,
        repo: &str,
        hook: &CreateWebhook,
    ) -> ForgeResult<ForgeWebhook> {
        let mut config = serde_json::Map::new();
        config.insert("url".into(), serde_json::Value::String(hook.url.clone()));
        config.insert(
            "content_type".into(),
            serde_json::Value::String("json".into()),
        );
        if let Some(s) = &hook.secret {
            config.insert("secret".into(), serde_json::Value::String(s.clone()));
        }
        let body = serde_json::json!({
            "config": config,
            "events": hook.events,
            "active": true,
        });
        let resp = self
            .check_response(
                self.request(
                    reqwest::Method::POST,
                    &format!("/repos/{owner}/{repo}/hooks"),
                )
                .json(&body)
                .send()
                .await
                .map_err(reqwest_err)?,
            )
            .await?;
        let created: GhWebhook = resp.json().await.map_err(reqwest_err)?;
        Ok(ForgeWebhook {
            id: created.id,
            url: hook.url.clone(),
            events: hook.events.clone(),
            active: created.active,
        })
    }
}

// ── GitHub API response types ───────────────────────────────────────────────

#[derive(Deserialize)]
struct GhIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    state: String,
    #[serde(default)]
    labels: Vec<GhLabel>,
    milestone: Option<GhMilestone>,
    #[serde(default)]
    assignees: Vec<GhUser>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
    html_url: String,
    pull_request: Option<serde_json::Value>,
}

impl GhIssue {
    fn into_forge_issue(self) -> ForgeIssue {
        ForgeIssue {
            number: self.number,
            title: self.title,
            body: self.body.unwrap_or_default(),
            state: if self.state == "closed" {
                IssueState::Closed
            } else {
                IssueState::Open
            },
            labels: self.labels.into_iter().map(|l| l.name).collect(),
            milestone: self.milestone.map(|m| m.title),
            assignees: self.assignees.into_iter().map(|u| u.login).collect(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            closed_at: self.closed_at,
            url: self.html_url,
        }
    }
}

#[derive(Deserialize)]
struct GhLabel {
    name: String,
    color: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct GhMilestone {
    title: String,
    description: Option<String>,
    state: String,
    due_on: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct GhUser {
    login: String,
}

#[derive(Deserialize)]
struct GhRepo {
    name: String,
    full_name: String,
    #[serde(default)]
    description: Option<String>,
    default_branch: String,
    clone_url: String,
    ssh_url: String,
    html_url: String,
    private: bool,
    fork: bool,
    archived: bool,
    updated_at: DateTime<Utc>,
}

impl GhRepo {
    fn into_forge_repo(self) -> ForgeRepo {
        ForgeRepo {
            name: self.name,
            full_name: self.full_name,
            description: self.description.unwrap_or_default(),
            default_branch: self.default_branch,
            clone_url: self.clone_url,
            ssh_url: self.ssh_url,
            html_url: self.html_url,
            private: self.private,
            fork: self.fork,
            archived: self.archived,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Deserialize)]
struct GhWebhook {
    id: u64,
    active: bool,
}
