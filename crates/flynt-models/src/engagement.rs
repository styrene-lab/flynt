//! Engagement model — partnerships, engagements, repo bindings.
//!
//! An [`Engagement`] is a bounded work effort (project, sprint, contract)
//! that spans one or more repos on one or more forges. Engagements group
//! into [`Partnership`]s for long-running relationships.
//!
//! Tasks can carry an `engagement_id` so the kanban can scope by engagement
//! and the agent can list/sync forge issues against the right repo binding.
//!
//! Ported from scribe (now absorbed). Drops codex-specific fields:
//! `RepoBinding` no longer carries `codex_project_id` or vault path
//! overrides — flynt is the vault, so binding lookup happens via
//! `engagement_id` on the task or via project structure.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use styrene_forge::{ForgeEndpoint, ForgeKind};
use uuid::Uuid;

// ── Newtype IDs ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PartnershipId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EngagementId(pub Uuid);

impl PartnershipId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}
impl EngagementId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}
impl Default for PartnershipId { fn default() -> Self { Self::new() } }
impl Default for EngagementId { fn default() -> Self { Self::new() } }

// ── Partnership ─────────────────────────────────────────────────────────────

/// A partnership groups related engagements (e.g., a client, a product line).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Partnership {
    pub id: PartnershipId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Engagements this partnership owns. Stored as IDs; the engagement
    /// records are the source of truth for everything else.
    #[serde(default)]
    pub engagements: Vec<EngagementId>,
    pub created_at: DateTime<Utc>,
}

impl Partnership {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: PartnershipId::new(),
            name: name.into(),
            description: None,
            engagements: Vec::new(),
            created_at: Utc::now(),
        }
    }
}

// ── Engagement ──────────────────────────────────────────────────────────────

/// A bounded work effort spanning one or more repos on a forge.
//
// PartialEq omitted: ForgeEndpoint doesn't impl it, and equality on a
// whole engagement isn't a meaningful op anyway. Compare by `id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Engagement {
    pub id: EngagementId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partnership_id: Option<PartnershipId>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Repos bound to this engagement. The engagement's default
    /// [`ForgeEndpoint`] applies unless a binding overrides it.
    #[serde(default)]
    pub repos: Vec<RepoBinding>,
    pub forge: ForgeEndpoint,
    #[serde(default)]
    pub status: EngagementStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EngagementStatus {
    #[default]
    Active,
    Paused,
    Completed,
    Archived,
}

impl Engagement {
    pub fn new(name: impl Into<String>, forge: ForgeEndpoint) -> Self {
        let now = Utc::now();
        Self {
            id: EngagementId::new(),
            partnership_id: None,
            name: name.into(),
            description: None,
            repos: Vec::new(),
            forge,
            status: EngagementStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == EngagementStatus::Active
    }
}

// ── Repo binding ────────────────────────────────────────────────────────────

/// Binds an engagement to a forge repo.
///
/// Flynt is the vault, so this no longer carries codex-era project /
/// vault path fields. To find tasks linked to a binding, look up tasks
/// with this engagement's `engagement_id` and matching `external_refs`
/// pointing at the binding's repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoBinding {
    /// Forge org / owner.
    pub forge_org: String,
    /// Forge repo name.
    pub forge_repo: String,
    /// Local clone path on disk, if checked out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<PathBuf>,
    /// Sync issues ↔ flynt tasks.
    #[serde(default = "default_true")]
    pub sync_issues: bool,
    /// Sync pull requests.
    #[serde(default)]
    pub sync_prs: bool,
    /// Override the engagement's forge kind for this repo (rare, but
    /// covers a partnership where one repo lives on a different forge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge_kind_override: Option<ForgeKind>,
}

fn default_true() -> bool { true }

impl RepoBinding {
    pub fn new(org: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            forge_org: org.into(),
            forge_repo: repo.into(),
            local_path: None,
            sync_issues: true,
            sync_prs: false,
            forge_kind_override: None,
        }
    }

    /// Full "org/repo" identifier.
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.forge_org, self.forge_repo)
    }

    /// Stable, deterministic ID for the binding — used to anchor task
    /// external_refs that point at this repo. Derived from the full
    /// name via UUIDv5 so a binding always resolves to the same ID
    /// across reloads.
    pub fn stable_id(&self) -> Uuid {
        Uuid::new_v5(&Uuid::NAMESPACE_URL, self.full_name().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint() -> ForgeEndpoint {
        ForgeEndpoint {
            id: "github".into(),
            kind: ForgeKind::GitHub,
            base_url: "https://api.github.com".into(),
            token_secret: None,
        }
    }

    #[test]
    fn partnership_starts_empty() {
        let p = Partnership::new("Acme Corp");
        assert_eq!(p.name, "Acme Corp");
        assert!(p.engagements.is_empty());
        assert!(p.description.is_none());
    }

    #[test]
    fn engagement_defaults_to_active() {
        let e = Engagement::new("Q2 Migration", endpoint());
        assert!(e.is_active());
        assert!(e.partnership_id.is_none());
        assert!(e.repos.is_empty());
    }

    #[test]
    fn repo_binding_full_name() {
        let b = RepoBinding::new("anthropics", "claude-code");
        assert_eq!(b.full_name(), "anthropics/claude-code");
        assert!(b.sync_issues);
        assert!(!b.sync_prs);
    }

    #[test]
    fn repo_binding_stable_id_is_deterministic() {
        let b1 = RepoBinding::new("org", "repo");
        let b2 = RepoBinding::new("org", "repo");
        assert_eq!(b1.stable_id(), b2.stable_id());
        let b3 = RepoBinding::new("org", "different");
        assert_ne!(b1.stable_id(), b3.stable_id());
    }

    #[test]
    fn engagement_status_default_is_active() {
        assert_eq!(EngagementStatus::default(), EngagementStatus::Active);
    }

    #[test]
    fn engagement_round_trips_through_toml() {
        let e = Engagement::new("Test", endpoint());
        let s = toml::to_string(&e).unwrap();
        let parsed: Engagement = toml::from_str(&s).unwrap();
        assert_eq!(parsed.id, e.id);
        assert_eq!(parsed.name, "Test");
        assert_eq!(parsed.status, EngagementStatus::Active);
    }

    #[test]
    fn repo_binding_omits_codex_fields() {
        // Sanity check that we dropped codex_project_id /
        // codex_vault_root / codex_project_sub_path.
        let b = RepoBinding::new("o", "r");
        let s = toml::to_string(&b).unwrap();
        assert!(!s.contains("codex"));
    }
}
