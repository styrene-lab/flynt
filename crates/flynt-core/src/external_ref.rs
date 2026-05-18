//! External reference rendering — smart badges for recognized URL patterns.
//!
//! Any URL in the app (notes, tasks, design nodes, agent messages) can be
//! rendered as a rich badge when the URL matches a known provider pattern.
//! No API calls — badges use public badge services (shields.io, badgen.net).
//!
//! Extension point: the `RefRenderer` trait allows plugins to add richer
//! integrations (API-backed title fetch, status sync) in the future.

use std::borrow::Cow;

/// A recognized external reference with rendering metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalRef {
    /// The original URL.
    pub url: String,
    /// Human-readable label (e.g., "org/repo#123").
    pub label: String,
    /// Which provider was matched.
    pub provider: Provider,
    /// Badge image URL (shields.io or similar), if available.
    pub badge_url: Option<String>,
    /// Provider icon/favicon URL.
    pub icon_url: Option<String>,
}

/// Known external providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    GitHub,
    GitLab,
    Linear,
    Notion,
    Jira,
    AzureDevOps,
    Forgejo,
    Generic,
}

impl Provider {
    pub fn name(&self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
            Self::Linear => "Linear",
            Self::Notion => "Notion",
            Self::Jira => "Jira",
            Self::AzureDevOps => "Azure DevOps",
            Self::Forgejo => "Forgejo",
            Self::Generic => "Link",
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            Self::GitHub => "ref-github",
            Self::GitLab => "ref-gitlab",
            Self::Linear => "ref-linear",
            Self::Notion => "ref-notion",
            Self::Jira => "ref-jira",
            Self::AzureDevOps => "ref-ado",
            Self::Forgejo => "ref-forgejo",
            Self::Generic => "ref-generic",
        }
    }
}

/// Parse a URL into a recognized external reference.
/// Returns `None` for unrecognized URLs (they render as plain links).
pub fn parse_ref(url: &str) -> ExternalRef {
    // GitHub: issues, PRs, discussions
    if let Some(r) = parse_github(url) {
        return r;
    }
    // GitLab: issues, MRs
    if let Some(r) = parse_gitlab(url) {
        return r;
    }
    // Linear: issues
    if let Some(r) = parse_linear(url) {
        return r;
    }
    // Notion: pages
    if let Some(r) = parse_notion(url) {
        return r;
    }
    // Jira: issues
    if let Some(r) = parse_jira(url) {
        return r;
    }
    // Azure DevOps: work items
    if let Some(r) = parse_ado(url) {
        return r;
    }
    // Forgejo/Gitea: issues, PRs (self-hosted, configurable domain)
    if let Some(r) = parse_forgejo(url) {
        return r;
    }

    // Generic fallback
    let label = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_string();
    ExternalRef {
        url: url.to_string(),
        label: truncate(&label, 60),
        provider: Provider::Generic,
        badge_url: None,
        icon_url: None,
    }
}

/// Render an external ref as an HTML badge/chip for inline display.
pub fn render_html(ext_ref: &ExternalRef) -> String {
    let css = ext_ref.provider.css_class();
    let provider = ext_ref.provider.name();
    let url = html_escape(&ext_ref.url);
    let label = html_escape(&ext_ref.label);

    if let Some(ref badge) = ext_ref.badge_url {
        let badge = html_escape(badge);
        format!(
            r#"<a href="{url}" class="external-ref {css}" target="_blank" title="{provider}: {label}"><img src="{badge}" alt="{label}" class="external-ref-badge" /><span class="external-ref-label">{label}</span></a>"#
        )
    } else {
        format!(
            r#"<a href="{url}" class="external-ref {css}" target="_blank" title="{provider}: {label}"><span class="external-ref-provider">{provider}</span><span class="external-ref-label">{label}</span></a>"#
        )
    }
}

// ── Provider parsers ────────────────────────────────────────────────────────

