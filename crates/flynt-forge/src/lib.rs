//! Forge integration for Flynt.
//!
//! Absorbed from `scribe`. Provides:
//! - REST clients implementing [`styrene_forge::ForgeClient`]
//!   (currently GitHub; Forgejo / GitLab can be added incrementally)
//! - A bidirectional [`sync::SyncEngine`] that diffs forge issues
//!   against local state and emits [`sync::SyncOp`] records
//! - A [`store::SyncStore`] holding `IssueMap` rows so we know which
//!   forge issue corresponds to which flynt task
//! - A pluggable [`auth::TokenResolver`] trait so the agent extension
//!   can wire omegon's `SecretsManager` without flynt-forge taking
//!   a direct dependency on omegon
//!
//! Out of scope (intentionally not ported from scribe):
//! - OAuth2 device flow / token store — use omegon's `SecretsManager`
//!   to resolve a PAT (or any other secret-resolution recipe)
//! - `codex_bridge.rs` and `mapper.rs` — flynt is the project, so forge
//!   issues map directly to `flynt_models::Task`
//! - Git credential helper (RFC 6579) — that ships as a separate
//!   `flynt-git-helper` binary in Phase 6

pub mod auth;
pub mod clients;
pub mod store;
pub mod sync;

// Re-export the styrene-forge contract so callers don't have to add it
// as a separate dep. Anything they need to construct or pattern-match
// against (ForgeIssue, ForgeKind, ForgeEndpoint, etc.) is here.
pub use styrene_forge::{
    CreateIssue, ForgeClient, ForgeEndpoint, ForgeError, ForgeIssue, ForgeKind, ForgeLabel,
    ForgeMilestone, ForgeRepo, ForgeResult, IssueState, ListOpts, UpdateIssue,
};

pub use auth::{StaticToken, TokenResolver};
pub use clients::GitHubForgeClient;
pub use store::{IssueMap, SyncStore};
pub use sync::{SyncEngine, SyncOp, content_hash, issue_hash};
