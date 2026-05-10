//! Git credential helper — RFC 6579 minimum.
//!
//! Git invokes credential helpers as separate processes via the
//! `credential.helper` config:
//!
//! ```text
//! [credential "https://github.com"]
//!     helper = !flynt-git-helper
//! ```
//!
//! The helper reads a key=value record from stdin (terminated by a
//! blank line), and for `get` writes a `username=...\npassword=...\n`
//! record back. `store` and `erase` are accepted but no-ops for now —
//! we don't persist tokens; the token comes from the environment
//! (set by the operator's shell profile or by omegon's secrets
//! hydration). That's the minimum that makes `git push` /
//! `git clone` against an authenticated forge work without prompting.
//!
//! Looks up tokens by host, in order:
//!   1. `FLYNT_<HOST_UPPER>_TOKEN` — e.g. FLYNT_GITHUB_COM_TOKEN
//!     Dots and hyphens in the host become underscores, so
//!     `git-codecommit.us-east-1.amazonaws.com` resolves to
//!     `FLYNT_GIT_CODECOMMIT_US_EAST_1_AMAZONAWS_COM_TOKEN`.
//!   2. for github.com specifically, `FLYNT_GITHUB_TOKEN` (matches
//!      what flynt-agent's bootstrap_secrets path uses)
//!   3. for github.com, `GITHUB_TOKEN` (CI convention)
//!
//! Only HTTPS is served; ssh has its own auth path. Username is
//! returned as `x-access-token` — GitHub's documented PAT-over-HTTPS
//! convention. **This is github-correct only.** GitLab PAT auth
//! conventionally expects `oauth2`; Forgejo accepts the literal
//! username. For now this helper is github-shaped; add a per-host
//! username override if you wire up a non-github forge that needs
//! it.
//!
//! `store` and `erase` are accepted but no-ops — we don't persist
//! tokens. If git asks the helper to ERASE a rejected token, we say
//! OK and the next request retries with the same env-derived token,
//! producing the same auth failure. The fix is to update the env
//! var; this is documented behaviour, not a bug.

use anyhow::Result;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

fn main() -> Result<()> {
    let op = std::env::args().nth(1).unwrap_or_default();
    match op.as_str() {
        "get" => handle_get(),
        // store / erase: accept the input record (so git doesn't
        // complain about a closed pipe) but do nothing. Persistence
        // is the operator's shell profile, not our concern.
        "store" | "erase" => {
            let _ = parse_credential_input(&mut io::stdin().lock());
            Ok(())
        }
        _ => Ok(()),
    }
}

fn handle_get() -> Result<()> {
    let fields = parse_credential_input(&mut io::stdin().lock())?;

    // Only HTTPS — ssh authenticates differently.
    if fields.get("protocol").map(String::as_str) != Some("https") {
        return Ok(());
    }
    let Some(host) = fields.get("host") else { return Ok(()); };

    let Some(token) = lookup_token(host) else { return Ok(()); };

    let mut out = io::stdout().lock();
    writeln!(out, "username=x-access-token")?;
    writeln!(out, "password={token}")?;
    // Trailing blank line terminates the protocol record.
    writeln!(out)?;
    Ok(())
}

/// Look up a token for `host`, trying each env-var convention in
/// order. Returns the first non-empty match.
pub(crate) fn lookup_token(host: &str) -> Option<String> {
    let host_only = host.split(':').next().unwrap_or(host);

    // Per-host explicit env: dots and hyphens → underscores, then
    // uppercased. AWS CodeCommit, hyphenated subdomains, etc., need
    // the hyphen substitution to produce a legal env var name.
    let host_env = format!(
        "FLYNT_{}_TOKEN",
        host_only.replace(['.', '-'], "_").to_uppercase(),
    );
    if let Ok(v) = std::env::var(&host_env)
        && !v.is_empty()
    {
        return Some(v);
    }

    // GitHub-specific aliases — match flynt-agent's secret name and
    // the standard CI convention.
    if host_only == "github.com" {
        for name in ["FLYNT_GITHUB_TOKEN", "GITHUB_TOKEN"] {
            if let Ok(v) = std::env::var(name)
                && !v.is_empty()
            {
                return Some(v);
            }
        }
    }
    None
}

