# Contributing to Flynt

Guidelines for building, testing, branching, and collaborating.

## Development Model

**Trunk-based development** on `main`. Direct commits for small, self-contained changes. Feature branches for multi-file or multi-session work.

### When to branch

| Scenario | Approach |
|---|---|
| Single-file fix, typo, config tweak | Commit directly to `main` |
| Multi-file feature or refactor | `feat/<name>` or `fix/<name>` branch + PR |
| Multi-session work | Feature branch, push regularly |

### Branch naming

```
feat/clone-project-dialog
fix/editor-cursor-position
refactor/sync-credentials
chore/bump-dependencies
test/git-sync-coverage
```

### Merging

- **Squash merge** for feature branches (clean history on main)
- **Fast-forward** for single-commit branches
- Delete the branch after merge

## Prerequisites

```sh
# Rust toolchain
rustup toolchain install stable

# Dioxus CLI (for desktop/mobile builds)
cargo install dioxus-cli

# macOS desktop
# Nothing extra — wry uses WKWebView

# Linux desktop
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libglib2.0-dev \
  libxdo-dev libayatana-appindicator3-dev pkg-config cmake

# iOS (macOS only)
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
```

## Build

```sh
# Check all crates
cargo check --workspace

# Desktop release
cd crates/flynt-app && dx build --platform desktop --release

# iOS release
cd crates/flynt-mobile && IPHONEOS_DEPLOYMENT_TARGET=17.0 dx build --platform ios --device --release

# Full release (macOS DMG + iOS IPA)
./scripts/build-release.sh 0.3.0
```

## Test

```sh
# All tests
cargo test -p flynt-core -p flynt-store

# Specific test file
cargo test -p flynt-store --test git_sync
cargo test -p flynt-store --test sandbox

# Single test
cargo test -p flynt-core -- decay_rate_none
```

### Test coverage expectations

Every PR should:
- Not break existing tests
- Add tests for new public functions
- Add regression tests for bug fixes

Current coverage: 212 tests across `flynt-core` (110) and `flynt-store` (102).

## Commits

[Conventional Commits](https://www.conventionalcommits.org/) required.

```
feat(sync): add SSH credential callbacks for git2
fix(editor): cursor starts at end of document, not position 0
test(git): add merge conflict and push rejection tests
chore: bump workspace version to 0.3.0
docs: add onboarding guide for testers
```

Commit messages explain *why*, not just *what*. Include the motivation in the body when the subject line isn't self-evident.

## Architecture rules

### No Node.js

All JavaScript (CodeMirror 6, Excalidraw) is vendored as pre-built static bundles in `crates/flynt-app/assets/vendor/`. No `npm`, no `node_modules`, no `package.json` in the app crates. The `site/` directory is the only place npm is used (for the Astro landing page).

### Markdown is canonical

The SQLite index is derived from the `.md` files on disk. It rebuilds from scratch on every `project.reindex()`. Never store authoritative data only in SQLite.

### No MCP

The agent extension uses ACP (Agent Client Protocol) over stdio, not MCP. Don't extend via MCP tools.

### Platform paths

- Use `dirs` crate for home/config/document directories — never hardcode `/tmp` or `$HOME`
- Gate macOS-specific code with `#[cfg(target_os = "macos")]`
- Vendor `libgit2` on all platforms (no system dependency)

### Error handling

- Never use `.unwrap()` in non-test code paths that touch user data
- Surface errors to the user with human-readable messages
- Never silently swallow errors — log at minimum, show in UI when possible

## Release process

```sh
# 1. Bump version in Cargo.toml
# 2. Build + sign + notarize
./scripts/build-release.sh X.Y.Z
xcrun notarytool submit dist/Flynt-X.Y.Z.dmg --apple-id "..." --team-id "..." --password "..." --wait
xcrun stapler staple dist/Flynt-X.Y.Z.dmg

# 3. Tag + push
git tag vX.Y.Z && git push origin vX.Y.Z

# 4. CI builds Linux + creates GitHub Release
```

## Secrets / token trust model

Forge tokens (GitHub PAT, GitLab token, Forgejo token) are held in a
single process-level `SecretBag` shared between the desktop client
and the embedded Omegon agent. Both surfaces — the GUI's metadata
strip pickers that push to upstream issues, and the agent tools
that call the same APIs — read tokens from the same source.

**The implication:** if you trust the agent with your forge tokens
(which you must, to use any forge integration), you trust the GUI
client with the same tokens. There's no adversarial boundary between
them. They're both operator-driven; both run with operator
permissions; both write through the same `flynt-forge::push_task`
seam.

Practical note: tokens come from `bootstrap_secrets` (omegon pushes
them at session start) or, as a fallback, from `FLYNT_GITHUB_TOKEN`
in the environment. They're never written to disk by Flynt itself —
the SecretBag is in-memory only.

## Project layout

```
Cargo.toml                  Workspace root
crates/
  flynt-core/               Models, parser, query engine, templates, graph
  flynt-store/              Project I/O, SQLite, git/iCloud sync, file watching
  flynt-app/                Desktop UI (Dioxus desktop)
  flynt-mobile/             iOS UI (Dioxus mobile)
  flynt-agent/              MCP extension binary for Omegon
site/                       flynt.styrene.io landing page (Astro)
scripts/
  build-release.sh          macOS DMG + iOS IPA build pipeline
docs/
  onboarding.md             Tester onboarding guide
  architecture.md           System architecture
```
