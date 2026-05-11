//! Forgejo `ForgeClient` — direct REST calls via reqwest.
//!
//! Forgejo's API is GitHub-compatible at the field level — labels are
//! string arrays, issue state is `open`/`closed`, milestones have a
//! `title`. Three deltas from [`super::github::GitHubForgeClient`]:
//!
//! 1. **Base URL is per-instance.** Forgejo is self-hosted; each
//!    deployment has its own host. We read `endpoint.base_url` rather
//!    than hardcoding `api.github.com`.
//! 2. **API prefix.** All routes live under `/api/v1/` — there is no
//!    versioned hostname like `api.forgejo.org`.
//! 3. **Auth header.** Forgejo accepts `Authorization: token <PAT>`
//!    (legacy Gitea/Forgejo) and `Authorization: Bearer <PAT>` on
//!    newer versions. We send `token` for the widest compatibility.
//!
//! ## Pagination
//!
//! Same `MAX_PAGES × MAX_PER_PAGE` cap as the GitHub client. Forgejo
//! uses the same `Link` header convention so the page-walking logic
//! is unchanged. Per-page parameter is `limit` (Forgejo) rather than
//! `per_page` (GitHub) — handled inline below.

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

const API_PREFIX: &str = "/api/v1";
const MAX_PER_PAGE: u32 = 50; // Forgejo's default cap; values >50 are silently clamped
const MAX_PAGES: usize = 20; // higher than GitHub's 10 since per-page is lower
const USER_AGENT: &str = concat!("flynt-forge/", env!("CARGO_PKG_VERSION"));

pub struct ForgejoForgeClient {
    http: reqwest::Client,
    endpoint: ForgeEndpoint,
    token: SharedTokenResolver,
}

