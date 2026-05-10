//! Token resolution.
//!
//! flynt-forge does not own its own secret store. Callers (typically the
//! flynt agent extension) pass in a [`TokenResolver`] that wraps whatever
//! secret-resolution path is appropriate for the runtime — usually
//! omegon's `SecretsManager::resolve(name)`. For tests / one-off uses,
//! [`StaticToken`] just hands back the same string every time.
//!
//! Why a trait and not a closure: forge clients store the resolver on
//! `&self` and call it potentially across `await` boundaries when token
//! rotation matters; a `Box<dyn TokenResolver>` is a better fit than
//! `Box<dyn Fn() -> ...>`.

use std::sync::Arc;

/// Resolves a named secret to a token string. None means "no token,
/// the client should fall back to anonymous requests" — useful for
/// public-only operations and for keeping client construction
/// infallible.
pub trait TokenResolver: Send + Sync {
    /// Resolve the token associated with this resolver. The name is
    /// the resolver's own configured secret name (e.g. `GITHUB_TOKEN`);
    /// callers don't pass the name in.
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
