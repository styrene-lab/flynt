# Codyx Local Launch Status

- `cargo check -p codex-app` passes.
- `cargo test -p codex-core -p codex-store` passes: 300 tests passed, 1 ignored.
- Local run command from repo root: `just run`.
- Hot/dev UI command from repo root: `just run-ui`.
- Explicit vault override options:
  - `CODEX_VAULT=/path/to/vault cargo run -p codex-app`
  - `cargo run -p codex-app -- --vault /path/to/vault`
- The app binary is named `codyx`, while crate/package names still use `codex-*`.
- There is a current default-vault mismatch:
  - App/bootstrap default: `~/Documents/Codyx`
  - Justfile default: `~/Documents/Codex`
- Current uncommitted changes observed:
  - `crates/codex-store/src/vault.rs`: NomadNet/Micron publication export additions.
  - `ai/memory/facts.db`: memory DB changed by agent activity.
- Recent session memory indicates in-app wikilink handling was fixed: `codex-note://` links are intercepted and resolved to notes instead of falling through to the OS/browser.

Conclusion: Codyx is launchable locally now. The main cleanup before polished local-dev status is resolving the `Codex` vs `Codyx` vault path mismatch in `Justfile`.