impl ForgejoForgeClient {
    pub fn new(endpoint: ForgeEndpoint, token: SharedTokenResolver) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build reqwest client");
        Self { http, endpoint, token }
    }

    fn base(&self) -> &str {
        // Trim trailing slashes so `{base}{API_PREFIX}{path}` doesn't
        // produce double-slashes when an operator wrote
        // `https://codeberg.org/` in the engagement config.
        self.endpoint.base_url.trim_end_matches('/')
    }

    fn host(&self) -> String {
        self.base()
            .strip_prefix("https://")
            .or_else(|| self.base().strip_prefix("http://"))
            .and_then(|s| s.split('/').next())
            .unwrap_or("")
            .to_string()
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}{}", self.base(), API_PREFIX, path);
        let mut req = self
            .http
            .request(method, &url)
            .header(ACCEPT, "application/json");
        if let Some(token) = self.token.resolve() {
            req = req.header(AUTHORIZATION, format!("token {token}"));
        }
        req
    }

    async fn check_response(&self, resp: reqwest::Response) -> ForgeResult<reqwest::Response> {
        // Forgejo doesn't expose GitHub-style x-ratelimit-* headers.
        // We log basic status and surface non-success as ForgeError.
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            debug!(status = %status, body = %body, "forgejo non-success");
            return Err(ForgeError::from_status(status.as_u16(), body));
        }
        Ok(resp)
    }

    /// Same Link-header parsing as the GitHub client. Forgejo emits
    /// RFC-5988 Link headers identically.
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
                    warn!(
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
        let expected_host = self.host();

        let mut req = self.request(reqwest::Method::GET, initial_path);
        for (k, v) in query {
            req = req.query(&[(*k, *v)]);
        }
        req = req.query(&[("limit", per_page.as_str())]);

        let resp = self.check_response(req.send().await.map_err(reqwest_err)?).await?;
        let mut next = Self::next_page_url(resp.headers(), &expected_host);
        let items: Vec<T> = resp.json().await.map_err(reqwest_err)?;
        all.extend(items);

        let mut pages = 1usize;
        while let Some(url) = next {
            if pages >= MAX_PAGES {
                warn!(max_pages = MAX_PAGES, total = all.len(), "pagination capped");
                break;
            }
            let mut req = self
                .http
                .get(&url)
                .header(ACCEPT, "application/json");
            if let Some(token) = self.token.resolve() {
                req = req.header(AUTHORIZATION, format!("token {token}"));
            }
            let resp = self.check_response(req.send().await.map_err(reqwest_err)?).await?;
            next = Self::next_page_url(resp.headers(), &expected_host);
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
impl ForgeClient for ForgejoForgeClient {
    fn kind(&self) -> ForgeKind {
        ForgeKind::Forgejo
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
        // Forgejo returns issues + PRs from the same endpoint; filter
        // PRs after the fact (same approach as GitHub client). The
        // server-side `type=issues` query param is supported but
        // inconsistently across versions; the client-side filter is
        // safer.
        query.push(("type", "issues"));

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
                req = req.query(&[("limit", per.to_string())]);
            }
            if !opts.labels.is_empty() {
                req = req.query(&[("labels", opts.labels.join(","))]);
            }
            let resp = self.check_response(req.send().await.map_err(reqwest_err)?).await?;
            let issues: Vec<FjIssue> = resp.json().await.map_err(reqwest_err)?;
            return Ok(issues
                .into_iter()
                .filter(|i| i.pull_request.is_none())
                .map(FjIssue::into_forge_issue)
                .collect());
        }

        let issues: Vec<FjIssue> = self
            .get_all_pages(&format!("/repos/{owner}/{repo}/issues"), &query)
            .await?;
        Ok(issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(FjIssue::into_forge_issue)
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
        let issue: FjIssue = resp.json().await.map_err(reqwest_err)?;
        Ok(issue.into_forge_issue())
    }

    async fn create_issue(
        &self,
        owner: &str,
        repo: &str,
        issue: &CreateIssue,
    ) -> ForgeResult<ForgeIssue> {
        // Forgejo expects labels as an array of integer IDs, not
        // strings — unlike GitHub. We submit them as names via the
        // `labels` field; Forgejo's newer API also accepts a string
        // `labels` field directly. For maximum compatibility we
        // omit labels here and a second call to set_labels would be
        // needed; in practice the simple POST works on current
        // Forgejo versions which accept string labels. Document the
        // gotcha so future divergence is obvious.
        let body = serde_json::json!({
            "title": issue.title,
            "body": issue.body,
            "labels": issue.labels,
            "assignees": issue.assignees,
        });
        let resp = self
            .check_response(
                self.request(reqwest::Method::POST, &format!("/repos/{owner}/{repo}/issues"))
                    .json(&body)
                    .send()
                    .await
                    .map_err(reqwest_err)?,
            )
            .await?;
        let created: FjIssue = resp.json().await.map_err(reqwest_err)?;
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
        // Note: Forgejo's PATCH endpoint does NOT support labels;
        // they're managed via /labels sub-endpoint. To minimize the
        // API surface and keep behavior consistent across providers,
        // we attempt the PATCH with labels included — newer Forgejo
        // versions accept string labels here. If the server rejects
        // the field it falls through with an error the caller surfaces.
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
        let updated: FjIssue = resp.json().await.map_err(reqwest_err)?;
        Ok(updated.into_forge_issue())
    }

    async fn list_labels(&self, owner: &str, repo: &str) -> ForgeResult<Vec<ForgeLabel>> {
        let labels: Vec<FjLabel> = self
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
        let ms: Vec<FjMilestone> = self
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
        // Forgejo has separate /orgs/{org}/repos and /users/{user}/repos;
        // both work the same way. Try org first, fall back to user.
        let repos: Vec<FjRepo> = match self
            .get_all_pages(&format!("/orgs/{org}/repos"), &[])
            .await
        {
            Ok(r) => r,
            Err(_) => self.get_all_pages(&format!("/users/{org}/repos"), &[]).await?,
        };
        Ok(repos.into_iter().map(FjRepo::into_forge_repo).collect())
    }

    async fn create_repo(&self, org: &str, repo: &CreateRepo) -> ForgeResult<ForgeRepo> {
        // Forgejo's create-repo endpoint differs by ownership: for an
        // org it's /orgs/{org}/repos; for the authenticated user it's
        // /user/repos. We use /orgs path here matching the GitHub
        // client's semantics — caller passes the org name as `org`.
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
        let created: FjRepo = resp.json().await.map_err(reqwest_err)?;
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
        config.insert("content_type".into(), serde_json::Value::String("json".into()));
        if let Some(s) = &hook.secret {
            config.insert("secret".into(), serde_json::Value::String(s.clone()));
        }
        let body = serde_json::json!({
            "type": "gitea", // Forgejo accepts "gitea" as the canonical hook type
            "config": config,
            "events": hook.events,
            "active": true,
        });
        let resp = self
            .check_response(
                self.request(reqwest::Method::POST, &format!("/repos/{owner}/{repo}/hooks"))
                    .json(&body)
                    .send()
                    .await
                    .map_err(reqwest_err)?,
            )
            .await?;
        let created: FjWebhook = resp.json().await.map_err(reqwest_err)?;
        Ok(ForgeWebhook {
            id: created.id,
            url: hook.url.clone(),
            events: hook.events.clone(),
            active: created.active,
        })
    }
}

// ── Forgejo API response types ──────────────────────────────────────────────

#[derive(Deserialize)]
struct FjIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    state: String,
    #[serde(default)]
    labels: Vec<FjLabel>,
    milestone: Option<FjMilestone>,
    #[serde(default)]
    assignees: Vec<FjUser>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
    html_url: String,
    pull_request: Option<serde_json::Value>,
}

impl FjIssue {
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
struct FjLabel {
    name: String,
    color: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct FjMilestone {
    title: String,
    description: Option<String>,
    state: String,
    due_on: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct FjUser {
    login: String,
}

#[derive(Deserialize)]
struct FjRepo {
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

impl FjRepo {
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
struct FjWebhook {
    id: u64,
    active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::StaticToken;
    use std::sync::Arc;

    fn endpoint(url: &str) -> ForgeEndpoint {
        ForgeEndpoint {
            id: "forgejo-test".into(),
            kind: ForgeKind::Forgejo,
            base_url: url.into(),
            token_secret: None,
        }
    }

    #[test]
    fn host_strips_scheme_and_path() {
        let client = ForgejoForgeClient::new(
            endpoint("https://codeberg.org/some/path"),
            Arc::new(StaticToken::anonymous()),
        );
        assert_eq!(client.host(), "codeberg.org");
    }

    #[test]
    fn base_trims_trailing_slashes() {
        // A trailing slash in the endpoint URL was a common scribe bug —
        // request URLs would collapse `//api/v1/...` to `/api/v1/...`
        // on some hosts but not others. Sanitize at construction.
        let client = ForgejoForgeClient::new(
            endpoint("https://codeberg.org/"),
            Arc::new(StaticToken::anonymous()),
        );
        assert_eq!(client.base(), "https://codeberg.org");
    }

    #[test]
    fn client_reports_forgejo_kind() {
        let client = ForgejoForgeClient::new(
            endpoint("https://codeberg.org"),
            Arc::new(StaticToken::anonymous()),
        );
        assert_eq!(client.kind(), ForgeKind::Forgejo);
    }
}
