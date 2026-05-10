//! Token resolution.
//!
//! flynt-forge does not own its own secret store. Callers (typically the
//! flynt agent extension) pass in a [`TokenResolver`] that wraps whatever
//! secret-resolution path is appropriate for the runtime — usually
//! omegon's `SecretsManager::resolve(name)`. For tests / one-off uses,
//! [`StaticToken`] just hands back the same string every time.
//!
//! ## Sync-only contract — important
//!
//! [`TokenResolver::resolve`] is **synchronous**. It is called inside
//! request-building code (off any `await` point) so client construction
//! and per-request header-stamping stay infallible and non-blocking.
//!
//! This has consequences for any backend that *needs* async I/O to
//! produce a token (HashiCorp Vault, OAuth refresh, etc.). Three
//! supported wiring patterns:
//!
//! 1. **Cache-warming** (recommended for omegon): call
//!    `SecretsManager::preflight_session_cache_async([...])` at startup
//!    so vault-backed and other async secrets are already in the sync
//!    cache by the time `resolve()` is called. This is what flynt-agent
//!    does at extension boot.
//!
//! 2. **Pre-resolved**: do the async resolution yourself once and pass
//!    the literal value via [`StaticToken::new`]. Fine for short-lived
//!    one-off operations.
//!
//! 3. **Sync-only backends** (env vars, OS keyring): `resolve()` works
//!    out of the box without any preflight.
//!
//! What does **not** work: handing in a closure that itself calls an
//! async secret backend at request time. Such a closure has nowhere
//! to `.await` and will return `None`, leading to silent unauthenticated
//! requests against forges that may then 401 or rate-limit.
//!
//! Why a trait and not a closure: forge clients store the resolver on
//! `&self` and resolve per-request so token rotation works; a
//! `Box<dyn TokenResolver>` is a slightly better fit than
//! `Box<dyn Fn() -> ...>` for that storage shape, but both are
//! supported (closures auto-impl the trait — see the impl below).

use std::sync::Arc;

/// Resolves a token used for forge authentication. Returns `None` when
/// no token is configured (or hasn't been warmed yet, see module doc) —
/// the client should fall back to anonymous requests, useful for
/// public-only operations and for keeping client construction
/// infallible.
pub trait TokenResolver: Send + Sync {
    /// Resolve the token. Each call may return a different value if the
    /// underlying secret rotates, so clients call this per-request
    /// rather than caching at construction time.
    fn resolve(&self) -> Option<String>;
}

impl<F> TokenResolver for F
where
    F: Fn() -> Option<String> + Send + Sync,
{
    fn resolve(&self) -> Option<String> {
        (self)()
    }
}

/// A resolver that always returns the same token. Mainly for tests and
/// for the rare "we already have the PAT in hand" case.
#[derive(Debug, Clone)]
pub struct StaticToken {
    token: Option<String>,
}

impl StaticToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self { token: Some(token.into()) }
    }

    /// Anonymous resolver — returns no token. Use for public-only flows.
    pub fn anonymous() -> Self {
        Self { token: None }
    }
}

impl TokenResolver for StaticToken {
    fn resolve(&self) -> Option<String> {
        self.token.clone()
    }
}

/// Type-erased handle so callers can swap resolvers freely without
/// generics propagating through every client.
pub type SharedTokenResolver = Arc<dyn TokenResolver>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_token_returns_value() {
        let r = StaticToken::new("ghp_xyz");
        assert_eq!(r.resolve().as_deref(), Some("ghp_xyz"));
    }

    #[test]
    fn anonymous_returns_none() {
        let r = StaticToken::anonymous();
        assert!(r.resolve().is_none());
    }

    #[test]
    fn closure_implements_resolver() {
        let r: Box<dyn TokenResolver> = Box::new(|| Some("from-closure".to_string()));
        assert_eq!(r.resolve().as_deref(), Some("from-closure"));
    }
}
