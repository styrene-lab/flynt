# Naming Migration History

## Timeline

| Date | Change | Scope |
|------|--------|-------|
| Pre-2026-04 | Original name: **Codex** | All surfaces |
| 2026-04-28 | Codex → **Codyx** | User-facing only (binary, UI, docs). Internal infra kept `codex-*` to avoid churn. |
| 2026-05-05 | Codyx → **Flynt** | Full rename — all surfaces including crates, bundle ID, config paths, env vars. |
| 2026-05-10 | "Vault" → **"Project"** | "Vault" wrongly evoked HashiCorp Vault / a secure enclave; Flynt's working directory is just a git-backed project. Renamed types, env vars, fields. Inner `EntityKind::Project` dissolved (one project per top-level dir). |

## Current State (v0.6.x)

The rename to **Flynt** is complete across all surfaces:

- Crate names: `flynt-core`, `flynt-store`, `flynt-app`, `flynt-agent`, `flynt-mobile`, `flynt-models`
- Module paths: `flynt_core::`, `flynt_store::`, etc.
- Bundle ID: `io.styrene.codex` **(kept for TestFlight continuity — will change to `io.styrene.flynt` when stable)**
- App Group: `group.io.styrene.codex` **(same — deferred)**
- Project config dir: `.flynt/`
- Local state dir: `.flynt-local/`
- Index database: `flynt-index.db`
- Env vars: `FLYNT_PROJECT` (legacy aliases: `FLYNT_VAULT`, `CODEX_VAULT`), `FLYNT_LOCAL_STATE`, `FLYNT_LAUNCHER_PROFILE`, `FLYNT_BUILD_HASH`
- OAuth scheme: `flynt-oauth`
- CSS classes: `.flynt-shell`, `.flynt-body`
- Binary name: `flynt`
- Display name: Flynt

## Backwards Compatibility

The following fallbacks exist for users migrating from pre-Flynt installations:

### Project config directory
- `Project::open()` checks for `.flynt/config.toml` first
- Falls back to `.codex/config.toml` if not found
- Auto-renames `.codex/` → `.flynt/` and `.codex-local/` → `.flynt-local/` on migration

### Launcher profile
- Checks `~/.config/flynt/launcher-profile.json` first
- Falls back to `~/.config/codex/launcher-profile.json`

### Environment variables
- `FLYNT_*` vars are primary
- `CODEX_*` vars are checked as fallback with deprecation warning
- Old var support will be removed in 2.0.0

## Decision Record

- **2026-04-28:** Product renamed from Codex to Codyx to avoid collision with OpenAI's `codex` CLI. User-facing surfaces updated immediately. Internal infrastructure deferred.
- **2026-05-05:** Full rename from Codyx to Flynt. Positioning as Obsidian competitor. All internal infrastructure (crates, config paths, env vars) renamed. Auto-migration added for existing projects. Bundle ID kept as `io.styrene.codex` to preserve TestFlight continuity — will change to `io.styrene.flynt` when a stable build ships.