/// Parse git-credential protocol input: key=value lines, blank line
/// terminates. Mirrors RFC 6579's helper protocol.
pub(crate) fn parse_credential_input(reader: &mut dyn BufRead) -> Result<HashMap<String, String>> {
    let mut fields = HashMap::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.trim().split_once('=') {
            fields.insert(k.to_string(), v.to_string());
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Process-wide env access serialization. cargo test runs cases
    /// in parallel by default; without this guard, two with_env blocks
    /// can race on the GITHUB_TOKEN/FLYNT_GITHUB_TOKEN keys this file
    /// exercises and one will see the other's intermediate state.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<F: FnOnce()>(pairs: &[(&str, Option<&str>)], f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot: Vec<(String, Option<String>)> = pairs.iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        // SAFETY: ENV_LOCK serializes all env mutation in this module.
        unsafe {
            for (k, v) in pairs {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
        f();
        unsafe {
            for (k, prior) in &snapshot {
                match prior {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    #[test]
    fn parse_strips_blank_terminator() {
        let mut r = std::io::Cursor::new("protocol=https\nhost=github.com\n\nignored=true\n");
        let f = parse_credential_input(&mut r).unwrap();
        assert_eq!(f.get("protocol").unwrap(), "https");
        assert_eq!(f.get("host").unwrap(), "github.com");
        assert!(!f.contains_key("ignored"));
    }

    #[test]
    fn lookup_returns_per_host_env() {
        with_env(
            &[
                ("FLYNT_FORGEJO_EXAMPLE_COM_TOKEN", Some("forge-token")),
                ("FLYNT_GITHUB_TOKEN", None),
                ("GITHUB_TOKEN", None),
            ],
            || {
                assert_eq!(
                    lookup_token("forgejo.example.com").as_deref(),
                    Some("forge-token")
                );
            },
        );
    }

    #[test]
    fn lookup_handles_hyphenated_host() {
        // AWS CodeCommit + many self-hosted forges have hyphens.
        // Our env-var derivation must convert '-' as well as '.'.
        with_env(
            &[
                ("FLYNT_GIT_CODECOMMIT_US_EAST_1_AMAZONAWS_COM_TOKEN", Some("aws-token")),
            ],
            || {
                assert_eq!(
                    lookup_token("git-codecommit.us-east-1.amazonaws.com").as_deref(),
                    Some("aws-token")
                );
            },
        );
    }

    #[test]
    fn lookup_strips_port_before_resolving() {
        with_env(
            &[
                ("FLYNT_LOCALHOST_TOKEN", Some("local-token")),
            ],
            || {
                assert_eq!(lookup_token("localhost:3000").as_deref(), Some("local-token"));
            },
        );
    }

    #[test]
    fn lookup_falls_through_github_aliases() {
        with_env(
            &[
                ("FLYNT_GITHUB_COM_TOKEN", None),
                ("FLYNT_GITHUB_TOKEN", None),
                ("GITHUB_TOKEN", Some("ci-token")),
            ],
            || {
                assert_eq!(lookup_token("github.com").as_deref(), Some("ci-token"));
            },
        );
        with_env(
            &[
                ("FLYNT_GITHUB_COM_TOKEN", None),
                ("FLYNT_GITHUB_TOKEN", Some("flynt-token")),
                ("GITHUB_TOKEN", Some("ci-token")),
            ],
            || {
                // FLYNT_GITHUB_TOKEN wins over GITHUB_TOKEN.
                assert_eq!(lookup_token("github.com").as_deref(), Some("flynt-token"));
            },
        );
    }

    #[test]
    fn lookup_returns_none_when_no_env_set() {
        with_env(
            &[
                ("FLYNT_NOWHERE_COM_TOKEN", None),
                ("FLYNT_GITHUB_TOKEN", None),
                ("GITHUB_TOKEN", None),
            ],
            || {
                assert!(lookup_token("nowhere.com").is_none());
            },
        );
    }
}