fn parse_github(url: &str) -> Option<ExternalRef> {
    // https://github.com/org/repo/issues/123
    // https://github.com/org/repo/pull/456
    // https://github.com/org/repo/discussions/789
    let path = url.strip_prefix("https://github.com/")?;
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 4 {
        return None;
    }

    let org = parts[0];
    let repo = parts[1];
    let kind = parts[2];
    let number = parts[3].split('?').next().unwrap_or(parts[3]);

    let (label, badge_url) = match kind {
        "issues" => (
            format!("{org}/{repo}#{number}"),
            Some(format!(
                "https://img.shields.io/github/issues/detail/state/{org}/{repo}/{number}?style=flat-square&label="
            )),
        ),
        "pull" => (
            format!("{org}/{repo}#{number}"),
            Some(format!(
                "https://img.shields.io/github/pulls/detail/state/{org}/{repo}/{number}?style=flat-square&label="
            )),
        ),
        "discussions" => (format!("{org}/{repo} discussion #{number}"), None),
        _ => return None,
    };

    Some(ExternalRef {
        url: url.to_string(),
        label,
        provider: Provider::GitHub,
        badge_url,
        icon_url: Some("https://github.githubassets.com/favicons/favicon.svg".into()),
    })
}

fn parse_gitlab(url: &str) -> Option<ExternalRef> {
    // https://gitlab.com/org/repo/-/issues/123
    // https://gitlab.com/org/repo/-/merge_requests/456
    let path = url.strip_prefix("https://gitlab.com/")?;
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 5 || parts[2] != "-" {
        return None;
    }

    let org = parts[0];
    let repo = parts[1];
    let kind = parts[3];
    let number = parts[4].split('?').next().unwrap_or(parts[4]);

    let label = match kind {
        "issues" => format!("{org}/{repo}#{number}"),
        "merge_requests" => format!("{org}/{repo}!{number}"),
        _ => return None,
    };

    Some(ExternalRef {
        url: url.to_string(),
        label,
        provider: Provider::GitLab,
        badge_url: None,
        icon_url: None,
    })
}

fn parse_linear(url: &str) -> Option<ExternalRef> {
    // https://linear.app/ORG/issue/ABC-123
    let path = url.strip_prefix("https://linear.app/")?;
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 3 || parts[1] != "issue" {
        return None;
    }

    Some(ExternalRef {
        url: url.to_string(),
        label: parts[2].to_string(),
        provider: Provider::Linear,
        badge_url: None,
        icon_url: None,
    })
}

fn parse_notion(url: &str) -> Option<ExternalRef> {
    // https://www.notion.so/Page-Title-abc123def456
    let path = url
        .strip_prefix("https://www.notion.so/")
        .or_else(|| url.strip_prefix("https://notion.so/"))?;
    let page = path.split('?').next().unwrap_or(path);
    let label = page.replace('-', " ");
    // Trim the 32-char hex ID suffix if present
    let label = if label.len() > 32 {
        let trimmed = &label[..label.len() - 33];
        trimmed.trim().to_string()
    } else {
        label
    };

    Some(ExternalRef {
        url: url.to_string(),
        label: truncate(&label, 50),
        provider: Provider::Notion,
        badge_url: None,
        icon_url: None,
    })
}

fn parse_jira(url: &str) -> Option<ExternalRef> {
    // https://org.atlassian.net/browse/PROJ-123
    if !url.contains(".atlassian.net/browse/") {
        return None;
    }
    let key = url.rsplit('/').next()?;
    if !key.contains('-') {
        return None;
    }

    Some(ExternalRef {
        url: url.to_string(),
        label: key.to_string(),
        provider: Provider::Jira,
        badge_url: None,
        icon_url: None,
    })
}

fn parse_ado(url: &str) -> Option<ExternalRef> {
    // https://dev.azure.com/org/project/_workitems/edit/12345
    let path = url.strip_prefix("https://dev.azure.com/")?;
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 4 || !parts.contains(&"_workitems") {
        return None;
    }

    let id = parts.last()?;

    Some(ExternalRef {
        url: url.to_string(),
        label: format!("#{id}"),
        provider: Provider::AzureDevOps,
        badge_url: None,
        icon_url: None,
    })
}

fn parse_forgejo(url: &str) -> Option<ExternalRef> {
    // Forgejo/Gitea instances are self-hosted, so we can't match on domain.
    // Instead, look for the Forgejo/Gitea URL pattern: /org/repo/issues/N or /org/repo/pulls/N
    // with a path that contains "/api/v1/" marker or specific Forgejo patterns.
    //
    // For now, match URLs that have been explicitly tagged with a forge: query param
    // (e.g., ?forge=forgejo) or that contain /src/ (Gitea/Forgejo file browser pattern).
    // Full integration will use scribe's forge registry to resolve known Forgejo domains.
    let path = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;

    // Check for ?forge=forgejo tag (explicit tagging by scribe)
    if !url.contains("forge=forgejo") && !url.contains("forge=gitea") {
        return None;
    }

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 4 {
        return None;
    }

    // parts[0] = domain, parts[1] = org, parts[2] = repo, parts[3] = kind
    let _domain = parts[0];
    let org = parts[1];
    let repo = parts[2];
    let kind = parts[3].split('?').next().unwrap_or(parts[3]);

    if parts.len() < 5 {
        return None;
    }
    let number = parts[4].split('?').next().unwrap_or(parts[4]);

    let label = match kind {
        "issues" => format!("{org}/{repo}#{number}"),
        "pulls" => format!("{org}/{repo}#{number}"),
        _ => return None,
    };

    Some(ExternalRef {
        url: url.to_string(),
        label,
        provider: Provider::Forgejo,
        badge_url: None,
        icon_url: None,
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn html_escape(s: &str) -> Cow<'_, str> {
    if s.contains('&') || s.contains('<') || s.contains('>') || s.contains('"') {
        Cow::Owned(
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;"),
        )
    } else {
        Cow::Borrowed(s)
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_issue() {
        let r = parse_ref("https://github.com/styrene-lab/flynt/issues/42");
        assert_eq!(r.provider, Provider::GitHub);
        assert_eq!(r.label, "styrene-lab/flynt#42");
        assert!(r.badge_url.is_some());
    }

    #[test]
    fn github_pr() {
        let r = parse_ref("https://github.com/styrene-lab/flynt/pull/1");
        assert_eq!(r.provider, Provider::GitHub);
        assert_eq!(r.label, "styrene-lab/flynt#1");
        assert!(r.badge_url.is_some());
    }

    #[test]
    fn gitlab_issue() {
        let r = parse_ref("https://gitlab.com/org/repo/-/issues/99");
        assert_eq!(r.provider, Provider::GitLab);
        assert_eq!(r.label, "org/repo#99");
    }

    #[test]
    fn gitlab_mr() {
        let r = parse_ref("https://gitlab.com/org/repo/-/merge_requests/5");
        assert_eq!(r.provider, Provider::GitLab);
        assert_eq!(r.label, "org/repo!5");
    }

    #[test]
    fn linear_issue() {
        let r = parse_ref("https://linear.app/styrene/issue/STY-456");
        assert_eq!(r.provider, Provider::Linear);
        assert_eq!(r.label, "STY-456");
    }

    #[test]
    fn jira_issue() {
        let r = parse_ref("https://myorg.atlassian.net/browse/PROJ-789");
        assert_eq!(r.provider, Provider::Jira);
        assert_eq!(r.label, "PROJ-789");
    }

    #[test]
    fn ado_work_item() {
        let r = parse_ref("https://dev.azure.com/org/project/_workitems/edit/12345");
        assert_eq!(r.provider, Provider::AzureDevOps);
        assert_eq!(r.label, "#12345");
    }

    #[test]
    fn notion_page() {
        let r = parse_ref("https://www.notion.so/My-Page-Title-abc123def456789012345678901234");
        assert_eq!(r.provider, Provider::Notion);
        assert!(!r.label.contains("abc123"));
    }

    #[test]
    fn generic_url() {
        let r = parse_ref("https://example.com/some/path");
        assert_eq!(r.provider, Provider::Generic);
        assert_eq!(r.label, "example.com/some/path");
    }

    #[test]
    fn render_github_badge() {
        let r = parse_ref("https://github.com/styrene-lab/flynt/issues/1");
        let html = render_html(&r);
        assert!(html.contains("external-ref"));
        assert!(html.contains("ref-github"));
        assert!(html.contains("img.shields.io"));
        assert!(html.contains("styrene-lab/flynt#1"));
    }

    #[test]
    fn render_generic_no_badge() {
        let r = parse_ref("https://example.com");
        let html = render_html(&r);
        assert!(html.contains("external-ref"));
        assert!(html.contains("ref-generic"));
        assert!(!html.contains("<img"));
    }
}
